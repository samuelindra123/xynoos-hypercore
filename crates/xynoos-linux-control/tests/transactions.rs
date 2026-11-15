#![cfg(target_os = "linux")]
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use xynoos_linux_control::{
    transaction::{run, Backend, DisposableCgroup, Identity, Outcome},
    Action, MAX_LOW_BYTES,
};
use xynoos_pressureiq::system::collect_system;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    dir: PathBuf,
    reads: usize,
    writes: usize,
    fail_write: Option<usize>,
    fail_after_write: Option<usize>,
    change_read: Option<(usize, u64)>,
    identities: usize,
    fail_identity: Option<usize>,
    unresolved: bool,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "xynoos-control-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("memory.low"), "4096").unwrap();
        Self {
            dir,
            reads: 0,
            writes: 0,
            fail_write: None,
            fail_after_write: None,
            change_read: None,
            identities: 0,
            fail_identity: None,
            unresolved: false,
        }
    }
    fn value(&self) -> u64 {
        fs::read_to_string(self.dir.join("memory.low"))
            .unwrap()
            .parse()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).unwrap();
    }
}
impl Backend for Fixture {
    fn identity(&mut self) -> Result<Identity, String> {
        self.identities += 1;
        if self.fail_identity == Some(self.identities) {
            return Err("injected identity failure".into());
        }
        let d = fs::metadata(&self.dir).map_err(|e| e.to_string())?;
        let f = fs::metadata(self.dir.join("memory.low")).map_err(|e| e.to_string())?;
        Ok(Identity {
            device: d.dev(),
            inode: d.ino(),
            control_inode: f.ino(),
        })
    }
    fn ready_to_restore(&mut self) -> Result<(), String> {
        if self.unresolved {
            Err("injected unreaped worker".into())
        } else {
            Ok(())
        }
    }
    fn read_low(&mut self) -> Result<u64, String> {
        self.reads += 1;
        if let Some((n, v)) = self.change_read {
            if self.reads == n {
                fs::write(self.dir.join("memory.low"), v.to_string()).unwrap();
            }
        }
        Ok(self.value())
    }
    fn write_low(&mut self, value: u64) -> Result<(), String> {
        self.writes += 1;
        if self.fail_write == Some(self.writes) {
            return Err("injected write failure".into());
        }
        fs::write(self.dir.join("memory.low"), value.to_string()).map_err(|e| e.to_string())?;
        if self.fail_after_write == Some(self.writes) {
            return Err("injected error after mutation".into());
        }
        Ok(())
    }
}

#[test]
fn identity_failure_never_triggers_blind_write_or_rollback() {
    for (failure, outcome, writes, value) in [
        (1, Outcome::Declined, 0, 4096),
        (2, Outcome::Conflict, 0, 4096),
        (3, Outcome::Conflict, 1, 8192),
        (4, Outcome::Conflict, 1, 8192),
        (5, Outcome::RollbackFailed, 2, 4096),
    ] {
        let mut f = Fixture::new();
        f.fail_identity = Some(failure);
        let report = run(&mut f, &collect_system(), action(), true);
        assert_eq!(report.outcome, outcome);
        assert_eq!(f.writes, writes);
        assert_eq!(f.value(), value);
    }
}
fn action() -> Action {
    Action::MemoryLow { bytes: 8192 }
}
#[test]
fn success_applies_and_restores() {
    let mut f = Fixture::new();
    let r = run(&mut f, &collect_system(), action(), true);
    assert_eq!(r.outcome, Outcome::Restored);
    assert_eq!(f.value(), 4096);
    assert_eq!(f.writes, 2);
}
#[test]
fn unauthorized_stale_and_bounds_never_write() {
    let mut f = Fixture::new();
    let mut s = collect_system();
    assert_eq!(run(&mut f, &s, action(), false).outcome, Outcome::Declined);
    s.monotonic -= Duration::from_secs(6);
    assert_eq!(run(&mut f, &s, action(), true).outcome, Outcome::Declined);
    assert_eq!(
        run(
            &mut f,
            &collect_system(),
            Action::MemoryLow {
                bytes: MAX_LOW_BYTES + 4096
            },
            true
        )
        .outcome,
        Outcome::Declined
    );
    assert_eq!(f.writes, 0);
}
#[test]
fn apply_failure_and_verification_mismatch() {
    let mut f = Fixture::new();
    f.fail_write = Some(1);
    let r = run(&mut f, &collect_system(), action(), true);
    assert_eq!(r.outcome, Outcome::ApplyFailed);
    assert_eq!(f.value(), 4096);
    let mut f = Fixture::new();
    f.change_read = Some((3, 4096));
    let r = run(&mut f, &collect_system(), action(), true);
    assert_eq!(r.outcome, Outcome::VerificationFailed);
    assert_eq!(f.value(), 4096);
}
#[test]
fn concurrent_modification_is_not_overwritten() {
    for read in [2, 3, 4] {
        let mut f = Fixture::new();
        f.change_read = Some((read, 12288));
        let r = run(&mut f, &collect_system(), action(), true);
        assert_eq!(r.outcome, Outcome::Conflict);
        assert_eq!(f.value(), 12288);
    }
}
#[test]
fn rollback_failure_is_explicit() {
    let mut f = Fixture::new();
    f.fail_write = Some(2);
    let r = run(&mut f, &collect_system(), action(), true);
    assert_eq!(r.outcome, Outcome::RollbackFailed);
    assert_eq!(f.value(), 8192);
}
#[test]
fn real_backend_refuses_unauthorized_and_non_cgroup_parent() {
    assert!(DisposableCgroup::create(std::path::Path::new("/sys/fs/cgroup/init"), false).is_err());
    let f = Fixture::new();
    assert!(DisposableCgroup::create(&f.dir, true).is_err());
    if let Ok(meta) = fs::metadata("/sys/fs/cgroup/init") {
        // SAFETY: geteuid has no preconditions.
        if meta.uid() != unsafe { libc::geteuid() } {
            assert!(
                DisposableCgroup::create(std::path::Path::new("/sys/fs/cgroup/init"), true)
                    .is_err()
            );
        }
    }
}

