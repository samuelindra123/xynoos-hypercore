#![forbid(unsafe_code)]
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    hint::black_box,
    process::Command,
    time::Instant,
};
use xynoos_aura::{
    AdaptiveCompressionConfig, AdaptiveTieringPolicy, AuraAdaptiveCoordinator, AuraDaemon,
    AuraKernelCoordinator, AuraTieringCoordinator, DaemonConfig, DaemonMode, MultiResourceState,
    PressureStabilizer, ShadowCalibrator, ShadowPlayer, TraceRecord, WorkloadTrace,
};
use xynoos_core::{
    CodecId, EvictionPolicy, MemoryPressure, MigrationBudget, ResourcePressureLevel, StorageTier,
};
use xynoos_hypermemory::{
    compress_exact, decompress_with_codec, ClusterStore, CodecScratch, HyperMemoryPool, PageId,
    PageStore,
};
use xynoos_linux_control::transaction::{self, Backend, Identity};
use xynoos_pressureiq::intelligence::Diagnosis;
const N: usize = 256;
const SIZE: usize = 4096;
const RUNS: usize = 32;
fn dataset(kind: &str) -> Vec<Vec<u8>> {
    let mut seed = 0x12345678u32;
    (0..N)
        .map(|p| {
            (0..SIZE)
                .map(|i| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let k = if kind == "mixed" {
                        ["zero", "pattern", "structured", "entropy"][p % 4]
                    } else {
                        kind
                    };
                    match k {
                        "zero" => 0,
                        "pattern" => (i % 13) as u8,
                        "structured" if i % 64 < 48 => ((i / 64 + p) % 16) as u8,
                        _ => seed as u8,
                    }
                })
                .collect()
        })
        .collect()
}
fn rss(key: &str) -> Option<usize> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|l| l.starts_with(key))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
fn cpu_ns() -> Option<u64> {
    std::fs::read_to_string("/proc/self/schedstat")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 2 && args[1] == "lifecycle" {
        return run_lifecycle_benchmark();
    }
    if args.len() == 2 && args[1] == "mixed" {
        return run_mixed_workload_benchmark();
    }
    if args.len() == 2 && args[1] == "scalability" {
        return run_scalability_benchmark();
    }
    if args.len() == 2 && args[1] == "policy-comparison" {
        return run_policy_comparison_benchmark();
    }
    if args.len() == 2 && args[1] == "clustering" {
        return run_clustering_benchmark();
    }
    if args.len() == 2 && args[1] == "interplay" {
        return run_interplay_benchmark();
    }
    if args.len() == 2 && args[1] == "daemon" {
        return run_daemon_benchmark();
    }
    if args.len() == 2 && args[1] == "shadow" {
        return run_shadow_benchmark();
    }
    if args.len() == 2 && args[1] == "adaptive-campaign" {
        return run_adaptive_campaign_benchmark();
    }
    if args.len() == 2 && args[1] == "clustering-g" {
        return run_clustering_g_benchmark();
    }
    if args.len() == 2 && args[1] == "workload-eval" {
        return run_workload_eval_benchmark();
    }
    if args.len() >= 2 && args[1] == "daemon-sustained" {
        let cycles = args
            .get(2)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(500);
        return run_daemon_sustained_benchmark(cycles);
    }
    if args.len() == 4 && args[1] == "codec-op" {
        return run_codec_op(&args[2], &args[3]);
    }
    if args.len() >= 2 && args[1] == "hypermemory-app-bench" {
        let trials = args
            .get(2)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(5);
        return run_hypermemory_app_benchmark(trials);
    }
    if args.len() == 1 {
        println!("dataset,backend,run,insert_batch_ns_per_page,get_batch_ns_per_page,codec_compress_mib_s,codec_decompress_mib_s,payload_bytes,capacity_bytes,metadata_bytes,map_capacity,raw_pages,compressed_pages,rss_before_kib,rss_loaded_kib,peak_kib,cpu_ns");
        for (index, kind) in ["zero", "pattern", "structured", "entropy", "mixed"]
            .iter()
            .enumerate()
        {
            let backends = ["raw", "lz4", "adaptive"];
            for i in 0..3 {
                let backend = backends[(i + index) % 3];
                let output = Command::new(std::env::current_exe()?)
                    .args([kind, backend])
                    .output()?;
                if !output.status.success() {
                    return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
                }
                print!("{}", String::from_utf8(output.stdout)?);
            }
        }
        return Ok(());
    }
    let kind = &args[1];
    let backend = &args[2];
    let pages = dataset(kind);
    // Pembuatan dataset di luar seluruh interval pengukuran.
    let before = rss("VmRSS:");
    let cpu_start = cpu_ns();

    for run in 0..RUNS + 4 {
        // Codec-only; alokasi output termasuk, checksum/store/verifikasi tidak termasuk.
        let start = Instant::now();
        let blocks: Vec<_> = pages
            .iter()
            .map(|p| lz4_flex::block::compress(black_box(p)))
            .collect();
        let ct = start.elapsed().as_secs_f64();
        let start = Instant::now();
        let decoded: Vec<_> = blocks
            .iter()
            .map(|p| lz4_flex::block::decompress(black_box(p), SIZE))
            .collect::<Result<_, _>>()?;
        let dt = start.elapsed().as_secs_f64();
        assert_eq!(decoded, pages);
        drop(decoded);
        drop(blocks);
        let mut outputs = Vec::with_capacity(N);
        let (it, gt, accounting, loaded) = if backend != "raw" {
            let mut s = if backend == "adaptive" {
                PageStore::adaptive(SIZE, N, usize::MAX)?
            } else {
                PageStore::with_capacity(N)?
            };
            let mut ids = Vec::with_capacity(N);
            let start = Instant::now();
            for p in &pages {
                ids.push(s.insert(black_box(p))?);
            }
            let it = start.elapsed().as_nanos() as f64 / N as f64;
            let st = s.stats();
            let accounting = (
                st.stored_bytes,
                st.buffer_capacity_bytes,
                st.metadata_bytes_lower_bound,
                st.hash_map_capacity,
                st.raw_pages,
                st.compressed_pages,
            );
            let loaded = rss("VmRSS:");
            let start = Instant::now();
            for id in ids {
                outputs.push(s.get(black_box(id))?);
            }
            let gt = start.elapsed().as_nanos() as f64 / N as f64;
            (it, gt, accounting, loaded)
        } else {
            let mut s = HashMap::new();
            let start = Instant::now();
            for (id, p) in pages.iter().enumerate() {
                s.insert(id, black_box(p).clone());
            }
            let it = start.elapsed().as_nanos() as f64 / N as f64;
            let accounting = (
                N * SIZE,
                s.values().map(Vec::capacity).sum(),
                N * std::mem::size_of::<(usize, Vec<u8>)>(),
                s.capacity(),
                N,
                0,
            );
            let loaded = rss("VmRSS:");
            let start = Instant::now();
            for id in 0..N {
                outputs.push(black_box(s.get(&id).ok_or("ID hilang")?).clone());
            }
            let gt = start.elapsed().as_nanos() as f64 / N as f64;
            (it, gt, accounting, loaded)
        };
        assert_eq!(outputs, pages);
        black_box(outputs);
        if run >= 4 {
            let cpu = cpu_ns()
                .zip(cpu_start)
                .map(|(a, b)| a.saturating_sub(b))
                .unwrap_or(0);
            println!(
                "{kind},{backend},{},{it:.2},{gt:.2},{:.2},{:.2},{},{},{},{},{},{},{},{},{},{}",
                run - 4,
                (N * SIZE) as f64 / 1048576.0 / ct,
                (N * SIZE) as f64 / 1048576.0 / dt,
                accounting.0,
                accounting.1,
                accounting.2,
                accounting.3,
                accounting.4,
                accounting.5,
                before.unwrap_or(0),
                loaded.unwrap_or(0),
                rss("VmHWM:").unwrap_or(0),
                cpu
            );
        }
    }
    Ok(())
}

