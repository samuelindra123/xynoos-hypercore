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
        let d = fs::metadata(&self.dir).map_err(|e| e.to_string())?;
        let f = fs::metadata(self.dir.join("memory.low")).map_err(|e| e.to_string())?;
        Ok(Identity {
            device: d.dev(),
            inode: d.ino(),
            control_inode: f.ino(),
        })
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
