use std::time::Instant;
use xynoos_aura::{
    physical::{fixture, Policy},
    selector::Selector,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("trace,seed,policy,selector,refaults,migrations,stall_ns,restore_ns,cpu_ns,io_bytes,physical_bytes,backing_bytes,evaluations,rejections,no_eligible,oscillations,event_ns");
    for kind in 0..9 {
        for seed in [1u64, 42, 2026] {
            let mut rng = seed;
            let trace: Vec<_> = (0..512)
                .map(|i| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    let page = match kind {
                        0 => i % 4,
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
                    let pressure = if kind == 3 {
                        (i * 1000 / 511) as u16
                    } else if kind == 7 {
                        if i % 2 == 0 {
                            690
                        } else {
                            710
                        }
                    } else {
                        800
                    };
                    (page, pressure)
                })
                .collect();
            for policy in [Policy::Lru, Policy::CompressionFirst, Policy::Aura] {
                for mode in ["historical", "bounded", "round_robin"] {
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
                    let mut sel = Selector::default();
                    let mut recency = [0u64; 16];
                    let mut elapsed = 0u128;
                    for (tick, &(page, pressure)) in trace.iter().enumerate() {
                        recency[page] = tick as u64 + 1;
                        let mut eligible = [true; 16];
                        eligible[page] = false;
                        let candidate = match mode {
                            "bounded" => sel.next(&eligible)?.ok_or("no eligible")?,
                            "round_robin" => tick % 16,
                            _ => (0..16).min_by_key(|&i| (recency[i], i)).ok_or("empty")?,
                        };
                        let start = Instant::now();
                        s.event_candidate(page, pressure, candidate)?;
                        elapsed += start.elapsed().as_nanos();
                    }
                    let m = &s.metrics;
                    println!("{kind},{seed},{policy:?},{mode},{},{},{},{},{},{},{},{},{},{},{},{},{elapsed}",m.refaults,m.migrations,m.stall_ns,m.restore_ns,m.cpu_ns,m.io_bytes,xynoos_aura::physical::physical_usage(&s.state)?,s.state.used_bytes[2],sel.evaluations,sel.rejected,sel.no_candidate,m.oscillations);
                }
            }
        }
    }
    Ok(())
}
