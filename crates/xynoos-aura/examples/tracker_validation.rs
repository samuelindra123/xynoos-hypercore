#[path = "support/tracker010.rs"]
mod tracker;
use std::sync::{atomic::Ordering, Arc, Barrier};
use tracker::*;
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn main() {
    validate_register();
    let old = std::hint::black_box(Box::new([1u8; 8]));
    begin();
    let epoch = EPOCH.load(Ordering::Relaxed);
    drop(old);
    assert_eq!(FREES.load(Ordering::Relaxed), 0);
    let mut v = Vec::<u8>::with_capacity(16);
    v.extend_from_slice(&[1; 16]);
    assert!(!try_begin());
    assert_eq!(EPOCH.load(Ordering::Relaxed), epoch);
    v.reserve(4096);
    assert_eq!(LIVE_BYTES.load(Ordering::Relaxed), v.capacity() as u64);
    ENABLED.store(false, Ordering::SeqCst);
    drop(v);
    assert_eq!(LIVE.load(Ordering::Relaxed), 0);
    let mut old = Vec::<u8>::with_capacity(4);
    old.extend_from_slice(&[1; 4]);
    begin();
    old.reserve(8192);
    assert_eq!(LIVE_BYTES.load(Ordering::Relaxed), old.capacity() as u64);
    ENABLED.store(false, Ordering::SeqCst);
    drop(old);
    assert_eq!(LIVE.load(Ordering::Relaxed), 0);
    begin();
    let z = Box::new(());
    drop(std::hint::black_box(z));
    assert_eq!(LIVE.load(Ordering::Relaxed), 0);
    ENABLED.store(false, Ordering::SeqCst);
    // Semua thread dibuat sebelum scope; attribution adalah global, bukan per-thread.
    let start = Arc::new(Barrier::new(5));
    let done = Arc::new(Barrier::new(5));
    let mut workers = Vec::new();
    for _ in 0..4 {
        let a = start.clone();
        let b = done.clone();
        workers.push(std::thread::spawn(move || {
            a.wait();
            for _ in 0..100 {
                let mut data = std::hint::black_box(Vec::with_capacity(16));
                data.extend_from_slice(&[3u8; 16]);
                data.reserve(128);
                assert_eq!(data[0], 3);
                drop(data);
            }
            b.wait();
        }));
    }
    begin();
    start.wait();
    done.wait();
    ENABLED.store(false, Ordering::SeqCst);
    for w in workers {
        w.join().unwrap();
    }
    assert_eq!(LIVE.load(Ordering::Relaxed), 0);
    assert_eq!(LIVE_BYTES.load(Ordering::Relaxed), 0);
    assert!(!INVALID.load(Ordering::Relaxed));
    assert!(ALLOCS.load(Ordering::Relaxed) >= 800);
    println!("PASS: epoch, pra/pasca scope, realloc tracked/untracked, reuse/tombstone/table-full, ZST, 4 thread x100 operasi. Realloc same-address diuji metadata register; System tidak dijamin memilih alamat sama.");
}
