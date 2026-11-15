//! Bounded read-only repeatability experiment; JSON lines on stdout.
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use xynoos_pressureiq::{cgroup_pressure, system, workload};
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
// The worker blocks on a pipe before allocating memory or performing work.
fn gated_memory_worker() -> Result<(), Box<dyn std::error::Error>> {
    let mut release = [0u8; 1];
    std::io::stdin().read_exact(&mut release)?;
    if release != [1] {
        return Err("invalid release".into());
    }
    let mut memory = vec![0u8; 32 * 1024 * 1024];
    for _ in 0..128 {
        for byte in memory.iter_mut().step_by(4096) {
            *byte = byte.wrapping_add(1);
        }
        std::hint::black_box(&memory);
    }
    Ok(())
}

fn controlled(parent: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use xynoos_linux_control::{
        transaction::{Backend, DisposableCgroup, Outcome},
        Action,
    };
    let snapshot = system::collect_system();
    let mut group = DisposableCgroup::create(parent, true)?;
    let action = Action::MemoryLow {
        bytes: 32 * 1024 * 1024,
    };
    let mut execution_ns = None;
    let report = xynoos_linux_control::transaction::run_during(
        &mut group,
        &snapshot,
        action,
        true,
        |target| {
            let mut child = Guard(
                Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                    .arg("gated-memory-worker")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .spawn()
                    .map_err(|e| e.to_string())?,
            );
            target.place_child(&child.0)?;
            target.verify_child()?;
            xynoos_linux_control::validate_telemetry(&snapshot, Instant::now(), action)?;
            if target.read_low()? != 32 * 1024 * 1024 {
                return Err("control changed before release".into());
            }
            let started = Instant::now();
            child
                .0
                .stdin
                .take()
                .ok_or("missing release pipe")?
                .write_all(&[1])
                .map_err(|e| e.to_string())?;
            loop {
                if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
                    execution_ns = Some(started.elapsed().as_nanos());
                    if !status.success() {
                        return Err(format!("worker failed: {status}"));
                    }
                    return Ok(());
                }
                if started.elapsed() > Duration::from_secs(10) {
                    return Err("worker timeout".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        },
    );
    let cleanup = if matches!(report.outcome, Outcome::Conflict | Outcome::RollbackFailed) {
        group.preserve();
        Err("target preserved after conflict or rollback failure".to_string())
    } else {
        group.cleanup()
    };
    let success = report.outcome == Outcome::Restored && cleanup.is_ok();
    println!(
        "{}",
        serde_json::json!({"record":"controlled", "execution_ns":execution_ns, "transaction":report, "cleanup":cleanup, "performance_effect":"NOT VERIFIED: no valid comparator"})
    );
    if !success {
        return Err("controlled experiment failed; see transaction and cleanup report".into());
    }
    Ok(())
}

fn validate_mode(mode: &str) -> Result<(), &'static str> {
    match mode {
        "observe" | "dry-run" => Ok(()),
        "controlled" => Err("controlled intervention unavailable: workload membership and transaction lifetime are not safely supported"),
        _ => Err("usage: optimization_experiment [observe|dry-run|controlled]"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 2 && args[1] == "gated-memory-worker" {
        return gated_memory_worker();
    }
    if args.get(1).is_some_and(|x| x == "controlled") {
        if args.len() != 5 || args[2] != "--authorize" || args[3] != "--parent" {
            return Err("usage: controlled --authorize --parent /absolute/delegated/parent".into());
        }
        return controlled(Path::new(&args[4]));
    }
    if args.len() == 3 && args[1] == "worker" {
        return worker(&args[2]);
    }
    let mode = args.get(1).map(String::as_str).unwrap_or("observe");
    if args.len() > 2 {
        return Err("usage: optimization_experiment [observe|dry-run|controlled]".into());
    }
    validate_mode(mode)?;
    println!(
        "{}",
        serde_json::json!({"record":"method", "mode":mode, "comparison":"unsampled versus sampled observation; no intervention", "pairs":3, "duration_ms":1200, "sample_period_ms":200, "memory_cap_bytes":33554432, "io_cap_bytes":8388608, "timeout_ms":5000, "ordering":"alternating AB/BA", "pressure_scope":"shared cgroup and visible system; not attributable to child", "performance_effect":"NOT VERIFIED"})
    );
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
                let mut scoped_engine = xynoos_decision::WorkloadEngine::default();
                let mut decisions = Vec::new();
                let mut collection_ns = 0u128;
                let mut peak_rss = None::<u64>;
                let mut audits = Vec::new();
                let mut cg_path = None;
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
                        let cg = cgroup_pressure::collect(&id);
                        if let Ok(c) = &cg {
                            cg_path = Some(c.cgroup.path.display().to_string());
                        }
                        if let Ok(rss) = p.memory_bytes.result {
                            peak_rss = Some(peak_rss.unwrap_or(0).max(rss));
                        }
                        let scoped = cg.as_ref().map(|c| {
                            scoped_engine.evaluate(
                                c,
                                &xynoos_decision::WorkloadScope::Shared,
                                Instant::now(),
                            )
                        });
                        let s = system::collect_system();
                        let d = engine.evaluate(&s, Instant::now(), None);
                        if mode == "dry-run" {
                            // This is an inspection of the observed shared cgroup, never authorization to write.
                            if let Ok(c) = &cg {
                                audits.push(xynoos_linux_control::prepare(
                                    &s,
                                    Instant::now(),
                                    &c.cgroup.path,
                                    xynoos_linux_control::Action::MemoryLow {
                                        bytes: 32 * 1024 * 1024,
                                    },
                                ));
                            }
                        }
                        collection_ns += t.elapsed().as_nanos();
                        decisions.push(format!(
                            "system={:?}:{:?} cgroup={:?}",
                            d.kind, d.pressure_percent, scoped
                        ));
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
                let delta_json = match delta {
                    Some(Ok(d)) => {
                        serde_json::json!({"cpu_ticks":d.cpu_ticks,"cpu_interval_ns":d.cpu_interval.as_nanos(),"read_bytes":d.read_bytes,"write_bytes":d.write_bytes,"io_interval_ns":d.io_interval.as_nanos()})
                    }
                    Some(Err(e)) => serde_json::json!({"error":format!("{e:?}")}),
                    None => serde_json::Value::Null,
                };
                println!(
                    "{}",
                    serde_json::json!({"record":"run", "mode":mode, "workload":kind, "repeat":repeat, "sampled":sampled, "wall_ns":start.elapsed().as_nanos(), "collection_ns":collection_ns, "process_pid":id.pid, "process_start_ticks":id.start_ticks, "sampled_peak_rss_bytes":peak_rss, "process_delta":delta_json, "shared_cgroup":cg_path, "decisions":decisions, "dry_run_audits":audits, "kernel_write":false, "cleanup":"worker exited; temporary I/O unlinked"})
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_mode;
    #[test]
    fn modes_fail_closed() {
        assert!(validate_mode("observe").is_ok());
        assert!(validate_mode("dry-run").is_ok());
        assert!(validate_mode("controlled").is_err());
        assert!(validate_mode("unknown").is_err());
    }
}
