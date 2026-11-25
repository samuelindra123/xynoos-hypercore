use std::{
    error::Error,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use xynoos_pressureiq::{
    collect, csv_row, sample, validate_sampling, Error as PressureError, CSV_HEADER,
};
mod product;
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("daemon") {
        return daemon_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("profile") {
        return profile_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("memory") {
        return memory_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("resources") {
        if args.len() != 1 && args[1..] != ["--json"] {
            return Err("usage: resources [--json]".into());
        }
        let start = std::time::Instant::now();
        let hardware = xynoos_pressureiq::hardware::collect(Path::new("/sys"));
        let snapshot = xynoos_pressureiq::system::collect_system();
        let contract = xynoos_pressureiq::system::contract::build(
            &snapshot,
            None,
            None,
            std::time::Instant::now(),
            Duration::from_secs(5),
        );
        let plan =
            xynoos_aura::resource::plan(&hardware, &contract, start.elapsed().as_millis() as u64);
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1, "hardware": hardware, "system": contract, "aura": plan
            }))?
        );
        return Ok(());
    }
    if product::run(&args)? {
        return Ok(());
    }
    if args.is_empty() {
        let mut store = xynoos_hypermemory::PageStore::with_capacity(1)?;
        let page = vec![0; 4096];
        let id = store.insert(&page)?;
        assert_eq!(store.get(id)?, page);
        println!("Halaman {:?}: {:?}", id, store.stats());
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("shadow") {
        return shadow_command(&args);
    }
    if args.first().map(String::as_str) == Some("system") {
        return system_command(&args);
    }
    if args.len() < 2 || args[0] != "pressure" {
        return Err("Gunakan: pressure snapshot | pressure sample [--interval-ms N] [--count N] [--output FILE]".into());
    }
    if args[1] == "snapshot" && (args.len() == 2 || args.len() == 3 && args[2] == "--human") {
        let s = collect();
        if args.len() == 3 {
            println!("PressureIQ — sumber /proc, pengambilan berurutan");
            match &s.psi.result {
                Ok(p) => println!(
                    "PSI some avg10: {:.2}% ; total: {} µs",
                    p.some.avg10, p.some.total_us
                ),
                Err(e) => println!("PSI: {e}"),
            }
            match &s.mem.result {
                Ok(m) => println!(
                    "MemTotal: {:?} byte; MemAvailable: {:?} byte",
                    m.bytes.get("MemTotal"),
                    m.bytes.get("MemAvailable")
                ),
                Err(e) => println!("MemInfo: {e}"),
            }
            match &s.vm.result {
                Ok(v) => println!("VMStat: {} counter tersedia", v.counters.len()),
                Err(e) => println!("VMStat: {e}"),
            }
            println!(
                "Parsial: {}; durasi snapshot: {} ns",
                s.partial(),
                s.elapsed_ns
            );
        } else {
            println!("{CSV_HEADER}\n{}", csv_row(&s));
        }
        if s.partial() {
            eprintln!(
                "Pengumpulan parsial: PSI={:?}; meminfo={:?}; vmstat={:?}",
                s.psi.result.as_ref().err(),
                s.mem.result.as_ref().err(),
                s.vm.result.as_ref().err()
            );
        }
        return Ok(());
    }
    if args[1] != "sample" {
        return Err("Perintah pressure tidak valid".into());
    }
    let (mut count, mut ms, mut output) = (10u32, 1000u64, None);
    let mut i = 2;
    while i < args.len() {
        if i + 1 >= args.len() {
            return Err("Parameter membutuhkan nilai".into());
        }
        match args[i].as_str() {
            "--count" => count = args[i + 1].parse()?,
            "--interval-ms" => ms = args[i + 1].parse()?,
            "--output" => output = Some(PathBuf::from(&args[i + 1])),
            _ => return Err("Parameter tidak dikenal".into()),
        }
        i += 2
    }
    let interval = Duration::from_millis(ms);
    validate_sampling(count, interval)?;
    let mut writer: Box<dyn Write> = match output {
        Some(path) => {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let resolved = std::fs::canonicalize(parent)?;
            if resolved.starts_with("/proc")
                || resolved.starts_with("/sys")
                || resolved.starts_with("/dev")
                || path.exists() && std::fs::canonicalize(&path)?.starts_with("/proc")
            {
                return Err("Output harus berupa file biasa di luar filesystem sistem".into());
            }
            Box::new(
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?,
            )
        }
        None => Box::new(io::stdout()),
    };
    writeln!(writer, "{CSV_HEADER}")?;
    sample(count, interval, |s| {
        if s.partial() {
            eprintln!(
                "Pengumpulan parsial: PSI={:?}; meminfo={:?}; vmstat={:?}",
                s.psi.result.as_ref().err(),
                s.mem.result.as_ref().err(),
                s.vm.result.as_ref().err()
            );
        }
        writeln!(writer, "{}", csv_row(s)).map_err(|e| PressureError::Io(e.kind().to_string()))
    })?;
    Ok(())
}

