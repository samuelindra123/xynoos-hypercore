//! Register pointer berbatas tanpa dereferensi atau alokasi dalam callback.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
pub struct Counting;
pub static ENABLED: AtomicBool = AtomicBool::new(false);
pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
pub static BYTES: AtomicU64 = AtomicU64::new(0);
pub static FREES: AtomicU64 = AtomicU64::new(0);
pub static LIVE: AtomicU64 = AtomicU64::new(0);
pub static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
pub static PEAK: AtomicU64 = AtomicU64::new(0);
pub static INVALID: AtomicBool = AtomicBool::new(false);
pub static EPOCH: AtomicU64 = AtomicU64::new(0);
static LOCK: AtomicBool = AtomicBool::new(false);
const N: usize = 4096;
static PTR: [AtomicUsize; N] = [const { AtomicUsize::new(0) }; N];
static SIZE: [AtomicUsize; N] = [const { AtomicUsize::new(0) }; N];
fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
    }
}
fn insert(p: usize, n: usize) {
    for offset in 0..N {
        let i = ((p >> 4) + offset) % N;
        let old = PTR[i].load(Ordering::Relaxed);
        if old == 0 || old == usize::MAX {
            PTR[i].store(p, Ordering::Relaxed);
            SIZE[i].store(n, Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(n as u64, Ordering::Relaxed);
            LIVE.fetch_add(1, Ordering::Relaxed);
            let live = LIVE_BYTES.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
            PEAK.fetch_max(live, Ordering::Relaxed);
            return;
        }
    }
    INVALID.store(true, Ordering::Relaxed);
}
fn remove(p: usize) -> bool {
    for offset in 0..N {
        let i = ((p >> 4) + offset) % N;
        let old = PTR[i].load(Ordering::Relaxed);
        if old == 0 {
            return false;
        }
        if old == p {
            PTR[i].store(usize::MAX, Ordering::Relaxed);
            LIVE.fetch_sub(1, Ordering::Relaxed);
            LIVE_BYTES.fetch_sub(SIZE[i].load(Ordering::Relaxed) as u64, Ordering::Relaxed);
            FREES.fetch_add(1, Ordering::Relaxed);
            return true;
        }
    }
    false
}
fn record_realloc(old: usize, new: usize, n: usize) {
    if new != 0 {
        let tracked = remove(old);
        if tracked || ENABLED.load(Ordering::Relaxed) {
            insert(new, n);
        }
    }
}
/// Dipanggil hanya ketika tidak ada objek scope terdahulu yang masih hidup.
pub fn try_begin() -> bool {
    lock();
    if LIVE.load(Ordering::Relaxed) != 0 {
        LOCK.store(false, Ordering::Release);
        return false;
    }
    for p in &PTR {
        p.store(0, Ordering::Relaxed);
    }
    for c in [&ALLOCS, &BYTES, &FREES, &LIVE, &LIVE_BYTES, &PEAK] {
        c.store(0, Ordering::Relaxed);
    }
    INVALID.store(false, Ordering::Relaxed);
    EPOCH.fetch_add(1, Ordering::Relaxed);
    ENABLED.store(true, Ordering::SeqCst);
    LOCK.store(false, Ordering::Release);
    true
}
pub fn begin() {
    assert!(try_begin(), "scope lama masih memiliki objek hidup");
}
/// Uji register metadata murni: alamat sintetis tidak pernah diteruskan ke System.
pub fn validate_register() {
    ENABLED.store(false, Ordering::SeqCst);
    begin();
    lock();
    insert(16, 7);
    let a = remove(16);
    let b = remove(16);
    insert(16, 9);
    let c = remove(16);
    insert(16, 7);
    record_realloc(16, 16, 13);
    let same_address =
        LIVE.load(Ordering::Relaxed) == 1 && LIVE_BYTES.load(Ordering::Relaxed) == 13;
    record_realloc(16, 0, 100);
    let failed = LIVE_BYTES.load(Ordering::Relaxed) == 13;
    record_realloc(16, 32, 17);
    remove(32);
    let reused = a && !b && c && same_address && failed && LIVE.load(Ordering::Relaxed) == 0;
    for i in 1..=N {
        insert(i * 16, 1);
    }
    insert((N + 1) * 16, 1);
    let full = INVALID.load(Ordering::Relaxed) && LIVE.load(Ordering::Relaxed) == N as u64;
    for i in 1..=N {
        remove(i * 16);
    }
    LOCK.store(false, Ordering::Release);
    ENABLED.store(false, Ordering::SeqCst);
    assert!(reused && full);
    assert_eq!(LIVE.load(Ordering::Relaxed), 0);
    begin();
    ENABLED.store(false, Ordering::SeqCst);
}
// SAFETY: pointer/Layout diteruskan identik ke System; register hanya menyimpan
// alamat numerik dan ukuran, tidak pernah mendereferensi pointer. Callback tanpa heap/logging.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            lock();
            if ENABLED.load(Ordering::Relaxed) {
                insert(p as usize, l.size());
            }
            LOCK.store(false, Ordering::Release);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        lock();
        remove(p as usize);
        LOCK.store(false, Ordering::Release);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        lock();
        let q = unsafe { System.realloc(p, l, n) };
        record_realloc(p as usize, q as usize, n);
        LOCK.store(false, Ordering::Release);
        q
    }
}
