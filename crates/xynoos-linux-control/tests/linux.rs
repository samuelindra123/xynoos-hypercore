#![cfg(target_os = "linux")]
use std::{
    path::Path,
    time::{Duration, Instant},
};
use xynoos_linux_control::{discover, execute, prepare, Action, Decision, MAX_LOW_BYTES};
use xynoos_pressureiq::system::collect_system;

#[test]
fn linux_discovery_and_read_only_decisions() {
    let caps = discover();
    if caps.cgroup2 {
        assert!(caps.controllers.is_some());
        let s = collect_system();
        let invalid = prepare(
            &s,
            Instant::now(),
            Path::new("/sys/fs/cgroup"),
            Action::MemoryLow { bytes: 0 },
        );
        assert!(matches!(invalid.decision, Decision::Declined(_)));
        assert!(matches!(execute(&invalid, false), Decision::Declined(_)));
        assert!(matches!(execute(&invalid, true), Decision::Declined(_)));
    }
}

#[test]
fn stale_bounds_and_unsafe_targets_are_declined() {
    let s = collect_system();
    for (now, target, bytes) in [
        (
            Instant::now() + Duration::from_secs(6),
            "/sys/fs/cgroup/init",
            0,
        ),
        (Instant::now(), "/sys/fs/cgroup/init", MAX_LOW_BYTES + 4096),
        (Instant::now(), "/sys/fs/cgroup/init", 1),
        (Instant::now(), "/sys/fs/cgroup/../init", 0),
        (Instant::now(), "/tmp", 0),
    ] {
        assert!(matches!(
            prepare(&s, now, Path::new(target), Action::MemoryLow { bytes }).decision,
            Decision::Declined(_)
        ));
    }
}

#[test]
fn telemetry_errors_are_not_accepted() {
    let mut s = collect_system();
    // Isolate meminfo from hosts where PSI is unavailable or restricted.
    s.memory.psi.result = Ok(xynoos_pressureiq::parse_psi(
        "some avg10=0 avg60=0 avg300=0 total=0\nfull avg10=0 avg60=0 avg300=0 total=0",
    )
    .unwrap());
    s.memory.mem.result = Err(xynoos_pressureiq::Error::Permission);
    let audit = prepare(
        &s,
        Instant::now(),
        Path::new("/sys/fs/cgroup/init"),
        Action::MemoryLow { bytes: 0 },
    );
    assert!(matches!(audit.decision, Decision::Declined(ref reason) if reason.contains("meminfo")));
}
