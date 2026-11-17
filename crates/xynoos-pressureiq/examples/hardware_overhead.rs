//! Bounded benchmark, never a service. Run under /usr/bin/time for CPU and peak RSS.
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn rss() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|l| {
            l.strip_prefix("VmRSS:")
                .and_then(|v| v.split_whitespace().next()?.parse().ok())
        })
}
fn main() {
    let mut times = Vec::with_capacity(20);
    for _ in 0..20 {
        let start = Instant::now();
        let h = xynoos_pressureiq::hardware::collect(Path::new("/sys"));
        std::hint::black_box(&h);
        times.push(start.elapsed().as_nanos());
        drop(h);
        std::thread::sleep(Duration::from_millis(100));
    }
    println!(
        "{}",
        serde_json::json!({"samples_ns":times,"idle_after_collection_rss_kib":rss(),"sleep_ms":2000,"samples":20})
    );
}
