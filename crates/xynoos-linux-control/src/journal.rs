//! Durable on-disk crash recovery journal for Linux cgroup v2 control transactions.
//!
//! Provides atomic, fail-closed persistence of cgroup intent before kernel mutation,
//! enabling deterministic recovery after ungraceful process termination (e.g. SIGKILL, crash, power loss).

use crate::{
    transaction::{Backend, Identity},
    Action,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static JOURNAL_SEQ: AtomicU64 = AtomicU64::new(1);

/// Lifecycle phases and states of a sustained cgroup control lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeasePhase {
    /// Intent recorded to disk before any kernel control mutation.
    Prepared,
    /// Pending alias for prepared state.
    Pending,
    /// Control value successfully written to cgroup and verified by readback.
    Applied,
    /// Restoration back to baseline original value is actively in progress.
    Restoring,
    /// Original baseline successfully restored and verified, or lease cleanly closed.
    Restored,
    /// RolledBack alias for restored state.
    RolledBack,
    /// External concurrent modification or readback mismatch detected; recovery refused fail-closed.
    Conflict,
    /// Invalidated due to boot ID mismatch, target disappearance, or replacement cgroup inode reuse.
    Invalidated,
}

/// Backward-compatible alias for journal lifecycle status.
pub type JournalStatus = LeasePhase;

/// Structured outcome of a durable recovery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryOutcome {
    NoJournal,
    AlreadyRestored,
    Restored,
    Conflict,
    Invalidated,
    RecoveryFailed,
}

/// Comprehensive report produced by recovery reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub outcome: RecoveryOutcome,
    pub original_value: Option<u64>,
    pub current_value: Option<u64>,
    pub requested_value: Option<u64>,
    pub events: Vec<String>,
}

/// Returns the effective user ID of the calling process in a safe manner.
pub fn current_euid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: geteuid is always safe to call and returns the effective user ID of the calling process.
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn default_schema_version() -> u32 {
    TransactionJournal::CURRENT_SCHEMA_VERSION
}

/// Durable transaction journal record persisted to disk before kernel state mutations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionJournal {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub lease_id: String,
    pub timestamp_epoch_secs: u64,
    #[serde(default)]
    pub boot_id: Option<String>,
    #[serde(default)]
    pub owner_uid: u32,
    #[serde(default)]
    pub process_pid: u32,
    #[serde(default)]
    pub process_start_ticks: Option<u64>,
    #[serde(default)]
    pub authorized_root: Option<String>,
    pub identity: Identity,
    pub action: Action,
    pub original_value: u64,
    pub requested_value: u64,
    #[serde(alias = "phase")]
    pub status: LeasePhase,
    #[serde(default)]
    pub integrity_crc: u32,
}

impl TransactionJournal {
    /// Schema version for current durable journal records.
    pub const CURRENT_SCHEMA_VERSION: u32 = 2;

    pub fn phase(&self) -> LeasePhase {
        self.status
    }
}

impl Default for TransactionJournal {
    fn default() -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            lease_id: String::new(),
            timestamp_epoch_secs: 0,
            boot_id: None,
            owner_uid: 0,
            process_pid: 0,
            process_start_ticks: None,
            authorized_root: None,
            identity: Identity::default(),
            action: Action::default(),
            original_value: 0,
            requested_value: 0,
            status: LeasePhase::Prepared,
            integrity_crc: 0,
        }
    }
}

impl TransactionJournal {
    /// Computes the IEEE CRC32 checksum over the serialized journal with integrity_crc set to 0.
    pub fn compute_crc(&self) -> io::Result<u32> {
        let mut copy = self.clone();
        copy.integrity_crc = 0;
        let bytes =
            serde_json::to_vec(&copy).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(crc32fast::hash(&bytes))
    }