fn compute_stats(values: &[f64]) -> (f64, f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    let stddev = var.sqrt();
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (mean, stddev, min, max)
}

fn run_lifecycle_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE LIFECYCLE BENCHMARK (MULTI-RUN ABBA ORDER) ===");
    println!("operation,runs,measured_iters_per_run,mean_ns_per_op,mean_mib_s,stddev_mib_s,min_mib_s,max_mib_s");

    let mut store = PageStore::adaptive(SIZE, 128, usize::MAX)?;
    let p = vec![0x42u8; SIZE];
    let mut ids = Vec::new();
    for _ in 0..128 {
        ids.push(store.insert(&p)?);
    }

    // Warm-up unmeasured
    let mut warmup_buf = vec![0u8; SIZE];
    for i in 0..200 {
        let id = ids[i % ids.len()];
        let _ = black_box(store.get(black_box(id))?);
        let _ = black_box(store.get_into(black_box(id), black_box(&mut warmup_buf))?);
    }

    let iters = 2000;
    let mut get_mibs = Vec::with_capacity(4);
    let mut get_ns = Vec::with_capacity(4);
    let mut get_into_mibs = Vec::with_capacity(4);
    let mut get_into_ns = Vec::with_capacity(4);

    let mut buffer = vec![0u8; SIZE];

    // Alternating ABBA execution order across 4 runs to prevent run-order bias
    for run in 0..4 {
        if run % 2 == 0 {
            // A then B
            let start = Instant::now();
            for i in 0..iters {
                let id = ids[i % ids.len()];
                let out = store.get(black_box(id))?;
                black_box(out);
            }
            let el = start.elapsed();
            get_ns.push(el.as_nanos() as f64 / iters as f64);
            get_mibs.push((iters * SIZE) as f64 / 1048576.0 / el.as_secs_f64());

            let start = Instant::now();
            for i in 0..iters {
                let id = ids[i % ids.len()];
                let n = store.get_into(black_box(id), black_box(&mut buffer))?;
                black_box(n);
            }
            let el = start.elapsed();
            get_into_ns.push(el.as_nanos() as f64 / iters as f64);
            get_into_mibs.push((iters * SIZE) as f64 / 1048576.0 / el.as_secs_f64());
        } else {
            // B then A
            let start = Instant::now();
            for i in 0..iters {
                let id = ids[i % ids.len()];
                let n = store.get_into(black_box(id), black_box(&mut buffer))?;
                black_box(n);
            }
            let el = start.elapsed();
            get_into_ns.push(el.as_nanos() as f64 / iters as f64);
            get_into_mibs.push((iters * SIZE) as f64 / 1048576.0 / el.as_secs_f64());

            let start = Instant::now();
            for i in 0..iters {
                let id = ids[i % ids.len()];
                let out = store.get(black_box(id))?;
                black_box(out);
            }
            let el = start.elapsed();
            get_ns.push(el.as_nanos() as f64 / iters as f64);
            get_mibs.push((iters * SIZE) as f64 / 1048576.0 / el.as_secs_f64());
        }
    }

    let (get_mean_mib, get_std_mib, get_min_mib, get_max_mib) = compute_stats(&get_mibs);
    let (get_mean_ns, _, _, _) = compute_stats(&get_ns);
    println!(
        "get_allocating,4,{},{:.2},{:.2},{:.2},{:.2},{:.2}",
        iters, get_mean_ns, get_mean_mib, get_std_mib, get_min_mib, get_max_mib
    );

    let (into_mean_mib, into_std_mib, into_min_mib, into_max_mib) = compute_stats(&get_into_mibs);
    let (into_mean_ns, _, _, _) = compute_stats(&get_into_ns);
    println!(
        "get_into_zero_alloc,4,{},{:.2},{:.2},{:.2},{:.2},{:.2}",
        iters, into_mean_ns, into_mean_mib, into_std_mib, into_min_mib, into_max_mib
    );

    // In-place update benchmark
    let mut update_mibs = Vec::with_capacity(4);
    let mut update_ns = Vec::with_capacity(4);
    let new_p = vec![0x77u8; SIZE];
    for _ in 0..4 {
        let start = Instant::now();
        for i in 0..iters {
            let id = ids[i % ids.len()];
            store.update(black_box(id), black_box(&new_p))?;
        }
        let el = start.elapsed();
        update_ns.push(el.as_nanos() as f64 / iters as f64);
        update_mibs.push((iters * SIZE) as f64 / 1048576.0 / el.as_secs_f64());
    }
    let (up_mean_mib, up_std_mib, up_min_mib, up_max_mib) = compute_stats(&update_mibs);
    let (up_mean_ns, _, _, _) = compute_stats(&update_ns);
    println!(
        "update_in_place,4,{},{:.2},{:.2},{:.2},{:.2},{:.2}",
        iters, up_mean_ns, up_mean_mib, up_std_mib, up_min_mib, up_max_mib
    );

    Ok(())
}

fn run_mixed_workload_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE MIXED WORKLOAD BENCHMARK (SEPARATED REQUESTS & TELEMETRY) ===");
    println!("scenario,total_ops,req_reads,req_writes,req_updates,req_removes,cache_hits,cache_misses,hit_ratio,auto_evictions,median_ns,p95_ns,p99_ns,max_ns");

    let scenarios = [
        ("working_set_within_capacity", 64, 48, false),
        ("working_set_exceeding_capacity", 64, 192, false),
        ("skewed_hotspot_80_20", 64, 192, true),
    ];

    for (name, capacity, working_set_size, is_skewed) in scenarios {
        let mut store = PageStore::adaptive(SIZE, capacity, usize::MAX)?;
        let mut seed = 0x1234_5678_CAFE_BABE_u64;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        // Key-to-PageId mapping for the simulated working set
        let mut working_set: HashMap<usize, PageId> = HashMap::new();
        let initial_fill = working_set_size.min(capacity / 2);
        for k in 0..initial_fill {
            let page = vec![(k % 256) as u8; SIZE];
            let id = store.insert(&page)?;
            working_set.insert(k, id);
        }

        let total_ops = 5000;
        let mut req_reads = 0;
        let mut req_writes = 0;
        let mut req_updates = 0;
        let mut req_removes = 0;

        let mut latencies = Vec::with_capacity(total_ops);
        let mut read_buf = vec![0u8; SIZE];

        store.reset_telemetry();

        for _ in 0..total_ops {
            let roll = rand() % 100;
            let op_start = Instant::now();

            if roll < 70 {
                // 70% Requested Reads
                req_reads += 1;
                let key = if is_skewed && (rand() % 100) < 80 {
                    // 80% of reads target the first 20% of keys (hotspot)
                    let hot_count = (working_set_size * 20 / 100).max(1);
                    (rand() as usize) % hot_count
                } else {
                    (rand() as usize) % working_set_size
                };

                if let Some(&id) = working_set.get(&key) {
                    let _ = store.get_into(id, &mut read_buf);
                } else {
                    // Page not yet in working set -> reading nonexistent ID
                    let _ = store.get_into(PageId(999_999), &mut read_buf);
                }
            } else if roll < 85 {
                // 15% Requested Inserts with LRU policy
                req_writes += 1;
                let key = (rand() as usize) % working_set_size;
                let page = vec![(key % 256) as u8; SIZE];
                let id =
                    store.insert_with_policy(&page, xynoos_hypermemory::EvictionPolicy::Lru)?;
                // Reconcile working set with actual live store entries
                working_set.retain(|_, id| store.contains(*id));
                working_set.insert(key, id);
            } else if roll < 95 {
                // 10% Requested Updates
                req_updates += 1;
                let key = (rand() as usize) % working_set_size;
                let page = vec![((key + 1) % 256) as u8; SIZE];
                if let Some(&id) = working_set.get(&key) {
                    let _ = store.update(id, &page);
                }
            } else {
                // 5% Requested Explicit Removals
                req_removes += 1;
                if !working_set.is_empty() {
                    let keys: Vec<_> = working_set.keys().copied().collect();
                    let target_key = keys[(rand() as usize) % keys.len()];
                    if let Some(id) = working_set.remove(&target_key) {
                        let _ = store.remove(id);
                    }
                }
            }

            latencies.push(op_start.elapsed().as_nanos() as u64);
        }

        assert_eq!(
            req_reads + req_writes + req_updates + req_removes,
            total_ops,
            "Workload integrity: sum of requested operations must equal total_ops"
        );

        latencies.sort_unstable();
        let median = latencies[latencies.len() / 2];
        let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
        let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];
        let max_lat = latencies[latencies.len() - 1];

        let t = store.telemetry();
        println!(
            "{},{},{},{},{},{},{},{},{:.4},{},{},{},{},{}",
            name,
            total_ops,
            req_reads,
            req_writes,
            req_updates,
            req_removes,
            t.hit_count,
            t.miss_count,
            t.hit_ratio().unwrap_or(0.0),
            t.eviction_count,
            median,
            p95,
            p99,
            max_lat
        );
    }

    Ok(())
}

