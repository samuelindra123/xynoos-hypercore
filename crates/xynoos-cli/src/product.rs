use serde_json::{json, Value};
use std::{
    error::Error,
    fs,
    io::IsTerminal,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use xynoos_pressureiq::system::{collect_system, cpu_utilization, SystemSnapshot};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const COMMIT: &str = env!("XYNOOS_SOURCE_COMMIT");
const MANAGER: &str = include_str!("../../../distribution/manager.py");

fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}
fn state_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .map(|p| p.join("xynoos"))
}
fn diagnostic_path() -> Option<PathBuf> {
    state_dir().map(|p| p.join("last-diagnostic.json"))
}
fn last_diagnostic() -> Option<Value> {
    let p = diagnostic_path()?;
    let meta = fs::symlink_metadata(&p).ok()?;
    if !meta.is_file() || meta.len() > 1_048_576 {
        return None;
    }
    serde_json::from_slice(&fs::read(p).ok()?).ok()
}
fn manager(args: &[String]) -> Result<Value, Box<dyn Error>> {
    let out = Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(MANAGER)
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr)
            .trim()
            .to_string()
            .into());
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}
fn capabilities(s: &SystemSnapshot) -> Value {
    let availability = |ok: bool| if ok { "AVAILABLE" } else { "UNAVAILABLE" };
    json!({
        "read_only_telemetry": availability(s.memory.mem.result.is_ok()),
        "memory_psi": availability(s.memory.psi.result.is_ok()),
        "cpu_psi": availability(s.cpu_psi.is_ok()), "io_psi": availability(s.io_psi.is_ok()),
        "cgroup_v2_observation": availability(s.cgroup.is_ok()),
        "policy_simulation": "EXPERIMENTAL", "cooperative_owned_memory": "EXPERIMENTAL",
        "physical_kernel_mutation": "NOT AUTHORIZED", "active_zram_tuning": "UNAVAILABLE",
        "desktop_optimization": "UNAVAILABLE"
    })
}
fn snapshot(s: &SystemSnapshot) -> Value {
    let mem = s.memory.mem.result.as_ref().ok();
    let bytes = |key: &str| mem.and_then(|m| m.bytes.get(key)).copied();
    let zram: Vec<_> = fs::read_dir("/sys/block")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("zram"))
        .take(64)
        .collect();
    let psi = |p: &Result<xynoos_pressureiq::Psi, xynoos_pressureiq::Error>| {
        p.as_ref().ok().map(|p| p.some.avg10)
    };
    json!({
        "kernel": read("/proc/sys/kernel/osrelease"), "distribution": read("/etc/os-release"),
        "architecture": std::env::consts::ARCH, "hostname": read("/proc/sys/kernel/hostname"),
        "logical_cpus": s.cpu.as_ref().ok().map(|c| c.logical_cpus),
        "memory_total_bytes": bytes("MemTotal"), "memory_available_bytes": bytes("MemAvailable"),
        "swap_total_bytes": bytes("SwapTotal"), "swap_free_bytes": bytes("SwapFree"),
        "swap_observation": read("/proc/swaps"), "zram_devices": zram,
        "cpu_psi_some_avg10_pct": psi(&s.cpu_psi), "memory_psi_some_avg10_pct": s.memory.psi.result.as_ref().ok().map(|p| p.some.avg10),
        "io_psi_some_avg10_pct": psi(&s.io_psi), "load_one": s.load_one.as_ref().ok(),
        "sequential": true, "collection_ns": s.elapsed_ns, "capabilities": capabilities(s)
    })
}
fn human_bytes(v: &Value) -> String {
    v.as_u64()
        .map(|n| format!("{:.2} GiB", n as f64 / 1_073_741_824.0))
        .unwrap_or_else(|| "unavailable".into())
}
fn text(v: &Value) -> String {
    v.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| v.to_string())
}
fn render(command: &str, v: &Value, color: bool) {
    let accent = if color { "\x1b[36m" } else { "" };
    let reset = if color { "\x1b[0m" } else { "" };
    println!("{accent} /X/  XYNOOS HYPERCORE{reset}");
    println!("      Linux Resource Intelligence");
    println!("      {VERSION} · Experimental Developer Preview\n");
    if command == "help" || command == "overview" {
        println!("Observe first. Physical kernel mutation: DISABLED.\n");
        println!("  doctor        Check host support and release integrity\n  status        Inspect pressure, swap and engine state\n  capabilities  Show available and experimental features\n  diagnose      Capture bounded read-only telemetry\n  benchmark     Measure telemetry overhead (no workload tuning)\n  update        Install the latest verified preview\n  rollback      Restore the previous verified version offline\n  uninstall     Remove owned software; preserve evidence\n");
        println!("Use --json for structured output; --no-color for plain text.");
        println!("Diagnose: --count 1..60 --interval-ms 100..10000 (max 60s)");
        println!("Update: --channel preview|stable or --version vX.Y.Z[-preview.N]");
        println!("Research compatibility: pressure, system, shadow");
        return;
    }
    if command == "capabilities" {
        if let Some(c) = v["capabilities"].as_object() {
            for (k, x) in c {
                println!("  {k:<30} {}", text(x));
            }
        }
        return;
    }
    if let Some(h) = v.get("host") {
        println!(
            "  Host          {} / {}",
            text(&h["hostname"]),
            text(&h["architecture"])
        );
        println!("  Kernel        {}", text(&h["kernel"]));
        let distro = h["distribution"]
            .as_str()
            .unwrap_or("unavailable")
            .lines()
            .find_map(|s| s.strip_prefix("PRETTY_NAME="))
            .unwrap_or("unavailable");
        println!("  Distribution  {}", distro.trim_matches('"'));
        println!(
            "  Memory        {} available / {} total",
            human_bytes(&h["memory_available_bytes"]),
            human_bytes(&h["memory_total_bytes"])
        );
        println!(
            "  Swap          {} total / {} free",
            human_bytes(&h["swap_total_bytes"]),
            human_bytes(&h["swap_free_bytes"])
        );
        println!("  ZRAM          {}", h["zram_devices"]);
        println!(
            "  PSI some 10s  CPU {}% / memory {}% / I/O {}%",
            h["cpu_psi_some_avg10_pct"], h["memory_psi_some_avg10_pct"], h["io_psi_some_avg10_pct"]
        );
    }
    println!("  Engine        on-demand observation; no daemon");
    println!("  Intervention  none; physical kernel mutation DISABLED");
    if let Some(i) = v.get("installation") {
        println!("  Installation  {}", text(&i["status"]));
        if i["version"].is_string() {
            println!(
                "  Release       {} / {}",
                text(&i["version"]),
                text(&i["channel"])
            );
            println!("  Integrity     {}", text(&i["integrity"]));
        } else {
            println!("  Detail        {}", text(&i["detail"]));
        }
    }
    if command == "doctor" {
        if let Some(c) = v["host"]["capabilities"].as_object() {
            println!();
            for (k, x) in c {
                println!("  {k:<30} {}", text(x));
            }
        }
        println!("\n  Permissions   read-only; root does not authorize intervention");
        println!("  Support       Linux x86_64; Debian 13 N5000 validation pending");
    }
    if let Some(d) = v.get("diagnostic") {
        println!(
            "  Diagnostic    {} samples / {} ms",
            d["count"], d["elapsed_ms"]
        );
        println!("  Observation   {}", text(&d["summary"]));
        println!("  Inference     {}", text(&d["inference"]));
    }
    if let Some(last) = v.get("last_diagnostic") {
        println!(
            "  Last capture  {}",
            last.get("summary")
                .map(text)
                .unwrap_or_else(|| "none".into())
        );
    }
    if command == "benchmark" {
        println!("  Measurement   {}", v["measurement"]);
    }
}

