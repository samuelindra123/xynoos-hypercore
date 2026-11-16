use std::{
    error::Error,
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::{Duration, Instant},
};
use xynoos_engine::{Engine, Mode};

const HELP: &str = "mvp observe [--count 1..60] [--interval-ms 100..60000] [--human]\nmvp simulate [--trace FILE] [--journal NEW_FILE] [--human]\nmvp controlled [--authorize] [--human] (always refuses physical writes)\nmvp recovery [--journal FILE] [--human] (read-only; never replays)";
pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let command = args.first().map(String::as_str).unwrap_or("help");
    if command == "help" && args.len() <= 1 {
        println!("{HELP}");
        return Ok(());
    }
    let mut human = false;
    let mut count = 1u32;
    let mut interval = 1000u64;
    let mut trace_path = None;
    let mut journal_path = None;
    let mut authorized = false;
    let mut seen = std::collections::BTreeSet::new();
    let mut i = 1;
    while i < args.len() {
        let key = args[i].as_str();
        if !seen.insert(key) {
            return Err("duplicate option".into());
        }
        match key {
            "--human" => human = true,
            "--authorize" if command == "controlled" => authorized = true,
            "--count" | "--interval-ms" if command == "observe" => {
                i += 1;
                let value = args.get(i).ok_or("missing sampling value")?;
                if key == "--count" {
                    count = value.parse()?;
                } else {
                    interval = value.parse()?;
                }
            }
            "--trace" if command == "simulate" => {
                i += 1;
                trace_path = Some(args.get(i).ok_or("missing trace path")?);
            }
            "--journal" if command == "simulate" || command == "recovery" => {
                i += 1;
                journal_path = Some(args.get(i).ok_or("missing journal path")?);
            }
            _ => return Err(format!("unsupported option; {HELP}").into()),
        }
        i += 1;
    }
    match command {
        "observe" | "controlled" => {
            xynoos_pressureiq::validate_sampling(count, Duration::from_millis(interval))?;
            let mut engine = Engine::default();
            for n in 0..count {
                if n > 0 {
                    std::thread::sleep(Duration::from_millis(interval));
                }
                let snapshot = xynoos_pressureiq::system::collect_system();
                let started = Instant::now();
                let mode = if command == "observe" {
                    Mode::Observe
                } else {
                    Mode::Controlled
                };
                let plan = engine.evaluate(&snapshot, started, mode, authorized, false);
                let decision_ns = started.elapsed().as_nanos();
                let contract = xynoos_pressureiq::system::contract::build(
                    &snapshot,
                    None,
                    None,
                    Instant::now(),
                    Duration::from_secs(5),
                );
                if human {
                    println!("PressureIQ → AURA: {:?}; {:?}; {:?}. Kernel writes locked. No workload buffers changed. Collection {} ns; decision {} ns.",
                        plan.action, plan.reason, plan.outcome, snapshot.elapsed_ns, decision_ns);
                } else {
                    let stdout = std::io::stdout();
                    let mut output = std::io::BufWriter::new(stdout.lock());
                    xynoos_engine::observation::write(&mut output, &plan, decision_ns, &contract)?;
                    output.write_all(b"\n")?;
                    output.flush()?;
                }
            }
            if command == "controlled" {
                return Err("physical mutation admission denied: exclusive delegation and physical recovery are NOT VERIFIED".into());
            }
        }
        "simulate" => {
            let trace = match trace_path {
                Some(path) => xynoos_engine::replay::Trace::parse(&read_trace(Path::new(path))?)?,
                None => xynoos_engine::replay::default_trace(),
            };
            // The command itself explicitly authorizes work on private simulated pages.
            let report = if let Some(path) = journal_path {
                let mut journal = xynoos_engine::recovery::Journal::create(Path::new(path))?;
                xynoos_engine::replay::run_with_evidence(&trace, Mode::Simulate, true, |entry| {
                    journal.append(entry)
                })?
            } else {
                xynoos_engine::replay::run(&trace, Mode::Simulate, true)?
            };
            if human {
                println!("SIMULATION: {} trace steps; {} safety refusals; {} private-buffer verifications; zero kernel mutations.", report.rows.len(), report.safety_refusals, report.owned_buffer_verifications);
                for row in &report.rows {
                    println!(
                        "step {}: {:?} / {:?}; stored payload {}/{} bytes; verified={}",
                        row.step,
                        row.plan.action,
                        row.plan.reason,
                        row.workload.stored_payload_bytes,
                        row.workload.original_bytes,
                        row.workload.byte_verification_passed
                    );
                }
                println!("Actual userspace work, synthetic pressure. No desktop performance benefit established.");
            } else {
                println!("{}", serde_json::to_string(&report)?);
            }
        }
        "recovery" => {
            let journal = journal_path
                .map(|p| xynoos_engine::recovery::inspect_file(Path::new(p)))
                .transpose()?;
            let report = serde_json::json!({
                "schema_version":1, "read_only":true, "physical_mutation_available":false,
                "automatic_recovery_available":false, "physical_recovery":"not_implemented_locked",
                "manual_intervention_required_for_physical_state":true,
                "capabilities":xynoos_linux_control::discover(), "journal":journal,
                "explanation":"Preserve unresolved records. Fresh exact invocation, descriptor identities, expected values, worker termination and exclusive ownership are required before any separately engineered recovery. This command never restores or deletes kernel state."
            });
            if human {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", serde_json::to_string(&report)?);
            }
        }
        _ => return Err(HELP.into()),
    }
    Ok(())
}
fn read_trace(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    // Symlinks are rejected, and named pipes cannot block this CLI.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err("trace must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes)?;
    Ok(bytes)
}