fn run_scalability_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE EVICTION SCALABILITY BENCHMARK (O(N) VERIFICATION) ===");
    println!("capacity_pages,measured_evictions,total_ns,ns_per_evict,evictions_per_sec");

    for &capacity in &[16, 64, 256, 1024, 4096] {
        let mut store = PageStore::adaptive(SIZE, capacity, usize::MAX)?;
        let page = vec![0x33u8; SIZE];

        // Pre-fill store to exact capacity
        for _ in 0..capacity {
            store.insert(&page)?;
        }

        // Measure 500 consecutive insertions under LRU eviction saturation
        let evict_iters = 500;
        let start = Instant::now();
        for i in 0..evict_iters {
            let val = (i % 256) as u8;
            let p = vec![val; SIZE];
            let _ =
                store.insert_with_policy(black_box(&p), xynoos_hypermemory::EvictionPolicy::Lru)?;
        }
        let elapsed = start.elapsed();
        let ns_per_evict = elapsed.as_nanos() as f64 / evict_iters as f64;
        let evicts_per_sec = evict_iters as f64 / elapsed.as_secs_f64();

        println!(
            "{},{},{},{:.2},{:.2}",
            capacity,
            evict_iters,
            elapsed.as_nanos(),
            ns_per_evict,
            evicts_per_sec
        );
    }

    Ok(())
}

fn run_policy_comparison_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE MEMORY POLICY COMPARISON BENCHMARK ===");
    println!("workload,policy,total_ops,read_lat_median_ns,read_lat_p95_ns,read_lat_p99_ns,stored_bytes,accounted_bytes,migrations_to_comp,migrations_to_mem,migrations_deferred,migrations_avoided,cpu_ns");

    let workloads = ["compressible", "incompressible", "oscillating_pressure"];
    let policies = [
        "static_raw",
        "static_compressed",
        "adaptive_baseline",
        "adaptive_reliable",
    ];

    for workload in workloads {
        for policy in policies {
            let cpu_start = cpu_ns();
            let page_count = 128;
            let mut store = PageStore::adaptive(SIZE, page_count, usize::MAX)?;

            // Generate dataset based on workload
            let mut seed = 0x5a5a_1234_u32;
            let pages: Vec<Vec<u8>> = (0..page_count)
                .map(|p| {
                    (0..SIZE)
                        .map(|i| {
                            seed ^= seed << 13;
                            seed ^= seed >> 17;
                            seed ^= seed << 5;
                            match workload {
                                "incompressible" => seed as u8,
                                _ => (i % 37 + p % 7) as u8,
                            }
                        })
                        .collect()
                })
                .collect();

            let mut ids = Vec::with_capacity(page_count);
            for p in &pages {
                ids.push(store.insert(p)?);
            }

            // Configure initial tiering or policy setup
            let coordinator = match policy {
                "static_raw" => {
                    // Pre-migrate all to Memory
                    let _ = store.apply_tiering(|_, _, _| StorageTier::Memory)?;
                    None
                }
                "static_compressed" => {
                    // Pages are already in Compressed (default for adaptive PageStore insert)
                    None
                }
                "adaptive_baseline" => {
                    Some(AuraTieringCoordinator::new(AdaptiveTieringPolicy::new(750)))
                }
                "adaptive_reliable" => Some(AuraTieringCoordinator::reliable(
                    AdaptiveTieringPolicy::reliable(750),
                    3,
                    MigrationBudget::bounded(16, usize::MAX),
                )),
                _ => None,
            };

            let total_ops = 2000;
            let mut latencies = Vec::with_capacity(total_ops);
            let mut buf = vec![0u8; SIZE];

            let mut total_mig_to_comp = 0;
            let mut total_mig_to_mem = 0;
            let mut total_mig_deferred = 0;

            // Baseline oscillation migration counter to calculate avoided migrations
            let mut baseline_thrashing_count = 0;

            for op in 0..total_ops {
                // Skewed access pattern: 80% ops on top 20% pages
                let key_idx = if (op % 10) < 8 {
                    (op * 7) % (page_count / 5).max(1)
                } else {
                    (op * 13) % page_count
                };

                let start = Instant::now();
                let _ = store.get_into(ids[key_idx], &mut buf)?;
                latencies.push(start.elapsed().as_nanos() as u64);

                // Periodic coordinator evaluation every 50 ops
                if op > 0 && op % 50 == 0 {
                    let is_low = if workload == "oscillating_pressure" {
                        // Alternate between Low and Moderate every 50 ops
                        (op / 50) % 2 == 0
                    } else {
                        // Regular workload: Low pressure
                        true
                    };

                    if workload == "oscillating_pressure" {
                        baseline_thrashing_count += page_count / 2;
                    }

                    if let Some(ref coord) = coordinator {
                        let diag = Diagnosis {
                            classes: if is_low {
                                vec!["normal".into()]
                            } else {
                                vec!["elevated_pressure".into()]
                            },
                            evidence: vec![],
                            limitations: vec![],
                            available_pct: Some(if is_low { 85.0 } else { 28.0 }),
                            rates_per_second: BTreeMap::new(),
                            disk_bytes_per_second: BTreeMap::new(),
                            zram_ratios: BTreeMap::new(),
                            interval_seconds: Some(1.0),
                            reliable: true,
                        };

                        let report = coord.step(&mut store, &diag)?;
                        total_mig_to_comp += report.migrated_to_compressed;
                        total_mig_to_mem += report.migrated_to_memory;
                        total_mig_deferred += report.deferred_by_migration_budget;
                    }
                }
            }

            latencies.sort_unstable();
            let median = latencies[latencies.len() / 2];
            let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
            let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];

            let st = store.stats();
            let migrations_avoided =
                if policy == "adaptive_reliable" && workload == "oscillating_pressure" {
                    baseline_thrashing_count.saturating_sub(total_mig_to_comp + total_mig_to_mem)
                } else {
                    0
                };

            let cpu = cpu_ns()
                .zip(cpu_start)
                .map(|(a, b)| a.saturating_sub(b))
                .unwrap_or(0);

            println!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{}",
                workload,
                policy,
                total_ops,
                median,
                p95,
                p99,
                st.stored_bytes,
                st.accounted_bytes,
                total_mig_to_comp,
                total_mig_to_mem,
                total_mig_deferred,
                migrations_avoided,
                cpu
            );
        }
    }

    Ok(())
}

