use std::{
    error::Error,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use xynoos_pressureiq::{
    collect, csv_row, sample, validate_sampling, Error as PressureError, CSV_HEADER,
};
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() {
        let mut store = xynoos_hypermemory::PageStore::with_capacity(1)?;
        let page = vec![0; 4096];
        let id = store.insert(&page)?;
        assert_eq!(store.get(id)?, page);
        println!("Halaman {:?}: {:?}", id, store.stats());
        return Ok(());
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