fn system_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    use xynoos_pressureiq::system::{collect_system, cpu_utilization, disk_delta};
    if args.len() >= 3 && args[1] == "snapshot" && args[2] == "--json" {
        let mut disk = None;
        let mut freshness_ms = 5000u64;
        let mut i = 3;
        while i < args.len() {
            let value = args.get(i + 1).ok_or("Missing option value")?;
            match args[i].as_str() {
                "--disk" => disk = Some(value.as_str()),
                "--freshness-ms" => freshness_ms = value.parse()?,
                _ => return Err("Unknown option".into()),
            }
            i += 2;
        }
        let s = collect_system();
        let contract = xynoos_pressureiq::system::contract::build(
            &s,
            None,
            disk,
            std::time::Instant::now(),
            Duration::from_millis(freshness_ms),
        );
        println!("{}", serde_json::to_string(&contract)?);
        return Ok(());
    }
    if args.len() == 2 && args[1] == "snapshot" {
        let s = collect_system();
        println!("CPU: {:?}\nCPU PSI: {:?}\nMemori: {:?}\nMemory PSI: {:?}\nVMStat: {:?}\nIO PSI: {:?}\nDisk: {:?}\nCgroup v2: {:?}\nFrekuensi CPU kHz: {:?}\nLoad 1 menit: {:?}\nDRM: {:?}\nThermal: {:?}\nPower: {:?}\nGPU utilisasi/video decode/akselerasi: tidak tersedia",s.cpu,s.cpu_psi,s.memory.mem.result,s.memory.psi.result,s.memory.vm.result,s.io_psi,s.disks,s.cgroup,s.cpu_frequency_khz,s.load_one,s.gpu,s.thermal,s.power);
        return Ok(());
    }
    if args.len() < 2 || args[1] != "sample" {
        return Err("Gunakan system snapshot atau system sample --count N --interval-ms N".into());
    }
    let (mut count, mut ms) = (5u32, 1000u64);
    let mut disk_name: Option<&str> = None;
    let mut json = false;
    let mut freshness_ms = 5000u64;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--json" {
            json = true;
            i += 1;
            continue;
        }
        if i + 1 >= args.len() {
            return Err("Nilai parameter hilang".into());
        }
        match args[i].as_str() {
            "--count" => count = args[i + 1].parse()?,
            "--interval-ms" => ms = args[i + 1].parse()?,
            "--disk" => disk_name = Some(&args[i + 1]),
            "--freshness-ms" => freshness_ms = args[i + 1].parse()?,
            _ => return Err("Parameter tidak dikenal".into()),
        }
        i += 2
    }
    let interval = Duration::from_millis(ms);
    validate_sampling(count, interval)?;
    if !json {
        println!("timestamp_unix_ms,elapsed_ms,cpu_util_pct,logical_cpus,mem_available_bytes,cpu_psi_some_pct,io_psi_some_pct,cgroup_cpu_usage_us,cgroup_memory_bytes,cgroup_read_bytes,cgroup_write_bytes,disk_major,disk_minor,disk_read_delta_bytes,disk_write_delta_bytes,gpu_cards,thermal_sensors,power_supplies,snapshot_ns,memory_snapshot_ns,memory_psi_some_pct,pgfault_total,pgmajfault_total");
    }
    let mut previous: Option<xynoos_pressureiq::system::SystemSnapshot> = None;
    for k in 0..count {
        if k > 0 {
            std::thread::sleep(interval)
        }
        let s = collect_system();
        if json {
            let contract = xynoos_pressureiq::system::contract::build(
                &s,
                previous.as_ref(),
                disk_name,
                std::time::Instant::now(),
                Duration::from_millis(freshness_ms),
            );
            println!("{}", serde_json::to_string(&contract)?);
            previous = Some(s);
            continue;
        }
        let elapsed = previous
            .as_ref()
            .and_then(|p| s.monotonic.checked_duration_since(p.monotonic));
        let cpu = previous
            .as_ref()
            .zip(s.cpu.as_ref().ok())
            .and_then(|(p, c)| {
                p.cpu
                    .as_ref()
                    .ok()
                    .and_then(|old| elapsed.and_then(|d| cpu_utilization(old, c, d).ok()))
            });
        // No disk is representative by default: the caller must choose one.
        let disk = disk_name.and_then(|name| {
            s.disks
                .as_ref()
                .ok()
                .and_then(|v| v.iter().find(|d| d.name == name))
        });
        let delta = previous
            .as_ref()
            .zip(disk)
            .filter(|_| elapsed.is_some_and(|d| !d.is_zero()))
            .and_then(|(p, d)| {
                p.disks
                    .as_ref()
                    .ok()
                    .and_then(|v| {
                        v.iter()
                            .find(|x| x.major == d.major && x.minor == d.minor && x.name == d.name)
                    })
                    .and_then(|old| disk_delta(old, d).ok())
            });
        let opt = |v: Option<u64>| v.map(|n| n.to_string()).unwrap_or_default();
        let cg = s.cgroup.as_ref().ok();
        let io = cg.and_then(|c| c.io_bytes.as_ref().ok());
        let ts = s
            .timestamp
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            ts,
            opt(elapsed.map(|d| d.as_millis() as u64)),
            cpu.map(|v| format!("{v:.3}")).unwrap_or_default(),
            opt(s.cpu.as_ref().ok().map(|v| v.logical_cpus as u64)),
            opt(s
                .memory
                .mem
                .result
                .as_ref()
                .ok()
                .and_then(|m| m.bytes.get("MemAvailable").copied())),
            s.cpu_psi
                .as_ref()
                .ok()
                .map(|p| p.some.avg10.to_string())
                .unwrap_or_default(),
            s.io_psi
                .as_ref()
                .ok()
                .map(|p| p.some.avg10.to_string())
                .unwrap_or_default(),
            opt(cg.and_then(|c| c.cpu_usage_us.as_ref().ok().copied())),
            opt(cg.and_then(|c| c.memory_current_bytes.as_ref().ok().copied())),
            opt(io.map(|x| x.0)),
            opt(io.map(|x| x.1)),
            opt(disk.map(|d| d.major as u64)),
            opt(disk.map(|d| d.minor as u64)),
            opt(delta.map(|d| d.0)),
            opt(delta.map(|d| d.1)),
            s.gpu
                .iter()
                .filter(|x| x.source.ends_with("/device/vendor"))
                .count(),
            s.thermal
                .iter()
                .filter(|x| x.source.ends_with("/temp"))
                .count(),
            s.power
                .iter()
                .filter(|x| x.source.ends_with("/type"))
                .count(),
            s.elapsed_ns,
            s.memory.elapsed_ns,
            s.memory
                .psi
                .result
                .as_ref()
                .ok()
                .map(|p| p.some.avg10.to_string())
                .unwrap_or_default(),
            opt(s
                .memory
                .vm
                .result
                .as_ref()
                .ok()
                .and_then(|v| v.counters.get("pgfault").copied())),
            opt(s
                .memory
                .vm
                .result
                .as_ref()
                .ok()
                .and_then(|v| v.counters.get("pgmajfault").copied()))
        );
        previous = Some(s);
    }
    Ok(())
}

