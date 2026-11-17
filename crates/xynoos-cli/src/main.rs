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
    if args.first().map(String::as_str) == Some("memory") {
        return memory_command(&args[1..]);
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
