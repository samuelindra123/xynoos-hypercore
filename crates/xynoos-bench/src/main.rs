#![forbid(unsafe_code)]
use std::{collections::HashMap, error::Error, hint::black_box, process::Command, time::Instant};
use xynoos_hypermemory::PageStore;
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

fn run_lifecycle_benchmark() -> Result<(), Box<dyn Error>> {
    println!("operation,iterations,total_ns,ns_per_op,throughput_mib_s,hit_ratio");
    let mut store = PageStore::adaptive(SIZE, 128, usize::MAX)?;
    let p = vec![0x42u8; SIZE];
    let mut ids = Vec::new();
    for _ in 0..128 {
        ids.push(store.insert(&p)?);
    }

    // 1. Benchmark allocating `get`
    let iters = 2000;
    let start = Instant::now();
    for i in 0..iters {
        let id = ids[i % ids.len()];
        let out = store.get(black_box(id))?;
        black_box(out);
    }
    let get_elapsed = start.elapsed();
    let get_ns_per_op = get_elapsed.as_nanos() as f64 / iters as f64;
    let get_mib_s = (iters * SIZE) as f64 / 1048576.0 / get_elapsed.as_secs_f64();
    println!(
        "get_allocating,{},{},{:.2},{:.2},{:.4}",
        iters,
        get_elapsed.as_nanos(),
        get_ns_per_op,
        get_mib_s,
        store.telemetry().hit_ratio().unwrap_or(0.0)
    );

    // 2. Benchmark zero-allocation `get_into`
    store.reset_telemetry();
    let mut buffer = vec![0u8; SIZE];
    let start = Instant::now();
    for i in 0..iters {
        let id = ids[i % ids.len()];
        let n = store.get_into(black_box(id), black_box(&mut buffer))?;
        black_box(n);
    }
    let get_into_elapsed = start.elapsed();
    let get_into_ns_per_op = get_into_elapsed.as_nanos() as f64 / iters as f64;
    let get_into_mib_s = (iters * SIZE) as f64 / 1048576.0 / get_into_elapsed.as_secs_f64();
    println!(
        "get_into_zero_alloc,{},{},{:.2},{:.2},{:.4}",
        iters,
        get_into_elapsed.as_nanos(),
        get_into_ns_per_op,
        get_into_mib_s,
        store.telemetry().hit_ratio().unwrap_or(0.0)
    );

    // 3. Benchmark in-place `update`
    store.reset_telemetry();
    let new_p = vec![0x77u8; SIZE];
    let start = Instant::now();
    for i in 0..iters {
        let id = ids[i % ids.len()];
        store.update(black_box(id), black_box(&new_p))?;
    }
    let update_elapsed = start.elapsed();
    let update_ns_per_op = update_elapsed.as_nanos() as f64 / iters as f64;
    let update_mib_s = (iters * SIZE) as f64 / 1048576.0 / update_elapsed.as_secs_f64();
    println!(
        "update_in_place,{},{},{:.2},{:.2},1.0000",
        iters,
        update_elapsed.as_nanos(),
        update_ns_per_op,
        update_mib_s
    );

    // 4. Benchmark continuous LRU eviction under capacity pressure
    let mut evict_store = PageStore::adaptive(SIZE, 64, usize::MAX)?;
    let evict_iters = 2000;
    let start = Instant::now();
    for i in 0..evict_iters {
        let val = (i % 256) as u8;
        let page = vec![val; SIZE];
        let _ = evict_store
            .insert_with_policy(black_box(&page), xynoos_hypermemory::EvictionPolicy::Lru)?;
    }
    let evict_elapsed = start.elapsed();
    let evict_ns_per_op = evict_elapsed.as_nanos() as f64 / evict_iters as f64;
    let evict_mib_s = (evict_iters * SIZE) as f64 / 1048576.0 / evict_elapsed.as_secs_f64();
    let t = evict_store.telemetry();
    println!(
        "lru_insert_eviction_pressure,{},{},{:.2},{:.2},evictions={}",
        evict_iters,
        evict_elapsed.as_nanos(),
        evict_ns_per_op,
        evict_mib_s,
        t.eviction_count
    );

    Ok(())
}

fn run_mixed_workload_benchmark() -> Result<(), Box<dyn Error>> {
    println!("workload,total_ops,reads,writes,updates,evictions,hit_ratio,median_ns,p95_ns,p99_ns");
    let mut store = PageStore::adaptive(SIZE, 128, usize::MAX)?;
    let mut seed = 0x87654321u64;
    let mut rand = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    let mut active_ids = Vec::new();
    for _ in 0..64 {
        let page = vec![(rand() % 256) as u8; SIZE];
        active_ids.push(store.insert(&page)?);
    }

    let mut latencies = Vec::with_capacity(5000);
    let mut read_buf = vec![0u8; SIZE];
    let ops = 5000;

    for _ in 0..ops {
        let roll = rand() % 100;
        let op_start = Instant::now();
        if roll < 70 && !active_ids.is_empty() {
            // 70% Reads
            let id = active_ids[(rand() as usize) % active_ids.len()];
            let _ = store.get_into(id, &mut read_buf)?;
        } else if roll < 85 {
            // 15% Inserts with LRU
            let page = vec![(rand() % 256) as u8; SIZE];
            let id = store.insert_with_policy(&page, xynoos_hypermemory::EvictionPolicy::Lru)?;
            active_ids.retain(|i| store.contains(*i));
            active_ids.push(id);
        } else if roll < 95 && !active_ids.is_empty() {
            // 10% Updates
            let id = active_ids[(rand() as usize) % active_ids.len()];
            let page = vec![(rand() % 256) as u8; SIZE];
            store.update(id, &page)?;
        } else if !active_ids.is_empty() {
            // 5% Explicit Evicts
            if let Ok(Some(evicted)) = store.evict() {
                active_ids.retain(|i| *i != evicted);
            }
        }
        latencies.push(op_start.elapsed().as_nanos() as u64);
    }

    latencies.sort_unstable();
    let median = latencies[latencies.len() / 2];
    let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
    let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];

    let t = store.telemetry();
    println!(
        "realistic_mixed_cache,{},{},{},{},{},{:.4},{},{},{}",
        ops,
        t.hit_count,
        t.insert_count,
        t.update_count,
        t.eviction_count,
        t.hit_ratio().unwrap_or(0.0),
        median,
        p95,
        p99
    );

    Ok(())
}
