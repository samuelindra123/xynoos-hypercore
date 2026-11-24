#![forbid(unsafe_code)]
use crate::interplay::{AuraKernelCoordinator, KernelTuningRecommendation};
use crate::tiering::{AdaptiveTieringPolicy, AuraTieringCoordinator};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use xynoos_core::MemoryPressure;
use xynoos_hypermemory::{ClusterStore, PageStore};
use xynoos_linux_control::{
    transaction::{self, Backend, Identity, Outcome},
    Action, MAX_HIGH_BYTES, MAX_LOW_BYTES,
};
use xynoos_pressureiq::{intelligence::Diagnosis, system::SystemSnapshot};

/// Mode operasional daemon HyperCore AURA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DaemonMode {
    /// Mode default teraman: hanya membaca telemetri, mengevaluasi status, dan mencatat diagnostik.
    /// Sama sekali tidak memutasi memori userspace dan tidak melakukan modifikasi kernel.
    #[default]
    ObserveOnly,
    /// Mode advisory: mengevaluasi tiering userspace dan menghasilkan rekomendasi cgroup,
    /// namun menahan eksekusi transaksi kernel (tidak melakukan penulisan cgroup).
    Advisory,
    /// Mode transaksional aktif: membutuhkan otorisasi eksplisit (`authorized: true`).
    /// Menerapkan penyesuaian cgroup terikat dengan pencatatan jurnal transaksi dan rollback otomatis.
    Transactional,
}

