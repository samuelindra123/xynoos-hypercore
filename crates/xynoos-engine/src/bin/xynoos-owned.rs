//! Independently bounded experimental worker. No system configuration or process targeting.
#![forbid(unsafe_code)]
use nix::sys::resource::{getrlimit, setrlimit, Resource};
use serde_json::json;
use std::{
    io::{self, BufRead, Read, Write},
    time::{Duration, Instant},
};
use xynoos_engine::owned::{Corpus, CorpusKind, MAX_REQUESTS};

fn limits() -> Result<(), Box<dyn std::error::Error>> {
    for (resource, maximum) in [
        (Resource::RLIMIT_AS, 128 * 1024 * 1024),
        (Resource::RLIMIT_CPU, 8),
        (Resource::RLIMIT_CORE, 0),
    ] {
        let (_, hard) = getrlimit(resource)?;
        let value = hard.min(maximum);
        setrlimit(resource, value, value)?;
        if getrlimit(resource)? != (value, value) {
            return Err("limit read-back mismatch".into());
        }
    }
    // Independent of the harness and stdin progress, including if its parent dies.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(25));
        std::process::exit(124);
    });
    Ok(())
}
fn self_memory() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut text = String::new();
    std::fs::File::open("/proc/self/status")?
        .take(16384)
        .read_to_string(&mut text)?;
    let kib = |key: &str| {
        text.lines().find_map(|line| {
            line.strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
    };
    Ok(json!({"rss_kib":kib("VmRSS:"),"hwm_kib":kib("VmHWM:")}))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (arm, kind) = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [arm @ ("baseline" | "observe"), "mixed"] => (*arm, CorpusKind::Mixed),
        ["active", "mixed", "--authorize-owned"] => ("active", CorpusKind::Mixed),
        [arm @ ("baseline" | "observe"), "entropy"] => (*arm, CorpusKind::Entropy),
        ["active", "entropy", "--authorize-owned"] => ("active", CorpusKind::Entropy),
        _ => return Err("usage: xynoos-owned baseline|observe mixed|entropy OR active mixed|entropy --authorize-owned".into()),
    };
    limits()?;
    let mut corpus = Corpus::new(kind);
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut output = io::BufWriter::new(io::stdout().lock());
    writeln!(
        output,
        "{}",
        json!({"type":"ready", "workload":"owned_lossless_16mib_v1",
        "arm":arm, "corpus":kind, "identity":corpus.identity(), "retained_bytes":corpus.retained(),
        "limits":{"address_space_bytes":getrlimit(Resource::RLIMIT_AS)?.1,
            "cpu_seconds":getrlimit(Resource::RLIMIT_CPU)?.1,"wall_seconds":25,"requests":MAX_REQUESTS},
        "kernel_mutations":0})
    )?;
    output.flush()?;
    let mut line = String::with_capacity(16);
    for _ in 0..=MAX_REQUESTS {
        line.clear();
        let n = std::io::Read::take(&mut input, 16).read_line(&mut line)?;
        if n == 0 || line == "finish\n" || line == "cancel\n" {
            let retained = corpus.retained();
            let t = Instant::now();
            corpus.restore(line != "finish\n")?;
            writeln!(
                output,
                "{}",
                json!({"type":"final", "restored":true, "verified":true,
                "cancelled":line != "finish\n", "retained_before_restore":retained,
                "retained_after_restore":corpus.retained(), "restore_ns":t.elapsed().as_nanos(),
                "self_memory_after_restore":self_memory()?})
            )?;
            output.flush()?;
            return Ok(());
        }
        if line != "work\n" {
            return Err("invalid bounded command".into());
        }
        let t = Instant::now();
        let policy = if arm == "baseline" {
            None
        } else {
            let snapshot = xynoos_pressureiq::system::collect_system();
            Some(corpus.evaluate(&snapshot, Instant::now(), arm == "active")?)
        };
        let service = Instant::now();
        let checksum = corpus.work()?;
        writeln!(
            output,
            "{}",
            json!({"type":"response","verified":true, "checksum":checksum,
            "service_ns":service.elapsed().as_nanos(),"elapsed_ns":t.elapsed().as_nanos(),
            "retained_bytes":corpus.retained(),"policy":policy,"kernel_mutations":0})
        )?;
        output.flush()?;
    }
    Err("request limit".into())
}
