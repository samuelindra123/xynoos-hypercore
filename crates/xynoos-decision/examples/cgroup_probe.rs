//! Read-only self-cgroup probe; no isolation is asserted.
use std::{
    thread,
    time::{Duration, Instant},
};
use xynoos_pressureiq::{cgroup_pressure, workload};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = workload::identify(std::process::id())?;
    let mut engine = xynoos_decision::WorkloadEngine::default();
    let mut previous = None;
    for _ in 0..2 {
        let start = Instant::now();
        let s = cgroup_pressure::collect(&id)?;
        let d = engine.evaluate(&s, &xynoos_decision::WorkloadScope::Shared, Instant::now());
        println!("identity={:?} cpu={:?} memory={:?} io={:?} isolation={:?} decision={:?} collection_us={}", s.cgroup, s.cpu.result, s.memory.result, s.io.result, s.isolation_check.result, d, start.elapsed().as_micros());
        if let Some(a) = previous.as_ref() {
            for name in ["cpu", "memory", "io"] {
                println!("{name} delta={:?}", cgroup_pressure::delta(a, &s, name));
            }
        }
        previous = Some(s);
        thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}
