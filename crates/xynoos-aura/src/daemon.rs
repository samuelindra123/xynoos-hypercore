#![forbid(unsafe_code)]
use crate::interplay::{AuraKernelCoordinator, KernelTuningRecommendation};
use crate::tiering::{AdaptiveTieringPolicy, AuraTieringCoordinator};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use xynoos_core::MemoryPressure;
use xynoos_hypermemory::{crc32_ieee, ClusterStore, PageStore};
use xynoos_linux_control::{
    transaction::{self, Backend, Identity, Outcome},
    Action, MAX_HIGH_BYTES, MAX_LOW_BYTES,
};
use xynoos_pressureiq::{
    intelligence::{collect as collect_mem_obs, diagnose, Diagnosis, Observation},
    system::SystemSnapshot,
};

const MAX_TELEMETRY_HISTORY: usize = 32;
const ERROR_LOG_COOLDOWN_CYCLES: u64 = 4;

fn current_boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn process_start_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rparen = stat.rfind(')')?;
    let after = stat.get(rparen + 2..)?;
    // Field 22 in /proc/<pid>/stat is index 19 after ') ' (field 3 is state at index 0)
    after.split_whitespace().nth(19)?.parse::<u64>().ok()
}

/// Read current resident set size (bytes) of this process from `/proc/self/statm`.
pub fn process_rss_bytes() -> Option<usize> {
    let statm = fs::read_to_string("/proc/self/statm").ok()?;
    let rss_pages = statm.split_whitespace().nth(1)?.parse::<usize>().ok()?;
    Some(rss_pages.saturating_mul(4096))
}

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
    #[serde(default = "default_adaptive_polling")]
    pub adaptive_polling: bool,
}

fn default_adaptive_polling() -> bool {
    true
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            mode: DaemonMode::ObserveOnly,
            interval_ms: 5000,
            max_backoff_ms: 60000,
            pid_file: Some(PathBuf::from("/run/xynoos/hypercore.pid")),
            journal_file: Some(PathBuf::from("/run/xynoos/hypercore_journal.json")),
            max_rss_bytes: 64 * 1024 * 1024, // 64 MiB hard limit
            default_protection_bytes: 16 * 1024 * 1024,
            default_throttle_bytes: 64 * 1024 * 1024,
            adaptive_polling: true,
        }
    }
}

/// Jurnal pemulihan crash untuk mencatat transaksi aktif ke penyimpanan lokal sebelum eksekusi.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionJournal {
    #[serde(default = "default_journal_schema")]
    pub schema_version: u32,
    pub timestamp_epoch_secs: u64,
    #[serde(default)]
    pub boot_id: Option<String>,
    #[serde(default)]
    pub process_pid: u32,
    #[serde(default)]
    pub process_start_ticks: Option<u64>,
    pub identity: Identity,
    pub action: Action,
    pub original_value: u64,
    pub requested_value: u64,
    pub status: JournalStatus,
    #[serde(default)]
    pub integrity_crc: u32,
}

fn default_journal_schema() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JournalStatus {
    Pending,
    Applied,
    RolledBack,
}

impl TransactionJournal {
    fn compute_crc(&self) -> io::Result<u32> {
        let mut copy = self.clone();
        copy.integrity_crc = 0;
        let bytes =
            serde_json::to_vec(&copy).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(crc32_ieee(&bytes))
    }

    /// Menulis status jurnal transaksi ke berkas secara atomik dan tahan crash:
    /// tulis ke file sementara -> flush + fsync -> rename atomik -> sync direktori induk.
    pub fn persist(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let mut stamped = self.clone();
        if stamped.schema_version == 0 {
            stamped.schema_version = 1;
        }
        if stamped.process_pid == 0 {
            stamped.process_pid = std::process::id();
        }
        if stamped.process_start_ticks.is_none() {
            stamped.process_start_ticks = process_start_ticks(stamped.process_pid);
        }
        if stamped.boot_id.is_none() {
            stamped.boot_id = current_boot_id();
        }
        stamped.integrity_crc = stamped.compute_crc()?;

        let data = serde_json::to_vec_pretty(&stamped)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("hypercore_journal.json");
        let tmp_path = parent.join(format!(".{file_name}.tmp.{}", std::process::id()));

        {
            let mut tmp = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;
            tmp.write_all(&data)?;
            tmp.flush()?;
            tmp.sync_all()?;
        }

        fs::rename(&tmp_path, path)?;
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    /// Membaca dan memverifikasi integritas jurnal transaksi dari disk jika tersedia.
    pub fn load(path: &Path) -> io::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(path)?;
        let journal: Self = serde_json::from_slice(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if journal.integrity_crc != 0 {
            let expected = journal.compute_crc()?;
            if journal.integrity_crc != expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal integrity CRC mismatch",
                ));
            }
        }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PidLockRecord {
    pid: u32,
    start_ticks: Option<u64>,
    boot_id: Option<String>,
}

