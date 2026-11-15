//! Eksperimen terbatas: 9 trace × 3 seed × 3 policy × 512 event.
use std::{hint::black_box, time::Instant};
use xynoos_aura::placement::{fixture, Policy};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("trace,seed,policy,refaults,restore_ns,stall_ns,migrations,compressions,cpu_ns,io_bytes,peak_ram_bytes,peak_compressed_bytes,peak_backing_bytes,fallbacks,oscillations,event_execution_ns,decision_probe_ns,profile_bytes");
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
            for policy in [Policy::Lru, Policy::CompressionFirst, Policy::Aura] {
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
                let mut event_ns = 0u128;
                let mut decision_ns = 0u128;
                for &(page, pressure) in &trace {
                    let start = Instant::now();
                    s.event(black_box(page), pressure)?;
                    event_ns += start.elapsed().as_nanos();
                    let mut probe = s.engine.clone();
                    let start = Instant::now();
                    black_box(probe.decide(&s.state, &s.pages[page], s.tick)?);
                    decision_ns += start.elapsed().as_nanos();
                }
                let elapsed = event_ns;
                let m = &s.metrics;
                println!(
                    "{name},{seed},{policy:?},{},{},{},{},{},{},{},{},{},{},{},{},{elapsed},{decision_ns},{}",
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
                    std::mem::size_of::<xynoos_aura::placement::PageGroupProfile>()
                );
            }
        }
    }
    Ok(())
}
