//! Bounded read-only Linux experiment. Run with cargo run --release -p xynoos-decision --example workload_validation.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use xynoos_pressureiq::{system, workload};
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn worker(kind: &str) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut data = vec![
        0u8;
        if kind == "memory" {
            32 * 1024 * 1024
        } else {
            4096
        }
    ];
    let path = std::env::temp_dir().join(format!("xynoos-017-{}", std::process::id()));
    let mut file = if kind == "io" {
        let f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        fs::remove_file(&path)?;
        Some(f)
    } else {
        None
    };
    let mut n = 1u64;
    let mut written = 0usize;
    while start.elapsed() < Duration::from_millis(1200) {
        match kind {
            "cpu" => {
                for _ in 0..100_000 {
                    n = n.wrapping_mul(1664525).wrapping_add(1013904223);
                }
                std::hint::black_box(n);
            }
            "memory" => {
                for v in data.iter_mut().step_by(4096) {
                    *v = v.wrapping_add(1);
                }
                std::hint::black_box(&data);
            }
            "io" => {
                if written < 8 * 1024 * 1024 {
                    file.as_mut().unwrap().write_all(&data)?;
                    written += data.len();
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            "idle" => std::thread::sleep(Duration::from_millis(10)),
            _ => return Err("unknown workload".into()),
        }
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 3 && args[1] == "worker" {
        return worker(&args[2]);
    }
    for kind in ["idle", "cpu", "memory", "io"] {
        for repeat in 0..3 {
            for sampled in if repeat % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let start = Instant::now();
                let mut child = Guard(
                    Command::new(std::env::current_exe()?)
                        .args(["worker", kind])
                        .stdout(Stdio::null())
                        .spawn()?,
                );
                let id = workload::identify(child.0.id())?;
                let mut first = None;
                let mut last = None;
                let mut engine = xynoos_decision::Engine::default();
                let mut decisions = Vec::new();
                let mut collection_ns = 0u128;
                let mut next_sample = Instant::now();
                loop {
                    if let Some(status) = child.0.try_wait()? {
                        if !status.success() {
                            return Err(format!("worker failed: {status}").into());
                        }
                        break;
                    }
                    if sampled && Instant::now() >= next_sample {
                        next_sample = Instant::now() + Duration::from_millis(200);
                        let t = Instant::now();
                        let p = workload::collect(&id);
                        let s = system::collect_system();
                        let d = engine.evaluate(&s, Instant::now(), None);
                        collection_ns += t.elapsed().as_nanos();
                        decisions.push(format!("{:?}:{:?}", d.kind, d.pressure_percent));
                        if first.is_none() {
                            first = Some(p.clone());
                        }
                        last = Some(p);
                    }
                    if let Some(status) = child.0.try_wait()? {
                        if !status.success() {
                            return Err(format!("worker failed: {status}").into());
                        }
                        break;
                    }
                    if start.elapsed() > Duration::from_secs(5) {
                        return Err("worker timeout".into());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let delta = first
                    .as_ref()
                    .zip(last.as_ref())
                    .map(|(a, b)| workload::delta(a, b));
                println!("kind={kind} repeat={repeat} sampled={sampled} wall_ms={} collection_us={} delta={delta:?} rss={:?} decisions={decisions:?}", start.elapsed().as_millis(), collection_ns/1000, last.as_ref().map(|s| &s.memory_bytes.result));
            }
        }
    }
    Ok(())
}