/// Mekanisme penguncian PID instans tunggal yang tahan terhadap PID reuse dan race condition.
pub struct PidLock {
    path: PathBuf,
}

impl PidLock {
    fn is_lock_holder_alive(path: &Path) -> Result<bool, String> {
        let Ok(content) = fs::read_to_string(path) else {
            return Ok(false);
        };
        let trimmed = content.trim();
        let current_boot = current_boot_id();

        if let Ok(rec) = serde_json::from_str::<PidLockRecord>(trimmed) {
            if let (Some(locked_boot), Some(host_boot)) = (&rec.boot_id, &current_boot) {
                if locked_boot != host_boot {
                    return Ok(false);
                }
            }
            let proc_path = PathBuf::from(format!("/proc/{}", rec.pid));
            if !proc_path.exists() {
                return Ok(false);
            }
            if let (Some(expected_ticks), Some(actual_ticks)) =
                (rec.start_ticks, process_start_ticks(rec.pid))
            {
                if expected_ticks != actual_ticks {
                    // PID was reused by a different process
                    return Ok(false);
                }
            }
            return Err(format!(
                "Daemon sudah berjalan dengan PID {} (lock: {})",
                rec.pid,
                path.display()
            ));
        }

        // Legacy plain-PID fallback
        if let Ok(pid) = trimmed.parse::<u32>() {
            if PathBuf::from(format!("/proc/{}", pid)).exists() {
                return Err(format!(
                    "Daemon sudah berjalan dengan PID {} (lock: {})",
                    pid,
                    path.display()
                ));
            }
        }
        Ok(false)
    }

    pub fn acquire(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Gagal membuat direktori lock {}: {}", parent.display(), e))?;
        }

        let current_pid = std::process::id();
        let record = PidLockRecord {
            pid: current_pid,
            start_ticks: process_start_ticks(current_pid),
            boot_id: current_boot_id(),
        };
        let payload = serde_json::to_vec(&record)
            .map_err(|e| format!("Gagal serialisasi PID lock: {}", e))?;

        for _attempt in 0..3 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(&payload)
                        .and_then(|_| file.sync_all())
                        .map_err(|e| format!("Gagal menulis PID lock: {}", e))?;
                    return Ok(Self { path });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    // Check if existing lock belongs to a live non-reused process
                    Self::is_lock_holder_alive(&path)?;
                    // Stale lock: remove and retry atomic creation
                    let _ = fs::remove_file(&path);
                }
                Err(e) => {
                    return Err(format!(
                        "Gagal membuat berkas PID lock di {}: {}",
                        path.display(),
                        e
                    ));
                }
            }
        }

        Err(format!(
            "Gagal memperoleh PID lock di {} setelah percobaan ulang",
            path.display()
        ))
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PressureHistoryEntry {
    raw: MemoryPressure,
    stabilized: MemoryPressure,
    available_pct_permille: u16,
}