#[test]
fn failed_apply_after_mutation_is_restored() {
    let mut f = Fixture::new();
    f.fail_after_write = Some(1);
    let report = run(&mut f, &collect_system(), action(), true);
    assert_eq!(report.outcome, Outcome::ApplyFailed);
    assert_eq!(f.value(), 4096);
    assert_eq!(f.writes, 2);
}

#[test]
fn rollback_verification_failure_is_reported() {
    let mut f = Fixture::new();
    f.change_read = Some((5, 12288));
    let report = run(&mut f, &collect_system(), action(), true);
    assert_eq!(report.outcome, Outcome::RollbackFailed);
    assert_eq!(f.value(), 12288);
}

#[test]
fn operation_runs_only_while_verified_policy_is_active() {
    use xynoos_linux_control::transaction::run_during;
    let mut f = Fixture::new();
    let report = run_during(&mut f, &collect_system(), action(), true, |target| {
        assert_eq!(target.read_low()?, 8192);
        Ok(())
    });
    assert_eq!(report.outcome, Outcome::Restored);
    assert_eq!(f.value(), 4096);
    assert!(report.events.iter().any(|e| e == "operation completed"));
}

#[test]
fn failed_operation_restores_and_reports_failure() {
    use xynoos_linux_control::transaction::run_during;
    let mut f = Fixture::new();
    let report = run_during(&mut f, &collect_system(), action(), true, |_| {
        Err("worker timeout".into())
    });
    assert_eq!(report.outcome, Outcome::OperationFailed);
    assert_eq!(f.value(), 4096);
    assert!(report.events.iter().any(|e| e.contains("worker timeout")));
}

#[test]
fn failed_authorization_and_verification_never_release_worker() {
    use xynoos_linux_control::transaction::run_during;
    for (authorized, fail_write, change_read) in [
        (false, None, None),
        (true, Some(1), None),
        (true, None, Some((3, 4096))),
    ] {
        let mut f = Fixture::new();
        f.fail_write = fail_write;
        f.change_read = change_read;
        let mut called = false;
        let report = run_during(&mut f, &collect_system(), action(), authorized, |_| {
            called = true;
            Ok(())
        });
        assert!(!called);
        assert_ne!(report.outcome, Outcome::Restored);
    }
}

#[test]
fn operation_conflict_is_not_overwritten() {
    use xynoos_linux_control::transaction::run_during;
    let mut f = Fixture::new();
    let report = run_during(&mut f, &collect_system(), action(), true, |target| {
        fs::write(target.dir.join("memory.low"), "12288").map_err(|e| e.to_string())?;
        Ok(())
    });
    assert_eq!(report.outcome, Outcome::Conflict);
    assert_eq!(f.value(), 12288);
    assert_eq!(f.writes, 1);
}

#[test]
fn unreaped_worker_blocks_restoration_and_preserves_control() {
    use xynoos_linux_control::transaction::run_during;
    let mut f = Fixture::new();
    let report = run_during(&mut f, &collect_system(), action(), true, |target| {
        target.unresolved = true;
        Err("injected termination failure".into())
    });
    assert_eq!(report.outcome, Outcome::Conflict);
    assert_eq!(f.value(), 8192);
    assert_eq!(f.writes, 1);
    assert!(report
        .events
        .iter()
        .any(|s| s.contains("restoration blocked")));
}