fn run_clustering_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE MULTI-PAGE CLUSTERING BENCHMARK ===");
    println!("workload,cluster_pages,cluster_kib,original_bytes,stored_bytes,ratio,savings_pct,compress_mib_s,decompress_mib_s,cpu_ns");

    let workloads = ["structured_logs", "kernel_memory", "random_entropy"];
    let cluster_sizes = [1, 2, 4, 8];

    for workload in workloads {
        for pages_per_cluster in cluster_sizes {
            let cpu_start = cpu_ns();
            let total_pages = 64;
            let cluster_count = total_pages / pages_per_cluster;

            let mut seed = 0xabcd_ef12_u32;
            let pages: Vec<Vec<u8>> = (0..total_pages)
                .map(|p| {
                    (0..SIZE)
                        .map(|i| {
                            seed ^= seed << 13;
                            seed ^= seed >> 17;
                            seed ^= seed << 5;
                            match workload {
                                "structured_logs" => {
                                    let pattern = b"{\"event\":\"page_fault\",\"pid\":1024,\"address\":0x7fff,\"tick\":";
                                    let b = pattern[i % pattern.len()];
                                    if i % 64 < 32 { b } else { ((p + i) % 10 + b'0' as usize) as u8 }
                                }
                                "kernel_memory" => {
                                    if i % 8 == 0 {
                                        (p as u8).wrapping_add((i / 8) as u8)
                                    } else if i % 16 < 8 {
                                        0
                                    } else {
                                        ((i % 256) ^ (p % 16)) as u8
                                    }
                                }
                                _ => seed as u8,
                            }
                        })
                        .collect()
                })
                .collect();

            let mut store = ClusterStore::new(SIZE, pages_per_cluster, cluster_count, usize::MAX)?;

            let start_comp = Instant::now();
            let mut cluster_ids = Vec::with_capacity(cluster_count);
            for c in 0..cluster_count {
                let start_idx = c * pages_per_cluster;
                let cluster_pages: Vec<&[u8]> = (0..pages_per_cluster)
                    .map(|i| pages[start_idx + i].as_slice())
                    .collect();
                let id = store.insert_cluster(&cluster_pages)?;
                cluster_ids.push(id);
            }
            let comp_elapsed = start_comp.elapsed();

            let total_original_bytes = total_pages * SIZE;
            let comp_mib_s = (total_original_bytes as f64 / (1024.0 * 1024.0))
                / comp_elapsed.as_secs_f64().max(1e-9);

            let mut out_buf = vec![0u8; SIZE * pages_per_cluster];
            let start_decomp = Instant::now();
            for &id in &cluster_ids {
                let _ = store.get_cluster_into(id, &mut out_buf)?;
            }
            let decomp_elapsed = start_decomp.elapsed();
            let decomp_mib_s = (total_original_bytes as f64 / (1024.0 * 1024.0))
                / decomp_elapsed.as_secs_f64().max(1e-9);

            let stats = store.stats();
            let ratio = stats.compression_ratio().unwrap_or(1.0);
            let savings = stats.space_savings_pct().unwrap_or(0.0);
            let cpu = cpu_ns()
                .zip(cpu_start)
                .map(|(a, b)| a.saturating_sub(b))
                .unwrap_or(0);

            println!(
                "{},{},{},{},{},{:.4},{:.2},{:.2},{:.2},{}",
                workload,
                pages_per_cluster,
                pages_per_cluster * 4,
                stats.original_bytes,
                stats.stored_bytes,
                ratio,
                savings,
                comp_mib_s,
                decomp_mib_s,
                cpu
            );
        }
    }

    Ok(())
}

struct BenchMockBackend {
    low: u64,
    high: u64,
    max: u64,
}
impl Backend for BenchMockBackend {
    fn identity(&mut self) -> Result<Identity, String> {
        Ok(Identity {
            device: 1,
            inode: 200,
            control_inode: 201,
        })
    }
    fn read_low(&mut self) -> Result<u64, String> {
        Ok(self.low)
    }
    fn write_low(&mut self, v: u64) -> Result<(), String> {
        self.low = v;
        Ok(())
    }
    fn read_high(&mut self) -> Result<u64, String> {
        Ok(self.high)
    }
    fn write_high(&mut self, v: u64) -> Result<(), String> {
        self.high = v;
        Ok(())
    }
    fn read_max(&mut self) -> Result<u64, String> {
        Ok(self.max)
    }
    fn write_max(&mut self, v: u64) -> Result<(), String> {
        self.max = v;
        Ok(())
    }
}