/// Konfigurasi runtime daemon AURA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub mode: DaemonMode,
    pub interval_ms: u64,
    pub max_backoff_ms: u64,
    pub pid_file: Option<PathBuf>,
    pub journal_file: Option<PathBuf>,
    pub max_rss_bytes: usize,
    pub default_protection_bytes: u64,
    pub default_throttle_bytes: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            mode: DaemonMode::ObserveOnly,
            interval_ms: 5000,
            max_backoff_ms: 60000,
            pid_file: Some(PathBuf::from("/run/xynoos/hypercore.pid")),
            journal_file: Some(PathBuf::from("/run/xynoos/hypercore_journal.json")),
            max_rss_bytes: 64 * 1024 * 1024, // 64 MiB limit
            default_protection_bytes: 16 * 1024 * 1024,
            default_throttle_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Jurnal pemulihan crash untuk mencatat transaksi aktif ke penyimpanan lokal sebelum eksekusi.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionJournal {
    pub timestamp_epoch_secs: u64,
    pub identity: Identity,
    pub action: Action,
    pub original_value: u64,
    pub requested_value: u64,
    pub status: JournalStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JournalStatus {
    Pending,
    Applied,
    RolledBack,
}

impl TransactionJournal {
    /// Menulis status jurnal transaksi ke berkas secara sinkron.
    pub fn persist(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, data)
    }

    /// Membaca jurnal transaksi dari disk jika tersedia.
    pub fn load(path: &Path) -> io::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(path)?;
        let journal: Self = serde_json::from_slice(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Some(journal))
    }

    /// Menghapus berkas jurnal setelah transaksi selesai dipulihkan/dibersihkan.
    pub fn cleanup(path: &Path) -> io::Result<()> {
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// Mekanisme penguncian PID instans tunggal untuk mencegah eksekusi ganda daemon.
pub struct PidLock {
    path: PathBuf,
}

impl PidLock {
    pub fn acquire(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(pid) = content.trim().parse::<u32>() {
                    // Cek apakah proses dengan PID tersebut masih aktif di /proc/<pid>
                    let proc_path = PathBuf::from(format!("/proc/{}", pid));
                    if proc_path.exists() {
                        return Err(format!(
                            "Daemon sudah berjalan dengan PID {} (lock: {})",
                            pid,
                            path.display()
                        ));
                    }
                }
            }
            // Jika PID file lama sudah stale, hapus
            let _ = fs::remove_file(&path);
        }

        let current_pid = std::process::id();
        let mut file = File::create(&path)
            .map_err(|e| format!("Gagal membuat berkas PID lock di {}: {}", path.display(), e))?;
        file.write_all(format!("{}\n", current_pid).as_bytes())
            .map_err(|e| format!("Gagal menulis PID ke lockfile: {}", e))?;

        Ok(Self { path })
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Diagnostik kesehatan runtime daemon untuk pemantauan terstruktur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonDiagnostics {
    pub mode: DaemonMode,
    pub uptime_secs: u64,
    pub cycles_completed: u64,
    pub cycles_failed: u64,
    pub current_backoff_ms: u64,
    pub last_raw_pressure: Option<MemoryPressure>,
    pub last_stabilized_pressure: Option<MemoryPressure>,
    pub last_recommendation: Option<KernelTuningRecommendation>,
    pub last_error: Option<String>,
    pub recovered_crashes_count: u64,
    pub active_cluster_pages: usize,
    pub cluster_savings_bytes: usize,
}

/// Koordinator daemon AURA terpadu.
pub struct AuraDaemon {
    pub config: DaemonConfig,
    pub coordinator: AuraKernelCoordinator,
    pub cluster_store: ClusterStore,
    pub page_store: PageStore,
    start_time: Instant,
    cycles_completed: u64,
    cycles_failed: u64,
    current_backoff_ms: u64,
    last_raw_pressure: Option<MemoryPressure>,
    last_stabilized_pressure: Option<MemoryPressure>,
    last_recommendation: Option<KernelTuningRecommendation>,
    last_error: Option<String>,
    recovered_crashes: u64,
}

impl AuraDaemon {
    pub fn new(config: DaemonConfig) -> Result<Self, xynoos_hypermemory::Error> {
        let tiering_coord = AuraTieringCoordinator::new(AdaptiveTieringPolicy::default());
        let mut kernel_coord = AuraKernelCoordinator::new(tiering_coord);
        kernel_coord.default_protection_bytes = config.default_protection_bytes;
        kernel_coord.default_throttle_bytes = config.default_throttle_bytes;

        let cluster_store = ClusterStore::with_default_clustering(64)?;
        let page_store = PageStore::with_capacity(128)?;

        Ok(Self {
            config,
            coordinator: kernel_coord,
            cluster_store,
            page_store,
            start_time: Instant::now(),
            cycles_completed: 0,
            cycles_failed: 0,
            current_backoff_ms: 0,
            last_raw_pressure: None,
            last_stabilized_pressure: None,
            last_recommendation: None,
            last_error: None,
            recovered_crashes: 0,
        })
    }

    /// Rekonsiliasi pemulihan crash pada startup: membaca jurnal transaksi sebelumnya,
    /// memverifikasi identitas cgroup, dan mengembalikan limit jika terjadi crash saat transaksi aktif.
    pub fn reconcile_crash_recovery<B: Backend + ?Sized>(
        &mut self,
        backend: &mut B,
    ) -> Result<bool, String> {
        let journal_path = match &self.config.journal_file {
            Some(p) => p,
            None => return Ok(false),
        };

        let journal = match TransactionJournal::load(journal_path) {
            Ok(Some(j)) => j,
            Ok(None) => return Ok(false),
            Err(e) => return Err(format!("Gagal membaca jurnal pemulihan: {}", e)),
        };

        if journal.status == JournalStatus::RolledBack {
            let _ = TransactionJournal::cleanup(journal_path);
            return Ok(false);
        }

        let current_id = backend.identity()?;
        if current_id != journal.identity {
            return Err(
                "Identitas cgroup tidak cocok dengan jurnal crash; rollback dicegah demi keamanan"
                    .into(),
            );
        }

        // Kembalikan ke original value
        backend.write_control(journal.action, journal.original_value)?;
        let restored_val = backend.read_control(journal.action)?;
        if restored_val != journal.original_value {
            return Err("Verifikasi nilai rollback setelah crash recovery gagal".into());
        }

        let _ = TransactionJournal::cleanup(journal_path);
        self.recovered_crashes += 1;
        Ok(true)
    }

    /// Menghitung batas penyesuaian kernel yang sadar-beban (*workload-aware*) berdasarkan
    /// konsumsi memori cgroup saat ini, mencegah penetapan batas statis yang buta.
    pub fn compute_workload_aware_action(
        &self,
        recommendation: &KernelTuningRecommendation,
        current_usage_bytes: u64,
    ) -> Option<Action> {
        match recommendation {
            KernelTuningRecommendation::None => None,
            KernelTuningRecommendation::ProtectEssential { .. } => {
                // Beri proteksi 110% dari penggunaan saat ini, dibatasi MAX_LOW_BYTES
                let target = (current_usage_bytes * 110 / 100).min(MAX_LOW_BYTES);
                let aligned = target - (target % 4096);
                if aligned == 0 {
                    None
                } else {
                    Some(Action::MemoryLow { bytes: aligned })
                }
            }
            KernelTuningRecommendation::ThrottleBackground { .. } => {
                // Terapkan throttling moderat pada 125% dari penggunaan saat ini
                let target = (current_usage_bytes * 125 / 100).min(MAX_HIGH_BYTES);
                let aligned = target - (target % 4096);
                if aligned == 0 {
                    None
                } else {
                    Some(Action::MemoryHigh { bytes: aligned })
                }
            }
            KernelTuningRecommendation::EmergencyLimit { .. } => {
                // Batas darurat: tekan alokasi ke 80% dari penggunaan saat ini
                let target = (current_usage_bytes * 80 / 100)
                    .max(4096)
                    .min(MAX_HIGH_BYTES);
                let aligned = target - (target % 4096);
                if aligned == 0 {
                    None
                } else {
                    Some(Action::MemoryHigh { bytes: aligned })
                }
            }
        }
    }

    /// Menjalankan satu siklus evaluasi daemon.
    pub fn step<B: Backend + ?Sized>(
        &mut self,
        diagnosis: &Diagnosis,
        system_snapshot: Option<&SystemSnapshot>,
        backend: Option<&mut B>,
    ) -> Result<DaemonDiagnostics, String> {
        let raw_pressure = crate::tiering::PressureBridge::evaluate_pressure(diagnosis);
        let stabilized_pressure = self
            .coordinator
            .tiering_coordinator
            .stabilizer
            .step(raw_pressure);

        self.last_raw_pressure = Some(raw_pressure);
        self.last_stabilized_pressure = Some(stabilized_pressure);

        let recommendation = self.coordinator.recommend(stabilized_pressure);
        self.last_recommendation = Some(recommendation.clone());

        match self.config.mode {
            DaemonMode::ObserveOnly => {
                // Tidak melakukan kompresi memori userspace dan tidak memutasi kernel
            }
            DaemonMode::Advisory => {
                // Terapkan tiering pada PageStore userspace saja
                let _ = self
                    .coordinator
                    .tiering_coordinator
                    .step_with_pressure(&mut self.page_store, stabilized_pressure);
            }
            DaemonMode::Transactional => {
                // Terapkan tiering userspace
                let _ = self
                    .coordinator
                    .tiering_coordinator
                    .step_with_pressure(&mut self.page_store, stabilized_pressure);

                // Terapkan transaksi kernel dengan pencatatan jurnal
                if let (Some(b), Some(snap)) = (backend, system_snapshot) {
                    if let Ok(id) = b.identity() {
                        let current_usage = b.read_low().unwrap_or(0);
                        if let Some(action) =
                            self.compute_workload_aware_action(&recommendation, current_usage)
                        {
                            if let Ok(orig) = b.read_control(action) {
                                if let Some(journal_path) = &self.config.journal_file {
                                    let journal = TransactionJournal {
                                        timestamp_epoch_secs: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_secs())
                                            .unwrap_or(0),
                                        identity: id,
                                        action,
                                        original_value: orig,
                                        requested_value: action.bytes(),
                                        status: JournalStatus::Pending,
                                    };
                                    let _ = journal.persist(journal_path);
                                }

                                let report = transaction::run(b, snap, action, true);
                                if report.outcome == Outcome::Restored {
                                    if let Some(journal_path) = &self.config.journal_file {
                                        let _ = TransactionJournal::cleanup(journal_path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        self.cycles_completed += 1;
        self.current_backoff_ms = 0; // Reset backoff jika siklus sukses
        self.last_error = None;

        Ok(self.diagnostics())
    }

    /// Menghasilkan snapshot diagnostik saat ini.
    pub fn diagnostics(&self) -> DaemonDiagnostics {
        let stats = self.cluster_store.stats();
        let savings = stats.original_bytes.saturating_sub(stats.stored_bytes);
        DaemonDiagnostics {
            mode: self.config.mode,
            uptime_secs: self.start_time.elapsed().as_secs(),
            cycles_completed: self.cycles_completed,
            cycles_failed: self.cycles_failed,
            current_backoff_ms: self.current_backoff_ms,
            last_raw_pressure: self.last_raw_pressure,
            last_stabilized_pressure: self.last_stabilized_pressure,
            last_recommendation: self.last_recommendation.clone(),
            last_error: self.last_error.clone(),
            recovered_crashes_count: self.recovered_crashes,
            active_cluster_pages: stats.page_count,
            cluster_savings_bytes: savings,
        }
    }

    /// Menangani kegagalan siklus dengan eksponensial backoff.
    pub fn record_failure(&mut self, err: String) {
        self.cycles_failed += 1;
        self.last_error = Some(err);
        if self.current_backoff_ms == 0 {
            self.current_backoff_ms = 1000;
        } else {
            self.current_backoff_ms = (self.current_backoff_ms * 2).min(self.config.max_backoff_ms);
        }
    }

    /// Menjalankan loop daemon terikat sampai sinyal stop diterima.
    pub fn run_with_signal(
        &mut self,
        stop_signal: Arc<AtomicBool>,
        max_cycles: Option<u64>,
    ) -> Result<DaemonDiagnostics, String> {
        let mut executed = 0;
        while !stop_signal.load(Ordering::SeqCst) {
            if let Some(max) = max_cycles {
                if executed >= max {
                    break;
                }
            }

            let diagnosis = Diagnosis {
                classes: vec!["normal".into()],
                evidence: Vec::new(),
                limitations: Vec::new(),
                available_pct: Some(45.0),
                rates_per_second: std::collections::BTreeMap::new(),
                disk_bytes_per_second: std::collections::BTreeMap::new(),
                zram_ratios: std::collections::BTreeMap::new(),
                interval_seconds: Some(5.0),
                reliable: true,
            };

            let _ = self.step::<transaction::DisposableCgroup>(&diagnosis, None, None);
            executed += 1;

            if max_cycles.is_none() {
                let sleep_duration = if self.current_backoff_ms > 0 {
                    Duration::from_millis(self.current_backoff_ms)
                } else {
                    Duration::from_millis(self.config.interval_ms)
                };
                std::thread::sleep(sleep_duration);
            }
        }

        Ok(self.diagnostics())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBackend {
        pub id: Identity,
        pub low: u64,
        pub high: u64,
        pub max: u64,
    }

    impl Backend for MockBackend {
        fn identity(&mut self) -> Result<Identity, String> {
            Ok(self.id)
        }
        fn read_low(&mut self) -> Result<u64, String> {
            Ok(self.low)
        }
        fn write_low(&mut self, val: u64) -> Result<(), String> {
            self.low = val;
            Ok(())
        }
        fn read_high(&mut self) -> Result<u64, String> {
            Ok(self.high)
        }
        fn write_high(&mut self, val: u64) -> Result<(), String> {
            self.high = val;
            Ok(())
        }
    }

    #[test]
    fn test_daemon_lifecycle_observe_only() {
        let mut config = DaemonConfig::default();
        config.mode = DaemonMode::ObserveOnly;
        let mut daemon = AuraDaemon::new(config).unwrap();

        let diag = Diagnosis {
            classes: vec!["normal".into()],
            evidence: Vec::new(),
            limitations: Vec::new(),
            available_pct: Some(50.0),
            rates_per_second: std::collections::BTreeMap::new(),
            disk_bytes_per_second: std::collections::BTreeMap::new(),
            zram_ratios: std::collections::BTreeMap::new(),
            interval_seconds: Some(1.0),
            reliable: true,
        };

        let res = daemon.step::<MockBackend>(&diag, None, None).unwrap();
        assert_eq!(res.mode, DaemonMode::ObserveOnly);
        assert_eq!(res.cycles_completed, 1);
        assert_eq!(res.last_raw_pressure, Some(MemoryPressure::Low));
    }

    #[test]
    fn test_daemon_workload_aware_action() {
        let config = DaemonConfig::default();
        let daemon = AuraDaemon::new(config).unwrap();

        // Workload current 10 MiB
        let current_usage = 10 * 1024 * 1024;
        let rec = KernelTuningRecommendation::ProtectEssential {
            bytes: 16 * 1024 * 1024,
        };
        let action = daemon
            .compute_workload_aware_action(&rec, current_usage)
            .unwrap();
        // 110% of 10 MiB = 11 MiB
        assert_eq!(action.bytes(), 11 * 1024 * 1024);

        // EmergencyLimit: 80% of 10 MiB = 8 MiB
        let rec_emerg = KernelTuningRecommendation::EmergencyLimit {
            bytes: 8 * 1024 * 1024,
        };
        let action_emerg = daemon
            .compute_workload_aware_action(&rec_emerg, current_usage)
            .unwrap();
        assert_eq!(action_emerg.bytes(), 8 * 1024 * 1024);
    }

    #[test]
    fn test_crash_journal_persistence_and_recovery() {
        let temp_dir = std::env::temp_dir().join(format!("xynoos_test_{}", std::process::id()));
        let journal_file = temp_dir.join("test_journal.json");

        let mut config = DaemonConfig::default();
        config.journal_file = Some(journal_file.clone());
        let mut daemon = AuraDaemon::new(config).unwrap();

        let mock_id = Identity {
            device: 1,
            inode: 2,
            control_inode: 3,
        };
        let mut backend = MockBackend {
            id: mock_id,
            low: 32 * 1024 * 1024, // modified state (e.g. crash left it at 32 MiB)
            high: 0,
            max: 0,
        };

        // Simulasikan jurnal tertinggal sebelum crash: original adalah 16 MiB
        let journal = TransactionJournal {
            timestamp_epoch_secs: 1234567,
            identity: mock_id,
            action: Action::MemoryLow {
                bytes: 32 * 1024 * 1024,
            },
            original_value: 16 * 1024 * 1024,
            requested_value: 32 * 1024 * 1024,
            status: JournalStatus::Applied,
        };
        journal.persist(&journal_file).unwrap();
        assert!(journal_file.exists());

        // Jalankan crash recovery
        let recovered = daemon.reconcile_crash_recovery(&mut backend).unwrap();
        assert!(recovered);
        // Pastikan nilai dikembalikan ke original_value (16 MiB)
        assert_eq!(backend.low, 16 * 1024 * 1024);
        assert!(!journal_file.exists()); // berkas jurnal dibersihkan
        assert_eq!(daemon.diagnostics().recovered_crashes_count, 1);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_pid_lock_enforcement() {
        let temp_dir = std::env::temp_dir().join(format!("xynoos_pid_{}", std::process::id()));
        let pid_path = temp_dir.join("daemon.pid");

        let lock1 = PidLock::acquire(pid_path.clone()).unwrap();
        assert!(pid_path.exists());

        // Lock kedua untuk PID yang sama harus gagal
        let lock2_err = PidLock::acquire(pid_path.clone());
        assert!(lock2_err.is_err());

        drop(lock1);
        assert!(!pid_path.exists());

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_daemon_exponential_backoff() {
        let mut daemon = AuraDaemon::new(DaemonConfig::default()).unwrap();
        assert_eq!(daemon.diagnostics().current_backoff_ms, 0);

        daemon.record_failure("error 1".into());
        assert_eq!(daemon.diagnostics().current_backoff_ms, 1000);

        daemon.record_failure("error 2".into());
        assert_eq!(daemon.diagnostics().current_backoff_ms, 2000);

        daemon.record_failure("error 3".into());
        assert_eq!(daemon.diagnostics().current_backoff_ms, 4000);
    }

    #[test]
    fn test_daemon_run_with_signal() {
        let mut daemon = AuraDaemon::new(DaemonConfig::default()).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let diag = daemon.run_with_signal(stop, Some(3)).unwrap();
        assert_eq!(diag.cycles_completed, 3);
    }
}
