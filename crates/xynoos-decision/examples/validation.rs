use std::time::{Duration, Instant};
use xynoos_decision::Engine;
use xynoos_pressureiq::{system::collect_system, Error};
fn main() {
    let s = collect_system();
    let mut engine = Engine::default();
    println!(
        "stale: {}",
        serde_json::to_string(&engine.evaluate(&s, s.monotonic + Duration::from_secs(6), None))
            .unwrap()
    );
    let mut partial = collect_system();
    partial.io_psi = Err(Error::Permission);
    println!(
        "injected partial: {}",
        serde_json::to_string(&engine.evaluate(&partial, Instant::now(), None)).unwrap()
    );
    let mut missing = collect_system();
    missing.memory.mem.result = Err(Error::Missing("injected meminfo".into()));
    println!(
        "injected missing: {}",
        serde_json::to_string(&engine.evaluate(&missing, Instant::now(), None)).unwrap()
    );
    if let Ok(cg) = &s.cgroup {
        let audit = xynoos_linux_control::prepare(
            &s,
            Instant::now(),
            &cg.path,
            xynoos_linux_control::Action::MemoryLow { bytes: 4096 },
        );
        println!(
            "real adapter inspection: {}",
            serde_json::to_string(&audit).unwrap()
        );
    }
    let start = Instant::now();
    for _ in 0..1000 {
        let s = collect_system();
        let target = s.cgroup.as_ref().ok().map(|c| c.path.as_path());
        std::hint::black_box(engine.evaluate(&s, Instant::now(), target));
    }
    println!(
        "complete collect + contract + decision, 1000 iterations: mean {} ns",
        start.elapsed().as_nanos() / 1000
    );
}