    /// Persists the transaction journal record to disk using an atomic, crash-resilient protocol:
    /// - Checks target is not a symlink.
    /// - Creates parent directory with restricted permissions (0o700).
    /// - Writes to a collision-free temporary file with 0o600 permissions.
    /// - Flushes and syncs temporary file data and metadata (fsync / sync_all).
    /// - Atomically renames temporary file over target path.
    /// - Syncs parent directory to guarantee durable directory entry on disk.
    pub fn persist(&self, path: &Path) -> io::Result<()> {
        if path.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "symlink substitution rejected for journal path",
            ));
        }

        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));

        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }

        let mut stamped = self.clone();
        if stamped.schema_version == 0 {
            stamped.schema_version = Self::CURRENT_SCHEMA_VERSION;
        }
        if stamped.process_pid == 0 {
            stamped.process_pid = std::process::id();
        }
        if stamped.owner_uid == 0 {
            #[cfg(unix)]
            {
                stamped.owner_uid = unsafe { libc::geteuid() };
            }
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
            .unwrap_or("lease_journal.json");

        let ts_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = JOURNAL_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp_path = parent.join(format!(
            ".{file_name}.tmp.{}.{}.{}",
            std::process::id(),
            ts_nanos,
            seq
        ));

        {
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut tmp = opts.open(&tmp_path)?;
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

    /// Loads and verifies the integrity of an on-disk journal file.
    /// Rejects symlinks, verifies CRC32 checksum, and quarantines corrupted records as forensic evidence.
    pub fn load(path: &Path) -> io::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        if path.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "symlink rejected for journal load",
            ));
        }

        let data = fs::read(path)?;
        let journal: Self = match serde_json::from_slice(&data) {
            Ok(j) => j,
            Err(e) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let corrupt_path = path.with_extension(format!("corrupt.{ts}"));
                let _ = fs::copy(path, corrupt_path);
                return Err(io::Error::new(io::ErrorKind::InvalidData, e));
            }
        };

        if journal.integrity_crc != 0 {
            let expected = journal.compute_crc()?;
            if journal.integrity_crc != expected {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let corrupt_path = path.with_extension(format!("corrupt.crc.{ts}"));
                let _ = fs::copy(path, corrupt_path);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal integrity CRC mismatch",
                ));
            }
        }
        Ok(Some(journal))
    }

    /// Safely removes a completed journal file.
    pub fn cleanup(path: &Path) -> io::Result<()> {
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// Reads the system's unique boot identifier from `/proc/sys/kernel/random/boot_id`.
pub fn current_boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|s| s.trim().to_string())
}

