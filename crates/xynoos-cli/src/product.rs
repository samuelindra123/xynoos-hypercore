use serde_json::{json, Value};
#[cfg(not(feature = "debian-package"))]
use std::process::Command;
use std::{
    error::Error,
    fs,
    io::IsTerminal,
    path::PathBuf,
    time::{Duration, Instant},
};
use xynoos_pressureiq::system::{collect_system, cpu_utilization, SystemSnapshot};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const COMMIT: &str = env!("XYNOOS_SOURCE_COMMIT");
#[cfg(not(feature = "debian-package"))]
const MANAGER: &str = include_str!("../../../distribution/manager.py");

fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn detect_init_system() -> Value {
    let systemd_runtime = std::path::Path::new("/run/systemd/system").is_dir();
    let pid1_comm = read("/proc/1/comm").map(|s| {
        s.chars()
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .take(32)
            .collect::<String>()
    });
    let init_kind = if systemd_runtime {
        "systemd".to_string()
    } else if pid1_comm.as_deref() == Some("init") && std::path::Path::new("/etc/init.d").is_dir() {
        "sysvinit".to_string()
    } else if let Some(ref comm) = pid1_comm {
        if comm.is_empty() {
            "unknown".to_string()
        } else {
            comm.clone()
        }
    } else {
        "unknown".to_string()
    };
    json!({
        "init_system": init_kind,
        "systemd_detected": systemd_runtime,
        "pid1_comm": pid1_comm,
    })
}

fn detect_daemon_state() -> Value {
    let pid_path = std::path::Path::new("/run/xynoos/hypercore.pid");
    let Ok(meta) = fs::symlink_metadata(pid_path) else {
        return json!({"state": "not_running", "pid_file": pid_path.display().to_string()});
    };
    if !meta.is_file() || meta.len() > 1024 {
        return json!({"state": "unknown", "pid_file": pid_path.display().to_string()});
    }
    let Ok(raw) = fs::read_to_string(pid_path) else {
        return json!({"state": "permission_denied", "pid_file": pid_path.display().to_string()});
    };
    let pid_opt = serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|v| v.get("pid").and_then(|p| p.as_u64()))
        .or_else(|| raw.trim().parse::<u64>().ok());
    match pid_opt {
        Some(pid) if std::path::Path::new(&format!("/proc/{pid}")).exists() => {
            json!({"state": "running", "pid": pid, "pid_file": pid_path.display().to_string()})
        }
        Some(pid) => {
            json!({"state": "stale_pid_file", "pid": pid, "pid_file": pid_path.display().to_string()})
        }
        None => {
            json!({"state": "unparseable_pid_file", "pid_file": pid_path.display().to_string()})
        }
    }
}

fn detect_storage_free_bytes() -> Option<u64> {
    #[cfg(unix)]
    unsafe {
        let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        #[allow(clippy::unnecessary_cast)]
        if libc::statvfs(c"/".as_ptr(), st.as_mut_ptr()) == 0 {
            let s = st.assume_init();
            return Some((s.f_bavail as u64).saturating_mul(s.f_frsize as u64));
        }
    }
    None
}

fn detect_recovery_journal() -> Value {
    let p = std::path::Path::new("/run/xynoos/hypercore_journal.json");
    if p.exists() {
        let size = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        json!({"status": "pending_journal_present", "path": p.display().to_string(), "size_bytes": size})
    } else {
        json!({"status": "clean_no_journal", "path": p.display().to_string()})
    }
}

