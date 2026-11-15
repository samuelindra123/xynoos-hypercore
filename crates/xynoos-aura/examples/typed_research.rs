//! Eksperimen allocator terpisah, single-thread; tidak dipakai library.
use std::{sync::atomic::Ordering, time::Instant};
use xynoos_aura::{
    physical::{fixture, Policy, Simulator},
    selector::Selector,
};
#[path = "support/tracker010.rs"]
mod tracker;
use tracker::*;
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn make(n: usize) -> Simulator {
    let base = fixture(Policy::Aura);
    let mut pages = Vec::new();
    for i in 0..n {
        let mut p = base.pages[i % 16].clone();
        p.id = i as u64;
        pages.push(p);
    }
    let mut state = base.state.clone();
    state.used_bytes = [n as u64 * 4096, 0, 0];
    state.capacity_bytes[0] = n as u64 * 4096;
    state.metadata_bytes = n as u64 * 80;
    state.physical_budget_bytes = state.used_bytes[0] + state.metadata_bytes;
    state.capacity_bytes[1] = n as u64 * 4096;
    state.capacity_bytes[2] = n as u64 * 8192;
    Simulator::new(state, pages, base.engine).expect("fixture valid")
}
fn run(
    n: usize,
    seed: u64,
    mode: &str,
    sample: u8,
    kind: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut s = make(n);
    for p in &mut s.pages {
        if kind == 4 {
            p.compressed_bytes = 4114;
        }
        if kind == 5 {
            p.compressed_bytes = 27;
        }
        if kind == 8 {
            p.confidence_permille = 0;
        }
    }
    let mut selector = Selector::default();
    let order: Vec<_> = (0..n).collect();
    let trace: Vec<_> = (0..128)
        .map(|i| {
            (
                match kind {
                    0 => i % n.min(4),
                    2 => (i / 16 * 4 + i % 4) % n,
                    6 => {
                        if i % 16 < 12 {
                            0
                        } else {
                            (i * 17 + seed as usize) % n
                        }
                    }
                    7 => i % n,
                    _ => (i * 17 + seed as usize) % n,
                },
                if kind == 3 {
                    (i * 1000 / 127) as u16
                } else if kind == 7 {
                    if i % 2 == 0 {
                        690
                    } else {
                        710
                    }
                } else {
                    800
                },
            )
        })
        .collect();
    let (mut inspection, mut eval) = (0u128, 0u64);
    tracker::begin();
    let start = Instant::now();
    for &(page, pressure) in &trace {
        if mode == "fast" {
            let (_, stats, _) =
                selector.fast_event_measured(&mut s, page, pressure, &order, &mut inspection)?;
            eval += stats.evaluations;
        } else {
            let t = Instant::now();
            let (candidate, stats) = selector.state_aware(&s, page, pressure, &order)?;
            inspection += t.elapsed().as_nanos();
            eval += stats.evaluations;
            if mode == "journal" {
                s.event_journal(page, pressure, candidate.unwrap_or(page))?;
            } else {
                s.event_candidate(page, pressure, candidate.unwrap_or(page))?;
            }
        }
    }
    let elapsed = start.elapsed().as_nanos();
    ENABLED.store(false, Ordering::SeqCst);
    let (a, b, d) = (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
        FREES.load(Ordering::Relaxed),
    );
    let m = &s.metrics;
    if sample > 0 {
        println!("{n},{seed},{mode},{sample},{elapsed},{inspection},{eval},{},{},{},{},{},{},{},{},{},{a},{b},{d},{},{},{},{}",if mode=="fast"{128}else{128*(n+1)},if mode=="fast"{128}else if mode=="journal"{128*2*n}else{128*(2*n+1)},m.refaults,m.migrations,m.stall_ns,m.cpu_ns,m.io_bytes,xynoos_aura::physical::physical_usage(&s.state)?,m.peak_total_bytes,LIVE.load(Ordering::Relaxed),LIVE_BYTES.load(Ordering::Relaxed),PEAK.load(Ordering::Relaxed),kind);
    }
    drop(s);
    assert_eq!(LIVE.load(Ordering::Relaxed), 0);
    assert!(!INVALID.load(Ordering::Relaxed));
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracker::validate_register();
    let old = std::hint::black_box(Box::new([1u8; 8]));
    tracker::begin();
    drop(old);
    assert_eq!(FREES.load(Ordering::Relaxed), 0);
    let mut test = Vec::with_capacity(4);
    test.extend_from_slice(&[2u8; 4]);
    test.reserve(128);
    assert!(PEAK.load(Ordering::Relaxed) >= test.capacity() as u64);
    ENABLED.store(false, Ordering::SeqCst);
    drop(test);
    assert_eq!(LIVE.load(Ordering::Relaxed), 0);
    println!("groups,seed,mode,sample,event_ns,inspection_ns,evaluations,estimated_access_replays,estimated_vec_clones,refaults,migrations,stall_ns,cpu_ns,io_bytes,physical_bytes,peak_modeled_bytes,alloc_count,alloc_bytes,dealloc_count,live_count,live_bytes,peak_live_bytes,trace");
    for kind in [0, 4, 7, 8] {
        for n in [1, 16, 64] {
            for seed in [1, 42, 2026] {
                let mut modes = ["clone", "journal", "fast"];
                modes.rotate_left((n + seed as usize) % 3);
                for mode in modes {
                    for sample in 0..4 {
                        run(n, seed, mode, sample, kind)?;
                    }
                }
            }
        }
    }
    Ok(())
}
