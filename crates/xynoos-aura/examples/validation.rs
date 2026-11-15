//! Eksperimen terbatas: 9 trace × 3 seed × 3 policy × 512 event.
use std::{hint::black_box, time::Instant};
use xynoos_aura::physical::{fixture, Policy};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("trace,seed,policy,refaults,restore_ns,stall_ns,migrations,compressions,cpu_ns,io_bytes,peak_ram_bytes,peak_compressed_bytes,peak_backing_bytes,fallbacks,oscillations,event_execution_ns,decision_probe_ns,profile_bytes,physical_bytes,peak_physical_bytes,temporary_bytes,capacity_violations,resident_compressed,backing_bytes");
    for (kind, name) in [
        "interactive",
        "background",
        "switching",
        "pressure",
        "entropy",
        "compressible",
        "bursty",
        "adversarial",
        "unreliable",
    ]
    .iter()
    .enumerate()
    {
        for seed in [1u64, 42, 2026] {
            let mut rng = seed;
            let trace: Vec<_> = (0..512)
                .map(|i| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    let page = match kind {
                        0 => i % 4,
                        1 => (rng % 16) as usize,
                        2 => (i / 32 % 4) * 4 + i % 4,
                        6 => {
                            if i % 32 < 24 {
                                0
                            } else {
                                (rng % 16) as usize
                            }
                        }
                        7 => i % 16,
                        _ => (rng % 16) as usize,
                    };
                    let pressure = match kind {
                        3 => (i * 1000 / 511) as u16,
                        7 => {
                            if i % 2 == 0 {
                                690
                            } else {
                                710
                            }
                        }
                        _ => 800,
                    };
                    (page, pressure)
                })
                .collect();
            for variant in [
                "lru",
                "compression",
                "aura",
                "old_fallback",
                "no_hysteresis",
                "no_cooldown",
                "no_confidence",
                "no_cost",
                "round_robin",
                "legacy_params",
            ] {
                let policy = match variant {
                    "lru" => Policy::Lru,
                    "compression" => Policy::CompressionFirst,
                    _ => Policy::Aura,
                };
                let mut s = fixture(policy);
                if kind == 4 {
                    for p in &mut s.pages {
                        p.compressed_bytes = 4114;
                    }
                }
                if kind == 5 {
                    for p in &mut s.pages {
                        p.compressed_bytes = 512;
                    }
                }
                if kind == 8 {
                    for p in &mut s.pages {
                        p.confidence_permille = 100;
                    }
                }
                s.state.metadata_bytes = 16 * 48;
                s.state.physical_budget_bytes = 65536 + 16 * 48;
                s.metrics.peak_physical_bytes = s.state.physical_budget_bytes;
                if variant == "legacy_params" {
                    s.engine.fallback_recency = false;
                }
                if variant == "old_fallback" {
                    s.engine.fallback_recency = false;
                }
                if variant == "no_hysteresis" {
                    s.engine.config.exit_pressure = 699;
                }
                if variant == "no_cooldown" {
                    s.engine.config.cooldown_ticks = 0;
                }
                if variant == "no_confidence" {
                    s.engine.config.confidence_min = 0;
                }
                if variant == "no_cost" {
                    s.engine.use_cost = false;
                }
                if variant != "legacy_params" {
                    for p in &mut s.pages {
                        let entropy = kind == 4 || (kind != 5 && p.id % 2 == 1);
                        p.compressed_bytes = if entropy { 4114 } else { 27 };
                        p.compress_ns = if entropy { 1355 } else { 718 };
                        p.decompress_ns = if entropy { 1977 } else { 1978 };
                    }
                }
                let mut recency = [0u64; 16];
                let mut event_ns = 0u128;
                let mut decision_ns = 0u128;
                for (tick, &(page, pressure)) in trace.iter().enumerate() {
                    recency[page] = tick as u64 + 1;
                    let candidate = if variant == "round_robin" || variant == "legacy_params" {
                        if variant == "legacy_params" {
                            (tick + 1) % 16
                        } else {
                            tick % 16
                        }
                    } else {
                        (0..16).min_by_key(|&i| (recency[i], i)).unwrap()
                    };
                    let start = Instant::now();
                    s.event_candidate(black_box(page), pressure, candidate)?;
                    event_ns += start.elapsed().as_nanos();
                    let mut probe = s.engine.clone();
                    let start = Instant::now();
                    black_box(probe.decide(&s.state, &s.pages[page], s.tick)?);
                    decision_ns += start.elapsed().as_nanos();
                }
                let elapsed = event_ns;
                let m = &s.metrics;
                println!(
                    "{name},{seed},{variant},{},{},{},{},{},{},{},{},{},{},{},{},{elapsed},{decision_ns},{},{},{},{},0,{},{}",
                    m.refaults,
                    m.restore_ns,
                    m.stall_ns,
                    m.migrations,
                    m.compressions,
                    m.cpu_ns,
                    m.io_bytes,
                    m.peak_bytes[0],
                    m.peak_bytes[1],
                    m.peak_bytes[2],
                    m.fallbacks,
                    m.oscillations,
                    std::mem::size_of::<xynoos_aura::physical::PageGroupProfile>(),
                    xynoos_aura::physical::physical_usage(&s.state)?,m.peak_physical_bytes,m.peak_temporary_bytes,s.state.used_bytes[1],s.state.used_bytes[2]
                );
            }
        }
    }
    Ok(())
}