fn shadow_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() != 3 {
        return Err("usage: shadow COUNT INTERVAL_MS".into());
    }
    let count: u32 = args[1].parse()?;
    let interval = Duration::from_millis(args[2].parse()?);
    validate_sampling(count, interval)?;
    let mut engine = xynoos_decision::Engine::default();
    for i in 0..count {
        if i > 0 {
            std::thread::sleep(interval);
        }
        let snapshot = xynoos_pressureiq::system::collect_system();
        let target = snapshot.cgroup.as_ref().ok().map(|c| c.path.as_path());
        let decision = engine.evaluate(&snapshot, std::time::Instant::now(), target);
        println!("{}", serde_json::to_string(&decision)?);
    }
    Ok(())
}

fn memory_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    use std::time::Instant;
    use xynoos_pressureiq::intelligence::{collect, diagnose, Observation};
    let mut human = false;
    let mut replay = None;
    let mut ms = 1000u64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--human" => human = true,
            "--interval-ms" => { i += 1; ms = args.get(i).ok_or("missing interval")?.parse()?; },
            "--replay" => { i += 1; replay = Some(args.get(i).ok_or("missing replay file")?); },
            _ => return Err("usage: memory [--interval-ms 100..60000] [--human] | memory --replay FILE [--human]".into()),
        }
        i += 1;
    }
    xynoos_pressureiq::validate_sampling(2, Duration::from_millis(ms))?;
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Evidence {
        first: Observation,
        second: Observation,
        interval_ms: u64,
        age_ms: u64,
    }
    let evidence = if let Some(path) = replay {
        use std::io::Read;
        let mut text = String::new();
        std::fs::File::open(path)?
            .take(1_048_577)
            .read_to_string(&mut text)?;
        if text.len() > 1_048_576 {
            return Err("evidence exceeds 1 MiB".into());
        }
        serde_json::from_str::<Evidence>(&text)?
    } else {
        let start = Instant::now();
        let first = collect();
        std::thread::sleep(Duration::from_millis(ms));
        let second_start = Instant::now();
        let second = collect();
        Evidence {
            first,
            second,
            interval_ms: second_start.duration_since(start).as_millis() as u64,
            age_ms: second_start.elapsed().as_millis() as u64,
        }
    };
    let diagnosis = diagnose(
        &evidence.first,
        &evidence.second,
        Duration::from_millis(evidence.interval_ms),
        Duration::from_millis(evidence.age_ms),
    );
    let decision = xynoos_aura::memory::decide(&diagnosis);
    if human {
        println!(
            "Memory intelligence: {} (confidence: {})",
            diagnosis.classes.join(", "),
            decision.confidence
        );
        for reason in &diagnosis.evidence {
            println!("  Evidence: {reason}");
        }
        for reason in &diagnosis.limitations {
            println!("  Limitation: {reason}");
        }
        println!(
            "  Available: {:?}% | rates (pages/s, allocstall events/s): {:?}",
            diagnosis.available_pct, diagnosis.rates_per_second
        );
        println!(
            "  AURA: {:?} — {}",
            decision.action,
            decision.recommendations.join(" ")
        );
        println!("  Read-only; no kernel or workload changes.");
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"schema_version":1,"evidence":evidence,"diagnosis":diagnosis,"aura":decision})
            )?
        );
    }
    Ok(())
}

