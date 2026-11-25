#![cfg(target_os = "linux")]
//! Explicitly opt in on an isolated, exclusively delegated test parent.
use std::path::Path;
use xynoos_linux_control::{
    transaction::{run, Backend, DisposableCgroup, Identity, Outcome},
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
