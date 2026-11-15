//! Eksperimen allocator terpisah, single-thread; tidak dipakai library.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};
use xynoos_aura::{
    physical::{fixture, Policy, Simulator},
    selector::Selector,
};
struct Counting;
static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static FREES: AtomicU64 = AtomicU64::new(0);
// SAFETY: semua operasi diteruskan ke System dengan pointer/layout yang sama;
// counter atomik tidak mengalokasikan dan benchmark berjalan satu thread.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() && ENABLED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if ENABLED.load(Ordering::Relaxed) {
            FREES.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() && ENABLED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(n as u64, Ordering::Relaxed);
            FREES.fetch_add(1, Ordering::Relaxed);
        }
        q
    }
}
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
fn run(n: usize, seed: u64, mode: &str, sample: u8) -> Result<(), Box<dyn std::error::Error>> {
    let mut s = make(n);
    let mut selector = Selector::default();
    let order: Vec<_> = (0..n).collect();
    let trace: Vec<_> = (0..128)
        .map(|i| {
            (
                (i * 17 + seed as usize) % n,
                if i % 3 == 0 { 690 } else { 800 },
            )
        })
        .collect();
    let (mut inspection, mut eval) = (0u128, 0u64);
    ALLOCS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    FREES.store(0, Ordering::Relaxed);
    ENABLED.store(true, Ordering::SeqCst);
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
        println!("{n},{seed},{mode},{sample},{elapsed},{inspection},{eval},{},{},{},{},{},{},{},{},{},{a},{b},{d}",if mode=="fast"{128}else{128*(n+1)},if mode=="fast"{128}else if mode=="journal"{128*2*n}else{128*(2*n+1)},m.refaults,m.migrations,m.stall_ns,m.cpu_ns,m.io_bytes,xynoos_aura::physical::physical_usage(&s.state)?,m.peak_total_bytes);
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    ALLOCS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    FREES.store(0, Ordering::Relaxed);
    ENABLED.store(true, Ordering::SeqCst);
    let check = std::hint::black_box(Box::new([7u8; 17]));
    assert_eq!(check[0], 7);
    drop(check);
    ENABLED.store(false, Ordering::SeqCst);
    assert!(
        ALLOCS.load(Ordering::Relaxed) >= 1
            && FREES.load(Ordering::Relaxed) >= 1
            && BYTES.load(Ordering::Relaxed) >= 17
    );
    println!("groups,seed,mode,sample,event_ns,inspection_ns,evaluations,estimated_access_replays,estimated_vec_clones,refaults,migrations,stall_ns,cpu_ns,io_bytes,physical_bytes,peak_modeled_bytes,alloc_count,alloc_bytes,dealloc_count");
    for n in [1, 4, 8, 16, 32, 64] {
        for seed in [1, 42, 2026] {
            let mut modes = ["clone", "journal", "fast"];
            modes.rotate_left((n + seed as usize) % 3);
            for mode in modes {
                for sample in 0..4 {
                    run(n, seed, mode, sample)?;
                }
            }
        }
    }
    Ok(())
}