fn daemon_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    let mut mode = xynoos_aura::DaemonMode::ObserveOnly;
    let mut interval_ms = 5000u64;
    let mut max_cycles = None;
    let mut human = false;
    let mut status = false;
    let mut pid_file: Option<PathBuf> = None;
    let mut no_pid_file = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                i += 1;
                match args.get(i).map(String::as_str) {
                    Some("observe-only") => mode = xynoos_aura::DaemonMode::ObserveOnly,
                    Some("advisory") => mode = xynoos_aura::DaemonMode::Advisory,
                    Some("transactional") => mode = xynoos_aura::DaemonMode::Transactional,
                    _ => return Err("Invalid mode. Allowed: observe-only, advisory, transactional".into()),
                }
            }
            "--interval-ms" => {
                i += 1;
                interval_ms = args.get(i).ok_or("missing interval-ms")?.parse()?;
            }
            "--max-cycles" => {
                i += 1;
                max_cycles = Some(args.get(i).ok_or("missing max-cycles")?.parse()?);
            }
            "--pid-file" => {
                i += 1;
                pid_file = Some(PathBuf::from(args.get(i).ok_or("missing pid-file")?));
            }
            "--no-pid-file" => no_pid_file = true,
            "--status" => status = true,
            "--human" => human = true,
            _ => return Err("usage: daemon [--mode observe-only|advisory|transactional] [--interval-ms N] [--max-cycles N] [--pid-file PATH | --no-pid-file] [--status] [--human]".into()),
        }
        i += 1;
    }

    let resolved_pid_file = if no_pid_file {
        None
    } else {
        pid_file.or_else(|| {
            // Bounded non-privileged runs without explicit --pid-file avoid /run permission errors
            if max_cycles.is_some() {
                None
            } else {
                Some(PathBuf::from("/run/xynoos/hypercore.pid"))
            }
        })
    };

    let config = xynoos_aura::DaemonConfig {
        mode,
        interval_ms,
        pid_file: resolved_pid_file.clone(),
        ..Default::default()
    };

    if status {
        let init_system = if std::path::Path::new("/run/systemd/system").is_dir() {
            "systemd".to_string()
        } else {
            std::fs::read_to_string("/proc/1/comm")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        };
        let cgroup_v2_available =
            std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "configured",
                "mode": config.mode,
                "supported_modes": ["observe-only", "advisory", "transactional"],
                "interval_ms": config.interval_ms,
                "pid_file": config.pid_file,
                "journal_file": config.journal_file,
                "adaptive_polling": config.adaptive_polling,
                "init_system": init_system,
                "cgroup_v2_available": cgroup_v2_available,
                "psi_available": {
                    "cpu": std::path::Path::new("/proc/pressure/cpu").exists(),
                    "memory": std::path::Path::new("/proc/pressure/memory").exists(),
                    "io": std::path::Path::new("/proc/pressure/io").exists(),
                },
            }))?
        );
        return Ok(());
    }

    let _pid_guard = match resolved_pid_file {
        Some(path) => Some(xynoos_aura::PidLock::acquire(path).map_err(Box::<dyn Error>::from)?),
        None => None,
    };

    let mut daemon = xynoos_aura::AuraDaemon::new(config)?;
    let stop_signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let diag = daemon
        .run_with_signal(stop_signal, max_cycles)
        .map_err(Box::<dyn Error>::from)?;

    if human {
        println!("AURA Daemon Status: {:?}", diag.mode);
        println!("  Uptime: {} s", diag.uptime_secs);
        println!(
            "  Cycles: {} completed, {} failed",
            diag.cycles_completed, diag.cycles_failed
        );
        println!("  Effective interval: {} ms", diag.effective_interval_ms);
        println!("  RSS bytes: {:?}", diag.rss_bytes);
        println!("  Last pressure: {:?}", diag.last_raw_pressure);
        println!("  Last recommendation: {:?}", diag.last_recommendation);
    } else {
        println!("{}", serde_json::to_string_pretty(&diag)?);
    }
    Ok(())
}

