//! A temporary memory.low transaction. The backend must represent a disposable,
//! isolated target and return a stable identity. No operation is atomic with other writers.
use crate::{validate_telemetry, Action};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::Path,
    time::Instant,
};
use xynoos_pressureiq::system::SystemSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
    pub control_inode: u64,
}

/// Implementations must confine operations to one disposable target.
pub trait Backend {
    fn identity(&mut self) -> Result<Identity, String>;
    fn read_low(&mut self) -> Result<u64, String>;
    fn write_low(&mut self, value: u64) -> Result<(), String>;
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub enum Outcome {
    Restored,
    Declined,
    Conflict,
    ApplyFailed,
    VerificationFailed,
    RollbackFailed,
    OperationFailed,
}
#[derive(Debug, Serialize)]
pub struct TransactionReport {
    pub outcome: Outcome,
    pub original: Option<u64>,
    pub requested: u64,
    pub identity: Option<Identity>,
    pub events: Vec<String>,
}

/// Apply temporarily and restore before returning. A conflict never triggers a blind
/// rollback. Even identity/value checks cannot close the kernel's check/write race.
pub fn run<B: Backend>(
    backend: &mut B,
    snapshot: &SystemSnapshot,
    action: Action,
    authorized: bool,
) -> TransactionReport {
    run_during(backend, snapshot, action, authorized, |_| Ok(()))
}

/// Invoke a bounded operation only after verified apply; always attempt guarded restoration.
/// The callback must stop all workload processes before returning, including on error.
pub fn run_during<B: Backend>(
    backend: &mut B,
    snapshot: &SystemSnapshot,
    action: Action,
    authorized: bool,
    operation: impl FnOnce(&mut B) -> Result<(), String>,
) -> TransactionReport {
    let Action::MemoryLow { bytes } = action;
    let mut report = TransactionReport {
        outcome: Outcome::Declined,
        original: None,
        requested: bytes,
        identity: None,
        events: Vec::new(),
    };
    if !authorized {
        report.events.push("explicit authorization required".into());
        return report;
    }
    if let Err(e) = validate_telemetry(snapshot, Instant::now(), action) {
        report.events.push(e);
        return report;
    }
    let id = match backend.identity() {
        Ok(v) => v,
        Err(e) => {
            report.events.push(e);
            return report;
        }
    };
    report.identity = Some(id);
    let original = match backend.read_low() {
        Ok(v) => v,
        Err(e) => {
            report.events.push(e);
            return report;
        }
    };
    report.original = Some(original);
    if let Err(e) = check(backend, id, original) {
        report.outcome = Outcome::Conflict;
        report.events.push(e);
        return report;
    }
    // Recheck freshness immediately before writing.
    if let Err(e) = validate_telemetry(snapshot, Instant::now(), action) {
        report.events.push(e);
        return report;
    }
    if original == bytes {
        report.outcome = Outcome::Restored;
        report
            .events
            .push("already at requested value; no write; operation declined".into());
        report.outcome = Outcome::Declined;
        return report;
    }
    let applied = backend.write_low(bytes);
    if let Err(e) = &applied {
        report.events.push(format!("apply failed: {e}"));
    }
    let verification = check(backend, id, bytes);
    if let Err(e) = &verification {
        report.events.push(format!("verification: {e}"));
    }
    if verification.is_err() {
        // A failed/partial write may have left the original value. Otherwise it is
        // ambiguous and must not be overwritten as though it were ours.
        if check(backend, id, original).is_ok() {
            report.outcome = if applied.is_err() {
                Outcome::ApplyFailed
            } else {
                Outcome::VerificationFailed
            };
            report.events.push("original state verified".into());
        } else {
            report.outcome = Outcome::Conflict;
            report
                .events
                .push("state ambiguous; rollback skipped".into());
        }
        return report;
    }
    // Never release a workload after a failed write, even if the requested value appeared.
    let mut operation_failed = false;
    if applied.is_ok() {
        if let Err(e) = operation(backend) {
            operation_failed = true;
            report.events.push(format!("operation failed: {e}"));
        } else {
            report.events.push("operation completed".into());
        }
    }
    // Revalidate immediately before restoring; never intentionally overwrite a known change.
    if let Err(e) = check(backend, id, bytes) {
        report.outcome = Outcome::Conflict;
        report.events.push(e);
        return report;
    }
    if let Err(e) = backend.write_low(original) {
        report.outcome = Outcome::RollbackFailed;
        report.events.push(format!("rollback write: {e}"));
        return report;
    }
    if let Err(e) = check(backend, id, original) {
        report.outcome = Outcome::RollbackFailed;
        report.events.push(format!("rollback verification: {e}"));
        return report;
    }
    report.outcome = if applied.is_err() {
        Outcome::ApplyFailed
    } else if operation_failed {
        Outcome::OperationFailed
    } else {
        Outcome::Restored
    };
    report
        .events
        .push("original state verified after restore".into());
    report
}
fn check<B: Backend>(backend: &mut B, id: Identity, value: u64) -> Result<(), String> {
    if backend.identity()? != id {
        return Err("target identity changed".into());
    }
    if backend.read_low()? != value {
        return Err("memory.low changed unexpectedly".into());
    }
    Ok(())
}

/// Owns a freshly created empty cgroup under a caller-specified delegated parent.
/// The caller must ensure no other unprivileged controller uses this subtree.
pub struct DisposableCgroup {
    parent: File,
    dir: File,
    control: File,
    name: std::ffi::CString,
    id: Identity,
    cleanup_on_drop: bool,
    member: Option<xynoos_pressureiq::workload::Identity>,
}
impl DisposableCgroup {
    /// Creates an empty child only after authorization and ownership/delegation checks.
    /// A non-root effective uid is required; no elevation is attempted.
    pub fn create(parent: &Path, authorized: bool) -> Result<Self, String> {
        if !authorized {
            return Err("explicit authorization required".into());
        }
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        if uid == 0 {
            return Err("root execution refused".into());
        }
        let parent = open_absolute_dir(parent)?;
        let meta = parent.metadata().map_err(|e| e.to_string())?;
        if meta.uid() != uid || meta.mode() & 0o022 != 0 {
            return Err(
                "parent must be exclusively owner-writable and owned by effective user".into(),
            );
        }
        let mut st = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: valid file descriptor and writable statfs buffer.
        if unsafe { libc::fstatfs(parent.as_raw_fd(), st.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        // SAFETY: successful fstatfs initialized st.
        if unsafe { st.assume_init() }.f_type as u64 != 0x6367_7270 {
            return Err("parent is not cgroup v2".into());
        }
        // parent is not the visible mount root: '..' must have same filesystem and different identity.
        let up = openat(
            parent.as_raw_fd(),
            c"..",
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let um = up.metadata().map_err(|e| e.to_string())?;
        if um.dev() != meta.dev() || um.ino() == meta.ino() {
            return Err("cgroup mount root refused".into());
        }
        let controllers = read_at(&parent, c"cgroup.subtree_control")?;
        if !controllers.split_whitespace().any(|v| v == "memory") {
            return Err("memory controller is not delegated to children".into());
        }
        let mut random = [0u8; 16];
        File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .map_err(|e| e.to_string())?;
        let name = std::ffi::CString::new(format!(
            "xynoos-test-{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
        .unwrap();
        // SAFETY: valid directory descriptor and NUL-terminated single component.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        let dir = openat(
            parent.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )
        .map_err(|e| {
            format!(
                "open created child {}: {e}; cleanup not attempted because identity is unknown",
                name.to_string_lossy()
            )
        })?;
        let created = dir.metadata().map_err(|e| {
            format!(
                "stat created child {}: {e}; cleanup not attempted because identity is unknown",
                name.to_string_lossy()
            )
        })?;
        let result = (|| {
            let dm = dir.metadata().map_err(|e| e.to_string())?;
            if dm.uid() != uid || dm.dev() != meta.dev() || dm.mode() & 0o022 != 0 {
                return Err("new cgroup ownership mismatch".into());
            }
            if !read_at(&dir, c"cgroup.procs")?.trim().is_empty() {
                return Err("new cgroup is not empty".into());
            }
            let control = openat(dir.as_raw_fd(), c"memory.low", libc::O_RDWR)?;
            let cm = control.metadata().map_err(|e| e.to_string())?;
            if cm.dev() != dm.dev() || cm.uid() != uid {
                return Err("control ownership or filesystem mismatch".into());
            }
            let id = Identity {
                device: dm.dev(),
                inode: dm.ino(),
                control_inode: cm.ino(),
            };
            Ok(Self {
                parent: parent.try_clone().map_err(|e| e.to_string())?,
                dir,
                control,
                name: name.clone(),
                id,
                cleanup_on_drop: true,
                member: None,
            })
        })();
        if let Err(error) = result {
            let same = openat(
                parent.as_raw_fd(),
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY,
            )
            .and_then(|f| f.metadata().map_err(|e| e.to_string()))
            .is_ok_and(|m| m.dev() == created.dev() && m.ino() == created.ino());
            if !same {
                return Err(format!(
                    "{error}; cleanup of {} skipped: identity changed or unavailable",
                    name.to_string_lossy()
                ));
            }
            // SAFETY: identity was checked; removal still races another writer.
            if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0
            {
                return Err(format!(
                    "{error}; cleanup of {} failed: {}",
                    name.to_string_lossy(),
                    io::Error::last_os_error()
                ));
            }
            return Err(error);
        }
        result
    }
}
impl DisposableCgroup {
    /// Place a child owned by this process. The caller must keep it gated until
    /// `verify_child` succeeds and must kill and reap it on every error path.
    pub fn place_child(&mut self, child: &std::process::Child) -> Result<(), String> {
        if self.member.is_some() {
            return Err("target already used".into());
        }
        self.identity()?;
        let id = xynoos_pressureiq::workload::identify(child.id())
            .map_err(|e| format!("child identity: {e:?}"))?;
        let control = openat(self.dir.as_raw_fd(), c"cgroup.procs", libc::O_WRONLY)?;
        use std::os::unix::fs::FileExt;
        let text = id.pid.to_string();
        let written = control
            .write_at(text.as_bytes(), 0)
            .map_err(|e| format!("placement: {e}"))?;
        if written != text.len() {
            return Err("short placement write".into());
        }
        self.member = Some(id);
        self.verify_child()
    }

    /// Verify exact single-process membership, process start time and cgroup identity.
    pub fn verify_child(&mut self) -> Result<(), String> {
        let id = self.member.clone().ok_or("no child placed")?;
        if xynoos_pressureiq::workload::identify(id.pid)
            .map_err(|e| format!("child identity: {e:?}"))?
            != id
        {
            return Err("child identity changed".into());
        }
        let snapshot = xynoos_pressureiq::cgroup_pressure::collect(&id)
            .map_err(|e| format!("membership: {e:?}"))?;
        if snapshot.cgroup.device != self.id.device || snapshot.cgroup.inode != self.id.inode {
            return Err("child cgroup identity mismatch".into());
        }
        self.identity()?;
        if read_at(&self.dir, c"cgroup.procs")?.trim() != id.pid.to_string() {
            return Err("target does not contain exactly the child".into());
        }
        Ok(())
    }

    /// Preserve the target for operator inspection after ambiguous state or rollback failure.
    pub fn preserve(mut self) {
        self.cleanup_on_drop = false;
    }

    /// Remove the empty disposable target explicitly and report cleanup errors.
    pub fn cleanup(mut self) -> Result<(), String> {
        // A failed explicit cleanup must not be followed by an unreported retry in Drop.
        self.cleanup_on_drop = false;
        self.identity()?;
        // SAFETY: valid descriptor and name; kernel rejects populated groups.
        if unsafe {
            libc::unlinkat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::AT_REMOVEDIR,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}
impl Backend for DisposableCgroup {
    fn identity(&mut self) -> Result<Identity, String> {
        let current = openat(
            self.parent.as_raw_fd(),
            &self.name,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let dm = current.metadata().map_err(|e| e.to_string())?;
        let control = openat(current.as_raw_fd(), c"memory.low", libc::O_RDONLY)?;
        let cm = control.metadata().map_err(|e| e.to_string())?;
        let members = read_at(&current, c"cgroup.procs")?;
        let members = members.trim();
        let allowed = match &self.member {
            None => members.is_empty(),
            Some(id) if members == id.pid.to_string() => {
                xynoos_pressureiq::workload::identify(id.pid).is_ok_and(|current| current == *id)
            }
            Some(id) if members.is_empty() => {
                matches!(
                    xynoos_pressureiq::workload::identify(id.pid),
                    Err(xynoos_pressureiq::Error::Unsupported)
                )
            }
            Some(_) => false,
        };
        if !allowed
            || !read_at(&current, c"cgroup.stat")?
                .lines()
                .any(|l| l.trim() == "nr_descendants 0")
        {
            return Err("unexpected cgroup membership or descendants".into());
        }
        let id = Identity {
            device: dm.dev(),
            inode: dm.ino(),
            control_inode: cm.ino(),
        };
        if id != self.id {
            return Err("cgroup identity changed".into());
        }
        Ok(id)
    }
    fn read_low(&mut self) -> Result<u64, String> {
        read_at(&self.dir, c"memory.low")?
            .trim()
            .parse()
            .map_err(|_| "invalid memory.low".into())
    }
    fn write_low(&mut self, value: u64) -> Result<(), String> {
        use std::os::unix::fs::FileExt;
        let s = value.to_string();
        let n = self
            .control
            .write_at(s.as_bytes(), 0)
            .map_err(|e| e.to_string())?;
        if n != s.len() {
            return Err("short control write".into());
        }
        Ok(())
    }
}
impl Drop for DisposableCgroup {
    fn drop(&mut self) {
        if !self.cleanup_on_drop {
            return;
        }
        // Do not deliberately remove a replacement target. The final check/removal is not atomic.
        if self.identity().is_err() {
            return;
        }
        // SAFETY: removal fails for a populated cgroup.
        unsafe {
            libc::unlinkat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::AT_REMOVEDIR,
            );
        }
    }
}
fn openat(fd: i32, name: &std::ffi::CStr, flags: i32) -> Result<File, String> {
    // SAFETY: valid C string; O_NOFOLLOW prevents final symlink traversal.
    let raw = unsafe {
        libc::openat(
            fd,
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if raw < 0 {
        Err(io::Error::last_os_error().to_string())
    } else {
        // SAFETY: openat returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(raw) })
    }
}
fn open_absolute_dir(path: &Path) -> Result<File, String> {
    use std::os::unix::ffi::OsStrExt;
    if !path.is_absolute() {
        return Err("parent must be absolute".into());
    }
    let mut dir = openat(libc::AT_FDCWD, c"/", libc::O_RDONLY | libc::O_DIRECTORY)?;
    for component in path.components().skip(1) {
        let std::path::Component::Normal(name) = component else {
            return Err("unsafe parent path".into());
        };
        let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| "invalid parent path")?;
        dir = openat(dir.as_raw_fd(), &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    }
    Ok(dir)
}
fn read_at(dir: &File, name: &std::ffi::CStr) -> Result<String, String> {
    let f = openat(dir.as_raw_fd(), name, libc::O_RDONLY)?;
    let mut s = String::new();
    f.take(4097)
        .read_to_string(&mut s)
        .map_err(|e| e.to_string())?;
    if s.len() > 4096 {
        return Err("control read too large".into());
    }
    Ok(s)
}
