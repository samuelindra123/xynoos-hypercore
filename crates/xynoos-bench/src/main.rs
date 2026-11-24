#![forbid(unsafe_code)]
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    hint::black_box,
    process::Command,
    time::Instant,
};
use xynoos_aura::{
    AdaptiveTieringPolicy, AuraKernelCoordinator, AuraTieringCoordinator,
    KernelTuningRecommendation,
};
use xynoos_core::{MigrationBudget, StorageTier};
use xynoos_hypermemory::{ClusterStore, PageId, PageStore};
use xynoos_linux_control::transaction::{self, Backend, Identity, Outcome};
use xynoos_pressureiq::intelligence::Diagnosis;
use xynoos_pressureiq::system::SystemSnapshot;
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

            let mut seed = 0xabcdef_12_u32;
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