/// Reads the process start time in jiffies from `/proc/[pid]/stat`.
pub fn process_start_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let close_paren = stat.rfind(')')?;
    let rest = &stat[close_paren + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// Attempts to parse mount ID from `/proc/self/mountinfo` for cgroup2 filesystem.
pub fn cgroup2_mount_id() -> Option<u64> {
    let mounts = fs::read_to_string("/proc/self/mountinfo").ok()?;
    for line in mounts.lines() {
        if line.contains(" - cgroup2 ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 0 {
                if let Ok(id) = parts[0].parse::<u64>() {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// Reconciles a durable on-disk lease journal against actual cgroup kernel state.
///
/// Implements idempotent, identity-aware, fail-closed recovery:
/// 1. Rejects stale journals from previous system boots (prevents clobbering newly created cgroups).
/// 2. Verifies cgroup identity (device, inode, control inode) to prevent inode recycling clobbering.
/// 3. Detects external administrator modifications and flags Conflict rather than blind overwrite.
/// 4. Restores baseline original value if and only if HyperCore's requested value remains active.
pub fn recover_durable_lease<B: Backend + ?Sized>(
    backend: &mut B,
    journal_path: &Path,
) -> Result<RecoveryReport, String> {
    let mut report = RecoveryReport {
        outcome: RecoveryOutcome::NoJournal,
        original_value: None,
        current_value: None,
        requested_value: None,
        events: Vec::new(),
    };

    let journal = match TransactionJournal::load(journal_path) {
        Ok(Some(j)) => j,
        Ok(None) => return Ok(report),
        Err(e) => {
            report.outcome = RecoveryOutcome::RecoveryFailed;
            report
                .events
                .push(format!("failed to load or verify journal: {e}"));
            return Err(format!("journal load failed: {e}"));
        }
    };

    report.original_value = Some(journal.original_value);
    report.requested_value = Some(journal.requested_value);

    // If journal already recorded clean restoration, clean up and exit.
    if matches!(
        journal.status,
        LeasePhase::Restored | LeasePhase::RolledBack
    ) {
        let _ = TransactionJournal::cleanup(journal_path);
        report.outcome = RecoveryOutcome::AlreadyRestored;
        report.events.push("journal already marked restored".into());
        return Ok(report);
    }

    // 1. Boot Identity Check: prevent restoring previous-boot values onto current boot cgroups.
    if let (Some(j_boot), Some(cur_boot)) = (&journal.boot_id, &current_boot_id()) {
        if j_boot != cur_boot {
            report.outcome = RecoveryOutcome::Invalidated;
            report.events.push(format!(
                "boot ID mismatch (journal: {j_boot}, current: {cur_boot}); previous-boot controls discarded fail-closed"
            ));
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = fs::rename(
                journal_path,
                journal_path.with_extension(format!("invalidated.boot.{ts}")),
            );
            return Ok(report);
        }
    }

    // 2. Target Identity Check: prevent restoring if cgroup was deleted or replaced.
    let current_id = match backend.identity() {
        Ok(id) => id,
        Err(e) => {
            report.outcome = RecoveryOutcome::Invalidated;
            report.events.push(format!(
                "target cgroup missing or identity unreachable ({e}); recovery invalidated"
            ));
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = fs::rename(
                journal_path,
                journal_path.with_extension(format!("invalidated.target.{ts}")),
            );
            return Ok(report);
        }
    };

    if current_id != journal.identity {
        report.outcome = RecoveryOutcome::Invalidated;
        report.events.push(format!(
            "cgroup identity mismatch (journal: {:?}, current: {:?}); target was recreated or replaced",
            journal.identity, current_id
        ));
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = fs::rename(
            journal_path,
            journal_path.with_extension(format!("invalidated.inode.{ts}")),
        );
        return Ok(report);
    }

    // 3. Inspect Current Value
    let current_val = match backend.read_control(journal.action) {
        Ok(val) => val,
        Err(e) => {
            report.outcome = RecoveryOutcome::RecoveryFailed;
            report
                .events
                .push(format!("failed to read control value during recovery: {e}"));
            return Err(format!("control read failed: {e}"));
        }
    };
    report.current_value = Some(current_val);

    // 4. Value Reconcile & Conflict Detection
    if current_val == journal.original_value {
        // Control is already at baseline
        report.outcome = RecoveryOutcome::Restored;
        report
            .events
            .push("control is already at original baseline value".into());
        let _ = TransactionJournal::cleanup(journal_path);
        return Ok(report);
    }

    if current_val != journal.requested_value {
        // Value was changed by an external actor (e.g. administrator or orchestrator)
        report.outcome = RecoveryOutcome::Conflict;
        report.events.push(format!(
            "external conflict detected: current value ({current_val}) matches neither original ({}) nor requested ({}); refusing rollback",
            journal.original_value, journal.requested_value
        ));
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = fs::rename(
            journal_path,
            journal_path.with_extension(format!("conflict.{ts}")),
        );
        return Ok(report);
    }

    // 5. Restore original baseline
    let mut mutating_journal = journal.clone();
    mutating_journal.status = LeasePhase::Restoring;
    let _ = mutating_journal.persist(journal_path);

    if let Err(e) = backend.write_control(journal.action, journal.original_value) {
        report.outcome = RecoveryOutcome::RecoveryFailed;
        report.events.push(format!("rollback write failed: {e}"));
        return Err(format!("rollback write failed: {e}"));
    }

    let verified_val = match backend.read_control(journal.action) {
        Ok(v) => v,
        Err(e) => {
            report.outcome = RecoveryOutcome::RecoveryFailed;
            report
                .events
                .push(format!("rollback verification read failed: {e}"));
            return Err(format!("verification read failed: {e}"));
        }
    };

    if verified_val != journal.original_value {
        report.outcome = RecoveryOutcome::Conflict;
        report.events.push(format!(
            "rollback verification failed: readback {verified_val} != expected original {}",
            journal.original_value
        ));
        return Ok(report);
    }

    report.outcome = RecoveryOutcome::Restored;
    report
        .events
        .push("successfully restored original baseline value and cleaned up journal".into());
    let _ = TransactionJournal::cleanup(journal_path);
    Ok(report)
}