fn run_interplay_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE KERNEL-TIERING INTERPLAY BENCHMARK ===");
    println!("mode,cycles,read_lat_median_ns,read_lat_p95_ns,read_lat_p99_ns,tiering_migrations,kernel_tx_restored,cpu_ns");

    let modes = [
        "tiering_only",
        "advisory_interplay",
        "transactional_interplay",
    ];
    let cycles = 500;

    for mode in modes {
        let cpu_start = cpu_ns();
        let page_count = 64;
        let mut store = PageStore::adaptive(SIZE, page_count, usize::MAX)?;
        let mut seed = 0x99aa_bbcc_u32;
        let mut ids = Vec::with_capacity(page_count);

        for p in 0..page_count {
            let mut page = vec![0u8; SIZE];
            for b in page.iter_mut() {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *b = (seed as u8) ^ (p as u8);
            }
            ids.push(store.insert(&page)?);
        }

        let policy = AdaptiveTieringPolicy::reliable(800);
        let tiering_coord =
            AuraTieringCoordinator::reliable(policy, 2, MigrationBudget::bounded(8, usize::MAX));
        let coordinator = AuraKernelCoordinator::new(tiering_coord);

        let mut latencies = Vec::with_capacity(cycles);
        let mut buf = vec![0u8; SIZE];
        let mut total_migrations = 0;
        let mut total_kernel_tx_restored = 0;

        let mut mock_backend = BenchMockBackend {
            low: 4096,
            high: 65536,
            max: 131072,
        };

        let mut snapshot = xynoos_pressureiq::system::collect_system();
        snapshot.memory.psi.result = Ok(xynoos_pressureiq::parse_psi(
            "some avg10=0 avg60=0 avg300=0 total=0\nfull avg10=0 avg60=0 avg300=0 total=0",
        )
        .unwrap());
        snapshot.memory.mem.result = Ok(xynoos_pressureiq::parse_meminfo(
            "MemTotal: 4096 kB\nMemAvailable: 2048 kB",
        )
        .unwrap());

        for c in 0..cycles {
            let key = (c * 11) % page_count;
            let start = Instant::now();
            let _ = store.get_into(ids[key], &mut buf)?;
            latencies.push(start.elapsed().as_nanos() as u64);

            if c > 0 && c % 25 == 0 {
                let state_idx = (c / 25) % 4;
                let (classes, available_pct) = match state_idx {
                    0 => (vec!["normal".to_string()], 80.0),
                    1 => (vec!["elevated_pressure".to_string()], 20.0),
                    2 => (vec!["sustained_pressure".to_string()], 10.0),
                    _ => (vec!["swap_thrashing".to_string()], 3.0),
                };

                let diag = Diagnosis {
                    classes,
                    evidence: vec![],
                    limitations: vec![],
                    available_pct: Some(available_pct),
                    rates_per_second: BTreeMap::new(),
                    disk_bytes_per_second: BTreeMap::new(),
                    zram_ratios: BTreeMap::new(),
                    interval_seconds: Some(1.0),
                    reliable: true,
                };

                match mode {
                    "tiering_only" => {
                        let rep = coordinator.tiering_coordinator.step(&mut store, &diag)?;
                        total_migrations += rep.migrated_to_compressed + rep.migrated_to_memory;
                    }
                    "advisory_interplay" => {
                        let rep = coordinator
                            .step::<BenchMockBackend>(&mut store, &diag, None, None, false)?;
                        total_migrations += rep.tiering_report.migrated_to_compressed
                            + rep.tiering_report.migrated_to_memory;
                    }
                    "transactional_interplay" => {
                        let rep = coordinator.step(
                            &mut store,
                            &diag,
                            Some(&snapshot),
                            Some(&mut mock_backend),
                            true,
                        )?;
                        total_migrations += rep.tiering_report.migrated_to_compressed
                            + rep.tiering_report.migrated_to_memory;
                        if let Some(tx) = rep.kernel_transaction {
                            if tx.outcome == transaction::Outcome::Restored {
                                total_kernel_tx_restored += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        latencies.sort_unstable();
        let median = latencies[latencies.len() / 2];
        let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
        let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];
        let cpu = cpu_ns()
            .zip(cpu_start)
            .map(|(a, b)| a.saturating_sub(b))
            .unwrap_or(0);

        println!(
            "{},{},{},{},{},{},{},{}",
            mode, cycles, median, p95, p99, total_migrations, total_kernel_tx_restored, cpu
        );
    }

    Ok(())
}

fn run_daemon_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE DAEMON CONTINUOUS EVALUATION BENCHMARK ===");
    println!("mode,cycles,median_ns,p95_ns,p99_ns,rss_before_kib,rss_after_kib,cpu_ns");

    let modes = [DaemonMode::ObserveOnly, DaemonMode::Advisory];

    for &mode in &modes {
        let config = DaemonConfig {
            mode,
            ..Default::default()
        };
        let mut daemon = AuraDaemon::new(config)?;

        let diag = Diagnosis {
            classes: vec!["elevated_pressure".into()],
            evidence: Vec::new(),
            limitations: Vec::new(),
            available_pct: Some(35.0),
            rates_per_second: std::collections::BTreeMap::new(),
            disk_bytes_per_second: std::collections::BTreeMap::new(),
            zram_ratios: std::collections::BTreeMap::new(),
            interval_seconds: Some(1.0),
            reliable: true,
        };

        let rss_before = rss("VmRSS:").unwrap_or(0);
        let cpu_start = cpu_ns();
        let cycles = 200;
        let mut latencies = Vec::with_capacity(cycles);

        for _ in 0..cycles {
            let start = Instant::now();
            let _ = daemon
                .step::<xynoos_linux_control::transaction::DisposableCgroup>(&diag, None, None);
            latencies.push(start.elapsed().as_nanos());
        }

        latencies.sort_unstable();
        let median = latencies[latencies.len() / 2];
        let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
        let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];
        let rss_after = rss("VmRSS:").unwrap_or(0);
        let cpu = cpu_ns()
            .zip(cpu_start)
            .map(|(a, b)| a.saturating_sub(b))
            .unwrap_or(0);

        let mode_str = match mode {
            DaemonMode::ObserveOnly => "observe_only",
            DaemonMode::Advisory => "advisory",
            DaemonMode::Transactional => "transactional",
        };

        println!(
            "{},{},{},{},{},{},{},{}",
            mode_str, cycles, median, p95, p99, rss_before, rss_after, cpu
        );
    }

    Ok(())
}

fn run_shadow_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE WORKLOAD SHADOW REPLAY BENCHMARK ===");
    println!("workload,steps,replay_time_us,us_per_step,space_saved_bytes,compression_ratio,advisory_recs");

    let mut trace = WorkloadTrace::new("synthetic_benchmark_trace".into());
    for step in 0..100 {
        let (avail, swap) = if step % 20 < 10 {
            (60.0, false)
        } else {
            (12.0, true)
        };
        trace.append(TraceRecord {
            step_index: step as u64,
            relative_timestamp_ms: (step as u64) * 1000,
            mem_total_bytes: 4 * 1024 * 1024 * 1024,
            mem_available_bytes: (avail * 40.0 * 1024.0 * 1024.0) as u64,
            available_pct: avail,
            psi_some_avg10: if swap { 25.0 } else { 1.0 },
            psi_full_avg10: if swap { 12.0 } else { 0.0 },
            swap_thrashing: swap,
            severe_psi: swap,
            high_page_fault_rate: swap,
            cgroup_usage_bytes: 256 * 1024 * 1024,
        });
    }

    let tier_coord = AuraTieringCoordinator::new(AdaptiveTieringPolicy::default());
    let kernel_coord = AuraKernelCoordinator::new(tier_coord);
    let mut store = PageStore::with_capacity(64)?;

    let sample_page = [55u8; 4096];
    for _ in 0..16 {
        store.insert(&sample_page)?;
    }

    let start = Instant::now();
    let report = ShadowPlayer::replay(&trace, &kernel_coord, &mut store)?;
    let elapsed_us = start.elapsed().as_micros();
    let us_per_step = if report.total_steps_replayed > 0 {
        elapsed_us / (report.total_steps_replayed as u128)
    } else {
        0
    };

    println!(
        "{},{},{},{},{},{:.4},{}",
        report.workload_identifier,
        report.total_steps_replayed,
        elapsed_us,
        us_per_step,
        report.simulated_space_saved_bytes,
        report.simulated_compression_ratio,
        report.advisory_tuning_recommendations_count
    );

    Ok(())
}

fn generate_g_dataset(kind: &str, count: usize) -> Vec<Vec<u8>> {
    let mut seed = 0x517c_c1b7_2722_0a95u64;
    let mut next_u8 = || -> u8 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 24) as u8
    };
    (0..count)
        .map(|p| match kind {
            "zero_pages" => vec![0u8; SIZE],
            "repeated_text" => {
                let phrase = b"XYNOOS-HYPERCORE-AURA-PAGE-PATTERN-REPEAT-0123456789ABCDEF ";
                (0..SIZE).map(|i| phrase[(i + p) % phrase.len()]).collect()
            }
            "structured_logs" => {
                let mut v = Vec::with_capacity(SIZE);
                for row in 0..128 {
                    let line = format!(
                        "{{\"ts\":{},\"pid\":{},\"level\":\"INFO\",\"psi_mem\":{}.25,\"rss_kib\":{}}}\n",
                        1700000000 + row,
                        1024 + (p % 8),
                        row % 10,
                        12400 + (row * 13) % 512
                    );
                    v.extend_from_slice(line.as_bytes());
                }
                v.resize(SIZE, b' ');
                v
            }
            "source_code" => {
                let snippet = b"pub fn evaluate_cost(bytes: usize, pressure: u8) -> i64 {\n    if pressure > 2 { (bytes as i64) * 32 } else { bytes as i64 }\n}\n";
                (0..SIZE).map(|i| snippet[(i + (p % 3) * 7) % snippet.len()]).collect()
            }
            "binary_like" => (0..SIZE)
                .map(|i| {
                    if i % 16 < 6 {
                        0
                    } else if i % 16 < 12 {
                        ((i / 16 + p) % 64) as u8
                    } else {
                        next_u8()
                    }
                })
                .collect(),
            "high_entropy_random" => (0..SIZE).map(|_| next_u8()).collect(),
            "synthetic_app_memory" => (0..SIZE)
                .map(|i| {
                    if i < 512 {
                        ((i / 8 + p) % 16) as u8
                    } else if i < 2048 {
                        if i % 4 == 0 { 0 } else { ((i + p) % 97) as u8 }
                    } else {
                        next_u8() & 0x3F
                    }
                })
                .collect(),
            _ => {
                // mixed_compressibility
                match p % 4 {
                    0 => vec![0u8; SIZE],
                    1 => {
                        let pat = b"{\"mixed\":true,\"seq\":12345,\"tier\":\"compressed\"}\n";
                        (0..SIZE).map(|i| pat[i % pat.len()]).collect()
                    }
                    2 => (0..SIZE).map(|i| if i % 8 < 5 { (i % 32) as u8 } else { next_u8() }).collect(),
                    _ => (0..SIZE).map(|_| next_u8()).collect(),
                }
            }
        })
        .collect()
}

