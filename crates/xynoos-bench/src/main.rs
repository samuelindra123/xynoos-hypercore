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
