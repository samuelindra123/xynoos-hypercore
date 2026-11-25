//! Conservative Linux cgroup v2 inspection and policy preparation.
//!
//! Transactions are bounded and restore best-effort; cgroupfs has no atomic compare-and-swap.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
    time::{Duration, Instant},
};
use xynoos_pressureiq::system::{
    contract::{self, Status},
    SystemSnapshot,
};

pub const MAX_LOW_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_HIGH_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_MAX_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_AGE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    MemoryLow { bytes: u64 },
    MemoryHigh { bytes: u64 },
    MemoryMax { bytes: u64 },
}

impl Default for Action {
    fn default() -> Self {
        Self::MemoryLow { bytes: 0 }
    }
}

impl Action {
    pub const fn control_filename(&self) -> &'static str {
        match self {
            Action::MemoryLow { .. } => "memory.low",
            Action::MemoryHigh { .. } => "memory.high",
            Action::MemoryMax { .. } => "memory.max",
        }
    }

    pub const fn bytes(&self) -> u64 {
        match *self {
            Action::MemoryLow { bytes }
            | Action::MemoryHigh { bytes }
            | Action::MemoryMax { bytes } => bytes,
        }
    }

    pub const fn max_permitted_bytes(&self) -> u64 {
        match self {
            Action::MemoryLow { .. } => MAX_LOW_BYTES,
            Action::MemoryHigh { .. } => MAX_HIGH_BYTES,
            Action::MemoryMax { .. } => MAX_MAX_BYTES,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub cgroup2: bool,
    pub mount: String,
    pub controllers: Option<String>,
    pub subtree_control: Option<String>,
    pub mount_writable: bool,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Decision {
    Inspected,
    Declined(String),
}
#[derive(Debug, Serialize)]
pub struct Audit {
    pub action: Action,
    pub target: String,
    pub decision: Decision,
    pub original_bytes: Option<u64>,
    pub device: Option<u64>,
    pub inode: Option<u64>,
    pub kernel_write_available: bool,
    /// Advisory effective-credential permission probe, not proof of delegation.
    pub control_writable: Option<bool>,
}

/// Inspect only the visible mount namespace. No host-wide scope is implied.
pub fn discover() -> Capabilities {
    let mount = Path::new("/sys/fs/cgroup");
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    let path = c"/sys/fs/cgroup";
    // SAFETY: path is NUL terminated and stat points to valid writable storage.
    let ok = unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) } == 0;
    // SAFETY: statfs initialized the structure when it returned success.
    let cgroup2 = ok && unsafe { stat.assume_init() }.f_type as u64 == 0x6367_7270;
    let mounts = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let line = mounts.lines().find(|line| {
        line.split(" - ")
            .next()
            .is_some_and(|pre| pre.split_whitespace().nth(4) == Some("/sys/fs/cgroup"))
            && line.contains(" - cgroup2 ")
    });
    let writable = line
        .and_then(|l| l.split_whitespace().nth(5))
        .is_some_and(|s| s.split(',').any(|x| x == "rw"));
    Capabilities {
        cgroup2,
        mount: mount.display().to_string(),
        controllers: fs::read_to_string(mount.join("cgroup.controllers"))
            .ok()
            .map(|s| s.trim().into()),
        subtree_control: fs::read_to_string(mount.join("cgroup.subtree_control"))
            .ok()
            .map(|s| s.trim().into()),
        mount_writable: writable,
        detail: if line.is_some() {
            "visible cgroup2 mount; delegation is target-specific"
        } else {
            "mount identity unavailable"
        }
        .into(),
    }
}

