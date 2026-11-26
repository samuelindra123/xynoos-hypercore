#![forbid(unsafe_code)]
use crate::compression_policy::{AdaptiveCompressionConfig, MultiResourceState};
use crate::interplay::{AuraKernelCoordinator, KernelTuningRecommendation};
use crate::tiering::{
    AdaptiveTieringPolicy, AdaptiveTieringReport, AuraAdaptiveCoordinator, AuraTieringCoordinator,
    PressureStabilizer,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use xynoos_core::{MemoryPressure, MigrationBudget, ResourcePressureLevel};
use xynoos_hypermemory::{ClusterStore, PageStore};
use xynoos_linux_control::{
    transaction::{self, Backend, Outcome},
    Action, MAX_HIGH_BYTES, MAX_LOW_BYTES,
};
use xynoos_pressureiq::{
    intelligence::Diagnosis,
    multi_resource::{
        collect_multi_resource, diagnose_multi_resource, MultiResourceDiagnosis,
        MultiResourceSnapshot,
    },
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

/// Kesalahan validasi konfigurasi runtime daemon AURA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigValidationError {
    InvalidInterval {
        min: u64,
        max: u64,
        actual: u64,
    },
    InvalidBackoff {
        interval: u64,
        actual: u64,
    },
    InvalidMaxRss {
        min: usize,
        max: usize,
        actual: usize,
    },
    InvalidProtectionBytes {
        max: u64,
        actual: u64,
    },
    InvalidThrottleBytes {
        max: u64,
        actual: u64,
    },
    InsecurePath {
        field: &'static str,
        path: PathBuf,
        reason: &'static str,
    },
    IoError {
        path: PathBuf,
        error: String,
    },
    DeserializationError(String),
}

impl std::fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInterval { min, max, actual } => {
                write!(
                    f,
                    "interval_ms {actual} out of bounds: must be between {min} and {max} ms"
                )
            }
            Self::InvalidBackoff { interval, actual } => {
                write!(
                    f,
                    "max_backoff_ms {actual} invalid: must be >= interval_ms ({interval} ms)"
                )
            }
            Self::InvalidMaxRss { min, max, actual } => {
                write!(
                    f,
                    "max_rss_bytes {actual} out of bounds: must be between {min} and {max} bytes"
                )
            }
            Self::InvalidProtectionBytes { max, actual } => {
                write!(
                    f,
                    "default_protection_bytes {actual} invalid: must not exceed {max} bytes"
                )
            }
            Self::InvalidThrottleBytes { max, actual } => {
                write!(
                    f,
                    "default_throttle_bytes {actual} invalid: must not exceed {max} bytes"
                )
            }
            Self::InsecurePath {
                field,
                path,
                reason,
            } => {
                write!(f, "{field} path '{}' is insecure: {reason}", path.display())
            }
            Self::IoError { path, error } => {
                write!(
                    f,
                    "failed to read configuration at '{}': {error}",
                    path.display()
                )
            }
            Self::DeserializationError(msg) => {
                write!(f, "configuration parse error: {msg}")
            }
        }
    }
}
impl std::error::Error for ConfigValidationError {}

/// Konfigurasi runtime daemon AURA terverifikasi dan deterministik.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonConfig {
    #[serde(default = "default_config_schema")]
    pub schema_version: u32,
    pub mode: DaemonMode,
    pub interval_ms: u64,
    pub max_backoff_ms: u64,
    pub pid_file: Option<PathBuf>,
    pub journal_file: Option<PathBuf>,
    #[serde(default = "default_active_interventions_file")]
    pub active_interventions_file: Option<PathBuf>,
    #[serde(default = "default_auth_file")]
    pub auth_file: Option<PathBuf>,
    pub max_rss_bytes: usize,
    pub default_protection_bytes: u64,
    pub default_throttle_bytes: u64,
    #[serde(default = "default_adaptive_polling")]
    pub adaptive_polling: bool,
    #[serde(default)]
    pub safety_guard: crate::safety_guard::SafetyGuardConfig,
}

fn default_config_schema() -> u32 {
    1
}

fn default_adaptive_polling() -> bool {
    true
}

fn default_auth_file() -> Option<PathBuf> {
    Some(PathBuf::from("/var/lib/xynoos/authorization.json"))
}

fn default_active_interventions_file() -> Option<PathBuf> {
    Some(PathBuf::from("/var/lib/xynoos/active_interventions.json"))
}

impl DaemonConfig {
    /// Memvalidasi seluruh batasan nilai konfigurasi sebelum aktivasi.
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if self.interval_ms < 10 || self.interval_ms > 3_600_000 {
            return Err(ConfigValidationError::InvalidInterval {
                min: 10,
                max: 3_600_000,
                actual: self.interval_ms,
            });
        }
        if self.max_backoff_ms < self.interval_ms {
            return Err(ConfigValidationError::InvalidBackoff {
                interval: self.interval_ms,
                actual: self.max_backoff_ms,
            });
        }
        if self.max_rss_bytes < 1024 * 1024 || self.max_rss_bytes > 16 * 1024 * 1024 * 1024 {
            return Err(ConfigValidationError::InvalidMaxRss {
                min: 1024 * 1024,
                max: 16 * 1024 * 1024 * 1024,
                actual: self.max_rss_bytes,
            });
        }
        if self.default_protection_bytes > 512 * 1024 * 1024 {
            return Err(ConfigValidationError::InvalidProtectionBytes {
                max: 512 * 1024 * 1024,
                actual: self.default_protection_bytes,
            });
        }
        if self.default_throttle_bytes > 2 * 1024 * 1024 * 1024 {
            return Err(ConfigValidationError::InvalidThrottleBytes {
                max: 2 * 1024 * 1024 * 1024,
                actual: self.default_throttle_bytes,
            });
        }

