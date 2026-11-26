use std::{
    error::Error,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use xynoos_pressureiq::{
    collect, csv_row, sample, validate_sampling, Error as PressureError, CSV_HEADER,
};
mod intelligence_cli;
mod product;
mod signals;

use intelligence_cli::{
    doctor_command, inspect_command, runtime_command, setup_command, tuning_command,
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let exe = std::env::args().next().unwrap_or_default();
    if exe.ends_with("xynoosd") {
        return daemon_command(&args);
    }

    if args.first().map(String::as_str) == Some("doctor") {
        return doctor_command(&args[1..]);
    }

    if args.first().map(String::as_str) == Some("inspect") {
        return inspect_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("runtime") {
        return runtime_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("tuning") {
        return tuning_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("setup") {
        return setup_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("transaction") {
        return transaction_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("config") {
        return config_command(&args[1..]);
    }
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

fn config_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() || args[0] != "validate" {
        return Err("usage: xynoos config validate [--file PATH] [--json]".into());
    }
    let mut file: Option<PathBuf> = None;
    let mut json_out = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--file" => {
                i += 1;
                file = Some(PathBuf::from(args.get(i).ok_or("missing file path")?));
            }
            "--json" => json_out = true,
            _ => return Err("usage: xynoos config validate [--file PATH] [--json]".into()),
        }
        i += 1;
    }

    let config_res = if let Some(path) = file {
        xynoos_aura::DaemonConfig::from_file(&path)
    } else {
        let default_path = Path::new("/etc/xynoos/hypercore.json");
        if default_path.exists() {
            xynoos_aura::DaemonConfig::from_file(default_path)
        } else {
            let def = xynoos_aura::DaemonConfig::default();
            def.validate().map(|_| def)
        }
    };

    match config_res {
        Ok(cfg) => {
            if json_out {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "valid",
                        "schema_version": cfg.schema_version,
                        "config": cfg
                    }))?
                );
            } else {
                println!(
                    "✓ Configuration is valid (schema_version {})",
                    cfg.schema_version
                );
                println!("  Mode: {:?}", cfg.mode);
                println!(
                    "  Interval: {} ms (max backoff {} ms)",
                    cfg.interval_ms, cfg.max_backoff_ms
                );
                println!("  Adaptive polling: {}", cfg.adaptive_polling);
                if let Some(ref p) = cfg.pid_file {
                    println!("  PID file: {}", p.display());
                }
                if let Some(ref j) = cfg.journal_file {
                    println!("  Recovery journal: {}", j.display());
                }
            }
            Ok(())
        }
        Err(e) => {
            if json_out {
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "invalid",
                        "error": e.to_string()
                    }))?
                );
            } else {
                eprintln!("✗ Configuration invalid: {e}");
            }
            Err("configuration validation failed".into())
        }
    }
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
            "run" => {}
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
            "--json" => human = false,
            _ => return Err("usage: daemon [--mode observe-only|advisory|transactional] [--interval-ms N] [--max-cycles N] [--pid-file PATH | --no-pid-file] [--status] [--human] [--json]".into()),
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

    signals::install_daemon_signals()?;
    signals::reset_stop_requested();

    let mut daemon = xynoos_aura::AuraDaemon::new(config)?;
    let stop_signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_watcher = std::sync::Arc::clone(&stop_signal);
    let watcher_handle = std::thread::spawn(move || {
        while !stop_watcher.load(std::sync::atomic::Ordering::SeqCst) {
            if signals::is_stop_requested() {
                stop_watcher.store(true, std::sync::atomic::Ordering::SeqCst);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    let diag = daemon
        .run_with_signal(stop_signal, max_cycles)
        .map_err(Box::<dyn Error>::from)?;

    signals::request_stop();
    let _ = watcher_handle.join();

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

fn transaction_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    use xynoos_aura::xynoos_linux_control::transaction::{
        Backend, DisposableCgroup, Outcome, SustainedLease, WorkloadCgroup,
    };
    use xynoos_aura::xynoos_linux_control::Action;

    if args.is_empty() || args[0] != "qualify" {
        return Err(
            "usage: transaction qualify [--parent PATH] [--journal PATH] [--workload-bytes N] [--lease-duration-ms N] [--json] [--human]"
                .into(),
        );
    }

    // 1. Rootless enforcement: must never execute as root
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return Err(
            "root execution refused: transactional qualification requires a non-root delegated cgroup"
                .into(),
        );
    }

    let mut parent_path: Option<PathBuf> = None;
    let mut journal_path: Option<PathBuf> = None;
    let mut workload_bytes = 16 * 1024 * 1024u64; // Default 16 MiB bounded workload
    let mut target_lease_hold_ms = 50u64; // Default 50ms sustained lease hold
    let mut json_out = false;
    let mut human_out = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--parent" => {
                i += 1;
                parent_path = Some(PathBuf::from(args.get(i).ok_or("missing --parent path")?));
            }
            "--journal" => {
                i += 1;
                journal_path = Some(PathBuf::from(args.get(i).ok_or("missing --journal path")?));
            }
            "--workload-bytes" => {
                i += 1;
                workload_bytes = args
                    .get(i)
                    .ok_or("missing --workload-bytes value")?
                    .parse()?;
                if workload_bytes > 32 * 1024 * 1024 {
                    return Err("workload-bytes exceeds safe qualification ceiling (32 MiB)".into());
                }
            }
            "--lease-duration-ms" => {
                i += 1;
                target_lease_hold_ms = args
                    .get(i)
                    .ok_or("missing --lease-duration-ms value")?
                    .parse()?;
                if target_lease_hold_ms > 10000 {
                    return Err(
                        "lease-duration-ms exceeds safe qualification ceiling (10000 ms)".into(),
                    );
                }
            }
            "--json" => json_out = true,
            "--human" => human_out = true,
            _ => return Err(format!("Unknown option: {}", args[i]).into()),
        }
        i += 1;
    }

    // Discover parent cgroup if not explicitly provided
    let parent = match parent_path {
        Some(p) => p,
        None => {
            let user_app_slice = PathBuf::from(format!(
                "/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service/app.slice"
            ));
            if user_app_slice.exists() {
                user_app_slice
            } else {
                return Err(
                    "BLOCKED: delegated cgroup parent not found; specify --parent explicitly"
                        .into(),
                );
            }
        }
    };

    if !parent.is_dir() {
        return Err(format!(
            "BLOCKED: parent cgroup directory '{}' does not exist",
            parent.display()
        )
        .into());
    }

    // Check subtree_control contains memory
    let subtree = std::fs::read_to_string(parent.join("cgroup.subtree_control"))
        .map_err(|e| format!("BLOCKED: unable to read parent cgroup.subtree_control: {e}"))?;
    if !subtree.split_whitespace().any(|c| c == "memory") {
        return Err(
            "BLOCKED: memory controller is not delegated in parent cgroup.subtree_control".into(),
        );
    }

    // Stage 1: Empty Disposable Cgroup Probe Transaction (DisposableCgroup + transaction::run)
    let mut probe_cg = DisposableCgroup::create(&parent, true)
        .map_err(|e| format!("Stage 1 creation failed: {e}"))?;
    let probe_orig_low = probe_cg
        .read_low()
        .map_err(|e| format!("Stage 1 read_low failed: {e}"))?;
    let probe_orig_high = probe_cg
        .read_high()
        .map_err(|e| format!("Stage 1 read_high failed: {e}"))?;
    let snap = xynoos_pressureiq::system::collect_system();
    let probe_action = Action::MemoryLow {
        bytes: 8 * 1024 * 1024,
    };
    let probe_rep = xynoos_aura::xynoos_linux_control::transaction::run(
        &mut probe_cg,
        &snap,
        probe_action,
        true,
    );
    if probe_rep.outcome != Outcome::Restored {
        return Err(format!(
            "Stage 1 probe transaction failed: outcome={:?}, events={:?}",
            probe_rep.outcome, probe_rep.events
        )
        .into());
    }
    let probe_restored_low = probe_cg
        .read_low()
        .map_err(|e| format!("Stage 1 final readback failed: {e}"))?;
    if probe_restored_low != probe_orig_low {
        return Err(format!(
            "Stage 1 restoration mismatch: expected {probe_orig_low}, got {probe_restored_low}"
        )
        .into());
    }
    probe_cg
        .cleanup()
        .map_err(|e| format!("Stage 1 cleanup failed: {e}"))?;

    // Stage 2: Bounded Workload Durable Lease Qualification (WorkloadCgroup + SustainedLease)
    let rand_id = format!(
        "{:08x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let child_name = format!("xynoos-qual-workload-{}", rand_id);
    let child_path = parent.join(&child_name);
    std::fs::create_dir(&child_path).map_err(|e| format!("Stage 2 mkdir failed: {e}"))?;

    // RAII guard to guarantee cleanup of child directory
    struct ChildDirGuard(PathBuf);
    impl Drop for ChildDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    let _dir_guard = ChildDirGuard(child_path.clone());

    let mut wl_backend = WorkloadCgroup::open(&child_path, true)
        .map_err(|e| format!("Stage 2 open WorkloadCgroup failed: {e}"))?;
    let wl_orig_low = wl_backend
        .read_low()
        .map_err(|e| format!("Stage 2 read_low failed: {e}"))?;
    let wl_orig_high = wl_backend
        .read_high()
        .map_err(|e| format!("Stage 2 read_high failed: {e}"))?;

    // Spawn bounded worker process using fork to preserve base libc6 compatibility (< 2.39)
    // Bidirectional pipes ensure memory page allocation is charged to the child cgroup
    let mut p2c_fds = [0i32; 2];
    let mut c2p_fds = [0i32; 2];
    if unsafe { libc::pipe(p2c_fds.as_mut_ptr()) } != 0 {
        return Err("Failed to create parent->child pipe for worker".into());
    }
    if unsafe { libc::pipe(c2p_fds.as_mut_ptr()) } != 0 {
        unsafe {
            libc::close(p2c_fds[0]);
            libc::close(p2c_fds[1]);
        }
        return Err("Failed to create child->parent pipe for worker".into());
    }

    let child_pid = unsafe { libc::fork() };
    if child_pid < 0 {
        unsafe {
            libc::close(p2c_fds[0]);
            libc::close(p2c_fds[1]);
            libc::close(c2p_fds[0]);
            libc::close(c2p_fds[1]);
        }
        return Err("Failed to fork qualification worker".into());
    }

    if child_pid == 0 {
        // Child process
        unsafe {
            libc::close(p2c_fds[1]);
            libc::close(c2p_fds[0]);
        }
        // Wait for parent to move this process to the child cgroup.procs
        let mut start_token = [0u8; 1];
        let n_start =
            unsafe { libc::read(p2c_fds[0], start_token.as_mut_ptr() as *mut libc::c_void, 1) };
        unsafe {
            libc::close(p2c_fds[0]);
        }
        if n_start != 1 {
            unsafe {
                libc::_exit(1);
            }
        }

        // Allocate bounded workload memory and touch pages *after* cgroup attachment
        // so Linux cgroup v2 memory charging accounts for these resident pages.
        let mut buf = vec![0u8; workload_bytes as usize];
        for chunk in buf.chunks_mut(4096) {
            chunk[0] = 42;
        }
        // Signal readiness to parent
        let ready = [1u8];
        unsafe {
            let _ = libc::write(c2p_fds[1], ready.as_ptr() as *const libc::c_void, 1);
            libc::close(c2p_fds[1]);
        }
        // Sleep until terminated by parent
        loop {
            unsafe {
                libc::pause();
            }
        }
    }

    // Parent process
    unsafe {
        libc::close(p2c_fds[0]);
        libc::close(c2p_fds[1]);
    }

    let worker_pid = child_pid as u32;

    // Attach worker to workload cgroup BEFORE memory faulting begins
    if let Err(e) = std::fs::write(child_path.join("cgroup.procs"), worker_pid.to_string()) {
        unsafe {
            libc::close(p2c_fds[1]);
            libc::close(c2p_fds[0]);
            libc::kill(child_pid, libc::SIGKILL);
            libc::waitpid(child_pid, std::ptr::null_mut(), 0);
        }
        return Err(format!("Failed to move worker to cgroup: {e}").into());
    }

    // Signal worker that cgroup placement is complete
    let start_signal = [1u8];
    let n_sig = unsafe { libc::write(p2c_fds[1], start_signal.as_ptr() as *const libc::c_void, 1) };
    unsafe {
        libc::close(p2c_fds[1]);
    }
    if n_sig != 1 {
        unsafe {
            libc::close(c2p_fds[0]);
            libc::kill(child_pid, libc::SIGKILL);
            libc::waitpid(child_pid, std::ptr::null_mut(), 0);
        }
        return Err("Failed to signal worker to begin memory allocation".into());
    }

    // Await worker allocation readiness
    let mut ready_buf = [0u8; 1];
    let n = unsafe { libc::read(c2p_fds[0], ready_buf.as_mut_ptr() as *mut libc::c_void, 1) };
    unsafe {
        libc::close(c2p_fds[0]);
    }
    if n != 1 {
        unsafe {
            libc::kill(child_pid, libc::SIGKILL);
            libc::waitpid(child_pid, std::ptr::null_mut(), 0);
        }
        return Err("Qualification worker failed to signal readiness".into());
    }

    let resident_bytes = wl_backend.read_current_bytes().unwrap_or(0);

    // Durable lease execution
    let journal = journal_path.unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "xynoos_qual_journal_{}_{}.json",
            std::process::id(),
            rand_id
        ))
    });
    let _ = std::fs::remove_file(&journal);

    let lease_action = Action::MemoryLow {
        bytes: 8 * 1024 * 1024,
    };
    let lease_t0 = std::time::Instant::now();
    let mut lease =
        SustainedLease::acquire_durable(&mut wl_backend, &snap, lease_action, true, &journal)
            .map_err(|r| {
                unsafe {
                    libc::kill(child_pid, libc::SIGKILL);
                    libc::waitpid(child_pid, std::ptr::null_mut(), 0);
                }
                format!(
                    "acquire_durable failed: outcome={:?}, events={:?}",
                    r.outcome, r.events
                )
            })?;

    let lease_applied_low = wl_backend.read_low()?;
    let journal_persisted = journal.exists();

    // Sustain lease hold for specified duration
    if target_lease_hold_ms > 0 {
        std::thread::sleep(Duration::from_millis(target_lease_hold_ms));
    }

    // Release lease
    let release_rep = lease.release(&mut wl_backend);
    let lease_duration_ms = lease_t0.elapsed().as_millis() as u64;
    let lease_restored_low = wl_backend.read_low()?;
    let journal_cleaned = !journal.exists();

    // Terminate worker
    unsafe {
        libc::kill(child_pid, libc::SIGKILL);
        libc::waitpid(child_pid, std::ptr::null_mut(), 0);
    }

    // Verification asserts
    if release_rep.outcome != Outcome::Restored {
        return Err(format!(
            "Stage 2 lease release failed: outcome={:?}, events={:?}",
            release_rep.outcome, release_rep.events
        )
        .into());
    }
    if lease_restored_low != wl_orig_low {
        return Err(format!(
            "Stage 2 restoration mismatch: expected {wl_orig_low}, got {lease_restored_low}"
        )
        .into());
    }
    if !journal_persisted || !journal_cleaned {
        return Err(format!(
            "Journal lifecycle failed: persisted={journal_persisted}, cleaned={journal_cleaned}"
        )
        .into());
    }

    // Explicitly remove directory before guard drops
    std::mem::forget(_dir_guard);
    std::fs::remove_dir(&child_path).map_err(|e| format!("Stage 2 rmdir failed: {e}"))?;

    let evidence = serde_json::json!({
        "schema_version": 1,
        "verdict": "PASS",
        "parent_cgroup": parent.display().to_string(),
        "stage1_probe": {
            "cgroup_type": "DisposableCgroup",
            "original_low_bytes": probe_orig_low,
            "original_high_bytes": probe_orig_high,
            "action_applied": "memory.low 8388608",
            "outcome": format!("{:?}", probe_rep.outcome),
            "restored_low_bytes": probe_restored_low,
            "disposable_cleaned": true
        },
        "stage2_workload_lease": {
            "cgroup_type": "WorkloadCgroup",
            "disposable_cgroup": child_path.display().to_string(),
            "worker_pid": worker_pid,
            "worker_workload_bytes": workload_bytes,
            "worker_resident_bytes": resident_bytes,
            "original_low_bytes": wl_orig_low,
            "original_high_bytes": wl_orig_high,
            "action_applied": "memory.low 8388608",
            "lease_applied_readback_bytes": lease_applied_low,
            "lease_release_outcome": format!("{:?}", release_rep.outcome),
            "restored_low_bytes": lease_restored_low,
            "lease_duration_ms": lease_duration_ms,
            "journal_persisted_verified": journal_persisted,
            "journal_cleaned_verified": journal_cleaned,
            "worker_terminated": true,
            "cgroup_removed": true
        },
        "isolation_verified": true,
        "restoration_verified": true
    });

    if human_out || (!json_out && std::io::stdout().is_terminal()) {
        println!("XYNOOS HyperCore — Controlled Physical Transactional Qualification");
        println!("  Parent cgroup: {}", parent.display());
        println!(
            "  Stage 1 (Disposable probe): outcome={:?}, restored_low={}",
            probe_rep.outcome, probe_restored_low
        );
        println!(
            "  Stage 2 (Workload lease): worker_pid={}, resident_bytes={}",
            worker_pid, resident_bytes
        );
        println!(
            "    Original low: {} B -> Applied: {} B -> Restored: {} B",
            wl_orig_low, lease_applied_low, lease_restored_low
        );
        println!(
            "    Journal: persisted={}, cleaned={}",
            journal_persisted, journal_cleaned
        );
        println!("    Duration: {} ms", lease_duration_ms);
        println!("  Cleanup: worker terminated, child cgroup removed");
        println!("  Verdict: PASS");
    } else {
        println!("{}", serde_json::to_string_pretty(&evidence)?);
    }

    Ok(())
}
