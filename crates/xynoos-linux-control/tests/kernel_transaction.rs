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