/// Diagnostik kesehatan runtime daemon untuk pemantauan terstruktur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonDiagnostics {
    pub mode: DaemonMode,
    pub uptime_secs: u64,
    pub cycles_completed: u64,
    pub cycles_failed: u64,
    pub current_backoff_ms: u64,
    pub effective_interval_ms: u64,
    pub last_raw_pressure: Option<MemoryPressure>,
    pub last_stabilized_pressure: Option<MemoryPressure>,
    pub last_recommendation: Option<KernelTuningRecommendation>,
    pub last_error: Option<String>,
    pub recovered_crashes_count: u64,
    pub active_cluster_pages: usize,
    pub cluster_savings_bytes: usize,
    pub rss_bytes: Option<usize>,
    pub rss_within_budget: bool,
    pub history_len: usize,
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
    steady_low_cycles: u64,
    suppressed_errors: u64,
    last_error_cycle: u64,
    telemetry_history: VecDeque<PressureHistoryEntry>,
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

        // Keep initial empty stores lightweight to minimize idle daemon RSS
        let cluster_store = ClusterStore::with_default_clustering(16)?;
        let page_store = PageStore::with_capacity(32)?;

        Ok(Self {
            config,
            coordinator: kernel_coord,
            cluster_store,
            page_store,
            start_time: Instant::now(),
            cycles_completed: 0,
            cycles_failed: 0,
            current_backoff_ms: 0,
            steady_low_cycles: 0,
            suppressed_errors: 0,
            last_error_cycle: 0,
            telemetry_history: VecDeque::with_capacity(MAX_TELEMETRY_HISTORY),
            last_raw_pressure: None,
            last_stabilized_pressure: None,
            last_recommendation: None,
            last_error: None,
            recovered_crashes: 0,
        })
    }

    pub fn effective_interval_ms(&self) -> u64 {
        if self.current_backoff_ms > 0 {
            return self.current_backoff_ms;
        }
        let base = self.config.interval_ms.max(100);
        if !self.config.adaptive_polling {
            return base;
        }
        match self.last_stabilized_pressure {
            Some(MemoryPressure::Low) if self.steady_low_cycles >= 3 => {
                base.saturating_mul(2).min(15_000)
            }
            Some(MemoryPressure::High | MemoryPressure::Critical) => (base / 2).max(250),
            _ => base,
        }
    }

    /// Rekonsiliasi pemulihan crash pada startup: membaca jurnal transaksi sebelumnya,
    /// memverifikasi boot ID, identitas cgroup, dan nilai aktual untuk deteksi konflik fail-closed.
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
            Err(e) => {
                return Err(format!(
                    "Gagal membaca/memverifikasi jurnal pemulihan: {}",
                    e
                ))
            }
        };

        if journal.status == JournalStatus::RolledBack {
            let _ = TransactionJournal::cleanup(journal_path);
            return Ok(false);
        }

        if let (Some(j_boot), Some(cur_boot)) = (&journal.boot_id, &current_boot_id()) {
            if j_boot != cur_boot {
                return Err(
                    "Boot ID berubah sejak transaksi dicatat; rollback cgroup pra-reboot dicegah (fail-closed)"
                        .into(),
                );
            }
        }

        let current_id = backend.identity()?;
        if current_id != journal.identity {
            return Err(
                "Identitas cgroup tidak cocok dengan jurnal crash; rollback dicegah demi keamanan"
                    .into(),
            );
        }

        let current_val = backend.read_control(journal.action)?;

        match journal.status {
            JournalStatus::Pending => {
                if current_val == journal.original_value {
                    // Crash occurred before the kernel write took place; safe to clean up.
                    let _ = TransactionJournal::cleanup(journal_path);
                    return Ok(false);
                }
                if current_val != journal.requested_value {
                    return Err(format!(
                        "Konflik eksternal terdeteksi pada jurnal Pending (nilai saat ini {}, diharapkan {} atau {}); rollback dibatalkan",
                        current_val, journal.original_value, journal.requested_value
                    ));
                }
            }
            JournalStatus::Applied => {
                if current_val == journal.original_value {
                    // Already restored before/during previous shutdown
                    let _ = TransactionJournal::cleanup(journal_path);
                    return Ok(false);
                }
                if current_val != journal.requested_value {
                    return Err(format!(
                        "Konflik eksternal terdeteksi pada jurnal Applied (nilai saat ini {}, diharapkan {}); rollback dibatalkan",
                        current_val, journal.requested_value
                    ));
                }
            }
            JournalStatus::RolledBack => {
                let _ = TransactionJournal::cleanup(journal_path);
                return Ok(false);
            }
        }

        // Restore original baseline and verify readback
        backend.write_control(journal.action, journal.original_value)?;
        let restored_val = backend.read_control(journal.action)?;
        if restored_val != journal.original_value {
            return Err("Verifikasi nilai rollback setelah crash recovery gagal".into());
        }

        let _ = TransactionJournal::cleanup(journal_path);
        self.recovered_crashes = self.recovered_crashes.saturating_add(1);
        Ok(true)
    }

    /// Menghitung batas penyesuaian kernel yang sadar-beban (*workload-aware*) berdasarkan
    /// konsumsi memori cgroup saat ini menggunakan aritmatika aman overflow.
    pub fn compute_workload_aware_action(
        &self,
        recommendation: &KernelTuningRecommendation,
        current_usage_bytes: u64,
    ) -> Option<Action> {
        if current_usage_bytes == 0 || current_usage_bytes == u64::MAX {
            return None;
        }
        let usage = current_usage_bytes as u128;
        match recommendation {
            KernelTuningRecommendation::None => None,
            KernelTuningRecommendation::ProtectEssential { .. } => {
                let target = ((usage.saturating_mul(110)) / 100).min(MAX_LOW_BYTES as u128) as u64;
                let aligned = target - (target % 4096);
                (aligned > 0).then_some(Action::MemoryLow { bytes: aligned })
            }
            KernelTuningRecommendation::ThrottleBackground { .. } => {
                let target = ((usage.saturating_mul(125)) / 100).min(MAX_HIGH_BYTES as u128) as u64;
                let aligned = target - (target % 4096);
                (aligned > 0).then_some(Action::MemoryHigh { bytes: aligned })
            }
            KernelTuningRecommendation::EmergencyLimit { .. } => {
                let target =
                    ((usage.saturating_mul(80)) / 100).clamp(4096, MAX_HIGH_BYTES as u128) as u64;
                let aligned = target - (target % 4096);
                (aligned > 0).then_some(Action::MemoryHigh { bytes: aligned })
            }
        }
    }

    fn record_history(
        &mut self,
        raw: MemoryPressure,
        stabilized: MemoryPressure,
        diag: &Diagnosis,
    ) {
        if self.telemetry_history.len() == MAX_TELEMETRY_HISTORY {
            self.telemetry_history.pop_front();
        }
        let avail_permille = diag
            .available_pct
            .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
            .map(|v| (v * 10.0) as u16)
            .unwrap_or(500);
        self.telemetry_history.push_back(PressureHistoryEntry {
            raw,
            stabilized,
            available_pct_permille: avail_permille,
        });
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
        if stabilized_pressure == MemoryPressure::Low {
            self.steady_low_cycles = self.steady_low_cycles.saturating_add(1);
        } else {
            self.steady_low_cycles = 0;
        }
        self.record_history(raw_pressure, stabilized_pressure, diagnosis);

        let recommendation = self.coordinator.recommend(stabilized_pressure);
        self.last_recommendation = Some(recommendation.clone());

        match self.config.mode {
            DaemonMode::ObserveOnly => {
                // Tidak melakukan kompresi memori userspace dan tidak memutasi kernel
            }
            DaemonMode::Advisory => {
                if let Err(e) = self
                    .coordinator
                    .tiering_coordinator
                    .step_with_pressure(&mut self.page_store, stabilized_pressure)
                {
                    let msg = format!("Advisory tiering error: {e}");
                    self.record_failure(msg.clone());
                    return Err(msg);
                }
            }
            DaemonMode::Transactional => {
                if let Err(e) = self
                    .coordinator
                    .tiering_coordinator
                    .step_with_pressure(&mut self.page_store, stabilized_pressure)
                {
                    let msg = format!("Transactional tiering error: {e}");
                    self.record_failure(msg.clone());
                    return Err(msg);
                }

                if let (Some(b), Some(snap)) = (backend, system_snapshot) {
                    if let Ok(id) = b.identity() {
                        // Prefer actual cgroup memory.current from snapshot; fall back to read_low only if non-zero
                        let current_usage = snap
                            .cgroup
                            .as_ref()
                            .ok()
                            .and_then(|c| c.memory_current_bytes.as_ref().ok().copied())
                            .filter(|&v| v > 0)
                            .or_else(|| b.read_low().ok().filter(|&v| v > 0))
                            .unwrap_or(0);

                        if let Some(action) =
                            self.compute_workload_aware_action(&recommendation, current_usage)
                        {
                            if let Ok(orig) = b.read_control(action) {
                                let wrote_journal = if let Some(journal_path) =
                                    &self.config.journal_file
                                {
                                    let journal = TransactionJournal {
                                        schema_version: 1,
                                        timestamp_epoch_secs: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_secs())
                                            .unwrap_or(0),
                                        boot_id: current_boot_id(),
                                        process_pid: std::process::id(),
                                        process_start_ticks: process_start_ticks(std::process::id()),
                                        identity: id,
                                        action,
                                        original_value: orig,
                                        requested_value: action.bytes(),
                                        status: JournalStatus::Pending,
                                        integrity_crc: 0,
                                    };
                                    journal.persist(journal_path).is_ok()
                                } else {
                                    false
                                };

                                let report = transaction::run(b, snap, action, true);
                                match report.outcome {
                                    Outcome::Restored
                                    | Outcome::Declined
                                    | Outcome::ApplyFailed => {
                                        if wrote_journal {
                                            if let Some(journal_path) = &self.config.journal_file {
                                                let _ = TransactionJournal::cleanup(journal_path);
                                            }
                                        }
                                    }
                                    Outcome::Conflict
                                    | Outcome::VerificationFailed
                                    | Outcome::RollbackFailed => {
                                        let msg = format!(
                                            "Kernel transaction ended with {:?}: {:?}",
                                            report.outcome, report.events
                                        );
                                        self.record_failure(msg);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        self.cycles_completed = self.cycles_completed.saturating_add(1);
        self.current_backoff_ms = 0;
        self.last_error = None;

        Ok(self.diagnostics())
    }

    /// Menghasilkan snapshot diagnostik saat ini.
    pub fn diagnostics(&self) -> DaemonDiagnostics {
        let acct = self.cluster_store.overhead_accounting();
        let rss_bytes = process_rss_bytes();
        let rss_within_budget = rss_bytes.is_none_or(|r| r <= self.config.max_rss_bytes);
        DaemonDiagnostics {
            mode: self.config.mode,
            uptime_secs: self.start_time.elapsed().as_secs(),
            cycles_completed: self.cycles_completed,
            cycles_failed: self.cycles_failed,
            current_backoff_ms: self.current_backoff_ms,
            effective_interval_ms: self.effective_interval_ms(),
            last_raw_pressure: self.last_raw_pressure,
            last_stabilized_pressure: self.last_stabilized_pressure,
            last_recommendation: self.last_recommendation.clone(),
            last_error: self.last_error.clone(),
            recovered_crashes_count: self.recovered_crashes,
            active_cluster_pages: self.cluster_store.stats().page_count,
            cluster_savings_bytes: acct.net_savings_bytes.max(0) as usize,
            rss_bytes,
            rss_within_budget,
            history_len: self.telemetry_history.len(),
        }
    }

    /// Menangani kegagalan siklus dengan eksponensial backoff dan rate limiting error.
    pub fn record_failure(&mut self, err: String) {
        self.cycles_failed = self.cycles_failed.saturating_add(1);
        let cur = self.cycles_completed.saturating_add(self.cycles_failed);
        if self.last_error.is_none()
            || cur.saturating_sub(self.last_error_cycle) >= ERROR_LOG_COOLDOWN_CYCLES
        {
            self.last_error = Some(err);
            self.last_error_cycle = cur;
            self.suppressed_errors = 0;
        } else {
            self.suppressed_errors = self.suppressed_errors.saturating_add(1);
        }
        if self.current_backoff_ms == 0 {
            self.current_backoff_ms = 1000;
        } else {
            self.current_backoff_ms = self
                .current_backoff_ms
                .saturating_mul(2)
                .min(self.config.max_backoff_ms);
        }
    }

    /// Menjalankan loop daemon terikat sampai sinyal stop diterima menggunakan telemetri PressureIQ nyata.
    pub fn run_with_signal(
        &mut self,
        stop_signal: Arc<AtomicBool>,
        max_cycles: Option<u64>,
    ) -> Result<DaemonDiagnostics, String> {
        let mut executed = 0u64;
        let mut prev_obs: Option<(Observation, Instant)> = None;

        while !stop_signal.load(Ordering::SeqCst) {
            if let Some(max) = max_cycles {
                if executed >= max {
                    break;
                }
            }

            let obs_now = collect_mem_obs();
            let now_inst = Instant::now();
            let diagnosis = if let Some((ref prev, prev_inst)) = prev_obs {
                let interval = now_inst
                    .checked_duration_since(prev_inst)
                    .unwrap_or(Duration::from_millis(100))
                    .max(Duration::from_millis(100));
                diagnose(prev, &obs_now, interval, Duration::ZERO)
            } else {
                // First cycle has no delta yet; emit conservative non-fabricated reliable=false diagnosis
                let avail_pct = match (obs_now.mem_total, obs_now.mem_available) {
                    (Some(t), Some(a)) if t > 0 && a <= t => Some(a as f64 * 100.0 / t as f64),
                    _ => None,
                };
                Diagnosis {
                    classes: vec!["incomplete_or_unreliable".into()],
                    evidence: Vec::new(),
                    limitations: vec!["initial daemon observation cycle awaiting delta".into()],
                    available_pct: avail_pct,
                    rates_per_second: std::collections::BTreeMap::new(),
                    disk_bytes_per_second: std::collections::BTreeMap::new(),
                    zram_ratios: std::collections::BTreeMap::new(),
                    interval_seconds: Some((self.config.interval_ms.max(100) as f64) / 1000.0),
                    reliable: false,
                }
            };
            prev_obs = Some((obs_now, now_inst));

            if let Err(err) = self.step::<transaction::DisposableCgroup>(&diagnosis, None, None) {
                self.record_failure(err);
            }
            executed = executed.saturating_add(1);

            if let Some(max) = max_cycles {
                if executed >= max {
                    break;
                }
            }

            if !stop_signal.load(Ordering::SeqCst) {
                let sleep_ms = if max_cycles.is_some() {
                    // Bounded CLI runs still observe real interval delta >= 100ms up to configured interval
                    self.effective_interval_ms().min(500)
                } else {
                    self.effective_interval_ms()
                };
                std::thread::sleep(Duration::from_millis(sleep_ms));
            }
        }

        Ok(self.diagnostics())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
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
        let config = DaemonConfig {
            mode: DaemonMode::ObserveOnly,
            ..Default::default()
        };
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
        assert_eq!(res.history_len, 1);
    }

    #[test]
    fn test_daemon_workload_aware_action_and_overflow_safety() {
        let config = DaemonConfig::default();
        let daemon = AuraDaemon::new(config).unwrap();

        let current_usage = 10 * 1024 * 1024;
        let rec = KernelTuningRecommendation::ProtectEssential {
            bytes: 16 * 1024 * 1024,
        };
        let action = daemon
            .compute_workload_aware_action(&rec, current_usage)
            .unwrap();
        assert_eq!(action.bytes(), 11 * 1024 * 1024);

        let rec_emerg = KernelTuningRecommendation::EmergencyLimit {
            bytes: 8 * 1024 * 1024,
        };
        let action_emerg = daemon
            .compute_workload_aware_action(&rec_emerg, current_usage)
            .unwrap();
        assert_eq!(action_emerg.bytes(), 8 * 1024 * 1024);

        // u64::MAX must never panic or overflow
        assert!(daemon
            .compute_workload_aware_action(&rec, u64::MAX)
            .is_none());
    }

    #[test]
    fn test_crash_journal_persistence_and_conflict_detection() {
        let temp_dir = std::env::temp_dir().join(format!("xynoos_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let journal_file = temp_dir.join("test_journal.json");

        let config = DaemonConfig {
            journal_file: Some(journal_file.clone()),
            ..Default::default()
        };
        let mut daemon = AuraDaemon::new(config).unwrap();

        let mock_id = Identity {
            device: 1,
            inode: 2,
            control_inode: 3,
        };
        let mut backend = MockBackend {
            id: mock_id,
            low: 32 * 1024 * 1024,
            high: 0,
            max: 0,
        };

        let journal = TransactionJournal {
            schema_version: 1,
            timestamp_epoch_secs: 1234567,
            boot_id: current_boot_id(),
            process_pid: std::process::id(),
            process_start_ticks: process_start_ticks(std::process::id()),
            identity: mock_id,
            action: Action::MemoryLow {
                bytes: 32 * 1024 * 1024,
            },
            original_value: 16 * 1024 * 1024,
            requested_value: 32 * 1024 * 1024,
            status: JournalStatus::Applied,
            integrity_crc: 0,
        };
        journal.persist(&journal_file).unwrap();
        assert!(journal_file.exists());

        // External conflict test: someone changed memory.low externally to 48 MiB
        backend.low = 48 * 1024 * 1024;
        assert!(daemon.reconcile_crash_recovery(&mut backend).is_err());
        assert_eq!(backend.low, 48 * 1024 * 1024); // fail-closed: not overwritten!

        // Normal crash recovery when value matches requested_value
        backend.low = 32 * 1024 * 1024;
        let recovered = daemon.reconcile_crash_recovery(&mut backend).unwrap();
        assert!(recovered);
        assert_eq!(backend.low, 16 * 1024 * 1024);
        assert!(!journal_file.exists());
        assert_eq!(daemon.diagnostics().recovered_crashes_count, 1);

        // Corrupted/partial journal test: fail closed
        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(&journal_file, "{\"partial\":").unwrap();
        assert!(daemon.reconcile_crash_recovery(&mut backend).is_err());

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_pid_lock_enforcement_and_pid_reuse_protection() {
        let temp_dir = std::env::temp_dir().join(format!("xynoos_pid_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let pid_path = temp_dir.join("daemon.pid");

        let lock1 = PidLock::acquire(pid_path.clone()).unwrap();
        assert!(pid_path.exists());

        // Same live process + matching start_ticks must be rejected
        assert!(PidLock::acquire(pid_path.clone()).is_err());

        drop(lock1);
        assert!(!pid_path.exists());

        // Simulate PID reuse: write current PID with wrong start_ticks
        fs::create_dir_all(&temp_dir).unwrap();
        let stale_rec = PidLockRecord {
            pid: std::process::id(),
            start_ticks: Some(u64::MAX),
            boot_id: current_boot_id(),
        };
        fs::write(&pid_path, serde_json::to_vec(&stale_rec).unwrap()).unwrap();
        let lock2 = PidLock::acquire(pid_path.clone()).unwrap();
        drop(lock2);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_daemon_exponential_backoff_and_bounded_history() {
        let mut daemon = AuraDaemon::new(DaemonConfig::default()).unwrap();
        assert_eq!(daemon.diagnostics().current_backoff_ms, 0);

        daemon.record_failure("error 1".into());
        assert_eq!(daemon.diagnostics().current_backoff_ms, 1000);

        daemon.record_failure("error 2".into());
        assert_eq!(daemon.diagnostics().current_backoff_ms, 2000);

        daemon.record_failure("error 3".into());
        assert_eq!(daemon.diagnostics().current_backoff_ms, 4000);

        let diag = Diagnosis {
            classes: vec!["normal".into()],
            evidence: Vec::new(),
            limitations: Vec::new(),
            available_pct: Some(80.0),
            rates_per_second: std::collections::BTreeMap::new(),
            disk_bytes_per_second: std::collections::BTreeMap::new(),
            zram_ratios: std::collections::BTreeMap::new(),
            interval_seconds: Some(1.0),
            reliable: true,
        };
        for _ in 0..64 {
            daemon.step::<MockBackend>(&diag, None, None).unwrap();
        }
        assert_eq!(daemon.diagnostics().history_len, MAX_TELEMETRY_HISTORY);
    }

    #[test]
    fn test_daemon_run_with_signal_real_telemetry() {
        let mut daemon = AuraDaemon::new(DaemonConfig {
            interval_ms: 100,
            ..Default::default()
        })
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let diag = daemon.run_with_signal(stop, Some(2)).unwrap();
        assert_eq!(diag.cycles_completed, 2);
    }
}