fn detect_cgroup_support() -> Value {
    let controllers_path = std::path::Path::new("/sys/fs/cgroup/cgroup.controllers");
    let cgroup_v2_available = controllers_path.exists();
    let controllers = fs::read_to_string(controllers_path)
        .ok()
        .map(|s| s.trim().to_string());
    let current_rel = fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|txt| {
            txt.lines()
                .find_map(|l| l.strip_prefix("0::"))
                .map(str::to_string)
        });
    let current_path = current_rel
        .as_deref()
        .map(|rel| format!("/sys/fs/cgroup{rel}"));
    let delegated_writable = current_path.as_deref().is_some_and(|p| {
        let sc = std::path::Path::new(p).join("cgroup.subtree_control");
        fs::OpenOptions::new().write(true).open(sc).is_ok()
    });
    json!({
        "cgroup_v2_available": cgroup_v2_available,
        "controllers": controllers,
        "current_cgroup": current_path,
        "delegated_writable": delegated_writable,
    })
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
pub(crate) fn last_diagnostic() -> Option<Value> {
    let p = diagnostic_path()?;
    let meta = fs::symlink_metadata(&p).ok()?;
    if !meta.is_file() || meta.len() > 1_048_576 {
        return None;
    }
    serde_json::from_slice(&fs::read(p).ok()?).ok()
}
pub(crate) fn installation_info() -> Value {
    manager(&["health".into(), "--json".into()])
        .unwrap_or_else(|e| json!({"status":"UNAVAILABLE","detail":e.to_string()}))
}
#[cfg(feature = "debian-package")]
fn manager(_args: &[String]) -> Result<Value, Box<dyn Error>> {
    Ok(
        json!({"status":"DEBIAN PACKAGE", "detail":"Managed by dpkg/APT; integrity is not checked here. Run dpkg --verify xynoos-hypercore."}),
    )
}
#[cfg(not(feature = "debian-package"))]
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
fn availability<T>(result: &Result<T, xynoos_pressureiq::Error>) -> &'static str {
    match result {
        Ok(_) => "AVAILABLE",
        Err(xynoos_pressureiq::Error::Permission) => "NOT AUTHORIZED",
        Err(_) => "UNAVAILABLE",
    }
}
fn capabilities(s: &SystemSnapshot) -> Value {
    json!({
        "read_only_telemetry": availability(&s.memory.mem.result),
        "memory_psi": availability(&s.memory.psi.result),
        "cpu_psi": availability(&s.cpu_psi), "io_psi": availability(&s.io_psi),
        "cgroup_v2_observation": availability(&s.cgroup),
        "init_system_detection": "AVAILABLE",
        "daemon_observe_only": "AVAILABLE",
        "universal_platform_discovery": "AVAILABLE",
        "hardware_intelligence_cpu": "AVAILABLE",
        "hardware_intelligence_gpu": "AVAILABLE",
        "hardware_intelligence_storage": "AVAILABLE",
        "hardware_intelligence_power": "AVAILABLE",
        "existing_tuning_audit": "AVAILABLE",
        "configuration_conflict_analysis": "AVAILABLE",
        "automatic_setup_planning": "AVAILABLE",
        "policy_simulation": "EXPERIMENTAL", "cooperative_owned_memory": "EXPERIMENTAL",
        "delegated_cgroup_control": "EXPERIMENTAL",
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
    let init = detect_init_system();
    let cgroup = detect_cgroup_support();
    let daemon = detect_daemon_state();
    let mut limitations = vec![
        "HyperCore does not transparently compress arbitrary application memory.".to_string(),
        "HyperCore does not replace Linux swap, zram, or zswap.".to_string(),
    ];
    if s.cpu_psi.is_err() || s.memory.psi.result.is_err() || s.io_psi.is_err() {
        limitations.push(
            "One or more PSI /proc/pressure interfaces are unavailable or restricted.".to_string(),
        );
    }
    if !cgroup["delegated_writable"].as_bool().unwrap_or(false) {
        limitations.push("No delegated writable cgroup v2 subtree for current process; transactional control remains disabled.".to_string());
    }
    json!({
        "kernel": read("/proc/sys/kernel/osrelease"), "distribution": read("/etc/os-release"),
        "architecture": std::env::consts::ARCH, "hostname": read("/proc/sys/kernel/hostname"),
        "init": init,
        "cgroup": cgroup,
        "daemon_state": daemon,
        "supported_runtime_modes": ["observe-only", "advisory", "transactional"],
        "psi_availability": {
            "cpu": s.cpu_psi.is_ok(),
            "memory": s.memory.psi.result.is_ok(),
            "io": s.io_psi.is_ok(),
        },
        "active_limitations": limitations,
        "logical_cpus": s.cpu.as_ref().ok().map(|c| c.logical_cpus),
        "memory_total_bytes": bytes("MemTotal"), "memory_available_bytes": bytes("MemAvailable"),
        "swap_total_bytes": bytes("SwapTotal"), "swap_free_bytes": bytes("SwapFree"),
        "swap_observation": read("/proc/swaps"), "zram_devices": zram,
        "cpu_psi_some_avg10_pct": psi(&s.cpu_psi), "memory_psi_some_avg10_pct": s.memory.psi.result.as_ref().ok().map(|p| p.some.avg10),
        "io_psi_some_avg10_pct": psi(&s.io_psi), "load_one": s.load_one.as_ref().ok(),
        "storage_available_bytes": detect_storage_free_bytes(),
        "recovery_journal": detect_recovery_journal(),
        "default_config_valid": xynoos_aura::DaemonConfig::default().validate().is_ok(),
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
    if command == "overview" {
        let h = &v["host"];
        println!(
            "  Linux {} / {} logical CPUs",
            text(&h["architecture"]),
            h["logical_cpus"]
        );
        println!(
            "  Memory: {} available / {} total",
            human_bytes(&h["memory_available_bytes"]),
            human_bytes(&h["memory_total_bytes"])
        );
        println!("  Read-only observation. No daemon or active intervention.\n");
        println!("  xynoos doctor   Check host support and release integrity\n  xynoos status   Inspect current pressure and swap\n  xynoos help     Explore all commands");
        return;
    }
    if command == "help" {
        println!("Observe first. Physical kernel mutation: DISABLED.\n");
        #[cfg(not(feature = "debian-package"))]
        println!("  doctor        Check host support and release integrity\n  status        Inspect pressure, swap and engine state\n  capabilities  Show available and experimental features\n  inspect       Inspect universal platform, hardware, and kernel capabilities\n  tuning audit  Audit existing system tuning, ownership, and conflicts\n  setup --plan  Generate a read-only automatic optimization plan\n  diagnose      Capture bounded read-only telemetry\n  benchmark     Measure telemetry overhead (no workload tuning)\n  update        Install the latest verified preview\n  rollback      Restore the previous verified version offline\n  uninstall     Remove owned software; preserve evidence\n");
        #[cfg(feature = "debian-package")]
        println!("  doctor        Check host support and package information\n  status        Inspect pressure, swap and engine state\n  capabilities  Show available and experimental features\n  inspect       Inspect universal platform, hardware, and kernel capabilities\n  tuning audit  Audit existing system tuning, ownership, and conflicts\n  setup --plan  Generate a read-only automatic optimization plan\n  diagnose      Capture bounded read-only telemetry\n  benchmark     Measure telemetry overhead (no workload tuning)\n");
        println!("Use --json for structured output; --no-color for plain text.");
        println!("Diagnose: --count 1..60 --interval-ms 100..10000 (max 60s)");
        #[cfg(not(feature = "debian-package"))]
        println!("Update: --channel preview|stable or --version vX.Y.Z[-preview.N]");
        println!("Research compatibility: pressure, system, shadow");
        #[cfg(feature = "debian-package")]
        println!("Debian package: use APT for update, rollback and removal. Diagnose writes JSON to stdout only; redirect it to retain evidence.");
        return;
    }
    if command == "capabilities" {
        if let Some(c) = v["capabilities"].as_object() {
            for (k, x) in c {
                println!("  {k:<30} {}", semantic_status(x, color));
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
        println!(
            "  Init / Cgroup {} / cgroup_v2={} (delegated_writable={})",
            text(&h["init"]["init_system"]),
            h["cgroup"]["cgroup_v2_available"],
            h["cgroup"]["delegated_writable"]
        );
        println!(
            "  Storage (/)   {}",
            human_bytes(&h["storage_available_bytes"])
        );
        println!("  Journal       {}", text(&h["recovery_journal"]["status"]));
        println!("  Daemon state  {}", text(&h["daemon_state"]["state"]));
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
                println!("  {k:<30} {}", semantic_status(x, color));
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
    if let Some(error) = v.get("evidence_save_error") {
        println!(
            "  Warning       Diagnostic evidence was not saved: {}",
            text(error)
        );
    }
}

fn semantic_status(value: &Value, color: bool) -> String {
    let status = text(value);
    if !color {
        return status;
    }
    let code = match status.as_str() {
        "AVAILABLE" => "32",
        "UNAVAILABLE" => "2",
        _ => "33",
    };
    format!("\x1b[{code}m{status}\x1b[0m")
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
    #[cfg(feature = "debian-package")]
    if matches!(command, "update" | "rollback" | "uninstall") {
        return Err("Managed by dpkg/APT: use apt install, apt install --allow-downgrades, or apt remove xynoos-hypercore; see /usr/share/doc/xynoos-hypercore/".into());
    }
    #[cfg(not(feature = "debian-package"))]
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
        if command == "overview" {
            v["host"] = snapshot(&collect_system());
        }
        if json_output {
            println!("{v}");
        } else {
            render(
                if command == "overview" {
                    "overview"
                } else {
                    "help"
                },
                &v,
                color,
            );
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
    if command == "doctor" {
        let inv = xynoos_pressureiq::discovery::inventory::SystemInventory::collect_live();
        let ledger = xynoos_linux_control::config::OwnershipLedger::load_from(
            std::path::Path::new("/var/lib/xynoos/owned-configs.json"),
        )
        .unwrap_or_default();

        let mut checks = Vec::new();
        let cgroup_ok = inv.platform.cgroup_hierarchy
            == xynoos_pressureiq::discovery::platform::CgroupHierarchy::V2Unified;
        checks.push(json!({
            "name": "Cgroup v2 Unified",
            "pass": cgroup_ok,
            "message": if cgroup_ok { "Unified hierarchy active" } else { "Legacy or Hybrid cgroups (limited isolation)" },
        }));

        let psi_ok = inv.platform.psi_available;
        checks.push(json!({
            "name": "Pressure Stall Information (PSI)",
            "pass": psi_ok,
            "message": if psi_ok { "PSI controllers available" } else { "PSI unavailable in kernel" },
        }));

        let active_oom_count = (inv.memory.oom_managers.earlyoom_active as usize)
            + (inv.memory.oom_managers.systemd_oomd_active as usize)
            + (inv.memory.oom_managers.nohang_active as usize)
            + (inv.memory.oom_managers.oomd_active as usize);
        checks.push(json!({
            "name": "OOM Manager Coexistence",
            "pass": active_oom_count <= 1,
            "message": if active_oom_count == 0 {
                "Kernel default OOM killer active"
            } else if active_oom_count == 1 {
                "Single active userspace manager (cooperative)"
            } else {
                "Multiple competing userspace OOM killers running"
            },
        }));

        let mut ledger_tampered = false;
        for entry in ledger.entries.values() {
            if ledger.check_entry_integrity(entry, std::path::Path::new("/etc"))
                == xynoos_linux_control::config::IntegrityStatus::ModifiedExternally
            {
                ledger_tampered = true;
                break;
            }
        }
        checks.push(json!({
            "name": "Configuration Ownership Ledger",
            "pass": !ledger_tampered,
            "message": if ledger_tampered { "External file tampering detected" } else { "Clean and synchronized" },
        }));

        v["checks"] = json!(checks);
        v["ledger_entries_count"] = json!(ledger.entries.len());
        v["active_conflicts_count"] = json!(inv.conflicts.len());
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
        #[cfg(feature = "debian-package")]
        {
            v["evidence_persistence"] = json!("stdout only; redirect output to retain evidence");
        }
        #[cfg(not(feature = "debian-package"))]
        {
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