        if let Some(ref p) = self.pid_file {
            validate_safe_path("pid_file", p)?;
        }
        if let Some(ref j) = self.journal_file {
            validate_safe_path("journal_file", j)?;
        }
        if let Some(ref act) = self.active_interventions_file {
            validate_safe_path("active_interventions_file", act)?;
        }
        if let Some(ref a) = self.auth_file {
            validate_safe_path("auth_file", a)?;
        }

        Ok(())
    }

    /// Membaca dan memvalidasi konfigurasi dari teks JSON.
    pub fn from_json_str(s: &str) -> Result<Self, ConfigValidationError> {
        let cfg: Self = serde_json::from_str(s)
            .map_err(|e| ConfigValidationError::DeserializationError(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Membaca dan memvalidasi berkas konfigurasi dari disk.
    pub fn from_file(path: &Path) -> Result<Self, ConfigValidationError> {
        let content = fs::read_to_string(path).map_err(|e| ConfigValidationError::IoError {
            path: path.to_path_buf(),
            error: e.to_string(),
        })?;
        Self::from_json_str(&content)
    }
}

fn validate_safe_path(field: &'static str, path: &Path) -> Result<(), ConfigValidationError> {
    if !path.is_absolute() {
        return Err(ConfigValidationError::InsecurePath {
            field,
            path: path.to_path_buf(),
            reason: "path must be absolute",
        });
    }
    let s = path.to_string_lossy();
    if s == "/"
        || s.starts_with("/etc")
        || s.starts_with("/proc")
        || s.starts_with("/sys")
        || s.starts_with("/dev")
    {
        return Err(ConfigValidationError::InsecurePath {
            field,
            path: path.to_path_buf(),
            reason: "system directory targets are strictly prohibited",
        });
    }
    Ok(())
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            mode: DaemonMode::ObserveOnly,
            interval_ms: 5000,
            max_backoff_ms: 60000,
            pid_file: Some(PathBuf::from("/run/xynoos/hypercore.pid")),
            journal_file: Some(PathBuf::from("/run/xynoos/hypercore_journal.json")),
            active_interventions_file: default_active_interventions_file(),
            auth_file: default_auth_file(),
            max_rss_bytes: 64 * 1024 * 1024, // 64 MiB hard limit
            default_protection_bytes: 16 * 1024 * 1024,
            default_throttle_bytes: 64 * 1024 * 1024,
            adaptive_polling: true,
            safety_guard: crate::safety_guard::SafetyGuardConfig::default(),
        }
    }
}

