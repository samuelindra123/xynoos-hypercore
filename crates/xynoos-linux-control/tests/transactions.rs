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
use xynoos_pressureiq::system::{collect_system as collect_host, SystemSnapshot};
fn collect_system() -> SystemSnapshot {
    let mut snapshot = collect_host();
    snapshot.memory.psi.result = Ok(xynoos_pressureiq::parse_psi(
        "some avg10=0 avg60=0 avg300=0 total=0\nfull avg10=0 avg60=0 avg300=0 total=0",
    )
    .unwrap());
    snapshot.memory.mem.result =
        Ok(xynoos_pressureiq::parse_meminfo("MemTotal: 4096 kB\nMemAvailable: 2048 kB").unwrap());
    snapshot
}
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
        fs::write(dir.join("memory.high"), "16384").unwrap();
        fs::write(dir.join("memory.max"), "65536").unwrap();
        Self {
            dir,
            reads: 0,
            writes: 0,
            fail_write: None,
            fail_after_write: None,
            change_read: None,
            identities: 0,
            fail_identity: None,
        }
    }
    fn value(&self) -> u64 {
        fs::read_to_string(self.dir.join("memory.low"))
            .unwrap()
            .parse()
            .unwrap()
    }
    fn value_high(&self) -> u64 {
        fs::read_to_string(self.dir.join("memory.high"))
            .unwrap()
            .parse()
            .unwrap()
    }
    fn value_max(&self) -> u64 {
        fs::read_to_string(self.dir.join("memory.max"))
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
            mount_id: None,
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
    fn read_high(&mut self) -> Result<u64, String> {
        self.reads += 1;
        Ok(self.value_high())
    }
    fn write_high(&mut self, value: u64) -> Result<(), String> {
        self.writes += 1;
        fs::write(self.dir.join("memory.high"), value.to_string()).map_err(|e| e.to_string())?;
        Ok(())
    }
    fn read_max(&mut self) -> Result<u64, String> {
        self.reads += 1;
        Ok(self.value_max())
    }
    fn write_max(&mut self, value: u64) -> Result<(), String> {
        self.writes += 1;
        fs::write(self.dir.join("memory.max"), value.to_string()).map_err(|e| e.to_string())?;
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
fn memory_high_transaction_lifecycle_and_restoration() {
    let mut f = Fixture::new();
    let action = Action::MemoryHigh { bytes: 32768 };
    let report = run(&mut f, &collect_system(), action, true);
    assert_eq!(report.outcome, Outcome::Restored);
    assert_eq!(f.value_high(), 16384);
    assert_eq!(f.writes, 2);
}

#[test]
fn memory_max_transaction_lifecycle_and_restoration() {
    let mut f = Fixture::new();
    let action = Action::MemoryMax { bytes: 131072 };
    let report = run(&mut f, &collect_system(), action, true);
    assert_eq!(report.outcome, Outcome::Restored);
    assert_eq!(f.value_max(), 65536);
    assert_eq!(f.writes, 2);
}

#[test]
fn memory_high_and_max_bounds_and_alignment_checks() {
    use xynoos_linux_control::{MAX_HIGH_BYTES, MAX_MAX_BYTES};
    let mut f = Fixture::new();
    let sys = collect_system();
    // Non-aligned high
    assert_eq!(
        run(&mut f, &sys, Action::MemoryHigh { bytes: 4095 }, true).outcome,
        Outcome::Declined
    );
    // Over max permitted high
    assert_eq!(
        run(
            &mut f,
            &sys,
            Action::MemoryHigh {
                bytes: MAX_HIGH_BYTES + 4096
            },
            true
        )
        .outcome,
        Outcome::Declined
    );
    // Non-aligned max
    assert_eq!(
        run(&mut f, &sys, Action::MemoryMax { bytes: 4097 }, true).outcome,
        Outcome::Declined
    );
    // Over max permitted max
    assert_eq!(
        run(
            &mut f,
            &sys,
            Action::MemoryMax {
                bytes: MAX_MAX_BYTES + 4096
            },
            true
        )
        .outcome,
        Outcome::Declined
    );
}

#[test]
fn workload_cgroup_rejects_system_infrastructure_targets() {
    use std::path::Path;
    use xynoos_linux_control::transaction::WorkloadCgroup;

    for prohibited in &[
        "/sys/fs/cgroup/system.slice",
        "/sys/fs/cgroup/user.slice",
        "/sys/fs/cgroup/machine.slice",
        "/sys/fs/cgroup/init.scope",
        "/sys/fs/cgroup/-.slice",
        "/sys/fs/cgroup/my-mount.mount",
        "/sys/fs/cgroup/swap-device.swap",
    ] {
        let res = WorkloadCgroup::open(Path::new(prohibited), true);
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(
            err.contains("system infrastructure cgroup targets are strictly prohibited"),
            "Expected prohibition for {}, got: {}",
            prohibited,
            err
        );
    }
}
