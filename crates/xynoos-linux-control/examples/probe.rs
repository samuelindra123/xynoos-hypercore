use std::{path::Path, time::Instant};
use xynoos_linux_control::{discover, execute, prepare, Action};
use xynoos_pressureiq::system::collect_system;
fn main() {
    let s = collect_system();
    let target = Path::new("/sys/fs/cgroup/init");
    let action = Action::MemoryLow { bytes: 0 };
    let caps = discover();
    let audit = prepare(&s, Instant::now(), target, action);
    println!("capabilities={}", serde_json::to_string(&caps).unwrap());
    println!("dry_run={}", serde_json::to_string(&audit).unwrap());
    println!("authorized_execution={:?}", execute(&audit, true));
    let n = 1000u32;
    let start = Instant::now();
    for _ in 0..n {
        std::hint::black_box(prepare(&s, Instant::now(), target, action));
    }
    println!(
        "prepare_mean_ns={} iterations={n}",
        start.elapsed().as_nanos() / u128::from(n)
    );
    let start = Instant::now();
    for _ in 0..n {
        std::hint::black_box(execute(&audit, false));
    }
    println!(
        "decline_mean_ns={} iterations={n}",
        start.elapsed().as_nanos() / u128::from(n)
    );
}