pub use xynoos_linux_control::journal::{
    reconcile_durable_interventions_sysfs, recover_durable_lease, DurableInterventionLedger,
    DurableInterventionRecord, JournalStatus, LeasePhase, RecoveryOutcome, RecoveryReport,
    TransactionJournal,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PidLockRecord {
    pid: u32,
    start_ticks: Option<u64>,
    boot_id: Option<String>,
}

/// Mekanisme penguncian PID instans tunggal yang tahan terhadap PID reuse dan race condition.
#[derive(Debug)]
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

        let json_target = trimmed
            .lines()
            .find(|l| l.trim_start().starts_with('{'))
            .unwrap_or(trimmed);

        if let Ok(rec) = serde_json::from_str::<PidLockRecord>(json_target) {
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

        // Standard plain-PID fallback (line 1 integer for start-stop-daemon compatibility)
        let first_line = trimmed.lines().next().unwrap_or(trimmed);
        if let Ok(pid) = first_line.trim().parse::<u32>() {
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
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "Refusing to acquire PID lock through existing symlink: {}",
                    path.display()
                ));
            }
        }

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
        let rec_json = serde_json::to_string(&record)
            .map_err(|e| format!("Gagal serialisasi PID lock: {}", e))?;
        // First line: numeric PID (for standard init / start-stop-daemon compatibility)
        // Second line: JSON record (for robust PID-reuse and boot verification)
        let payload = format!("{}\n{}\n", current_pid, rec_json);

        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }

        for _attempt in 0..3 {
            match opts.open(&path) {
                Ok(mut file) => {
                    file.write_all(payload.as_bytes())
                        .and_then(|_| file.sync_all())
                        .map_err(|e| format!("Gagal menulis PID lock: {}", e))?;
                    return Ok(Self { path });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    // Check if existing lock belongs to a live non-reused process
                    Self::is_lock_holder_alive(&path)?;
                    // Stale lock: check not a symlink before removal
                    if let Ok(meta) = fs::symlink_metadata(&path) {
                        if meta.file_type().is_symlink() {
                            return Err(format!(
                                "Stale PID lock target is a symlink: {}",
                                path.display()
                            ));
                        }
                    }
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
        if let Ok(meta) = fs::symlink_metadata(&self.path) {
            if !meta.file_type().is_symlink() {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PressureHistoryEntry {
    raw: MemoryPressure,
    stabilized: MemoryPressure,
    available_pct_permille: Option<u16>,
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
    #[serde(default)]
    pub last_cpu_pressure: Option<ResourcePressureLevel>,
    #[serde(default)]
    pub last_io_pressure: Option<ResourcePressureLevel>,
    #[serde(default)]
    pub adaptive_transitions_total: u64,
    pub last_recommendation: Option<KernelTuningRecommendation>,
    pub last_error: Option<String>,
    pub recovered_crashes_count: u64,
    pub active_cluster_pages: usize,
    pub cluster_savings_bytes: usize,
    pub rss_bytes: Option<usize>,
    pub rss_within_budget: bool,
    pub history_len: usize,
    #[serde(default)]
    pub safety_guard: Option<crate::safety_guard::SafetyGuardStatus>,
    #[serde(default)]
    pub workload_classification: Option<crate::workload_controller::WorkloadClass>,
    #[serde(default)]
    pub coordinator_state: Option<crate::continuous_coordinator::CoordinatorState>,
    #[serde(default)]
    pub active_interventions: Vec<crate::continuous_coordinator::InterventionRecord>,
    #[serde(default)]
    pub polling_tier: Option<crate::telemetry_scheduler::PollingTier>,
    #[serde(default)]
    pub circuit_breaker_active: bool,
    #[serde(default)]
    pub authorization_scope: Option<String>,
    #[serde(default)]
    pub last_telemetry: Option<crate::telemetry_scheduler::TelemetrySample>,
}

/// Koordinator daemon AURA terpadu.
pub struct AuraDaemon {
    pub config: DaemonConfig,
    pub safety_guard: crate::safety_guard::RuntimeSafetyGuard,
    pub coordinator: AuraKernelCoordinator,
    pub adaptive_coordinator: AuraAdaptiveCoordinator,
    pub cluster_store: ClusterStore,
    pub page_store: PageStore,
    pub telemetry_scheduler: crate::telemetry_scheduler::TelemetryScheduler,
    pub continuous_coordinator: crate::continuous_coordinator::ContinuousCoordinator,
    pub runtime_auth: xynoos_linux_control::config::RuntimeAuthorizationContract,
    pub last_workload: Option<crate::workload_controller::WorkloadClass>,
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
    last_cpu_pressure: Option<ResourcePressureLevel>,
    last_io_pressure: Option<ResourcePressureLevel>,
    adaptive_transitions_total: u64,
    last_recommendation: Option<KernelTuningRecommendation>,
    last_error: Option<String>,
    recovered_crashes: u64,
}

impl AuraDaemon {
    pub fn new(config: DaemonConfig) -> Result<Self, xynoos_hypermemory::Error> {
        config
            .validate()
            .map_err(|_| xynoos_hypermemory::Error::InvalidCapacity)?;
        let safety_guard =
            crate::safety_guard::RuntimeSafetyGuard::new(config.safety_guard.clone());
        let tiering_coord = AuraTieringCoordinator::new(AdaptiveTieringPolicy::default());
        let mut kernel_coord = AuraKernelCoordinator::new(tiering_coord);
        kernel_coord.default_protection_bytes = config.default_protection_bytes;
        kernel_coord.default_throttle_bytes = config.default_throttle_bytes;

        let adaptive_coordinator = AuraAdaptiveCoordinator::new(
            AdaptiveCompressionConfig::default(),
            PressureStabilizer::new(3, 0),
            MigrationBudget::bounded(32, 256 * 1024),
        );

        // Keep initial empty stores lightweight to minimize idle daemon RSS
        let cluster_store = ClusterStore::with_default_clustering(16)?;
        let page_store = PageStore::with_capacity(32)?;

        let telemetry_scheduler = crate::telemetry_scheduler::TelemetryScheduler::new(
            PathBuf::from("/proc"),
            PathBuf::from("/sys"),
            config.interval_ms,
        );
        let continuous_coordinator = crate::continuous_coordinator::ContinuousCoordinator::new();
        let runtime_auth = if let Some(ref auth_p) = config.auth_file {
            xynoos_linux_control::config::RuntimeAuthorizationContract::load_from(auth_p)
                .unwrap_or_default()
        } else {
            xynoos_linux_control::config::RuntimeAuthorizationContract::default()
        };

        Ok(Self {
            config,
            safety_guard,
            coordinator: kernel_coord,
            adaptive_coordinator,
            cluster_store,
            page_store,
            telemetry_scheduler,
            continuous_coordinator,
            runtime_auth,
            last_workload: None,
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
            last_cpu_pressure: None,
            last_io_pressure: None,
            adaptive_transitions_total: 0,
            last_recommendation: None,
            last_error: None,
            recovered_crashes: 0,
        })
    }

    /// Memperbarui konfigurasi daemon secara atomik setelah validasi menyeluruh.
    pub fn reload_config(&mut self, new_config: DaemonConfig) -> Result<(), ConfigValidationError> {
        new_config.validate()?;
        self.coordinator.default_protection_bytes = new_config.default_protection_bytes;
        self.coordinator.default_throttle_bytes = new_config.default_throttle_bytes;
        self.config = new_config;
        Ok(())
    }

    pub fn effective_interval_ms(&self) -> u64 {
        if self.current_backoff_ms > 0 {
            return self.current_backoff_ms;
        }
        let base = self.config.interval_ms.max(100);
        if !self.config.adaptive_polling {
            return base;
        }
        self.telemetry_scheduler.next_polling_interval().as_millis() as u64
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

        let rep = recover_durable_lease(backend, journal_path)?;
        match rep.outcome {
            RecoveryOutcome::Restored => {
                self.recovered_crashes = self.recovered_crashes.saturating_add(1);
                Ok(true)
            }
            RecoveryOutcome::AlreadyRestored | RecoveryOutcome::NoJournal => Ok(false),
            RecoveryOutcome::Conflict => Err(format!(
                "Konflik eksternal terdeteksi pada jurnal: {:?}; rollback dibatalkan",
                rep.events
            )),
            RecoveryOutcome::Invalidated => Err(format!(
                "Pemulihan crash tidak valid atau usang: {:?}; rollback dibatalkan (fail-closed)",
                rep.events
            )),
            RecoveryOutcome::RecoveryFailed => {
                Err(format!("Pemulihan crash gagal: {:?}", rep.events))
            }
        }
    }

    /// Rekonsiliasi pemulihan crash multi-siklus pada startup: membaca durable ledger
    /// (/var/lib/xynoos/active_interventions.json), memverifikasi boot ID, identitas inode,
    /// mendeteksi konflik eksternal, dan mengembalikan baseline original secara fail-closed.
    pub fn reconcile_startup_crash_recovery(
        &mut self,
        cgroup_root: Option<&Path>,
    ) -> Result<RecoveryReport, String> {
        if let Some(ref ledger_path) = self.config.active_interventions_file {
            if ledger_path.exists() {
                let rep = reconcile_durable_interventions_sysfs(ledger_path, cgroup_root)?;
                if rep.outcome == RecoveryOutcome::Restored {
                    self.recovered_crashes = self.recovered_crashes.saturating_add(1);
                }
                return Ok(rep);
            }
        }
        Ok(RecoveryReport {
            outcome: RecoveryOutcome::NoJournal,
            original_value: None,
            current_value: None,
            requested_value: None,
            events: Vec::new(),
        })
    }

    /// Menerapkan intervensi cgroup secara tahan banting (durable) dengan mendaftarkannya
    /// ke memory coordinator dan menyimpannya ke ledger persisten di disk sebelum eksekusi kernel.
    #[allow(clippy::too_many_arguments)]
    pub fn record_and_persist_intervention(
        &mut self,
        target_id: &str,
        control: &str,
        cgroup_inode: u64,
        control_inode: u64,
        original_value: u64,
        applied_value: u64,
        lease_duration_ms: u64,
        sample: &crate::telemetry_scheduler::TelemetrySample,
        now_ms: u64,
    ) {
        self.continuous_coordinator.record_intervention_applied(
            target_id,
            control,
            original_value,
            applied_value,
            lease_duration_ms,
            sample,
            now_ms,
        );

        if let Some(ref ledger_path) = self.config.active_interventions_file {
            let mut ledger = DurableInterventionLedger::load(ledger_path)
                .ok()
                .flatten()
                .unwrap_or_else(|| {
                    DurableInterventionLedger::new(
                        xynoos_linux_control::journal::current_boot_id().unwrap_or_default(),
                    )
                });

            let record = DurableInterventionRecord {
                lease_id: format!(
                    "{}-{}-{}-{}",
                    std::process::id(),
                    cgroup_inode,
                    control,
                    now_ms
                ),
                target_id: target_id.to_string(),
                control: control.to_string(),
                cgroup_inode,
                control_inode,
                original_value,
                applied_value,
                applied_at_epoch_ms: now_ms,
                lease_duration_ms,
                boot_id: xynoos_linux_control::journal::current_boot_id().unwrap_or_default(),
                owner_uid: xynoos_linux_control::current_euid(),
                process_pid: std::process::id(),
                status: LeasePhase::Applied,
            };

            ledger.upsert_record(record);
            let _ = ledger.persist(ledger_path);
        }
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
                // ThrottleBackground is designed to allocate headroom (125% of working set) bounded by MAX_HIGH_BYTES.
                // If current usage already exceeds or equals MAX_HIGH_BYTES, or if target <= current_usage_bytes,
                // clamping to MAX_HIGH_BYTES would set memory.high below the active working set,
                // inducing severe synchronous direct kernel reclaim and crippling the workload.
                // In such cases, the recommendation must be safely declined (None).
                if current_usage_bytes >= MAX_HIGH_BYTES {
                    return None;
                }
                let target = ((usage.saturating_mul(125)) / 100).min(MAX_HIGH_BYTES as u128) as u64;
                let aligned = target - (target % 4096);
                if aligned <= current_usage_bytes {
                    None
                } else {
                    Some(Action::MemoryHigh { bytes: aligned })
                }
            }
            KernelTuningRecommendation::EmergencyLimit { .. } => {
                let target =
                    ((usage.saturating_mul(80)) / 100).clamp(4096, MAX_HIGH_BYTES as u128) as u64;
                let aligned = target - (target % 4096);
                (aligned > 0).then_some(Action::MemoryHigh { bytes: aligned })
            }
        }
    }

    /// Menghitung batas penyesuaian kernel yang sadar-beban dengan pemeriksaan batasan
    /// administrator yang sudah ada sebelumnya agar tidak melemahkan batas atau proteksi pra-ada.
    pub fn compute_workload_aware_action_with_admin_guard(
        &self,
        recommendation: &KernelTuningRecommendation,
        current_usage_bytes: u64,
        pre_existing_control: Option<u64>,
    ) -> Option<Action> {
        let action = self.compute_workload_aware_action(recommendation, current_usage_bytes)?;
        if let Some(orig) = pre_existing_control {
            match action {
                Action::MemoryHigh { bytes } | Action::MemoryMax { bytes } => {
                    // Jika administrator sudah menyetel batas yang lebih ketat (< u64::MAX),
                    // usulan nilai yang lebih tinggi akan memperlemah batas administrator.
                    if orig != u64::MAX && bytes > orig {
                        return None;
                    }
                }
                Action::MemoryLow { bytes } => {
                    // Jika administrator sudah memberikan proteksi memory.low yang lebih tinggi,
                    // usulan nilai yang lebih rendah akan memperlemah proteksi administrator.
                    if orig > 0 && bytes < orig {
                        return None;
                    }
                }
            }
        }
        Some(action)
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
            .map(|v| (v * 10.0) as u16);
        self.telemetry_history.push_back(PressureHistoryEntry {
            raw,
            stabilized,
            available_pct_permille: avail_permille,
        });
    }

    fn execute_advisory_or_adaptive_tiering(
        &mut self,
        stabilized_pressure: MemoryPressure,
        multi_state: Option<&MultiResourceState>,
    ) -> Result<Option<AdaptiveTieringReport>, String> {
        if self.page_store.stats().page_count == 0 {
            return Ok(None);
        }
        if let Some(state) = multi_state {
            let report = self
                .adaptive_coordinator
                .step_adaptive(&mut self.page_store, state)
                .map_err(|e| format!("Adaptive tiering error: {e}"))?;
            self.adaptive_transitions_total = self
                .adaptive_transitions_total
                .saturating_add(report.policy_transitions as u64);
            Ok(Some(report))
        } else {
            self.coordinator
                .tiering_coordinator
                .step_with_pressure(&mut self.page_store, stabilized_pressure)
                .map_err(|e| format!("Tiering error: {e}"))?;
            Ok(None)
        }
    }

    /// Menjalankan satu siklus evaluasi daemon dengan konteks multi-sumber penuh (CPU/Memory/IO PSI + cgroup).
    pub fn step_multi_resource<B: Backend + ?Sized>(
        &mut self,
        multi: &MultiResourceDiagnosis,
        system_snapshot: Option<&SystemSnapshot>,
        backend: Option<&mut B>,
    ) -> Result<DaemonDiagnostics, String> {
        let raw_pressure =
            crate::tiering::PressureBridge::evaluate_pressure(&multi.memory_diagnosis);
        let multi_state = MultiResourceState::from_psi_percentages(
            raw_pressure,
            multi.cpu_psi_some_10,
            multi.io_psi_some_10,
            multi.mem_available_bytes,
            multi.memory_pressure_reliable,
        );
        self.last_cpu_pressure = Some(multi_state.cpu_pressure);
        self.last_io_pressure = Some(multi_state.io_pressure);
        self.step_inner(
            &multi.memory_diagnosis,
            Some(&multi_state),
            system_snapshot,
            backend,
        )
    }

    /// Menjalankan satu siklus evaluasi daemon.
    pub fn step<B: Backend + ?Sized>(
        &mut self,
        diagnosis: &Diagnosis,
        system_snapshot: Option<&SystemSnapshot>,
        backend: Option<&mut B>,
    ) -> Result<DaemonDiagnostics, String> {
        self.step_inner(diagnosis, None, system_snapshot, backend)
    }

    fn step_inner<B: Backend + ?Sized>(
        &mut self,
        diagnosis: &Diagnosis,
        multi_state: Option<&MultiResourceState>,
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
        let mut step_had_error = false;
        self.safety_guard.tick_cycle();

        match self.config.mode {
            DaemonMode::ObserveOnly => {
                // Tidak melakukan kompresi memori userspace dan tidak memutasi kernel
            }
            DaemonMode::Advisory => {
                if let Err(msg) =
                    self.execute_advisory_or_adaptive_tiering(stabilized_pressure, multi_state)
                {
                    self.record_failure(msg.clone());
                    return Err(msg);
                }
            }
            DaemonMode::Transactional => {
                if let Err(msg) =
                    self.execute_advisory_or_adaptive_tiering(stabilized_pressure, multi_state)
                {
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
                            let cpu_psi = snap.cpu_psi.as_ref().ok().map(|p| p.some.avg10);
                            let io_psi = snap.io_psi.as_ref().ok().map(|p| p.some.avg10);
                            let mem_psi =
                                snap.memory.psi.result.as_ref().ok().map(|p| p.some.avg10);

                            match self.safety_guard.can_intervene(cpu_psi, io_psi, mem_psi) {
                                Err(rejection) => {
                                    let msg = format!(
                                        "Safety guard declined kernel intervention: {rejection}"
                                    );
                                    self.record_failure(msg);
                                }
                                Ok(()) => {
                                    if let Ok(orig) = b.read_control(action) {
                                        // Refuse to weaken pre-existing administrator limits or protections
                                        let weakens_admin = match action {
                                            Action::MemoryHigh { bytes }
                                            | Action::MemoryMax { bytes } => {
                                                orig != u64::MAX && bytes > orig
                                            }
                                            Action::MemoryLow { bytes } => orig > 0 && bytes < orig,
                                        };
                                        if weakens_admin {
                                            let msg = format!(
                                                "Declined kernel intervention: action {:?} would weaken pre-existing administrator configuration ({orig})",
                                                action
                                            );
                                            self.record_failure(msg);
                                        } else {
                                            let wrote_journal = if let Some(journal_path) =
                                                &self.config.journal_file
                                            {
                                                let journal = TransactionJournal {
                                                    schema_version:
                                                        TransactionJournal::CURRENT_SCHEMA_VERSION,
                                                    lease_id: format!(
                                                        "{}-{}-{}",
                                                        std::process::id(),
                                                        id.inode,
                                                        action.control_filename()
                                                    ),
                                                    timestamp_epoch_secs:
                                                        std::time::SystemTime::now()
                                                            .duration_since(std::time::UNIX_EPOCH)
                                                            .map(|d| d.as_secs())
                                                            .unwrap_or(0),
                                                    boot_id: current_boot_id(),
                                                    owner_uid: xynoos_linux_control::current_euid(),
                                                    process_pid: std::process::id(),
                                                    process_start_ticks: process_start_ticks(
                                                        std::process::id(),
                                                    ),
                                                    authorized_root: None,
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
                                                Outcome::Restored => {
                                                    let dir = match action {
                                                        Action::MemoryLow { .. } => 1,
                                                        Action::MemoryHigh { .. } => -1,
                                                        Action::MemoryMax { .. } => -1,
                                                    };
                                                    self.safety_guard.record_success(dir);
                                                    if wrote_journal {
                                                        if let Some(journal_path) =
                                                            &self.config.journal_file
                                                        {
                                                            let _ = TransactionJournal::cleanup(
                                                                journal_path,
                                                            );
                                                        }
                                                    }
                                                }
                                                Outcome::Declined | Outcome::ApplyFailed => {
                                                    self.safety_guard.record_failure(&format!(
                                                        "{:?}",
                                                        report.outcome
                                                    ));
                                                    if wrote_journal {
                                                        if let Some(journal_path) =
                                                            &self.config.journal_file
                                                        {
                                                            let _ = TransactionJournal::cleanup(
                                                                journal_path,
                                                            );
                                                        }
                                                    }
                                                }
                                                Outcome::Conflict | Outcome::VerificationFailed => {
                                                    let msg = format!(
                                                        "Kernel transaction ended with {:?}: {:?}",
                                                        report.outcome, report.events
                                                    );
                                                    self.safety_guard.record_failure(&msg);
                                                    self.record_failure(msg);
                                                    step_had_error = true;
                                                }
                                                Outcome::RollbackFailed => {
                                                    let msg = format!(
                                                        "Kernel transaction ended with {:?}: {:?}",
                                                        report.outcome, report.events
                                                    );
                                                    self.safety_guard.record_rollback_failure(&msg);
                                                    self.record_failure(msg);
                                                    step_had_error = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !step_had_error {
            self.cycles_completed = self.cycles_completed.saturating_add(1);
            self.current_backoff_ms = 0;
            self.last_error = None;
        }

        Ok(self.diagnostics())
    }

    /// Menghasilkan snapshot diagnostik saat ini.
    pub fn diagnostics(&self) -> DaemonDiagnostics {
        let acct = self.cluster_store.overhead_accounting();
        let rss_bytes = process_rss_bytes();
        let rss_within_budget = rss_bytes.is_none_or(|r| r <= self.config.max_rss_bytes);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let cb_active = self
            .continuous_coordinator
            .is_circuit_breaker_tripped(now_ms);
        let last_telemetry = self.telemetry_scheduler.last_sample().cloned();
        let polling_tier = Some(self.telemetry_scheduler.current_tier());
        let coord_state = Some(self.continuous_coordinator.state);
        let active_interventions = self.continuous_coordinator.active_interventions.clone();
        let auth_scope = if self.runtime_auth.is_revoked {
            Some("revoked".into())
        } else {
            Some(format!("{:?}", self.runtime_auth.allowed_scopes))
        };

        DaemonDiagnostics {
            mode: self.config.mode,
            uptime_secs: self.start_time.elapsed().as_secs(),
            cycles_completed: self.cycles_completed,
            cycles_failed: self.cycles_failed,
            current_backoff_ms: self.current_backoff_ms,
            effective_interval_ms: self.effective_interval_ms(),
            last_raw_pressure: self.last_raw_pressure,
            last_stabilized_pressure: self.last_stabilized_pressure,
            last_cpu_pressure: self.last_cpu_pressure,
            last_io_pressure: self.last_io_pressure,
            adaptive_transitions_total: self.adaptive_transitions_total,
            last_recommendation: self.last_recommendation.clone(),
            last_error: self.last_error.clone(),
            recovered_crashes_count: self.recovered_crashes,
            active_cluster_pages: self.cluster_store.stats().page_count,
            cluster_savings_bytes: acct.net_savings_bytes.max(0) as usize,
            rss_bytes,
            rss_within_budget,
            history_len: self.telemetry_history.len(),
            safety_guard: Some(self.safety_guard.status()),
            workload_classification: self.last_workload,
            coordinator_state: coord_state,
            active_interventions,
            polling_tier,
            circuit_breaker_active: cb_active,
            authorization_scope: auth_scope,
            last_telemetry,
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

    /// Revert all currently active temporary interventions to their baseline original values.
    pub fn revert_all_active_interventions(&mut self) -> usize {
        let mut count = 0;
        let items: Vec<_> = std::mem::take(&mut self.continuous_coordinator.active_interventions);
        for mut record in items {
            let cg_clean = record.target_id.trim_start_matches('/');
            if !cg_clean.is_empty() && !cg_clean.contains("..") {
                let target_path = PathBuf::from("/sys/fs/cgroup")
                    .join(cg_clean)
                    .join(&record.control);
                if target_path.exists() && !target_path.is_symlink() {
                    let _ = fs::write(&target_path, record.original_value.to_string().as_bytes());
                }
            }
            record.status = crate::continuous_coordinator::InterventionStatus::Reverted;
            record.message = "Intervention safely reverted to baseline".into();
            self.continuous_coordinator.history.push_back(record);
            count += 1;
        }
        if let Some(ref ledger_path) = self.config.active_interventions_file {
            let _ = DurableInterventionLedger::cleanup(ledger_path);
        }
        while self.continuous_coordinator.history.len() > 32 {
            self.continuous_coordinator.history.pop_front();
        }
        count
    }

    /// Menjalankan loop daemon terikat sampai sinyal stop diterima menggunakan telemetri multi-sumber PressureIQ nyata.
    pub fn run_with_signal(
        &mut self,
        stop_signal: Arc<AtomicBool>,
        max_cycles: Option<u64>,
    ) -> Result<DaemonDiagnostics, String> {
        // Reconcile any orphaned active interventions from prior ungraceful crash (SIGKILL/power loss)
        let _ = self.reconcile_startup_crash_recovery(None);

        let mut executed = 0u64;
        let mut prev_snap: Option<MultiResourceSnapshot> = None;

        while !stop_signal.load(Ordering::SeqCst) {
            if let Some(max) = max_cycles {
                if executed >= max {
                    break;
                }
            }

            // Adaptive Telemetry & Workload Intelligence
            let sample = self.telemetry_scheduler.collect_sample(None);
            let (workload, _) = crate::workload_controller::WorkloadController::classify(&sample);
            self.last_workload = Some(workload);

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            // Dynamically verify and reload authorization contract
            if let Some(ref auth_p) = self.config.auth_file {
                if let Ok(loaded_auth) =
                    xynoos_linux_control::config::RuntimeAuthorizationContract::load_from(auth_p)
                {
                    if (loaded_auth.is_revoked || now_ms > loaded_auth.expires_at_unix_ms)
                        && !self.runtime_auth.is_revoked
                    {
                        self.revert_all_active_interventions();
                    }
                    self.runtime_auth = loaded_auth;
                }
            }

            let reverted = self
                .continuous_coordinator
                .evaluate_closed_loop(&sample, now_ms);
            for rec in reverted {
                let cg_clean = rec.target_id.trim_start_matches('/');
                if !cg_clean.is_empty() && !cg_clean.contains("..") {
                    let target_path = PathBuf::from("/sys/fs/cgroup")
                        .join(cg_clean)
                        .join(&rec.control);
                    if target_path.exists() && !target_path.is_symlink() {
                        let _ = fs::write(&target_path, rec.original_value.to_string().as_bytes());
                    }
                }
                if let Some(ref ledger_path) = self.config.active_interventions_file {
                    if let Ok(Some(mut ledger)) = DurableInterventionLedger::load(ledger_path) {
                        ledger.remove_record(&rec.target_id, &rec.control);
                        if ledger.records.is_empty() {
                            let _ = DurableInterventionLedger::cleanup(ledger_path);
                        } else {
                            let _ = ledger.persist(ledger_path);
                        }
                    }
                }
            }

            let snap_now = collect_multi_resource(None);
            let now_inst = Instant::now();
            if let Some(ref prev) = prev_snap {
                let multi_diag = diagnose_multi_resource(
                    prev,
                    &snap_now,
                    now_inst,
                    Duration::from_millis(self.config.interval_ms.saturating_mul(2).max(5000)),
                );
                if let Err(err) = self.step_multi_resource::<transaction::DisposableCgroup>(
                    &multi_diag,
                    None,
                    None,
                ) {
                    self.record_failure(err);
                }
            } else {
                // First cycle has no delta yet; emit conservative non-fabricated reliable=false diagnosis
                let avail_pct = match (
                    snap_now.memory_obs.mem_total,
                    snap_now.memory_obs.mem_available,
                ) {
                    (Some(t), Some(a)) if t > 0 && a <= t => Some(a as f64 * 100.0 / t as f64),
                    _ => None,
                };
                let initial_diag = Diagnosis {
                    classes: vec!["incomplete_or_unreliable".into()],
                    evidence: Vec::new(),
                    limitations: vec!["initial daemon observation cycle awaiting delta".into()],
                    available_pct: avail_pct,
                    rates_per_second: std::collections::BTreeMap::new(),
                    disk_bytes_per_second: std::collections::BTreeMap::new(),
                    zram_ratios: std::collections::BTreeMap::new(),
                    interval_seconds: Some((self.config.interval_ms.max(100) as f64) / 1000.0),
                    reliable: false,
                };
                let multi_initial = MultiResourceDiagnosis {
                    memory_diagnosis: initial_diag,
                    cpu_psi_some_10: snap_now.cpu_psi.as_ref().ok().map(|p| p.some.avg10),
                    cpu_psi_full_10: snap_now
                        .cpu_psi
                        .as_ref()
                        .ok()
                        .and_then(|p| p.full.as_ref().map(|f| f.avg10)),
                    cpu_utilization_pct: None,
                    io_psi_some_10: snap_now.io_psi.as_ref().ok().map(|p| p.some.avg10),
                    io_psi_full_10: snap_now
                        .io_psi
                        .as_ref()
                        .ok()
                        .and_then(|p| p.full.as_ref().map(|f| f.avg10)),
                    mem_available_bytes: snap_now.memory_obs.mem_available,
                    mem_total_bytes: snap_now.memory_obs.mem_total,
                    cgroup_memory_current_bytes: snap_now
                        .cgroup_memory_current
                        .as_ref()
                        .ok()
                        .copied(),
                    cpu_pressure_reliable: false,
                    io_pressure_reliable: false,
                    memory_pressure_reliable: false,
                    telemetry_age_ms: 0,
                    limitations: vec!["initial daemon observation cycle awaiting delta".into()],
                };
                if let Err(err) = self.step_multi_resource::<transaction::DisposableCgroup>(
                    &multi_initial,
                    None,
                    None,
                ) {
                    self.record_failure(err);
                }
            }
            prev_snap = Some(snap_now);
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

        // On shutdown: safely release all active temporary interventions
        self.revert_all_active_interventions();
        self.continuous_coordinator.state =
            crate::continuous_coordinator::CoordinatorState::ShuttingDown;

        Ok(self.diagnostics())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xynoos_linux_control::transaction::Identity;

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

        let rec_throttle = KernelTuningRecommendation::ThrottleBackground {
            bytes: 64 * 1024 * 1024,
        };
        // Workload with headroom available under MAX_HIGH_BYTES (512 MiB)
        let action_throttle = daemon
            .compute_workload_aware_action(&rec_throttle, 100 * 1024 * 1024)
            .unwrap();
        assert_eq!(action_throttle.bytes(), 125 * 1024 * 1024);

        // Workload near ceiling: 450 MiB * 1.25 = 562.5 MiB, clamped to MAX_HIGH_BYTES (512 MiB)
        let action_near_ceiling = daemon
            .compute_workload_aware_action(&rec_throttle, 450 * 1024 * 1024)
            .unwrap();
        assert_eq!(action_near_ceiling.bytes(), MAX_HIGH_BYTES);

        // Large workloads >= MAX_HIGH_BYTES (512 MiB, 600 MiB) must be safely declined (None)
        // to prevent destructive synchronous direct page reclaim loops.
        assert!(daemon
            .compute_workload_aware_action(&rec_throttle, MAX_HIGH_BYTES)
            .is_none());
        assert!(daemon
            .compute_workload_aware_action(&rec_throttle, 600 * 1024 * 1024)
            .is_none());

        // Weakening administrator limits must be safely declined (None)
        // 1. Proposed memory.high (125 MiB) higher than pre-existing limit (100 MiB) -> declined
        assert!(daemon
            .compute_workload_aware_action_with_admin_guard(
                &rec_throttle,
                100 * 1024 * 1024,
                Some(100 * 1024 * 1024)
            )
            .is_none());
        // 2. Proposed memory.high (125 MiB) under unconstrained pre-existing limit (u64::MAX) -> allowed
        assert!(daemon
            .compute_workload_aware_action_with_admin_guard(
                &rec_throttle,
                100 * 1024 * 1024,
                Some(u64::MAX)
            )
            .is_some());
        // 3. Proposed memory.low (11 MiB) lower than pre-existing protection (64 MiB) -> declined
        assert!(daemon
            .compute_workload_aware_action_with_admin_guard(
                &rec,
                10 * 1024 * 1024,
                Some(64 * 1024 * 1024)
            )
            .is_none());
        // 4. Proposed memory.low (11 MiB) higher than unconfigured protection (0) -> allowed
        assert!(daemon
            .compute_workload_aware_action_with_admin_guard(&rec, 10 * 1024 * 1024, Some(0))
            .is_some());

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
            mount_id: None,
        };
        let mut backend = MockBackend {
            id: mock_id,
            low: 32 * 1024 * 1024,
            high: 0,
            max: 0,
        };

        let journal = TransactionJournal {
            schema_version: 1,
            lease_id: "test-lease".into(),
            timestamp_epoch_secs: 1234567,
            boot_id: current_boot_id(),
            owner_uid: 1000,
            process_pid: std::process::id(),
            process_start_ticks: process_start_ticks(std::process::id()),
            authorized_root: None,
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

        // Re-write journal for normal crash recovery test
        journal.persist(&journal_file).unwrap();

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

    #[test]
    fn test_transactional_conflict_preserves_backoff_and_error() {
        struct ConflictBackend {
            id: Identity,
        }
        impl Backend for ConflictBackend {
            fn identity(&mut self) -> Result<Identity, String> {
                Ok(self.id)
            }
            fn read_low(&mut self) -> Result<u64, String> {
                // First read returns 10 MiB, subsequent reads change unexpectedly -> Conflict
                static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let n = CALLS.fetch_add(1, Ordering::SeqCst);
                if n <= 1 {
                    Ok(10 * 1024 * 1024)
                } else {
                    Ok(12 * 1024 * 1024)
                }
            }
            fn write_low(&mut self, _v: u64) -> Result<(), String> {
                Ok(())
            }
        }

        let mut daemon = AuraDaemon::new(DaemonConfig {
            mode: DaemonMode::Transactional,
            journal_file: None,
            ..Default::default()
        })
        .unwrap();
        let diag = Diagnosis {
            classes: vec!["elevated_pressure".into()],
            evidence: Vec::new(),
            limitations: Vec::new(),
            available_pct: Some(20.0),
            rates_per_second: std::collections::BTreeMap::new(),
            disk_bytes_per_second: std::collections::BTreeMap::new(),
            zram_ratios: std::collections::BTreeMap::new(),
            interval_seconds: Some(1.0),
            reliable: true,
        };
        let mut snap = xynoos_pressureiq::system::collect_system();
        snap.memory.psi.result = Ok(xynoos_pressureiq::parse_psi(
            "some avg10=5.0 avg60=1.0 avg300=0.0 total=100\n",
        )
        .unwrap());
        snap.memory.mem.result = Ok(xynoos_pressureiq::parse_meminfo(
            "MemTotal: 4096000 kB\nMemAvailable: 1024000 kB\n",
        )
        .unwrap());
        let mut backend = ConflictBackend {
            id: Identity {
                device: 1,
                inode: 2,
                control_inode: 3,
                mount_id: None,
            },
        };

        let out = daemon.step(&diag, Some(&snap), Some(&mut backend)).unwrap();
        assert_eq!(out.cycles_failed, 1);
        assert!(out.current_backoff_ms >= 1000);
        assert!(out.last_error.is_some());
    }

    #[test]
    fn test_step_multi_resource_drives_adaptive_coordinator() {
        let mut daemon = AuraDaemon::new(DaemonConfig {
            mode: DaemonMode::Advisory,
            ..Default::default()
        })
        .unwrap();
        daemon.page_store = PageStore::adaptive(4096, 8, usize::MAX).unwrap();
        for i in 0..6u8 {
            daemon
                .page_store
                .insert_with_codec(
                    &[b'A' + i; 4096],
                    xynoos_core::CodecId::Raw,
                    xynoos_core::EvictionPolicy::Reject,
                )
                .unwrap();
        }
        let multi = MultiResourceDiagnosis {
            memory_diagnosis: Diagnosis {
                classes: vec!["sustained_pressure".into()],
                evidence: Vec::new(),
                limitations: Vec::new(),
                available_pct: Some(10.0),
                rates_per_second: std::collections::BTreeMap::new(),
                disk_bytes_per_second: std::collections::BTreeMap::new(),
                zram_ratios: std::collections::BTreeMap::new(),
                interval_seconds: Some(1.0),
                reliable: true,
            },
            cpu_psi_some_10: Some(2.0),
            cpu_psi_full_10: None,
            cpu_utilization_pct: Some(15.0),
            io_psi_some_10: Some(1.0),
            io_psi_full_10: None,
            mem_available_bytes: Some(256 * 1024 * 1024),
            mem_total_bytes: Some(4 * 1024 * 1024 * 1024),
            cgroup_memory_current_bytes: None,
            cpu_pressure_reliable: true,
            io_pressure_reliable: true,
            memory_pressure_reliable: true,
            telemetry_age_ms: 10,
            limitations: Vec::new(),
        };

        let out = daemon
            .step_multi_resource::<MockBackend>(&multi, None, None)
            .unwrap();
        assert_eq!(out.last_cpu_pressure, Some(ResourcePressureLevel::Low));
        assert_eq!(out.last_io_pressure, Some(ResourcePressureLevel::Low));
        assert!(out.adaptive_transitions_total >= 1);
    }

    #[test]
    fn test_daemon_config_strict_validation_and_reload() {
        let valid = DaemonConfig::default();
        assert!(valid.validate().is_ok());

        let mut invalid_interval = valid.clone();
        invalid_interval.interval_ms = 5; // < 10 ms
        assert!(matches!(
            invalid_interval.validate(),
            Err(ConfigValidationError::InvalidInterval { .. })
        ));

        let mut invalid_backoff = valid.clone();
        invalid_backoff.interval_ms = 5000;
        invalid_backoff.max_backoff_ms = 1000; // < interval
        assert!(matches!(
            invalid_backoff.validate(),
            Err(ConfigValidationError::InvalidBackoff { .. })
        ));

        let mut invalid_pid = valid.clone();
        invalid_pid.pid_file = Some(PathBuf::from("/etc/xynoos.pid")); // /etc forbidden
        assert!(matches!(
            invalid_pid.validate(),
            Err(ConfigValidationError::InsecurePath { .. })
        ));

        let mut daemon = AuraDaemon::new(valid.clone()).unwrap();
        let mut new_cfg = valid.clone();
        new_cfg.default_protection_bytes = 32 * 1024 * 1024;
        assert!(daemon.reload_config(new_cfg).is_ok());
        assert_eq!(
            daemon.coordinator.default_protection_bytes,
            32 * 1024 * 1024
        );
    }
}
