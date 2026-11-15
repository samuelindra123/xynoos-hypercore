use std::time::Instant;
use xynoos_aura::{
    physical::{fixture, Policy},
    selector::Selector,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("trace,seed,policy,selector,refaults,migrations,stall_ns,restore_ns,cpu_ns,io_bytes,physical_bytes,backing_bytes,evaluations,rejections,no_eligible,oscillations,event_ns,inspection_ns,sample,feasible,capacity_rejections,budget_rejections,no_feasible,no_policy_migration,peak_total,metadata,resident_raw,resident_compressed");
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
                let mut modes = ["historical", "bounded", "journal", "fast"];
                modes.rotate_left((seed as usize) % 4);
                for mode in modes {
                    for sample in 0..4 {
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
                        let calibration = [
                            (27, 718, 1978),
                            (39, 727, 4412),
                            (1381, 3236, 2612),
                            (4114, 1354, 1977),
                            (1390, 1567, 2816),
                        ];
                        for (i, p) in s.pages.iter_mut().enumerate() {
                            let c = calibration[if kind == 4 {
                                3
                            } else if kind == 5 {
                                0
                            } else {
                                i % 5
                            }];
                            p.compressed_bytes = c.0;
                            p.compress_ns = c.1;
                            p.decompress_ns = c.2;
                        }
                        s.state.stall_budget_override = Some(s.state.physical_budget_bytes);
                        if mode == "accounting_only" || mode == "combined" {
                            s.state.physical_budget_bytes = u64::MAX;
                        }
                        if mode == "stall_only" || mode == "combined" {
                            s.state.stall_raw_only = true;
                        }
                        let mut inspections = xynoos_aura::selector::SearchStats::default();
                        let mut sel = Selector::default();
                        let mut recency = [0u64; 16];
                        let mut elapsed = 0u128;
                        let mut inspection_ns = 0u128;
                        for (tick, &(page, pressure)) in trace.iter().enumerate() {
                            recency[page] = tick as u64 + 1;
                            let start = Instant::now();
                            let order: Vec<_> = (0..16).collect();
                            if mode == "fast" {
                                let (_, stats, _) = sel.fast_event_measured(
                                    &mut s,
                                    page,
                                    pressure,
                                    &order,
                                    &mut inspection_ns,
                                )?;
                                inspections.evaluations += stats.evaluations;
                                inspections.rejected += stats.rejected;
                                inspections.feasible += stats.feasible;
                                inspections.capacity += stats.capacity;
                                inspections.budget += stats.budget;
                                inspections.no_feasible += stats.no_feasible;
                                inspections.no_policy_migration += stats.no_policy_migration;
                            } else {
                                let candidate = if mode == "historical" {
                                    (0..16).min_by_key(|&i| (recency[i], i)).unwrap()
                                } else {
                                    let inspection_start = Instant::now();
                                    let (candidate, stats) =
                                        sel.state_aware(&s, page, pressure, &order)?;
                                    inspection_ns += inspection_start.elapsed().as_nanos();
                                    inspections.evaluations += stats.evaluations;
                                    inspections.rejected += stats.rejected;
                                    inspections.feasible += stats.feasible;
                                    inspections.capacity += stats.capacity;
                                    inspections.budget += stats.budget;
                                    inspections.no_feasible += stats.no_feasible;
                                    inspections.no_policy_migration += stats.no_policy_migration;
                                    candidate.unwrap_or(page)
                                };
                                if mode == "journal" {
                                    s.event_journal(page, pressure, candidate)?;
                                } else {
                                    s.event_candidate(page, pressure, candidate)?;
                                }
                            }
                            if mode == "historical" || mode == "round_robin" {
                                inspections.evaluations += 1;
                                if s.metrics.last_feasible_mask == 0 {
                                    inspections.rejected += 1;
                                    inspections.no_feasible += 1;
                                } else {
                                    inspections.feasible += 1;
                                }
                                inspections.capacity += s.metrics.last_capacity_rejections;
                                inspections.budget += s.metrics.last_budget_rejections;
                            }
                            elapsed += start.elapsed().as_nanos();
                        }
                        let m = &s.metrics;
                        if sample > 0 {
                            println!("{kind},{seed},{policy:?},{mode},{},{},{},{},{},{},{},{},{},{},{},{},{elapsed},{inspection_ns},{sample},{},{},{},{},{},{},{},{},{}",m.refaults,m.migrations,m.stall_ns,m.restore_ns,m.cpu_ns,m.io_bytes,xynoos_aura::physical::physical_usage(&s.state)?,s.state.used_bytes[2],inspections.evaluations,inspections.rejected,sel.no_candidate,m.oscillations,inspections.feasible,inspections.capacity,inspections.budget,inspections.no_feasible,inspections.no_policy_migration,m.peak_total_bytes,s.state.metadata_bytes,s.state.used_bytes[0],s.state.used_bytes[1]);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