pub fn run(args: &[String]) -> Result<bool, Box<dyn Error>> {
    if args
        .first()
        .is_some_and(|a| matches!(a.as_str(), "pressure" | "system" | "shadow"))
    {
        return Ok(false);
    }
    let json_output = args.iter().any(|a| a == "--json");
    let color = std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").unwrap_or_default() != "dumb"
        && !args.iter().any(|a| a == "--no-color");
    let filtered: Vec<_> = args
        .iter()
        .filter(|a| a.as_str() != "--json" && a.as_str() != "--no-color")
        .cloned()
        .collect();
    let command = filtered.first().map(String::as_str).unwrap_or("overview");
    let mut v = json!({"schema_version":1,"product":"Xynoos HyperCore", "version":VERSION,
        "source_commit":COMMIT,"channel":"preview", "physical_kernel_mutation":false, "intervention_active":false});
    if command == "--version" || command == "-V" {
        if filtered.len() != 1 {
            return Err("Unexpected version argument".into());
        }
        if json_output {
            println!("{v}");
        } else {
            println!("Xynoos HyperCore {VERSION} ({COMMIT}) — Experimental Developer Preview");
        }
        return Ok(true);
    }
    if matches!(command, "update" | "rollback" | "uninstall") {
        if !json_output {
            let status = Command::new("python3")
                .args(["-I", "-c", MANAGER])
                .args(&filtered)
                .status()?;
            if !status.success() {
                return Err("Distribution command failed; current installation preserved where recovery is available".into());
            }
            return Ok(true);
        }
        let mut a = filtered.clone();
        a.push("--json".into());
        let result = manager(&a)?;
        if json_output {
            println!("{result}");
        } else {
            println!("Xynoos HyperCore · {}", text(&result["result"]));
            if let Some(t) = result.get("version") {
                println!("Version: {}", text(t));
            }
            if let Some(p) = result.get("path") {
                println!("Path: {}", text(p));
            }
        }
        return Ok(true);
    }
    if matches!(command, "help" | "--help" | "-h" | "overview") {
        if filtered.len() > 1 {
            return Err("Use xynoos help".into());
        }
        v["commands"] = json!([
            "doctor",
            "status",
            "capabilities",
            "diagnose",
            "benchmark",
            "update",
            "rollback",
            "uninstall"
        ]);
        if json_output {
            println!("{v}");
        } else {
            render("help", &v, color);
        }
        return Ok(true);
    }
    if !matches!(
        command,
        "doctor" | "status" | "capabilities" | "diagnose" | "benchmark"
    ) {
        return Err(format!("Unknown command '{command}'. Use xynoos help.").into());
    }
    let (mut count, mut interval) = (5u32, 500u64);
    let mut i = 1;
    while i < filtered.len() {
        if command != "diagnose" {
            return Err("Unexpected option; use xynoos help".into());
        }
        let value = filtered.get(i + 1).ok_or("Missing option value")?;
        match filtered[i].as_str() {
            "--count" => count = value.parse()?,
            "--interval-ms" => interval = value.parse()?,
            _ => return Err("Unknown diagnostic option".into()),
        }
        i += 2;
    }
    if !(1..=60).contains(&count)
        || !(100..=10000).contains(&interval)
        || u64::from(count - 1) * interval > 60000
    {
        return Err(
            "Diagnostic bounds: count 1..60, interval 100..10000 ms, duration <=60s".into(),
        );
    }
    let s = collect_system();
    if command == "capabilities" {
        v["capabilities"] = capabilities(&s);
    } else {
        v["host"] = snapshot(&s);
        v["installation"] = manager(&["health".into(), "--json".into()])
            .unwrap_or_else(|e| json!({"status":"UNAVAILABLE","detail":e.to_string()}));
        v["last_diagnostic"] = last_diagnostic().unwrap_or(Value::Null);
    }
    if command == "diagnose" {
        let started = Instant::now();
        let mut samples = vec![snapshot(&s)];
        let mut previous = s;
        for _ in 1..count {
            std::thread::sleep(Duration::from_millis(interval));
            let next = collect_system();
            let mut sample = snapshot(&next);
            sample["observed_cpu_utilization_pct"] = json!(previous
                .cpu
                .as_ref()
                .ok()
                .zip(next.cpu.as_ref().ok())
                .and_then(|(a, b)| cpu_utilization(
                    a,
                    b,
                    next.monotonic.duration_since(previous.monotonic)
                )
                .ok()));
            samples.push(sample);
            previous = next;
        }
        let d = json!({"count":count,"elapsed_ms":started.elapsed().as_millis(),"samples":samples,
            "summary":"Read-only counters captured; missing fields remain null.",
            "inference":"Counters alone do not establish an application bottleneck or performance benefit.",
            "timestamp_unix_s":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs()});
        // The manager creates private paths and writes atomically under its lock.
        let saved = Command::new("python3")
            .args(["-I", "-c", MANAGER, "record-diagnostic"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn();
        match saved {
            Ok(mut child) => {
                use std::io::Write;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(serde_json::to_string(&d)?.as_bytes())?;
                }
                let output = child.wait_with_output()?;
                if !output.status.success() {
                    v["evidence_save_error"] = json!(String::from_utf8_lossy(&output.stderr));
                }
            }
            Err(e) => v["evidence_save_error"] = json!(e.to_string()),
        }
        v["diagnostic"] = d;
    }
    if command == "benchmark" {
        let start = Instant::now();
        let n = 20;
        for _ in 0..n {
            std::hint::black_box(collect_system());
        }
        v["measurement"] = json!({"operation":"sequential telemetry collection", "iterations":n,
            "total_ns":start.elapsed().as_nanos(),"scope":"this process on this host", "desktop_performance_benefit":"NOT PROVEN"});
    }
    if json_output {
        println!("{v}");
    } else {
        render(command, &v, color);
    }
    Ok(true)
}