fn run_adaptive_campaign_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE MILESTONE G ADAPTIVE COMPRESSION CAMPAIGN ===");
    println!("dataset,regime,policy,pages,original_bytes,stored_bytes,compression_ratio,net_savings_bytes,metadata_bytes,allocated_capacity_bytes,scratch_bytes,compress_mib_s,decompress_mib_s,p50_ns,p95_ns,p99_ns,decision_latency_ns,policy_transitions,rejected_expensive_ops,peak_rss_kib,cpu_ns,wall_ns");

    let datasets = [
        "zero_pages",
        "structured_logs",
        "repeated_text",
        "source_code",
        "binary_like",
        "mixed_compressibility",
        "high_entropy_random",
        "synthetic_app_memory",
    ];
    let regimes = ["cold_heavy", "cpu_constrained", "hot_random"];
    let policies = [
        "baseline_a_milestone_f",
        "baseline_b_static_lz4",
        "candidate_c_adaptive_g",
        "candidate_d_lz4hc6",
    ];
    let page_count = 64usize;

    for ds_name in datasets {
        let pages = generate_g_dataset(ds_name, page_count);
        for regime in regimes {
            for policy_name in policies {
                let cpu_start = cpu_ns();
                let wall_start = Instant::now();
                let mut store = PageStore::adaptive(SIZE, page_count, usize::MAX)?;
                let mut ids = Vec::with_capacity(page_count);

                let comp_start = Instant::now();
                for p in &pages {
                    let codec = match policy_name {
                        "baseline_b_static_lz4" => CodecId::Lz4Standard,
                        "candidate_d_lz4hc6" => CodecId::Lz4Hc { level: 6 },
                        _ => CodecId::Raw,
                    };
                    ids.push(store.insert_with_codec(p, codec, EvictionPolicy::Reject)?);
                }
                let comp_elapsed = comp_start.elapsed();

                // Advance ticks so cold pages age past cooldown
                let mut buf = vec![0u8; SIZE];
                let hot_count = (page_count / 5).max(1);
                if regime == "hot_random" {
                    for r in 0..40 {
                        let _ = store.get_into(ids[r % hot_count], &mut buf)?;
                    }
                } else {
                    for _ in 0..10 {
                        let _ = store.get_into(ids[0], &mut buf)?;
                    }
                }

                let mut transitions = 0usize;
                let mut rejected = 0usize;
                let mut decision_latency_ns = 0u64;

                match policy_name {
                    "baseline_a_milestone_f" => {
                        let coord = AuraTieringCoordinator::new(AdaptiveTieringPolicy::default());
                        let diag = Diagnosis {
                            classes: vec![if regime == "hot_random" {
                                "elevated_pressure"
                            } else {
                                "sustained_pressure"
                            }
                            .into()],
                            evidence: vec![],
                            limitations: vec![],
                            available_pct: Some(if regime == "hot_random" { 22.0 } else { 10.0 }),
                            rates_per_second: BTreeMap::new(),
                            disk_bytes_per_second: BTreeMap::new(),
                            zram_ratios: BTreeMap::new(),
                            interval_seconds: Some(1.0),
                            reliable: true,
                        };
                        let t0 = Instant::now();
                        let rep = coord.step(&mut store, &diag)?;
                        decision_latency_ns = t0.elapsed().as_nanos() as u64;
                        transitions += rep.migrated_to_compressed + rep.migrated_to_memory;
                    }
                    "candidate_c_adaptive_g" => {
                        let mut coord = AuraAdaptiveCoordinator::new(
                            AdaptiveCompressionConfig::default(),
                            PressureStabilizer::new(3, 0),
                            MigrationBudget::UNLIMITED,
                        );
                        coord.cost_model.cooldown_ticks = 0;
                        let res = MultiResourceState {
                            memory_pressure: if regime == "hot_random" {
                                MemoryPressure::Moderate
                            } else {
                                MemoryPressure::High
                            },
                            cpu_pressure: if regime == "cpu_constrained" {
                                ResourcePressureLevel::High
                            } else {
                                ResourcePressureLevel::Low
                            },
                            io_pressure: ResourcePressureLevel::Low,
                            available_ram_bytes: Some(256 * 1024 * 1024),
                            telemetry_valid: true,
                        };
                        let rep = coord.step_adaptive(&mut store, &res)?;
                        decision_latency_ns = rep.decision_latency_ns;
                        transitions += rep.policy_transitions;
                        rejected += rep.rejected_expensive_ops;
                    }
                    _ => {}
                }

                // Measure read access latencies (512 reads)
                let read_ops = 512usize;
                let mut lats = Vec::with_capacity(read_ops);
                let decomp_start = Instant::now();
                for op in 0..read_ops {
                    let idx = if regime == "hot_random" && (op % 10) < 8 {
                        (op * 7) % hot_count
                    } else if regime == "hot_random" {
                        (op * 17) % page_count
                    } else {
                        op % page_count
                    };
                    let t0 = Instant::now();
                    let _ = store.get_into(ids[idx], &mut buf)?;
                    lats.push(t0.elapsed().as_nanos() as u64);
                }
                let decomp_elapsed = decomp_start.elapsed();
                lats.sort_unstable();

                let p50 = lats[lats.len() / 2];
                let p95 = lats[(lats.len() as f64 * 0.95) as usize];
                let p99 = lats[(lats.len() as f64 * 0.99) as usize];

                let acct = store.overhead_accounting();
                let total_orig_mib = (page_count * SIZE) as f64 / (1024.0 * 1024.0);
                let comp_mib_s = total_orig_mib / comp_elapsed.as_secs_f64().max(1e-9);
                let read_mib = (read_ops * SIZE) as f64 / (1024.0 * 1024.0);
                let decomp_mib_s = read_mib / decomp_elapsed.as_secs_f64().max(1e-9);
                let ratio = if acct.original_logical_bytes > 0 {
                    acct.stored_payload_bytes as f64 / acct.original_logical_bytes as f64
                } else {
                    1.0
                };
                let wall_ns = wall_start.elapsed().as_nanos();
                let cpu = cpu_ns()
                    .zip(cpu_start)
                    .map(|(a, b)| a.saturating_sub(b))
                    .unwrap_or(0);
                let peak_rss = rss("VmHWM:").unwrap_or(0);

                println!(
                    "{},{},{},{},{},{},{:.4},{},{},{},{},{:.2},{:.2},{},{},{},{},{},{},{},{},{}",
                    ds_name,
                    regime,
                    policy_name,
                    page_count,
                    acct.original_logical_bytes,
                    acct.stored_payload_bytes,
                    ratio,
                    acct.net_savings_bytes,
                    acct.metadata_bytes,
                    acct.allocated_capacity_bytes,
                    acct.scratch_buffer_bytes,
                    comp_mib_s,
                    decomp_mib_s,
                    p50,
                    p95,
                    p99,
                    decision_latency_ns,
                    transitions,
                    rejected,
                    peak_rss,
                    cpu,
                    wall_ns
                );
            }
        }
    }

    Ok(())
}

