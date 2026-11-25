#![cfg(target_os = "linux")]
//! Explicitly opt in on an isolated, exclusively delegated test parent.
use std::path::Path;
use xynoos_linux_control::{
    transaction::{
        run, Backend, DisposableCgroup, Identity, Outcome, SustainedLease, WorkloadCgroup,
    },
    Action,
};

struct Observed(DisposableCgroup, Vec<(u64, Result<u64, String>)>);
impl Backend for Observed {
    fn identity(&mut self) -> Result<Identity, String> {
        self.0.identity()
    }
    fn read_low(&mut self) -> Result<u64, String> {
        self.0.read_low()
    }
    fn write_low(&mut self, value: u64) -> Result<(), String> {
        self.0.write_low(value)?;
        let observed = self.0.read_low();
        self.1.push((value, observed));
        Ok(())
    }
}

#[test]
#[ignore = "requires an isolated delegated cgroup v2 parent and explicit opt-in"]
fn kernel_apply_restore_and_cleanup() {
    assert_eq!(
        std::env::var("XYNOOS_KERNEL_TEST").as_deref(),
        Ok("1"),
        "explicit opt-in required"
    );
    let parent = std::env::var("XYNOOS_CGROUP_PARENT")
        .expect("set XYNOOS_CGROUP_PARENT to an isolated delegated parent");
    let mut target = Observed(
        DisposableCgroup::create(Path::new(&parent), true).expect("create disposable child"),
        Vec::new(),
    );
    let original = target.read_low().expect("read original memory.low");
    let requested = if original == 4096 { 8192 } else { 4096 };
    let report = run(
        &mut target,
        &xynoos_pressureiq::system::collect_system(),
        Action::MemoryLow { bytes: requested },
        true,
    );
    let final_value = target.read_low();
    let observations = std::mem::take(&mut target.1);
    let cleanup = target.0.cleanup();
    assert!(
        cleanup.is_ok(),
        "cleanup failed: {cleanup:?}; report: {report:?}"
    );
    assert_eq!(report.outcome, Outcome::Restored, "{report:?}");
    assert_eq!(observations.len(), 2, "{observations:?}");
    assert_eq!(observations[0], (requested, Ok(requested)));
    assert_eq!(observations[1], (original, Ok(original)));
    assert_eq!(final_value, Ok(original));
}

#[test]
#[ignore = "requires an isolated delegated cgroup v2 parent and explicit opt-in"]
fn kernel_multi_action_high_max_unauthorized_and_cleanup() {
    assert_eq!(
        std::env::var("XYNOOS_KERNEL_TEST").as_deref(),
        Ok("1"),
        "explicit opt-in required"
    );
    let parent = std::env::var("XYNOOS_CGROUP_PARENT")
        .expect("set XYNOOS_CGROUP_PARENT to an isolated delegated parent");
    let mut target =
        DisposableCgroup::create(Path::new(&parent), true).expect("create disposable child");
    let sys = xynoos_pressureiq::system::collect_system();

    // Unauthorized must decline without writing
    let decl = run(
        &mut target,
        &sys,
        Action::MemoryHigh { bytes: 8_388_608 },
        false,
    );
    assert_eq!(decl.outcome, Outcome::Declined);

    // MemoryHigh apply & restore
    let orig_high = target.read_high().expect("read original memory.high");
    let rep_high = run(
        &mut target,
        &sys,
        Action::MemoryHigh { bytes: 8_388_608 },
        true,
    );
    assert_eq!(rep_high.outcome, Outcome::Restored, "{rep_high:?}");
    assert_eq!(target.read_high().unwrap(), orig_high);

    // MemoryMax apply & restore
    let orig_max = target.read_max().expect("read original memory.max");
    let rep_max = run(
        &mut target,
        &sys,
        Action::MemoryMax { bytes: 16_777_216 },
        true,
    );
    assert_eq!(rep_max.outcome, Outcome::Restored, "{rep_max:?}");
    assert_eq!(target.read_max().unwrap(), orig_max);

    target.cleanup().expect("cleanup disposable cgroup");
}

