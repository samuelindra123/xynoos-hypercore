//! Fixed owned workload and component profiler. No control interfaces or shell execution.
#![forbid(unsafe_code)]
use serde_json::json;
use std::{
    hint::black_box,
    io::{self, BufRead, Write},
    time::{Duration, Instant},
};
use xynoos_engine::{Engine, Mode};

fn timed(mut f: impl FnMut(), count: usize) -> Vec<u128> {
    (0..count)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_nanos()
        })
        .collect()
}
fn profile() -> Result<(), Box<dyn std::error::Error>> {
    let collection = timed(
        || {
            black_box(xynoos_pressureiq::system::collect_system());
        },
        24,
    );
    let trace = xynoos_engine::replay::default_trace();
    let (mut snapshot, now) = trace.steps[0].snapshot(Instant::now())?;
    let mut engine = Engine::default();
    let mut tick = now;
    let state = timed(
        || {
            tick += Duration::from_millis(100);
            snapshot.monotonic = tick;
            snapshot.memory.monotonic = tick;
            black_box(engine.evaluate(&snapshot, tick, Mode::Observe, false, false));
        },
        4000,
    );
    use xynoos_aura::{Baseline, DecisionPolicy};
    let aura = timed(
        || {
            black_box(Baseline.decide(
                black_box(xynoos_core::MemoryPressure::High),
                black_box(xynoos_core::CostEstimate {
                    latency_ns: 0,
                    bytes: 4096,
                }),
            ));
        },
        4000,
    );
    let page = vec![17u8; 4096];
    let hypermemory = timed(
        || {
            let mut store = xynoos_hypermemory::PageStore::adaptive(4096, 64, 512 * 1024).unwrap();
            let ids: Vec<_> = (0..64)
                .map(|_| store.insert(black_box(&page)).unwrap())
                .collect();
            for id in ids {
                assert_eq!(store.get(id).unwrap(), page);
            }
            black_box(store.stats());
        },
        100,
    );
    let live = xynoos_pressureiq::system::collect_system();
    let plan = engine.evaluate(&live, Instant::now(), Mode::Observe, false, false);
    let contract = xynoos_pressureiq::system::contract::build(
        &live,
        None,
        None,
        Instant::now(),
        Duration::from_secs(5),
    );
    let contract_build = timed(
        || {
            black_box(xynoos_pressureiq::system::contract::build(
                &live,
                None,
                None,
                Instant::now(),
                Duration::from_secs(5),
            ));
        },
        400,
    );
    let report = timed(
        || {
            let mut bytes = Vec::new();
            xynoos_engine::observation::write(&mut bytes, &plan, 100, &contract).unwrap();
            black_box(bytes);
        },
        1000,
    );
    println!(
        "{}",
        json!({"schema_version":1, "evidence":"local_component_timings_not_physical_benefit", "units":"ns", "pressureiq_collection":collection, "state_machine_with_aura":state, "aura_only":aura, "hypermemory_64_page_verified_roundtrip":hypermemory, "contract_build":contract_build, "report_build_serialize":report})
    );
    Ok(())
}
fn workload() -> Result<(), Box<dyn std::error::Error>> {
    // Exactly 2 MiB of primary buffers; no memory pressure proportional to host RAM.
    let data: Vec<u8> = (0..1024 * 1024)
        .map(|i| ((i * 17 + i / 4096) % 251) as u8)
        .collect();
    let mut copy = vec![0; data.len()];
    let expected = crc32fast::hash(&data);
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    writeln!(
        output,
        "{}",
        json!({"ready":true,"workload":"copy_crc_1mib_x16_v1","crc32":expected,"primary_buffer_bytes":2*data.len()})
    )?;
    output.flush()?;
    // A bounded reader rejects oversized input before allocating it in full.
    let mut line = String::with_capacity(8);
    for _ in 0..1001 {
        line.clear();
        let n = std::io::Read::take(&mut input, 16).read_line(&mut line)?;
        if n == 0 {
            return Ok(());
        }
        if line != "work\n" {
            return Err("expected bounded work command".into());
        }
        let t = Instant::now();
        for _ in 0..16 {
            copy.copy_from_slice(black_box(&data));
            if crc32fast::hash(black_box(&copy)) != expected {
                return Err("integrity failure".into());
            }
        }
        let elapsed = t.elapsed().as_nanos();
        writeln!(
            output,
            "{}",
            json!({"elapsed_ns":elapsed,"crc32":expected,"verified":true})
        )?;
        output.flush()?;
    }
    Err("request limit".into())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "profile" => profile(),
        [command] if command == "workload" => workload(),
        _ => Err("usage: xynoos-perf profile|workload; no active optimization available".into()),
    }
}