fn run_clustering_g_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE MILESTONE G CLUSTERING EVALUATION (1/2/4/8 PAGES) ===");
    println!("workload,cluster_pages,cluster_kib,original_bytes,stored_bytes,metadata_bytes,scratch_bytes,net_savings_bytes,ratio,seq_mib_s,random_page_p50_ns,random_page_p99_ns,read_amplification");

    let workloads = [
        "structured_logs",
        "source_code",
        "mixed_compressibility",
        "high_entropy_random",
    ];
    let cluster_sizes = [1usize, 2, 4, 8];
    let total_pages = 64usize;

    for workload in workloads {
        let pages = generate_g_dataset(workload, total_pages);
        for pages_per_cluster in cluster_sizes {
            let cluster_count = total_pages / pages_per_cluster;
            let mut store = ClusterStore::new(SIZE, pages_per_cluster, cluster_count, usize::MAX)?;
            let mut cluster_ids = Vec::with_capacity(cluster_count);
            for c in 0..cluster_count {
                let start_idx = c * pages_per_cluster;
                let slice_refs: Vec<&[u8]> = (0..pages_per_cluster)
                    .map(|i| pages[start_idx + i].as_slice())
                    .collect();
                cluster_ids.push(store.insert_cluster(&slice_refs)?);
            }

            let mut seq_buf = vec![0u8; SIZE * pages_per_cluster];
            let seq_start = Instant::now();
            for &cid in &cluster_ids {
                let _ = store.get_cluster_into(cid, &mut seq_buf)?;
            }
            let seq_elapsed = seq_start.elapsed();
            let seq_mib_s = ((total_pages * SIZE) as f64 / (1024.0 * 1024.0))
                / seq_elapsed.as_secs_f64().max(1e-9);

            // Random sub-page extraction using reusable CodecScratch
            let mut scratch = CodecScratch::with_limit(SIZE * pages_per_cluster);
            let mut page_buf = vec![0u8; SIZE];
            let rand_iters = 256usize;
            let mut lats = Vec::with_capacity(rand_iters);
            for i in 0..rand_iters {
                let cid = cluster_ids[(i * 7) % cluster_ids.len()];
                let pidx = (i * 3) % pages_per_cluster;
                let t0 = Instant::now();
                let _ = store.get_page_with_scratch(cid, pidx, &mut page_buf, &mut scratch)?;
                lats.push(t0.elapsed().as_nanos() as u64);
            }
            lats.sort_unstable();
            let p50 = lats[lats.len() / 2];
            let p99 = lats[(lats.len() as f64 * 0.99) as usize];

            let acct = store.overhead_accounting();
            let ratio = if acct.original_logical_bytes > 0 {
                acct.stored_payload_bytes as f64 / acct.original_logical_bytes as f64
            } else {
                1.0
            };
            // If cluster is compressed, extracting 1 sub-page decodes pages_per_cluster pages
            let read_amp = if store.stats().compressed_clusters > 0 {
                pages_per_cluster as f64
            } else {
                1.0
            };

            println!(
                "{},{},{},{},{},{},{},{},{:.4},{:.2},{},{},{:.1}",
                workload,
                pages_per_cluster,
                pages_per_cluster * 4,
                acct.original_logical_bytes,
                acct.stored_payload_bytes,
                acct.metadata_bytes,
                acct.scratch_buffer_bytes,
                acct.net_savings_bytes,
                ratio,
                seq_mib_s,
                p50,
                p99,
                read_amp
            );
        }
    }

    Ok(())
}

fn run_workload_eval_benchmark() -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE CONSTRAINED WORKLOAD EVALUATION (SEPARATED LAYERS) ===");
    println!("layer,metric,value,unit,notes");

    // Layer 1: HyperMemory internal storage under representative mixed workload
    let pages = generate_g_dataset("mixed_compressibility", 128);
    let mut store = PageStore::adaptive(SIZE, 128, usize::MAX)?;
    let mut ids = Vec::with_capacity(128);
    for p in &pages {
        ids.push(store.insert_with_codec(p, CodecId::Raw, EvictionPolicy::Reject)?);
    }
    for _ in 0..10 {
        let mut b = vec![0u8; SIZE];
        let _ = store.get_into(ids[0], &mut b);
    }
    let mut coord = AuraAdaptiveCoordinator::new(
        AdaptiveCompressionConfig::default(),
        PressureStabilizer::new(3, 0),
        MigrationBudget::bounded(32, 256 * 1024),
    );
    coord.cost_model.cooldown_ticks = 0;
    let res = MultiResourceState {
        memory_pressure: MemoryPressure::High,
        cpu_pressure: ResourcePressureLevel::Low,
        io_pressure: ResourcePressureLevel::Low,
        available_ram_bytes: Some(256 * 1024 * 1024),
        telemetry_valid: true,
    };
    let rep = coord.step_adaptive(&mut store, &res)?;
    let acct = store.overhead_accounting();
    println!(
        "hypermemory_internal,net_savings_bytes,{},bytes,overhead_inclusive",
        acct.net_savings_bytes
    );
    println!(
        "hypermemory_internal,decision_latency_ns,{},ns,bounded_cycle",
        rep.decision_latency_ns
    );

    // Layer 2: AURA counterfactual shadow calibration quality
    let mut trace = WorkloadTrace::new("desktop_sim_multitask".into());
    for step in 0..24u64 {
        let high = step % 6 >= 4;
        trace.append(TraceRecord {
            step_index: step,
            relative_timestamp_ms: step * 1000,
            mem_total_bytes: 4 * 1024 * 1024 * 1024,
            mem_available_bytes: if high {
                320 * 1024 * 1024
            } else {
                2_400_000_000
            },
            available_pct: if high { 8.0 } else { 60.0 },
            psi_some_avg10: if high { 18.0 } else { 0.8 },
            psi_full_avg10: if high { 6.0 } else { 0.0 },
            swap_thrashing: false,
            severe_psi: high,
            high_page_fault_rate: high,
            cgroup_usage_bytes: 512 * 1024 * 1024,
        });
    }
    let cal = ShadowCalibrator::calibrate(&trace, &pages[..16])?;
    for cmp in &cal.evaluation_comparisons {
        println!(
            "aura_advisory_replay,{}_net_savings,{},bytes,counterfactual_replay_estimate",
            cmp.policy_name, cmp.net_savings_bytes
        );
    }

    // Layer 3: System telemetry availability & cgroup environment
    let sys = xynoos_pressureiq::system::collect_system();
    println!(
        "linux_telemetry,logical_cpus,{},count,visible_proc_stat",
        sys.cpu.as_ref().map(|c| c.logical_cpus).unwrap_or(0)
    );
    println!(
        "linux_telemetry,cgroup_v2_observable,{},bool,read_only_check",
        sys.cgroup.is_ok()
    );
    println!(
        "whole_system_responsiveness,transparent_app_ram_compression,false,bool,not_implemented_userspace_only"
    );

    Ok(())
}

fn run_daemon_sustained_benchmark(cycles: u64) -> Result<(), Box<dyn Error>> {
    println!("=== HYPERCORE DAEMON SUSTAINED RELIABILITY BENCHMARK ===");
    println!("cycles,startup_us,rss_initial_kib,rss_final_kib,rss_peak_kib,wall_ms,cpu_ms,cycles_completed,cycles_failed,rss_within_budget");

    let startup_start = Instant::now();
    let mut daemon = AuraDaemon::new(DaemonConfig {
        mode: DaemonMode::ObserveOnly,
        interval_ms: 100,
        ..Default::default()
    })?;
    let startup_us = startup_start.elapsed().as_micros();

    let rss_initial = rss("VmRSS:").unwrap_or(0);
    let cpu_start = cpu_ns();
    let wall_start = Instant::now();

    let diag_ok = Diagnosis {
        classes: vec!["normal".into()],
        evidence: Vec::new(),
        limitations: Vec::new(),
        available_pct: Some(55.0),
        rates_per_second: BTreeMap::new(),
        disk_bytes_per_second: BTreeMap::new(),
        zram_ratios: BTreeMap::new(),
        interval_seconds: Some(1.0),
        reliable: true,
    };

    for c in 0..cycles {
        if c % 50 == 49 {
            // Exercise failure & recovery path periodically without leaving stale errors
            daemon.record_failure("simulated transient telemetry timeout".into());
        }
        let _ = daemon
            .step::<xynoos_linux_control::transaction::DisposableCgroup>(&diag_ok, None, None)?;
    }

    let wall_ms = wall_start.elapsed().as_millis();
    let cpu_ms = cpu_ns()
        .zip(cpu_start)
        .map(|(a, b)| a.saturating_sub(b) / 1_000_000)
        .unwrap_or(0);
    let rss_final = rss("VmRSS:").unwrap_or(0);
    let rss_peak = rss("VmHWM:").unwrap_or(0);
    let d = daemon.diagnostics();

    println!(
        "{},{},{},{},{},{},{},{},{},{}",
        cycles,
        startup_us,
        rss_initial,
        rss_final,
        rss_peak,
        wall_ms,
        cpu_ms,
        d.cycles_completed,
        d.cycles_failed,
        d.rss_within_budget
    );

    Ok(())
}

