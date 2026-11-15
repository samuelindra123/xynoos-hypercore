//! Bounded read-only repeatability experiment; JSON lines on stdout.
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
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
static CANCELLED: AtomicBool = AtomicBool::new(false);

extern "C" fn cancel_handler(_: libc::c_int) {
    CANCELLED.store(true, Ordering::Relaxed);
}

fn install_cancellation() -> Result<(), String> {
    // SAFETY: sigaction is initialized before use and the handler only stores an atomic flag.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = cancel_handler as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        for signal in [libc::SIGINT, libc::SIGTERM] {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(format!(
                    "install signal handler: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
    }
    Ok(())
}

fn cancellation_check() -> Result<(), String> {
    if CANCELLED.load(Ordering::Relaxed) {
        Err("cancelled".into())
    } else {
        Ok(())
    }
}

// On failure return after a bounded interval; the caller must preserve the target.
fn stop_and_reap(child: &mut Child) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|e| format!("child status: {e}"))?
        .is_some()
    {
        return Ok(());
    }
    let kill_error = child.kill().err();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Err(e) => {
                return Err(format!(
                    "cannot verify child reaping: {e}; kill error: {kill_error:?}"
                ))
            }
            Ok(None) if Instant::now() >= deadline => {
                return Err(format!(
                    "child termination not verified; kill error: {kill_error:?}"
                ))
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

// The worker blocks on a pipe before allocating memory or performing work.
fn gated_memory_worker(passes: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut release = [0u8; 1];
    std::io::stdin().read_exact(&mut release)?;
    if release != [1] {
        return Err("invalid release".into());
    }
    let mut memory = vec![0u8; 32 * 1024 * 1024];
    for _ in 0..passes {
        for byte in memory.iter_mut().step_by(4096) {
            *byte = byte.wrapping_add(1);
        }
        std::hint::black_box(&memory);
    }
    Ok(())
}

// Observation only: two identical bounded processes; no policy is applied.
fn memory_repeatability() -> Result<(), Box<dyn std::error::Error>> {
    for repeat in 0..3 {
        for sampled in if repeat % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let mut competitor = Guard(
                Command::new(std::env::current_exe()?)
                    .arg("gated-memory-competitor")
                    .stdin(Stdio::piped())
                    .spawn()?,
            );
            let mut worker = Guard(
                Command::new(std::env::current_exe()?)
                    .arg("gated-memory-worker")
                    .stdin(Stdio::piped())
                    .spawn()?,
            );
            let mut sample_ns = 0;
            let mut pressure_before = None;
            if sampled {
                let t = Instant::now();
                pressure_before = system::collect_system()
                    .memory
                    .psi
                    .result
                    .ok()
                    .map(|p| p.some.total_us);
                sample_ns += t.elapsed().as_nanos();
            }
            competitor
                .0
                .stdin
                .take()
                .ok_or("competitor pipe")?
                .write_all(&[1])?;
            let start = Instant::now();
            worker
                .0
                .stdin
                .take()
                .ok_or("worker pipe")?
                .write_all(&[1])?;
            let status = loop {
                if let Some(status) = worker.0.try_wait()? {
                    break status;
                }
                if start.elapsed() > Duration::from_secs(10) {
                    return Err("worker timeout".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            let elapsed = start.elapsed().as_nanos();
            if !status.success() {
                return Err("worker failed".into());
            }
            let competitor_running_at_finish = competitor.0.try_wait()?.is_none();
            let mut pressure_after = None;
            if sampled {
                let t = Instant::now();
                pressure_after = system::collect_system()
                    .memory
                    .psi
                    .result
                    .ok()
                    .map(|p| p.some.total_us);
                sample_ns += t.elapsed().as_nanos();
            }
            // Both workers have fixed work; wait only until the same bounded deadline.
            loop {
                if let Some(status) = competitor.0.try_wait()? {
                    if !status.success() {
                        return Err("competitor failed".into());
                    }
                    break;
                }
                if start.elapsed() > Duration::from_secs(10) {
                    return Err("competitor timeout".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            println!(
                "{}",
                serde_json::json!({"record":"memory_repeatability", "repeat":repeat,
                "sampled":sampled, "worker_passes":128, "competitor_passes":512,
                "memory_bytes_per_process":33554432, "completion_ns":elapsed,
                "competitor_running_at_worker_finish":competitor_running_at_finish,
                "collection_ns":sample_ns, "visible_system_memory_psi_some_total_before_us":pressure_before,
                "visible_system_memory_psi_some_total_after_us":pressure_after,
                "kernel_intervention":false, "performance_effect":"NOT ESTABLISHED"})
            );
        }
    }
    Ok(())
}

const MIN_AVAILABLE_BYTES: u64 = 200 * 1024 * 1024;

fn require_available_memory(bytes: Option<u64>) -> Result<u64, String> {
    match bytes {
        Some(value) if value >= MIN_AVAILABLE_BYTES => Ok(value),
        Some(value) => Err(format!("MemAvailable {value} below 200 MiB minimum")),
        None => Err("MemAvailable unavailable".into()),
    }
}

fn available_memory(snapshot: &system::SystemSnapshot) -> Result<u64, String> {
    require_available_memory(
        snapshot
            .memory
            .mem
            .result
            .as_ref()
            .ok()
            .and_then(|m| m.bytes.get("MemAvailable").copied()),
    )
}

fn preflight() -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let snapshot = system::collect_system();
    let available = snapshot
        .memory
        .mem
        .result
        .as_ref()
        .ok()
        .and_then(|m| m.bytes.get("MemAvailable").copied());
    let identity = workload::identify(std::process::id())?;
    let cgroup = cgroup_pressure::collect(&identity);
    let caps = xynoos_linux_control::discover();
    let cgroup_path = cgroup
        .as_ref()
        .ok()
        .map(|c| c.cgroup.path.display().to_string());
    println!(
        "{}",
        serde_json::json!({"record":"preflight", "pid1":fs::read_to_string("/proc/1/comm").ok().map(|s|s.trim().to_string()),
        "cgroup_membership":fs::read_to_string("/proc/self/cgroup").ok(), "cgroup_path":cgroup_path,
        "cgroup_error":cgroup.err().map(|e|format!("{e:?}")), "capabilities":caps,
        "mem_available_bytes":available, "memory_gate":require_available_memory(available),
        "controlled_mode":"DISABLED: transient-unit ownership and controller activation not verified",
        "elapsed_ns":start.elapsed().as_nanos()})
    );
    Ok(())
}

fn validate_parent_state(
    procs: &str,
    controllers: &str,
    enabled: &str,
) -> Result<(), &'static str> {
    if !procs.trim().is_empty() {
        return Err("parent contains processes");
    }
    if !controllers.split_whitespace().any(|c| c == "memory") {
        return Err("memory controller unavailable");
    }
    if enabled
        .split_whitespace()
        .any(|c| !["cpu", "memory", "pids"].contains(&c))
    {
        return Err("unexpected enabled controller");
    }
    Ok(())
}

// Read-only service verification. It is not authorization to activate controllers.
fn manager_preflight(unit: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !unit.starts_with("xynoos-018d-")
        || !unit.ends_with(".service")
        || !unit
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
    {
        return Err("invalid transient unit name".into());
    }
    let output = Command::new("systemctl")
        .args([
            "--user",
            "show",
            unit,
            "--property=MainPID",
            "--property=ControlGroup",
            "--property=Delegate",
            "--property=DelegateSubgroup",
        ])
        .output()?;
    if !output.status.success() {
        return Err("cannot inspect transient service".into());
    }
    let props = String::from_utf8(output.stdout)?;
    let field = |name: &str| props.lines().find_map(|l| l.strip_prefix(name));
    let membership = fs::read_to_string("/proc/self/cgroup")?;
    let relative = membership
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or("no unified membership")?;
    let expected = format!(
        "{}/manager",
        field("ControlGroup=").ok_or("missing unit cgroup")?
    );
    if relative != expected
        || field("MainPID=") != Some(&std::process::id().to_string())
        || field("Delegate=") != Some("yes")
        || field("DelegateSubgroup=") != Some("manager")
    {
        return Err("manager/unit identity or delegation mismatch".into());
    }
    let path =
        system::parse_cgroup_path(&membership, &fs::read_to_string("/proc/self/mountinfo")?)?;
    if path.file_name().and_then(|s| s.to_str()) != Some("manager") {
        return Err("manager subgroup mismatch".into());
    }
    let parent = path.parent().ok_or("no delegated parent")?;
    let inspection = xynoos_linux_control::transaction::inspect_manager_parent(parent)?;
    let memory = available_memory(&system::collect_system());
    let parent_state = validate_parent_state(
        if inspection.parent_empty {
            ""
        } else {
            "populated"
        },
        &inspection.controllers,
        &inspection.enabled,
    );
    let eligible = parent_state.is_ok() && memory.is_ok();
    println!(
        "{}",
        serde_json::json!({"record":"manager_preflight", "unit":unit, "manager_pid":std::process::id(),
        "manager_path":path, "parent_path":parent, "inspection":inspection, "parent_state":parent_state,
        "memory_gate":memory, "eligible_for_further_validation":eligible,
        "controller_activation":"DISABLED", "kernel_control_write":false})
    );
    if !eligible {
        return Err("delegation preflight declined".into());
    }
    Ok(())
}

#[allow(dead_code)]
fn controlled(parent: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use xynoos_linux_control::{
        transaction::{Backend, DisposableCgroup, Outcome},
        Action,
    };
    install_cancellation()?;
    cancellation_check()?;
    let snapshot = system::collect_system();
    available_memory(&snapshot)?;
    cancellation_check()?;
    let mut group = DisposableCgroup::create(parent, true)?;
    if let Err(e) = cancellation_check() {
        let cleanup = group.cleanup();
        return Err(format!("{e}; disposable target cleanup: {cleanup:?}").into());
    }
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
            cancellation_check()?;
            let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                .arg("gated-memory-worker")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .map_err(|e| format!("worker spawn: {e}"))?;
            let spawned = Instant::now();
            let result = (|| -> Result<(), String> {
                cancellation_check()?;
                target.place_child(&child)?;
                cancellation_check()?;
                target.verify_child()?;
                available_memory(&system::collect_system())?;
                if spawned.elapsed() >= Duration::from_secs(5) {
                    return Err("worker timeout before release".into());
                }
                xynoos_linux_control::validate_telemetry(&snapshot, Instant::now(), action)?;
                if target.read_low()? != 32 * 1024 * 1024 {
                    return Err("control changed before release".into());
                }
                cancellation_check()?;
                let started = Instant::now();
                child
                    .stdin
                    .take()
                    .ok_or("missing release pipe")?
                    .write_all(&[1])
                    .map_err(|e| e.to_string())?;
                loop {
                    cancellation_check()?;
                    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                        execution_ns = Some(started.elapsed().as_nanos());
                        if !status.success() {
                            return Err(format!("worker failed: {status}"));
                        }
                        return Ok(());
                    }
                    if spawned.elapsed() > Duration::from_secs(5) {
                        return Err("worker timeout".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })();
            if let Err(e) = stop_and_reap(&mut child) {
                target.mark_unresolved();
                return Err(format!(
                    "{result:?}; {e}; target preserved and restoration blocked"
                ));
            }
            result
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
    if args.len() == 3 && args[1] == "manager-preflight" {
        return manager_preflight(&args[2]);
    }
    if args.len() == 2 && args[1] == "preflight" {
        return preflight();
    }
    if args.len() == 2 && args[1] == "gated-memory-worker" {
        return gated_memory_worker(128);
    }
    if args.len() == 2 && args[1] == "gated-memory-competitor" {
        return gated_memory_worker(512);
    }
    if args.len() == 2 && args[1] == "memory-repeatability" {
        return memory_repeatability();
    }
    if args.get(1).is_some_and(|x| x == "controlled") {
        if args.len() != 5 || args[2] != "--authorize" || args[3] != "--parent" {
            return Err("usage: controlled --authorize --parent /absolute/delegated/parent".into());
        }
        return Err("controlled mode disabled: delegated transient unit verification and lifecycle safety gates remain incomplete".into());
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
    use super::{
        require_available_memory, stop_and_reap, validate_mode, validate_parent_state,
        MIN_AVAILABLE_BYTES,
    };
    use std::process::{Command, Stdio};
    #[test]
    fn stop_reaps_child_and_is_idempotent() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        stop_and_reap(&mut child).unwrap();
        stop_and_reap(&mut child).unwrap();
        assert!(child.try_wait().unwrap().is_some());
    }
    #[test]
    fn parent_preflight_refuses_unsafe_state() {
        assert!(validate_parent_state("", "cpu memory pids", "cpu pids").is_ok());
        assert!(validate_parent_state("123", "memory", "").is_err());
        assert!(validate_parent_state("", "cpu pids", "").is_err());
        assert!(validate_parent_state("", "memory", "io").is_err());
    }
    #[test]
    fn memory_guard_rejects_missing_and_low() {
        assert!(require_available_memory(None).is_err());
        assert!(require_available_memory(Some(MIN_AVAILABLE_BYTES - 1)).is_err());
        assert_eq!(
            require_available_memory(Some(MIN_AVAILABLE_BYTES)),
            Ok(MIN_AVAILABLE_BYTES)
        );
    }
    #[test]
    fn modes_fail_closed() {
        assert!(validate_mode("observe").is_ok());
        assert!(validate_mode("dry-run").is_ok());
        assert!(validate_mode("controlled").is_err());
        assert!(validate_mode("unknown").is_err());
    }
}