#[test]
#[ignore = "requires an isolated delegated cgroup v2 parent and explicit opt-in"]
fn kernel_real_cgroup_conflict_and_restoration() {
    assert_eq!(
        std::env::var("XYNOOS_KERNEL_TEST").as_deref(),
        Ok("1"),
        "explicit opt-in required"
    );
    let parent = std::env::var("XYNOOS_CGROUP_PARENT")
        .expect("set XYNOOS_CGROUP_PARENT to an isolated delegated parent");
    let mut target =
        DisposableCgroup::create(Path::new(&parent), true).expect("create disposable child");

    let original_low = target.read_low().expect("read original memory.low");
    // Simulate an interrupted write leaving memory.low at 8192 instead of original_low
    let modified = if original_low == 8192 { 16384 } else { 8192 };
    target
        .write_low(modified)
        .expect("write modified memory.low");
    assert_eq!(target.read_low().unwrap(), modified);

    // Restore back to original_low and verify identity + readback
    let _id = target.identity().expect("verify cgroup identity");
    target.write_low(original_low).expect("restore memory.low");
    assert_eq!(target.read_low().unwrap(), original_low);

    target.cleanup().expect("cleanup disposable cgroup");
}

#[test]
#[ignore = "requires an isolated delegated cgroup v2 parent and explicit opt-in"]
fn kernel_sustained_workload_lease_and_recreation_fail_closed() {
    use std::io::Write;
    assert_eq!(
        std::env::var("XYNOOS_KERNEL_TEST").as_deref(),
        Ok("1"),
        "explicit opt-in required"
    );
    let parent = std::env::var("XYNOOS_CGROUP_PARENT")
        .expect("set XYNOOS_CGROUP_PARENT to an isolated delegated parent");
    let wl_dir = Path::new(&parent).join("xynoos-workload-lease-test");
    let _ = std::fs::remove_dir(&wl_dir);
    std::fs::create_dir(&wl_dir).expect("create workload cgroup");

    // Spawn a bounded child process inside wl_dir
    let mut child = std::process::Command::new("sleep")
        .arg("2")
        .spawn()
        .expect("spawn sleep child");
    let mut procs = std::fs::OpenOptions::new()
        .write(true)
        .open(wl_dir.join("cgroup.procs"))
        .expect("open cgroup.procs");
    writeln!(procs, "{}", child.id()).expect("move child into workload cgroup");

    let mut target = WorkloadCgroup::open(&wl_dir, true).expect("open WorkloadCgroup");
    let sys = xynoos_pressureiq::system::collect_system();
    let orig_low = target.read_low().unwrap();
    let req_low = if orig_low == 4_194_304 {
        8_388_608
    } else {
        4_194_304
    };
    let mut lease = SustainedLease::acquire(
        &mut target,
        &sys,
        Action::MemoryLow { bytes: req_low },
        true,
    )
    .expect("acquire sustained memory.low lease");
    assert_eq!(target.read_low().unwrap(), req_low);
    assert!(target.read_current_bytes().is_ok());

    let release_report = lease.release(&mut target);
    assert_eq!(release_report.outcome, Outcome::Restored);
    assert_eq!(target.read_low().unwrap(), orig_low);

    let _ = child.wait();
    std::fs::remove_dir(&wl_dir).expect("remove empty workload cgroup");

    // Verify cgroup disappearance & recreation triggers identity mismatch fail-closed
    std::fs::create_dir(&wl_dir).expect("create first cgroup");
    let mut first = WorkloadCgroup::open(&wl_dir, true).expect("open first");
    std::fs::remove_dir(&wl_dir).expect("remove first");
    std::fs::create_dir(&wl_dir).expect("recreate cgroup at same path");
    assert!(
        first.identity().is_err(),
        "recreated cgroup at same path must fail identity verification"
    );
    std::fs::remove_dir(&wl_dir).expect("cleanup recreated cgroup");
}