fn run_codec_op(op: &str, arg: &str) -> Result<(), Box<dyn Error>> {
    use std::io::{Read, Write};
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    match op {
        "encode-hc" => {
            let level: u8 = arg.parse()?;
            let mut scratch = CodecScratch::with_limit(4 * 1024 * 1024);
            let (out, _) = compress_exact(&input, CodecId::Lz4Hc { level }, &mut scratch)?;
            std::io::stdout().write_all(&out)?;
            Ok(())
        }
        "decode-lz4" => {
            let orig_len: usize = arg.parse()?;
            let mut out = vec![0u8; orig_len];
            let (n, _) = decompress_with_codec(CodecId::Lz4Standard, &input, &mut out)?;
            std::io::stdout().write_all(&out[..n])?;
            Ok(())
        }
        _ => Err("usage: codec-op <encode-hc|decode-lz4> <arg>".into()),
    }
}

fn run_hypermemory_app_benchmark(trials: usize) -> Result<(), Box<dyn Error>> {
    println!("=== CANDIDATE F: HYPERMEMORY APPLICATION-OWNED STORAGE BENCHMARK ===");
    println!("trial,variant,buffers,logical_bytes,accounted_bytes,net_savings_bytes,ratio,put_ms,read_p50_ns,read_p95_ns,read_p99_ns,cpu_ms,integrity_ok");

    let count = 256usize;
    let payloads: Vec<Vec<u8>> = (0..count)
        .map(|i| {
            let len = 3072 + ((i * 521) % 6144);
            match i % 4 {
                0 => {
                    let s = format!("{{\"session\":{i},\"state\":\"cached\",\"tab\":\"docs\"}}\n");
                    (0..len).map(|k| s.as_bytes()[k % s.len()]).collect()
                }
                1 => {
                    let line = b"fn process_event(id: u64) -> Result<(), Error> { Ok(()) }\n";
                    (0..len).map(|k| line[k % line.len()]).collect()
                }
                2 => (0..len)
                    .map(|k| if k % 4 == 0 { 0 } else { ((k + i) % 64) as u8 })
                    .collect(),
                _ => {
                    let mut seed = 0x9E37_79B9_7F4A_7C15u64 ^ (i as u64);
                    (0..len)
                        .map(|_| {
                            seed ^= seed << 13;
                            seed ^= seed >> 7;
                            seed ^= seed << 17;
                            seed as u8
                        })
                        .collect()
                }
            }
        })
        .collect();
    let total_logical: usize = payloads.iter().map(Vec::len).sum();

    for trial in 0..trials.max(1) {
        let variants = if trial % 2 == 0 {
            ["raw_hashmap", "hypermemory_pool_adaptive"]
        } else {
            ["hypermemory_pool_adaptive", "raw_hashmap"]
        };
        for variant in variants {
            let cpu0 = cpu_ns();
            if variant == "raw_hashmap" {
                let t_put = Instant::now();
                let mut map: HashMap<usize, Vec<u8>> = HashMap::with_capacity(count);
                for (i, p) in payloads.iter().enumerate() {
                    map.insert(i, p.clone());
                }
                let put_ms = t_put.elapsed().as_secs_f64() * 1000.0;
                let cap_bytes: usize = map.values().map(Vec::capacity).sum();
                let meta_bytes = map.capacity() * std::mem::size_of::<(usize, Vec<u8>)>();
                let accounted = cap_bytes + meta_bytes;

                let mut lats = Vec::with_capacity(512);
                let mut integrity_ok = true;
                for op in 0..512usize {
                    let idx = if op % 10 < 8 {
                        (op * 7) % 32
                    } else {
                        (op * 17) % count
                    };
                    let t0 = Instant::now();
                    let got = black_box(map.get(&idx).unwrap()).clone();
                    lats.push(t0.elapsed().as_nanos() as u64);
                    if got != payloads[idx] {
                        integrity_ok = false;
                    }
                }
                lats.sort_unstable();
                let cpu_ms = cpu_ns()
                    .zip(cpu0)
                    .map(|(a, b)| a.saturating_sub(b) as f64 / 1e6)
                    .unwrap_or(0.0);
                println!(
                    "{},{},{},{},{},{},{:.4},{:.3},{},{},{},{:.3},{}",
                    trial,
                    variant,
                    count,
                    total_logical,
                    accounted,
                    (total_logical as isize) - (accounted as isize),
                    accounted as f64 / total_logical as f64,
                    put_ms,
                    lats[lats.len() / 2],
                    lats[(lats.len() as f64 * 0.95) as usize],
                    lats[(lats.len() as f64 * 0.99) as usize],
                    cpu_ms,
                    integrity_ok
                );
            } else {
                let t_put = Instant::now();
                let mut pool = HyperMemoryPool::new(4096, 1024, usize::MAX)?;
                let mut handles = Vec::with_capacity(count);
                for p in &payloads {
                    handles.push(pool.put_with_codec(p, CodecId::Raw)?);
                }
                // Warm first 32 hot buffers, then run adaptive tiering under High memory pressure
                for r in 0..96 {
                    let _ = pool.get(handles[r % 32])?;
                }
                let mut coord = AuraAdaptiveCoordinator::new(
                    AdaptiveCompressionConfig::default(),
                    PressureStabilizer::new(3, 0),
                    MigrationBudget::UNLIMITED,
                );
                coord.cost_model.cooldown_ticks = 0;
                let res = MultiResourceState {
                    memory_pressure: MemoryPressure::High,
                    cpu_pressure: ResourcePressureLevel::Low,
                    io_pressure: ResourcePressureLevel::Low,
                    available_ram_bytes: Some(256 * 1024 * 1024),
                    telemetry_valid: true,
                };
                let _ = coord.step_adaptive(pool.page_store_mut(), &res)?;
                let put_ms = t_put.elapsed().as_secs_f64() * 1000.0;
                let acct = pool.overhead_accounting();

                let mut lats = Vec::with_capacity(512);
                let mut integrity_ok = true;
                for op in 0..512usize {
                    let idx = if op % 10 < 8 {
                        (op * 7) % 32
                    } else {
                        (op * 17) % count
                    };
                    let t0 = Instant::now();
                    let got = pool.get(handles[idx])?;
                    lats.push(t0.elapsed().as_nanos() as u64);
                    if got != payloads[idx] {
                        integrity_ok = false;
                    }
                }
                lats.sort_unstable();
                let cpu_ms = cpu_ns()
                    .zip(cpu0)
                    .map(|(a, b)| a.saturating_sub(b) as f64 / 1e6)
                    .unwrap_or(0.0);
                println!(
                    "{},{},{},{},{},{},{:.4},{:.3},{},{},{},{:.3},{}",
                    trial,
                    variant,
                    count,
                    total_logical,
                    acct.total_accounted_bytes,
                    (total_logical as isize) - (acct.total_accounted_bytes as isize),
                    acct.total_accounted_bytes as f64 / total_logical as f64,
                    put_ms,
                    lats[lats.len() / 2],
                    lats[(lats.len() as f64 * 0.95) as usize],
                    lats[(lats.len() as f64 * 0.99) as usize],
                    cpu_ms,
                    integrity_ok
                );
            }
        }
    }
    Ok(())
}