fn profile_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() {
        return Err("usage: profile record [--output FILE] [--count N] | profile replay --trace FILE [--human] | profile calibrate --trace FILE [--human]".into());
    }
    match args[0].as_str() {
        "record" => {
            let mut count = 5usize;
            let mut interval_ms = 1000u64;
            let mut output = PathBuf::from("workload_trace.json");
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--count" => {
                        i += 1;
                        count = args.get(i).ok_or("missing count")?.parse()?;
                    }
                    "--interval-ms" => {
                        i += 1;
                        interval_ms = args.get(i).ok_or("missing interval-ms")?.parse()?;
                    }
                    "--output" => {
                        i += 1;
                        output = PathBuf::from(args.get(i).ok_or("missing output path")?);
                    }
                    _ => {
                        return Err(
                            "usage: profile record [--count N] [--interval-ms N] [--output FILE]"
                                .into(),
                        )
                    }
                }
                i += 1;
            }

            let mut trace = xynoos_aura::WorkloadTrace::new("cli-recorded-workload".into());
            for step in 0..count {
                let sys = xynoos_pressureiq::system::collect_system();
                let mem_map = sys.memory.mem.result.as_ref().ok();
                let mem_total = mem_map
                    .and_then(|m| m.bytes.get("MemTotal"))
                    .copied()
                    .unwrap_or(0);
                let mem_avail = mem_map
                    .and_then(|m| m.bytes.get("MemAvailable"))
                    .copied()
                    .unwrap_or(0);
                let avail_pct = if mem_total > 0 && mem_avail <= mem_total {
                    (mem_avail as f64) * 100.0 / (mem_total as f64)
                } else {
                    0.0
                };
                let psi_some = sys
                    .memory
                    .psi
                    .result
                    .as_ref()
                    .ok()
                    .map(|p| p.some.avg10)
                    .unwrap_or(0.0);
                let psi_full = sys
                    .memory
                    .psi
                    .result
                    .as_ref()
                    .ok()
                    .and_then(|p| p.full.as_ref().map(|f| f.avg10))
                    .unwrap_or(0.0);
                let cgroup_usage = sys
                    .cgroup
                    .as_ref()
                    .ok()
                    .and_then(|c| c.memory_current_bytes.as_ref().ok().copied())
                    .unwrap_or(0);

                let diag = xynoos_aura::TraceRecord {
                    step_index: step as u64,
                    relative_timestamp_ms: (step as u64) * interval_ms,
                    mem_total_bytes: mem_total,
                    mem_available_bytes: mem_avail,
                    available_pct: avail_pct,
                    psi_some_avg10: psi_some,
                    psi_full_avg10: psi_full,
                    swap_thrashing: false,
                    severe_psi: psi_full >= 1.0 || psi_some >= 10.0,
                    high_page_fault_rate: false,
                    cgroup_usage_bytes: cgroup_usage,
                };
                trace.append(diag);
                if step + 1 < count {
                    std::thread::sleep(Duration::from_millis(interval_ms));
                }
            }
            trace.save_to_file(&output)?;
            println!(
                "Recorded {} trace steps to {}",
                trace.total_steps,
                output.display()
            );
            Ok(())
        }
        "replay" => {
            let mut trace_path = None;
            let mut human = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--trace" => {
                        i += 1;
                        trace_path = Some(PathBuf::from(args.get(i).ok_or("missing trace path")?));
                    }
                    "--human" => human = true,
                    _ => return Err("usage: profile replay --trace FILE [--human]".into()),
                }
                i += 1;
            }
            let path = trace_path.ok_or("Missing --trace FILE")?;
            let trace = xynoos_aura::WorkloadTrace::load_from_file(&path)?;

            let tier_coord = xynoos_aura::AuraTieringCoordinator::new(
                xynoos_aura::AdaptiveTieringPolicy::default(),
            );
            let kernel_coord = xynoos_aura::AuraKernelCoordinator::new(tier_coord);
            let mut store = xynoos_hypermemory::PageStore::with_capacity(64)?;

            let report = xynoos_aura::ShadowPlayer::replay(&trace, &kernel_coord, &mut store)?;
            if human {
                println!(
                    "Shadow Profiler Replay Report: {}",
                    report.workload_identifier
                );
                println!("  Steps replayed: {}", report.total_steps_replayed);
                println!("  Duration: {} ms", report.simulated_duration_ms);
                println!("  Peak pressure: {:?}", report.peak_pressure);
                println!("  Migrations: {}", report.total_pages_migrated);
                println!(
                    "  Space saved: {} bytes",
                    report.simulated_space_saved_bytes
                );
                println!(
                    "  Compression ratio: {:.3}",
                    report.simulated_compression_ratio
                );
                println!(
                    "  Advisory recommendations: {}",
                    report.advisory_tuning_recommendations_count
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            Ok(())
        }
        "calibrate" => {
            let mut trace_path = None;
            let mut human = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--trace" => {
                        i += 1;
                        trace_path = Some(PathBuf::from(args.get(i).ok_or("missing trace path")?));
                    }
                    "--human" => human = true,
                    _ => return Err("usage: profile calibrate --trace FILE [--human]".into()),
                }
                i += 1;
            }
            let path = trace_path.ok_or("Missing --trace FILE")?;
            let trace = xynoos_aura::WorkloadTrace::load_from_file(&path)?;
            let sample_pages: Vec<Vec<u8>> = (0..16)
                .map(|p| {
                    let mut v = Vec::with_capacity(4096);
                    for row in 0..128 {
                        let s = format!(
                            "{{\"page\":{},\"row\":{},\"workload\":\"shadow\"}}\n",
                            p, row
                        );
                        v.extend_from_slice(s.as_bytes());
                    }
                    v.resize(4096, b' ');
                    v
                })
                .collect();
            let report = xynoos_aura::ShadowCalibrator::calibrate(&trace, &sample_pages)
                .map_err(Box::<dyn Error>::from)?;
            if human {
                println!(
                    "Shadow Calibration Report (advisory_only={}, kind={})",
                    report.advisory_only, report.measurement_kind
                );
                println!(
                    "  Splits: cal={}, val={}, eval={} (overfitting_guard={})",
                    report.calibration_steps,
                    report.validation_steps,
                    report.evaluation_steps,
                    report.overfitting_guard_triggered
                );
                for cmp in &report.evaluation_comparisons {
                    println!(
                        "  {} -> net_savings={} B, transitions={}, utility={}",
                        cmp.policy_name,
                        cmp.net_savings_bytes,
                        cmp.total_transitions,
                        cmp.utility_score
                    );
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            Ok(())
        }
        _ => Err("usage: profile record | profile replay | profile calibrate".into()),
    }
}