fn validate_telemetry(
    snapshot: &SystemSnapshot,
    now: Instant,
    action: Action,
) -> Result<(), String> {
    let bytes = action.bytes();
    let max = action.max_permitted_bytes();
    let name = action.control_filename();
    if bytes > max || bytes % 4096 != 0 {
        return Err(format!("{name} outside bounds or not 4096-byte aligned"));
    }
    let age = now
        .checked_duration_since(snapshot.monotonic)
        .ok_or("monotonic clock moved backwards")?;
    if age > MAX_AGE {
        return Err("telemetry stale".into());
    }
    let contract = contract::build(snapshot, None, None, now, MAX_AGE);
    if contract.schema_version != 1 || !contract.sequential {
        return Err("unsupported telemetry contract".into());
    }
    for key in ["memory_psi", "meminfo"] {
        let r = contract
            .readings
            .get(key)
            .ok_or("missing required telemetry")?;
        if r.status != Status::Available
            || r.error.is_some()
            || r.value.is_none()
            || r.age_ns > MAX_AGE.as_nanos()
            || r.scope != "visible_system"
        {
            return Err(format!("unusable telemetry: {key}"));
        }
    }
    Ok(())
}

pub mod journal;
pub mod transaction;

pub use journal::{
    current_euid, recover_durable_lease, LeasePhase, RecoveryOutcome, RecoveryReport,
    TransactionJournal,
};

/// Builds the v1 contract at decision time, so its age cannot be frozen by a caller.
/// Only memory PSI and meminfo are used; their scope is visible-system, not target-specific.
pub fn prepare(snapshot: &SystemSnapshot, now: Instant, target: &Path, action: Action) -> Audit {
    let mut audit = Audit {
        action,
        target: target.display().to_string(),
        decision: Decision::Declined("not validated".into()),
        original_bytes: None,
        device: None,
        inode: None,
        kernel_write_available: false,
        control_writable: None,
    };
    let result = (|| -> Result<(), String> {
        validate_telemetry(snapshot, now, action)?;

        let caps = discover();
        if !caps.cgroup2 {
            return Err("cgroup v2 mount unavailable".into());
        }
        let root = Path::new("/sys/fs/cgroup");
        let relative = target
            .strip_prefix(root)
            .map_err(|_| "target outside visible cgroup mount")?;
        if relative.components().count() != 1
            || !matches!(relative.components().next(), Some(Component::Normal(_)))
        {
            return Err("target must be a direct, non-root child of the visible mount".into());
        }
        let meta = fs::symlink_metadata(target).map_err(|e| format!("target: {e}"))?;
        if !meta.is_dir()
            || meta.file_type().is_symlink()
            || meta.dev() != fs::metadata(root).map_err(|e| e.to_string())?.dev()
        {
            return Err("invalid target identity".into());
        }
        let filename = action.control_filename();
        let file = target.join(filename);
        let fm = fs::symlink_metadata(&file).map_err(|e| format!("{filename}: {e}"))?;
        if !fm.is_file() || fm.file_type().is_symlink() || fm.dev() != meta.dev() {
            return Err("invalid control identity".into());
        }
        use std::os::unix::ffi::OsStrExt;
        let name =
            std::ffi::CString::new(file.as_os_str().as_bytes()).map_err(|_| "invalid path")?;
        // SAFETY: name is NUL terminated; faccessat only checks effective access.
        audit.control_writable = Some(
            unsafe { libc::faccessat(libc::AT_FDCWD, name.as_ptr(), libc::W_OK, libc::AT_EACCESS) }
                == 0,
        );
        let content = fs::read_to_string(&file).map_err(|e| format!("read {filename}: {e}"))?;
        let trimmed = content.trim();
        let value = if trimmed == "max" {
            u64::MAX
        } else {
            trimmed
                .parse::<u64>()
                .map_err(|_| format!("invalid {filename}"))?
        };
        audit.original_bytes = Some(value);
        audit.device = Some(meta.dev());
        audit.inode = Some(meta.ino());
        Ok(())
    })();
    audit.decision = match result {
        Ok(()) => Decision::Inspected,
        Err(e) => Decision::Declined(e),
    };
    audit
}

/// Kernel writes are explicitly unavailable, including with caller authorization.
/// This prevents silently treating path ownership as exclusive delegation.
pub fn execute(_prepared: &Audit, authorized: bool) -> Decision {
    if !authorized {
        Decision::Declined("explicit authorization required".into())
    } else {
        Decision::Declined(
            "kernel execution disabled: exclusive control and rollback cannot be guaranteed".into(),
        )
    }
}

pub mod config;
pub mod planning;
