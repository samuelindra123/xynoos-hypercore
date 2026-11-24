#![forbid(unsafe_code)]
use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    fmt,
    mem::size_of,
};
pub use xynoos_core::{
    ClusterId, CostEstimate, EvictionPolicy, MigrationBudget, PageId, PageLifecycleState,
    StorageTier,
};

pub mod cluster;
pub use cluster::*;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPageSize,
    InvalidCapacity,
    PayloadChecksumMismatch,
    ChecksumMismatch,
    Allocation,
    BudgetExceeded,
    InvalidLength { expected: usize, actual: usize },
    Full,
    NotFound,
    IdExhausted,
    Decompression,
    OutputBufferTooSmall { min_required: usize, actual: usize },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

/// Telemetri observabilitas operasi memory store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Telemetry {
    pub hit_count: u64,
    pub miss_count: u64,
    pub eviction_count: u64,
    pub insert_count: u64,
    pub update_count: u64,
    pub remove_count: u64,
    pub corruption_count: u64,
}

impl Telemetry {
    pub fn hit_ratio(&self) -> Option<f64> {
        let total = self.hit_count.checked_add(self.miss_count)?;
        (total != 0).then(|| self.hit_count as f64 / total as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub page_count: usize,
    pub capacity_pages: usize,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub compressed_allocated_bytes: usize,
    pub metadata_bytes_lower_bound: usize,
    pub hash_map_capacity: usize,
    pub store_struct_bytes: usize,
    pub raw_pages: usize,
    pub compressed_pages: usize,
    pub raw_bytes: usize,
    pub stored_bytes: usize,
    pub buffer_capacity_bytes: usize,
    pub budget_bytes: usize,
    pub accounted_bytes: usize,
}
impl Stats {
    pub fn compression_ratio(&self) -> Option<f64> {
        (self.original_bytes != 0).then(|| self.stored_bytes as f64 / self.original_bytes as f64)
    }
}

/// Laporan hasil migrasi tiering adaptif antar tingkatan memori.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TieringReport {
    pub inspected_pages: usize,
    pub migrated_to_compressed: usize,
    pub migrated_to_memory: usize,
    pub already_in_tier: usize,
    pub no_change: usize,
    pub deferred_by_migration_budget: usize,
    pub rejected_by_storage_budget: usize,
    pub failed_migration: usize,
    pub net_accounted_delta: isize,
}
/// Batas ukuran simulasi untuk membatasi alokasi setiap pengambilan.
pub const MAX_PAGE_SIZE: usize = 1024 * 1024;
struct Entry {
    payload: Vec<u8>,
    payload_crc: u32,
    original_crc: u32,
    raw: bool,
    inserted_tick: u64,
    last_accessed_tick: Cell<u64>,
}
pub struct PageStore {
    page_size: usize,
    capacity: usize,
    next: u64,
    adaptive: bool,
    budget: usize,
    accounted: usize,
    pages: HashMap<PageId, Entry>,
    evicted_history: VecDeque<PageId>,
    tick: Cell<u64>,
    hit_count: Cell<u64>,
    miss_count: Cell<u64>,
    eviction_count: Cell<u64>,
    insert_count: Cell<u64>,
    update_count: Cell<u64>,
    remove_count: Cell<u64>,
    corruption_count: Cell<u64>,
}
impl PageStore {
    pub fn new(page_size: usize, capacity: usize) -> Result<Self, Error> {
        if page_size == 0 || page_size > MAX_PAGE_SIZE {
            return Err(Error::InvalidPageSize);
        }
        if capacity
            .checked_mul(lz4_flex::block::get_maximum_output_size(page_size))
            .is_none()
            || capacity.checked_mul(size_of::<(PageId, Entry)>()).is_none()
        {
            return Err(Error::InvalidCapacity);
        }
        Ok(Self {
            page_size,
            capacity,
            next: 0,
            adaptive: false,
            budget: usize::MAX,
            accounted: 0,
            pages: HashMap::new(),
            evicted_history: VecDeque::new(),
            tick: Cell::new(0),
            hit_count: Cell::new(0),
            miss_count: Cell::new(0),
            eviction_count: Cell::new(0),
            insert_count: Cell::new(0),
            update_count: Cell::new(0),
            remove_count: Cell::new(0),
            corruption_count: Cell::new(0),
        })
    }
    /// Membuat store adaptif dengan batas kapasitas buffer dan metadata entri aktif.
    pub fn adaptive(page_size: usize, capacity: usize, budget: usize) -> Result<Self, Error> {
        let mut store = Self::new(page_size, capacity)?;
        store.adaptive = true;
        store.budget = budget;
        Ok(store)
    }
    /// Membuat store 4 KiB; konfigurasi berlebihan ditolak.
    pub fn with_capacity(capacity: usize) -> Result<Self, Error> {
        Self::new(4096, capacity)
    }

    fn advance_tick(&self) -> Result<u64, Error> {
        let current = self.tick.get();
        let next = current.checked_add(1).ok_or(Error::IdExhausted)?;
        self.tick.set(next);
        Ok(current)
    }

    /// Memeriksa apakah suatu ID halaman saat ini aktif di store.
    pub fn contains(&self, id: PageId) -> bool {
        self.pages.contains_key(&id)
    }

    /// Mengetahui status siklus hidup halaman.
    pub fn lifecycle_state(&self, id: PageId) -> PageLifecycleState {
        if self.pages.contains_key(&id) {
            PageLifecycleState::Active
        } else if id.0 >= self.next {
            PageLifecycleState::NeverCreated
        } else if self.evicted_history.contains(&id) {
            PageLifecycleState::Evicted
        } else {
            PageLifecycleState::Removed
        }
    }

    /// Menyisipkan halaman dengan kebijakan penggusuran yang ditentukan.
    /// Memiliki jaminan transaksi fail-closed: jika terjadi error (alokasi, kehabisan ID/tick,
    /// atau budget tidak mencukupi), tidak ada halaman yang tergusur dan state tetap utuh.
    pub fn insert_with_policy(
        &mut self,
        data: &[u8],
        policy: EvictionPolicy,
    ) -> Result<PageId, Error> {
        if data.len() != self.page_size {
            return Err(Error::InvalidLength {
                expected: self.page_size,
                actual: data.len(),
            });
        }
        if self.capacity == 0 {
            return Err(Error::Full);
        }

        // 1. Pre-validasi kehabisan ID dan Tick SEBELUM melakukan penggusuran!
        let next_id = self.next.checked_add(1).ok_or(Error::IdExhausted)?;
        let id = PageId(self.next);
        let tick = self.tick.get();
        let next_tick = tick.checked_add(1).ok_or(Error::IdExhausted)?;

        // 2. Pre-kompresi dan evaluasi biaya
        let compressed = lz4_flex::block::compress(data);
        // Vec capacity tidak pernah lebih kecil dari len; lewati alokasi raw jika
        // kompresi sudah pasti lebih kecil dari kandidat raw mana pun.
        let (payload, raw) = if self.adaptive && compressed.capacity() >= data.len() {
            let mut raw = Vec::new();
            raw.try_reserve_exact(data.len())
                .map_err(|_| Error::Allocation)?;
            if raw.capacity() <= compressed.capacity() {
                raw.extend_from_slice(data);
                (raw, true)
            } else {
                (compressed, false)
            }
        } else {
            (compressed, false)
        };
        let cost = payload
            .capacity()
            .checked_add(size_of::<(PageId, Entry)>())
            .ok_or(Error::BudgetExceeded)?;

        if cost > self.budget {
            return Err(Error::BudgetExceeded);
        }

        // 3. Pre-reserve slot HashMap SEBELUM penggusuran untuk menjamin alokasi sukses
        self.pages.try_reserve(1).map_err(|_| Error::Allocation)?;

        // 4. Eksekusi penggusuran jika diizinkan policy
        if policy != EvictionPolicy::Reject {
            while self.pages.len() >= self.capacity {
                if self.evict_with_policy(policy)?.is_none() {
                    return Err(Error::Full);
                }
            }
            while match self.accounted.checked_add(cost) {
                Some(acc) => acc > self.budget,
                None => true,
            } {
                if self.evict_with_policy(policy)?.is_none() {
                    return Err(Error::BudgetExceeded);
                }
            }
        } else {
            if self.pages.len() >= self.capacity {
                return Err(Error::Full);
            }
            let accounted = self
                .accounted
                .checked_add(cost)
                .ok_or(Error::BudgetExceeded)?;
            if accounted > self.budget {
                return Err(Error::BudgetExceeded);
            }
        }

        // 5. Commit point non-fallible
        let entry = Entry {
            payload_crc: crc32fast::hash(&payload),
            original_crc: crc32fast::hash(data),
            payload,
            raw,
            inserted_tick: tick,
            last_accessed_tick: Cell::new(tick),
        };
        self.pages.insert(id, entry);
        self.next = next_id;
        self.tick.set(next_tick);
        self.accounted = self.accounted.checked_add(cost).expect("checked budget");
        self.insert_count
            .set(self.insert_count.get().saturating_add(1));
        Ok(id)
    }

    /// Menyisipkan halaman (kebijakan default: menolak bila kapasitas atau budget penuh).
    pub fn insert(&mut self, data: &[u8]) -> Result<PageId, Error> {
        self.insert_with_policy(data, EvictionPolicy::Reject)
    }

    /// Mengambil dekompresi halaman langsung ke buffer caller tanpa alokasi baru.
    /// Memverifikasi CRC payload dan CRC data asli.
    pub fn get_into(&self, id: PageId, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() < self.page_size {
            return Err(Error::OutputBufferTooSmall {
                min_required: self.page_size,
                actual: out.len(),
            });
        }
        let data = match self.pages.get(&id) {
            Some(d) => d,
            None => {
                self.miss_count.set(self.miss_count.get().saturating_add(1));
                return Err(Error::NotFound);
            }
        };
        if crc32fast::hash(&data.payload) != data.payload_crc {
            self.corruption_count
                .set(self.corruption_count.get().saturating_add(1));
            return Err(Error::PayloadChecksumMismatch);
        }
        let written = if data.raw {
            if data.payload.len() != self.page_size {
                return Err(Error::Decompression);
            }
            out[..self.page_size].copy_from_slice(&data.payload);
            self.page_size
        } else {
            let n = lz4_flex::block::decompress_into(&data.payload, &mut out[..self.page_size])
                .map_err(|_| Error::Decompression)?;
            if n != self.page_size {
                return Err(Error::Decompression);
            }
            n
        };
        if crc32fast::hash(&out[..self.page_size]) != data.original_crc {
            self.corruption_count
                .set(self.corruption_count.get().saturating_add(1));
            return Err(Error::ChecksumMismatch);
        }
        let tick = self.advance_tick()?;
        data.last_accessed_tick.set(tick);
        self.hit_count.set(self.hit_count.get().saturating_add(1));
        Ok(written)
    }

    /// Mengambil halaman dan mengembalikannya dalam Vec<u8> baru.
    pub fn get(&self, id: PageId) -> Result<Vec<u8>, Error> {
        let mut output = Vec::new();
        output
            .try_reserve_exact(self.page_size)
            .map_err(|_| Error::Allocation)?;
        output.resize(self.page_size, 0);
        self.get_into(id, &mut output)?;
        Ok(output)
    }

    /// Membaca halaman tanpa memperbarui last_accessed_tick atau hit counter.
    /// Berguna untuk inspeksi diagnostik non-intrusif.
    pub fn peek(&self, id: PageId) -> Result<Vec<u8>, Error> {
        let data = self.pages.get(&id).ok_or(Error::NotFound)?;
        if crc32fast::hash(&data.payload) != data.payload_crc {
            return Err(Error::PayloadChecksumMismatch);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(self.page_size)
            .map_err(|_| Error::Allocation)?;
        output.resize(self.page_size, 0);
        let written = if data.raw {
            if data.payload.len() != self.page_size {
                return Err(Error::Decompression);
            }
            output.copy_from_slice(&data.payload);
            self.page_size
        } else {
            lz4_flex::block::decompress_into(&data.payload, &mut output)
                .map_err(|_| Error::Decompression)?
        };
        if written != self.page_size {
            return Err(Error::Decompression);
        }
        if crc32fast::hash(&output) != data.original_crc {
            return Err(Error::ChecksumMismatch);
        }
        Ok(output)
    }

    /// Memperbarui isi halaman yang sudah ada secara atomik.
    /// Bila gagal (misal ukuran salah atau budget terlampaui), state lama tetap utuh.
    pub fn update(&mut self, id: PageId, data: &[u8]) -> Result<(), Error> {
        if data.len() != self.page_size {
            return Err(Error::InvalidLength {
                expected: self.page_size,
                actual: data.len(),
            });
        }
        let old_cost = {
            let old_entry = match self.pages.get(&id) {
                Some(e) => e,
                None => {
                    return Err(Error::NotFound);
                }
            };
            old_entry
                .payload
                .capacity()
                .checked_add(size_of::<(PageId, Entry)>())
                .expect("validated cost")
        };

        // Pre-validasi kehabisan tick sebelum mutasi
        let tick = self.tick.get();
        let next_tick = tick.checked_add(1).ok_or(Error::IdExhausted)?;

        let compressed = lz4_flex::block::compress(data);
        let (payload, raw) = if self.adaptive && compressed.capacity() >= data.len() {
            let mut raw = Vec::new();
            raw.try_reserve_exact(data.len())
                .map_err(|_| Error::Allocation)?;
            if raw.capacity() <= compressed.capacity() {
                raw.extend_from_slice(data);
                (raw, true)
            } else {
                (compressed, false)
            }
        } else {
            (compressed, false)
        };

        let new_cost = payload
            .capacity()
            .checked_add(size_of::<(PageId, Entry)>())
            .ok_or(Error::BudgetExceeded)?;

        let new_accounted = if new_cost >= old_cost {
            let diff = new_cost - old_cost;
            let acc = self
                .accounted
                .checked_add(diff)
                .ok_or(Error::BudgetExceeded)?;
            if acc > self.budget {
                return Err(Error::BudgetExceeded);
            }
            acc
        } else {
            let diff = old_cost - new_cost;
            self.accounted
                .checked_sub(diff)
                .expect("accounting invariant")
        };

        let entry = self.pages.get_mut(&id).expect("checked existence");
        entry.payload_crc = crc32fast::hash(&payload);
        entry.original_crc = crc32fast::hash(data);
        entry.payload = payload;
        entry.raw = raw;
        entry.last_accessed_tick.set(tick);
        self.tick.set(next_tick);
        self.accounted = new_accounted;
        self.update_count
            .set(self.update_count.get().saturating_add(1));
        Ok(())
    }

    fn remove_internal(&mut self, id: PageId, is_eviction: bool) -> Result<(), Error> {
        let entry = match self.pages.remove(&id) {
            Some(e) => e,
            None => {
                return Err(Error::NotFound);
            }
        };
        let cost = entry
            .payload
            .capacity()
            .checked_add(size_of::<(PageId, Entry)>())
            .expect("validated entry cost");
        self.accounted = self
            .accounted
            .checked_sub(cost)
            .expect("accounting invariant");
        if is_eviction {
            self.evicted_history.push_back(id);
            if self.evicted_history.len() > self.capacity.max(16) {
                self.evicted_history.pop_front();
            }
            self.eviction_count
                .set(self.eviction_count.get().saturating_add(1));
        } else {
            self.evicted_history.retain(|i| *i != id);
            self.remove_count
                .set(self.remove_count.get().saturating_add(1));
        }
        Ok(())
    }

    /// Menggusur sebuah halaman sesuai kebijakan penggusuran.
    pub fn evict_with_policy(&mut self, policy: EvictionPolicy) -> Result<Option<PageId>, Error> {
        if self.pages.is_empty() || policy == EvictionPolicy::Reject {
            return Ok(None);
        }
        let candidate_id = match policy {
            EvictionPolicy::Lru => self
                .pages
                .iter()
                .min_by_key(|(id, e)| (e.last_accessed_tick.get(), id.0))
                .map(|(id, _)| *id),
            EvictionPolicy::Fifo => self
                .pages
                .iter()
                .min_by_key(|(id, e)| (e.inserted_tick, id.0))
                .map(|(id, _)| *id),
            EvictionPolicy::Reject => None,
        };

        if let Some(id) = candidate_id {
            self.remove_internal(id, true)?;
            Ok(Some(id))
        } else {
            Ok(None)
        }
    }

    /// Menggusur satu halaman menggunakan LRU.
    pub fn evict(&mut self) -> Result<Option<PageId>, Error> {
        self.evict_with_policy(EvictionPolicy::Lru)
    }

    /// Menghapus halaman dari store secara eksplisit.
    pub fn remove(&mut self, id: PageId) -> Result<(), Error> {
        self.remove_internal(id, false)
    }

    /// Mengetahui tingkat penyimpanan (tier) halaman saat ini: Memory (raw) atau Compressed.
    pub fn page_tier(&self, id: PageId) -> Result<StorageTier, Error> {
        match self.pages.get(&id) {
            Some(entry) => {
                if entry.raw {
                    Ok(StorageTier::Memory)
                } else {
                    Ok(StorageTier::Compressed)
                }
            }
            None => Err(Error::NotFound),
        }
    }

    /// Menghitung estimasi biaya halaman (latensi akses dan kapasitas memori).
    pub fn page_cost_estimate(&self, id: PageId) -> Result<CostEstimate, Error> {
        let entry = self.pages.get(&id).ok_or(Error::NotFound)?;
        let bytes = entry
            .payload
            .capacity()
            .checked_add(size_of::<(PageId, Entry)>())
            .ok_or(Error::BudgetExceeded)?;
        let recency_penalty = self
            .tick
            .get()
            .saturating_sub(entry.last_accessed_tick.get())
            .saturating_mul(150);
        let base_latency: u64 = if entry.raw { 200 } else { 1200 };
        let latency_ns = base_latency.saturating_add(recency_penalty);
        Ok(CostEstimate { latency_ns, bytes })
    }

    /// Memigrasikan halaman secara in-place ke tingkatan penyimpanan target (Memory atau Compressed).
    /// Mempertahankan jaminan konsistensi transaksional fail-closed: jika migrasi gagal
    /// (misal karena alokasi atau pelampauan budget saat ekspansi ke raw), halaman tetap utuh.
    pub fn migrate_page(&mut self, id: PageId, target: StorageTier) -> Result<bool, Error> {
        let entry = self.pages.get(&id).ok_or(Error::NotFound)?;
        let current_tier = if entry.raw {
            StorageTier::Memory
        } else {
            StorageTier::Compressed
        };
        if current_tier == target {
            return Ok(false);
        }

        let old_cost = entry
            .payload
            .capacity()
            .checked_add(size_of::<(PageId, Entry)>())
            .expect("validated cost");

        match target {
            StorageTier::Compressed => {
                let decompressed = &entry.payload;
                if crc32fast::hash(decompressed) != entry.payload_crc {
                    self.corruption_count
                        .set(self.corruption_count.get().saturating_add(1));
                    return Err(Error::PayloadChecksumMismatch);
                }
                let compressed = lz4_flex::block::compress(decompressed);
                // Perlindungan halaman inkompresibel (Phase E):
                // Jika kompresi tidak menghemat ruang memori, pertahankan di tier raw
                // agar tidak terjadi pembengkakan memori akibat header LZ4.
                if compressed.capacity() >= decompressed.capacity() {
                    return Ok(false);
                }
                let new_cost = compressed
                    .capacity()
                    .checked_add(size_of::<(PageId, Entry)>())
                    .ok_or(Error::BudgetExceeded)?;

                let new_accounted = if new_cost >= old_cost {
                    let diff = new_cost - old_cost;
                    let acc = self
                        .accounted
                        .checked_add(diff)
                        .ok_or(Error::BudgetExceeded)?;
                    if acc > self.budget {
                        return Err(Error::BudgetExceeded);
                    }
                    acc
                } else {
                    let diff = old_cost - new_cost;
                    self.accounted
                        .checked_sub(diff)
                        .expect("accounting invariant")
                };

                let entry = self.pages.get_mut(&id).expect("checked existence");
                entry.payload = compressed;
                entry.raw = false;
                entry.payload_crc = crc32fast::hash(&entry.payload);
                self.accounted = new_accounted;
                self.update_count
                    .set(self.update_count.get().saturating_add(1));
                Ok(true)
            }
            StorageTier::Memory => {
                if crc32fast::hash(&entry.payload) != entry.payload_crc {
                    self.corruption_count
                        .set(self.corruption_count.get().saturating_add(1));
                    return Err(Error::PayloadChecksumMismatch);
                }
                let mut raw_buf = Vec::new();
                raw_buf
                    .try_reserve_exact(self.page_size)
                    .map_err(|_| Error::Allocation)?;
                raw_buf.resize(self.page_size, 0);
                let n = lz4_flex::block::decompress_into(&entry.payload, &mut raw_buf)
                    .map_err(|_| Error::Decompression)?;
                if n != self.page_size {
                    return Err(Error::Decompression);
                }
                if crc32fast::hash(&raw_buf) != entry.original_crc {
                    self.corruption_count
                        .set(self.corruption_count.get().saturating_add(1));
                    return Err(Error::ChecksumMismatch);
                }

                let new_cost = raw_buf
                    .capacity()
                    .checked_add(size_of::<(PageId, Entry)>())
                    .ok_or(Error::BudgetExceeded)?;

                let new_accounted = if new_cost >= old_cost {
                    let diff = new_cost - old_cost;
                    let acc = self
                        .accounted
                        .checked_add(diff)
                        .ok_or(Error::BudgetExceeded)?;
                    if acc > self.budget {
                        return Err(Error::BudgetExceeded);
                    }
                    acc
                } else {
                    let diff = old_cost - new_cost;
                    self.accounted
                        .checked_sub(diff)
                        .expect("accounting invariant")
                };

                let entry = self.pages.get_mut(&id).expect("checked existence");
                entry.payload = raw_buf;
                entry.raw = true;
                entry.payload_crc = crc32fast::hash(&entry.payload);
                self.accounted = new_accounted;
                self.update_count
                    .set(self.update_count.get().saturating_add(1));
                Ok(true)
            }
        }
    }

    /// Menerapkan fungsi keputusan tiering dengan batas anggaran migrasi per siklus evaluasi.
    pub fn apply_tiering_with_budget<F>(
        &mut self,
        budget: MigrationBudget,
        mut decide: F,
    ) -> Result<TieringReport, Error>
    where
        F: FnMut(PageId, StorageTier, CostEstimate) -> StorageTier,
    {
        let mut ids: Vec<PageId> = self.pages.keys().copied().collect();
        ids.sort_by_key(|id| id.0);

        let initial_accounted = self.accounted;
        let mut report = TieringReport {
            inspected_pages: ids.len(),
            ..Default::default()
        };

        let mut migrated_pages_count = 0usize;
        let mut migrated_bytes_count = 0usize;

        for id in ids {
            let current_tier = self.page_tier(id)?;
            let cost = self.page_cost_estimate(id)?;
            let target = decide(id, current_tier, cost);
            if target == current_tier {
                report.already_in_tier += 1;
                report.no_change += 1;
            } else {
                // Periksa batas anggaran migrasi (rate limiting)
                if migrated_pages_count >= budget.max_pages
                    || migrated_bytes_count.saturating_add(cost.bytes) > budget.max_bytes
                {
                    report.deferred_by_migration_budget += 1;
                    report.no_change += 1;
                    continue;
                }

                match self.migrate_page(id, target) {
                    Ok(true) => {
                        migrated_pages_count += 1;
                        migrated_bytes_count = migrated_bytes_count.saturating_add(cost.bytes);
                        if target == StorageTier::Compressed {
                            report.migrated_to_compressed += 1;
                        } else {
                            report.migrated_to_memory += 1;
                        }
                    }
                    Ok(false) => {
                        report.already_in_tier += 1;
                        report.no_change += 1;
                    }
                    Err(Error::BudgetExceeded) => {
                        report.rejected_by_storage_budget += 1;
                        report.no_change += 1;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        }

        // Phase F: Keamanan Numerik & Akuntansi Terverifikasi
        report.net_accounted_delta = if self.accounted >= initial_accounted {
            let diff = self.accounted - initial_accounted;
            isize::try_from(diff).unwrap_or(isize::MAX)
        } else {
            let diff = initial_accounted - self.accounted;
            match isize::try_from(diff) {
                Ok(v) => -v,
                Err(_) => isize::MIN,
            }
        };
        Ok(report)
    }

    /// Menerapkan fungsi keputusan tiering ke seluruh halaman aktif secara deterministik (tanpa batas budget).
    pub fn apply_tiering<F>(&mut self, decide: F) -> Result<TieringReport, Error>
    where
        F: FnMut(PageId, StorageTier, CostEstimate) -> StorageTier,
    {
        self.apply_tiering_with_budget(MigrationBudget::UNLIMITED, decide)
    }

    /// Mendapatkan snapshot telemetri operasional.
    pub fn telemetry(&self) -> Telemetry {
        Telemetry {
            hit_count: self.hit_count.get(),
            miss_count: self.miss_count.get(),
            eviction_count: self.eviction_count.get(),
            insert_count: self.insert_count.get(),
            update_count: self.update_count.get(),
            remove_count: self.remove_count.get(),
            corruption_count: self.corruption_count.get(),
        }
    }

    /// Mereset counter telemetri operasional menjadi nol.
    pub fn reset_telemetry(&self) {
        self.hit_count.set(0);
        self.miss_count.set(0);
        self.eviction_count.set(0);
        self.insert_count.set(0);
        self.update_count.set(0);
        self.remove_count.set(0);
        self.corruption_count.set(0);
    }
    pub fn stats(&self) -> Stats {
        Stats {
            raw_pages: self.pages.values().filter(|e| e.raw).count(),
            compressed_pages: self.pages.values().filter(|e| !e.raw).count(),
            raw_bytes: self
                .pages
                .values()
                .filter(|e| e.raw)
                .map(|e| e.payload.len())
                .fold(0usize, |a, b| {
                    a.checked_add(b).expect("accounting invariant")
                }),
            stored_bytes: self
                .pages
                .values()
                .map(|e| e.payload.len())
                .fold(0usize, |a, b| {
                    a.checked_add(b).expect("accounting invariant")
                }),
            buffer_capacity_bytes: self
                .pages
                .values()
                .map(|e| e.payload.capacity())
                .fold(0usize, |a, b| {
                    a.checked_add(b).expect("accounting invariant")
                }),
            budget_bytes: self.budget,
            accounted_bytes: self.accounted,
            hash_map_capacity: self.pages.capacity(),
            store_struct_bytes: size_of::<Self>(),
            page_count: self.pages.len(),
            capacity_pages: self.capacity,
            original_bytes: self
                .pages
                .len()
                .checked_mul(self.page_size)
                .expect("validated capacity"),
            compressed_bytes: self
                .pages
                .values()
                .filter(|e| !e.raw)
                .map(|e| e.payload.len())
                .fold(0usize, |a, b| {
                    a.checked_add(b).expect("accounting invariant")
                }),
            compressed_allocated_bytes: self
                .pages
                .values()
                .filter(|e| !e.raw)
                .map(|e| e.payload.capacity())
                .fold(0usize, |a, b| {
                    a.checked_add(b).expect("accounting invariant")
                }),
            metadata_bytes_lower_bound: self
                .pages
                .len()
                .checked_mul(size_of::<(PageId, Entry)>())
                .expect("validated capacity"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_and_statistics() {
        let mut s = PageStore::with_capacity(3).unwrap();
        let mut x = 1u32;
        let random: Vec<u8> = (0..4096)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        let pages = [
            vec![0; 4096],
            (0..4096).map(|i| (i % 13) as u8).collect(),
            random,
        ];
        let ids: Vec<_> = pages.iter().map(|p| s.insert(p).unwrap()).collect();
        for (id, p) in ids.iter().zip(pages.iter()) {
            assert_eq!(&s.get(*id).unwrap(), p);
        }
        let st = s.stats();
        assert_eq!(st.page_count, 3);
        assert_eq!(st.original_bytes, 12288);
        assert_eq!(
            st.compressed_bytes,
            s.pages.values().map(|e| e.payload.len()).sum()
        );
        assert_eq!(
            st.compression_ratio(),
            Some(st.compressed_bytes as f64 / 12288.0)
        );
        assert_eq!(s.insert(&pages[0]), Err(Error::Full));
        s.remove(ids[0]).unwrap();
        assert_eq!(s.get(ids[0]), Err(Error::NotFound));
        assert_eq!(s.stats().page_count, 2);
    }
    #[test]
    fn invalid() {
        assert!(matches!(PageStore::new(0, 1), Err(Error::InvalidPageSize)));
        let mut s = PageStore::new(8, 1).unwrap();
        assert!(matches!(
            s.insert(&[0; 7]),
            Err(Error::InvalidLength { .. })
        ));
        assert_eq!(s.remove(PageId(99)), Err(Error::NotFound));
        assert_eq!(s.stats().compression_ratio(), None);
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::*;
    #[test]
    fn corruption_and_isolation() {
        let mut s = PageStore::with_capacity(2).unwrap();
        let a = s.insert(&[0; 4096]).unwrap();
        let b = s.insert(&[1; 4096]).unwrap();
        s.pages.get_mut(&a).unwrap().payload[0] ^= 1;
        let before = s.stats();
        assert_eq!(s.get(a), Err(Error::PayloadChecksumMismatch));
        assert_eq!(s.stats(), before);
        assert_eq!(s.get(b).unwrap(), vec![1; 4096]);
    }
    #[test]
    fn truncation_and_decoder_failure() {
        let mut s = PageStore::with_capacity(1).unwrap();
        let a = s.insert(&[0; 4096]).unwrap();
        let e = s.pages.get_mut(&a).unwrap();
        e.payload.truncate(1);
        assert_eq!(s.get(a), Err(Error::PayloadChecksumMismatch));
        let e = s.pages.get_mut(&a).unwrap();
        e.payload_crc = crc32fast::hash(&e.payload);
        assert_eq!(s.get(a), Err(Error::Decompression));
    }
    #[test]
    fn checksum_and_length() {
        let mut s = PageStore::with_capacity(1).unwrap();
        let a = s.insert(&[0; 4096]).unwrap();
        s.pages.get_mut(&a).unwrap().original_crc ^= 1;
        assert_eq!(s.get(a), Err(Error::ChecksumMismatch));
        let e = s.pages.get_mut(&a).unwrap();
        e.payload = lz4_flex::block::compress(&[0; 16]);
        e.payload_crc = crc32fast::hash(&e.payload);
        assert_eq!(s.get(a), Err(Error::Decompression));
    }
    #[test]
    fn failures_preserve_state() {
        let mut s = PageStore::with_capacity(1).unwrap();
        let a = s.insert(&[0; 4096]).unwrap();
        let st = s.stats();
        assert_eq!(s.insert(&[1; 4096]), Err(Error::Full));
        assert!(s.insert(&[]).is_err());
        assert_eq!(s.remove(PageId(99)), Err(Error::NotFound));
        assert_eq!(s.stats(), st);
        s.remove(a).unwrap();
        let b = s.insert(&[0; 4096]).unwrap();
        assert_ne!(a, b);
        assert_eq!(s.get(a), Err(Error::NotFound));
        s.remove(b).unwrap();
        s.next = u64::MAX;
        let st = s.stats();
        assert_eq!(s.insert(&[0; 4096]), Err(Error::IdExhausted));
        assert_eq!(s.stats(), st);
        assert!(matches!(
            PageStore::new(MAX_PAGE_SIZE + 1, 1),
            Err(Error::InvalidPageSize)
        ));
        assert!(matches!(
            PageStore::new(4096, usize::MAX),
            Err(Error::InvalidCapacity)
        ));
    }
    #[test]
    fn generated_operation_sequences() {
        let mut seed = 0x12345678u32;
        for size in [1, 16, 4096, 8192] {
            let mut s = PageStore::new(size, 16).unwrap();
            let mut model = Vec::new();
            for step in 0..512 {
                let data: Vec<_> = (0..size)
                    .map(|i| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        if step % 3 == 0 {
                            0
                        } else if step % 3 == 1 {
                            (i % 7) as u8
                        } else {
                            seed as u8
                        }
                    })
                    .collect();
                if model.len() == 16 {
                    let (id, _) = model.remove(0);
                    s.remove(id).unwrap();
                    assert_eq!(s.get(id), Err(Error::NotFound));
                }
                let id = s.insert(&data).unwrap();
                model.push((id, data));
                for (id, data) in &model {
                    assert_eq!(s.get(*id).unwrap(), *data);
                }
                assert_eq!(s.stats().original_bytes, model.len() * size);
            }
        }
    }
}

#[cfg(test)]
mod malformed_tests {
    use super::*;
    #[test]
    fn bounded_malformed_blocks() {
        let mut s = PageStore::new(64, 1).unwrap();
        let id = s.insert(&[0; 64]).unwrap();
        let mut seed = 42u32;
        for len in 0..256 {
            let e = s.pages.get_mut(&id).unwrap();
            e.payload = (0..len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                })
                .collect();
            e.payload_crc = crc32fast::hash(&e.payload);
            let before = s.stats();
            let result = s.get(id);
            if let Ok(data) = result {
                assert_eq!(data, vec![0; 64]);
            }
            assert_eq!(s.stats(), before);
        }
    }
}

#[cfg(test)]
mod adaptive_tests {
    use super::*;
    #[test]
    fn mixed_and_budget() {
        let mut seed = 123u32;
        let random: Vec<u8> = (0..4096)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let mut s = PageStore::adaptive(4096, 3, 10000).unwrap();
        let a = s.insert(&random).unwrap();
        let b = s.insert(&[0; 4096]).unwrap();
        assert_eq!(s.get(a).unwrap(), random);
        assert_eq!(s.get(b).unwrap(), vec![0; 4096]);
        let st = s.stats();
        assert_eq!((st.raw_pages, st.compressed_pages), (1, 1));
        assert_eq!(
            st.accounted_bytes,
            st.buffer_capacity_bytes + st.metadata_bytes_lower_bound
        );
        s.remove(a).unwrap();
        assert_eq!(s.stats().raw_pages, 0);
        let mut t = PageStore::adaptive(4096, 1, 1).unwrap();
        let before = t.stats();
        assert_eq!(t.insert(&[0; 4096]), Err(Error::BudgetExceeded));
        assert_eq!(t.stats(), before);
    }
    #[test]
    fn raw_corruption_is_detected() {
        let mut s = PageStore::adaptive(1, 1, 100).unwrap();
        let id = s.insert(&[42]).unwrap();
        assert!(s.pages[&id].raw);
        s.pages.get_mut(&id).unwrap().payload[0] ^= 1;
        assert_eq!(s.get(id), Err(Error::PayloadChecksumMismatch));
    }
}

#[cfg(test)]
mod budget_boundary_tests {
    use super::*;
    #[test]
    fn boundary_and_reuse() {
        let input = [0; 4096];
        let mut probe = PageStore::adaptive(4096, 2, usize::MAX).unwrap();
        let id = probe.insert(&input).unwrap();
        let cost = probe.stats().accounted_bytes;
        for budget in [cost, cost + 1] {
            let mut s = PageStore::adaptive(4096, 1, budget).unwrap();
            let a = s.insert(&input).unwrap();
            assert_eq!(s.stats().accounted_bytes, cost);
            let before = s.stats();
            assert_eq!(s.insert(&input), Err(Error::Full));
            assert_eq!(s.stats(), before);
            s.remove(a).unwrap();
            assert_eq!(s.stats().accounted_bytes, 0);
            assert_ne!(a, s.insert(&input).unwrap());
        }
        let mut s = PageStore::adaptive(4096, 2, cost - 1).unwrap();
        let before = s.stats();
        assert_eq!(s.insert(&input), Err(Error::BudgetExceeded));
        assert_eq!(s.stats(), before);
        assert_eq!(s.next, 0);
        probe.remove(id).unwrap();
        assert_eq!(probe.stats().accounted_bytes, 0);
    }
    #[test]
    fn overflow_and_raw_integrity() {
        assert!(matches!(
            PageStore::adaptive(4096, usize::MAX, usize::MAX),
            Err(Error::InvalidCapacity)
        ));
        let mut s = PageStore::adaptive(1, 1, usize::MAX).unwrap();
        let id = s.insert(&[42]).unwrap();
        assert!(s.pages[&id].raw);
        s.pages.get_mut(&id).unwrap().original_crc ^= 1;
        assert_eq!(s.get(id), Err(Error::ChecksumMismatch));
        let e = s.pages.get_mut(&id).unwrap();
        e.payload.clear();
        e.payload_crc = crc32fast::hash(&e.payload);
        assert_eq!(s.get(id), Err(Error::Decompression));
    }
    #[test]
    fn mixed_sequence() {
        let mut s = PageStore::adaptive(128, 4, 4096).unwrap();
        let mut seed = 12u32;
        let mut live = Vec::new();
        for n in 0..128 {
            let page: Vec<u8> = (0..128)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    if n % 2 == 0 {
                        0
                    } else {
                        seed as u8
                    }
                })
                .collect();
            if live.len() == 4 {
                let (id, _) = live.remove(0);
                s.remove(id).unwrap();
                assert_eq!(s.get(id), Err(Error::NotFound));
            }
            let id = s.insert(&page).unwrap();
            live.push((id, page));
            for (id, p) in &live {
                assert_eq!(s.get(*id).unwrap(), *p);
            }
            let st = s.stats();
            assert_eq!(st.raw_pages + st.compressed_pages, live.len());
            assert_eq!(st.stored_bytes, st.raw_bytes + st.compressed_bytes);
            assert_eq!(
                st.accounted_bytes,
                st.buffer_capacity_bytes + st.metadata_bytes_lower_bound
            );
        }
    }
}

#[cfg(test)]
mod lifecycle_and_retrieval_tests {
    use super::*;

    #[test]
    fn contains_and_lifecycle_state() {
        let mut s = PageStore::with_capacity(2).unwrap();

        // NeverCreated sebelum halaman pernah diterbitkan
        assert_eq!(
            s.lifecycle_state(PageId(0)),
            PageLifecycleState::NeverCreated
        );
        assert_eq!(
            s.lifecycle_state(PageId(999)),
            PageLifecycleState::NeverCreated
        );
        assert!(!s.contains(PageId(0)));

        let a = s.insert(&[10; 4096]).unwrap();
        assert!(s.contains(a));
        assert_eq!(s.lifecycle_state(a), PageLifecycleState::Active);

        let unk = PageId(999);
        assert!(!s.contains(unk));
        assert_eq!(s.lifecycle_state(unk), PageLifecycleState::NeverCreated);

        // Penghapusan eksplisit
        s.remove(a).unwrap();
        assert!(!s.contains(a));
        assert_eq!(s.lifecycle_state(a), PageLifecycleState::Removed);

        // Penghapusan berulang menghasilkan NotFound dan tetap Removed
        assert_eq!(s.remove(a), Err(Error::NotFound));
        assert_eq!(s.lifecycle_state(a), PageLifecycleState::Removed);

        // Penggusuran mencatat status Evicted
        let p1 = s.insert(&[1; 4096]).unwrap();
        let p2 = s.insert(&[2; 4096]).unwrap();
        let p3 = s
            .insert_with_policy(&[3; 4096], EvictionPolicy::Lru)
            .unwrap();
        assert_eq!(s.lifecycle_state(p1), PageLifecycleState::Evicted);
        assert_eq!(s.lifecycle_state(p2), PageLifecycleState::Active);
        assert_eq!(s.lifecycle_state(p3), PageLifecycleState::Active);
    }

    #[test]
    fn get_into_and_buffer_bounds() {
        let mut s = PageStore::with_capacity(2).unwrap();
        let payload = vec![0xAB; 4096];
        let id = s.insert(&payload).unwrap();

        let mut small_buf = [0u8; 1024];
        assert_eq!(
            s.get_into(id, &mut small_buf),
            Err(Error::OutputBufferTooSmall {
                min_required: 4096,
                actual: 1024
            })
        );

        let mut exact_buf = [0u8; 4096];
        let n = s.get_into(id, &mut exact_buf).unwrap();
        assert_eq!(n, 4096);
        assert_eq!(&exact_buf[..], &payload[..]);

        let mut large_buf = [0u8; 8192];
        let n = s.get_into(id, &mut large_buf).unwrap();
        assert_eq!(n, 4096);
        assert_eq!(&large_buf[..4096], &payload[..]);
    }

    #[test]
    fn peek_does_not_advance_tick_or_increment_hit_counter() {
        let mut s = PageStore::with_capacity(2).unwrap();
        let payload = vec![0x55; 4096];
        let id = s.insert(&payload).unwrap();

        let initial_telemetry = s.telemetry();
        assert_eq!(initial_telemetry.hit_count, 0);

        let peeked = s.peek(id).unwrap();
        assert_eq!(peeked, payload);
        assert_eq!(s.telemetry().hit_count, 0);

        let read = s.get(id).unwrap();
        assert_eq!(read, payload);
        assert_eq!(s.telemetry().hit_count, 1);
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn update_basic_and_integrity() {
        let mut s = PageStore::adaptive(4096, 2, usize::MAX).unwrap();
        let v1 = vec![0x11; 4096];
        let v2 = vec![0x22; 4096];
        let id = s.insert(&v1).unwrap();
        assert_eq!(s.get(id).unwrap(), v1);

        s.update(id, &v2).unwrap();
        assert_eq!(s.get(id).unwrap(), v2);
        assert_eq!(s.telemetry().update_count, 1);

        assert_eq!(s.update(PageId(999), &v2), Err(Error::NotFound));
        assert_eq!(
            s.update(id, &[0; 100]),
            Err(Error::InvalidLength {
                expected: 4096,
                actual: 100
            })
        );
    }

    #[test]
    fn update_budget_overflow_is_atomic() {
        let input1 = [0u8; 4096];
        let mut probe = PageStore::adaptive(4096, 2, usize::MAX).unwrap();
        let id_probe = probe.insert(&input1).unwrap();
        let cost1 = probe.stats().accounted_bytes;
        probe.remove(id_probe).unwrap();

        let mut s = PageStore::adaptive(4096, 2, cost1).unwrap();
        let id = s.insert(&input1).unwrap();

        let mut seed = 999u32;
        let random_page: Vec<u8> = (0..4096)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();

        let before_stats = s.stats();
        let res = s.update(id, &random_page);
        assert_eq!(res, Err(Error::BudgetExceeded));
        assert_eq!(s.stats(), before_stats);
        assert_eq!(s.get(id).unwrap(), input1);
    }
}

#[cfg(test)]
mod eviction_tests {
    use super::*;

    #[test]
    fn reject_policy_never_evicts() {
        let mut s = PageStore::with_capacity(2).unwrap();
        let a = s.insert(&[1; 4096]).unwrap();
        let b = s.insert(&[2; 4096]).unwrap();
        assert_eq!(s.insert(&[3; 4096]), Err(Error::Full));
        assert!(s.contains(a));
        assert!(s.contains(b));
        assert_eq!(s.evict_with_policy(EvictionPolicy::Reject), Ok(None));
    }

    #[test]
    fn lru_eviction_order() {
        let mut s = PageStore::with_capacity(3).unwrap();
        let a = s.insert(&[1; 4096]).unwrap();
        let b = s.insert(&[2; 4096]).unwrap();
        let c = s.insert(&[3; 4096]).unwrap();

        // Access A to make B the least recently accessed
        let _ = s.get(a).unwrap();

        // Inserting D with LRU policy should evict B
        let d = s
            .insert_with_policy(&[4; 4096], EvictionPolicy::Lru)
            .unwrap();
        assert!(s.contains(a));
        assert!(!s.contains(b));
        assert!(s.contains(c));
        assert!(s.contains(d));
        assert_eq!(s.telemetry().eviction_count, 1);

        // Access C, so A becomes LRU
        let _ = s.get(c).unwrap();
        let e = s
            .insert_with_policy(&[5; 4096], EvictionPolicy::Lru)
            .unwrap();
        assert!(!s.contains(a));
        assert!(s.contains(c));
        assert!(s.contains(d));
        assert!(s.contains(e));
        assert_eq!(s.telemetry().eviction_count, 2);
    }

    #[test]
    fn fifo_eviction_order() {
        let mut s = PageStore::with_capacity(3).unwrap();
        let a = s.insert(&[1; 4096]).unwrap();
        let b = s.insert(&[2; 4096]).unwrap();
        let c = s.insert(&[3; 4096]).unwrap();

        // Access A - should NOT prevent FIFO from evicting A because A was inserted first!
        let _ = s.get(a).unwrap();

        let d = s
            .insert_with_policy(&[4; 4096], EvictionPolicy::Fifo)
            .unwrap();
        assert!(!s.contains(a));
        assert!(s.contains(b));
        assert!(s.contains(c));
        assert!(s.contains(d));
        assert_eq!(s.telemetry().eviction_count, 1);
    }

    #[test]
    fn budget_constrained_lru_eviction() {
        let page = [0u8; 4096];
        let mut probe = PageStore::adaptive(4096, 5, usize::MAX).unwrap();
        let id = probe.insert(&page).unwrap();
        let single_cost = probe.stats().accounted_bytes;
        probe.remove(id).unwrap();

        // Budget fits exactly 2 pages
        let mut s = PageStore::adaptive(4096, 5, single_cost * 2).unwrap();
        let p1 = s.insert(&page).unwrap();
        let p2 = s.insert(&page).unwrap();
        assert_eq!(s.stats().page_count, 2);

        // Third insert with LRU must evict p1 to satisfy budget
        let p3 = s.insert_with_policy(&page, EvictionPolicy::Lru).unwrap();
        assert_eq!(s.stats().page_count, 2);
        assert!(!s.contains(p1));
        assert!(s.contains(p2));
        assert!(s.contains(p3));
        assert!(s.stats().accounted_bytes <= s.stats().budget_bytes);
    }

    #[test]
    fn explicit_evict() {
        let mut s = PageStore::with_capacity(2).unwrap();
        assert_eq!(s.evict(), Ok(None));
        let a = s.insert(&[1; 4096]).unwrap();
        let b = s.insert(&[2; 4096]).unwrap();

        assert_eq!(s.evict(), Ok(Some(a)));
        assert!(!s.contains(a));
        assert!(s.contains(b));
        assert_eq!(s.telemetry().eviction_count, 1);
    }

    #[test]
    fn eviction_transaction_safety_on_id_or_tick_or_budget_failure() {
        let mut s = PageStore::with_capacity(2).unwrap();
        let a = s.insert(&[1; 4096]).unwrap();
        let b = s.insert(&[2; 4096]).unwrap();
        let before_stats = s.stats();
        let before_telemetry = s.telemetry();

        // 1. Next ID exhausted: must NOT evict a or b!
        s.next = u64::MAX;
        let res = s.insert_with_policy(&[3; 4096], EvictionPolicy::Lru);
        assert_eq!(res, Err(Error::IdExhausted));
        assert!(s.contains(a));
        assert!(s.contains(b));
        assert_eq!(s.stats(), before_stats);
        assert_eq!(
            s.telemetry().eviction_count,
            before_telemetry.eviction_count
        );

        // Restore next
        s.next = 2;

        // 2. Tick exhausted: must NOT evict a or b!
        s.tick.set(u64::MAX);
        let res = s.insert_with_policy(&[3; 4096], EvictionPolicy::Lru);
        assert_eq!(res, Err(Error::IdExhausted));
        assert!(s.contains(a));
        assert!(s.contains(b));
        assert_eq!(s.stats(), before_stats);
        assert_eq!(
            s.telemetry().eviction_count,
            before_telemetry.eviction_count
        );

        // Restore tick
        s.tick.set(2);

        // 3. Candidate larger than total budget: must NOT evict b1!
        let mut budget_store = PageStore::adaptive(4096, 2, 2000).unwrap();
        let b1 = budget_store.insert(&[0; 4096]).unwrap();
        let mut random_page = vec![0u8; 4096];
        let mut seed = 42u32;
        for byte in random_page.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *byte = seed as u8;
        }
        let res = budget_store.insert_with_policy(&random_page, EvictionPolicy::Lru);
        assert_eq!(res, Err(Error::BudgetExceeded));
        assert!(budget_store.contains(b1));
        assert_eq!(budget_store.telemetry().eviction_count, 0);
    }
}

#[cfg(test)]
mod telemetry_tests {
    use super::*;

    #[test]
    fn telemetry_counters_and_ratio() {
        let mut s = PageStore::with_capacity(3).unwrap();
        let a = s.insert(&[1; 4096]).unwrap();
        let b = s.insert(&[2; 4096]).unwrap();

        // Hits and misses
        let _ = s.get(a);
        let _ = s.get(b);
        let _ = s.get(PageId(999));

        let t = s.telemetry();
        assert_eq!(t.insert_count, 2);
        assert_eq!(t.hit_count, 2);
        assert_eq!(t.miss_count, 1);
        let ratio = t.hit_ratio().unwrap();
        assert!((ratio - (2.0 / 3.0)).abs() < 1e-6);

        // Update
        s.update(a, &[3; 4096]).unwrap();
        assert_eq!(s.telemetry().update_count, 1);

        // Remove
        s.remove(b).unwrap();
        assert_eq!(s.telemetry().remove_count, 1);

        // Reset
        s.reset_telemetry();
        assert_eq!(s.telemetry(), Telemetry::default());
        assert_eq!(s.telemetry().hit_ratio(), None);
    }
}

#[cfg(test)]
mod property_invariant_tests {
    use super::*;

    #[test]
    fn extensive_randomized_invariants() {
        let mut seed = 0xCAFEBABE88u64;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        for page_size in [64, 256, 1024] {
            for capacity in [4, 8, 16] {
                let budget = (capacity / 2 + 1) * (page_size + size_of::<(PageId, Entry)>() + 64);
                let mut s = PageStore::adaptive(page_size, capacity, budget).unwrap();
                let mut oracle: HashMap<PageId, Vec<u8>> = HashMap::new();

                for _step in 0..1000 {
                    let op = (rand() % 7) as u8;
                    match op {
                        0 => {
                            // Insert with Reject policy
                            let data: Vec<u8> = (0..page_size).map(|_| rand() as u8).collect();
                            if let Ok(id) = s.insert(&data) {
                                oracle.insert(id, data);
                            }
                        }
                        1 => {
                            // Insert with LRU policy
                            let data: Vec<u8> = (0..page_size).map(|_| rand() as u8).collect();
                            if let Ok(id) = s.insert_with_policy(&data, EvictionPolicy::Lru) {
                                // Some page might have been evicted, reconcile with oracle
                                oracle.retain(|id, _| s.contains(*id));
                                oracle.insert(id, data);
                            }
                        }
                        2 => {
                            // Insert with FIFO policy
                            let data: Vec<u8> = (0..page_size).map(|_| rand() as u8).collect();
                            if let Ok(id) = s.insert_with_policy(&data, EvictionPolicy::Fifo) {
                                oracle.retain(|id, _| s.contains(*id));
                                oracle.insert(id, data);
                            }
                        }
                        3 => {
                            // Update
                            if !oracle.is_empty() {
                                let keys: Vec<_> = oracle.keys().copied().collect();
                                let target_id = keys[(rand() as usize) % keys.len()];
                                let new_data: Vec<u8> =
                                    (0..page_size).map(|_| rand() as u8).collect();
                                if s.update(target_id, &new_data).is_ok() {
                                    oracle.insert(target_id, new_data);
                                }
                            }
                        }
                        4 => {
                            // Get / Get_into / Peek
                            if !oracle.is_empty() {
                                let keys: Vec<_> = oracle.keys().copied().collect();
                                let target_id = keys[(rand() as usize) % keys.len()];
                                let expected = &oracle[&target_id];

                                assert_eq!(&s.peek(target_id).unwrap(), expected);

                                let mut buf = vec![0u8; page_size];
                                let n = s.get_into(target_id, &mut buf).unwrap();
                                assert_eq!(n, page_size);
                                assert_eq!(&buf, expected);

                                assert_eq!(&s.get(target_id).unwrap(), expected);
                            }
                        }
                        5 => {
                            // Explicit Remove
                            if !oracle.is_empty() {
                                let keys: Vec<_> = oracle.keys().copied().collect();
                                let target_id = keys[(rand() as usize) % keys.len()];
                                s.remove(target_id).unwrap();
                                oracle.remove(&target_id);
                            }
                        }
                        _ => {
                            // Evict
                            if let Ok(Some(evicted_id)) = s.evict() {
                                oracle.remove(&evicted_id);
                            }
                        }
                    }

                    // Strict Invariant Assertions on Every Step:
                    let st = s.stats();
                    assert_eq!(
                        st.accounted_bytes,
                        st.buffer_capacity_bytes + st.metadata_bytes_lower_bound,
                        "Invariant: accounted == buffer_capacity + metadata"
                    );
                    assert_eq!(
                        st.page_count,
                        st.raw_pages + st.compressed_pages,
                        "Invariant: page_count == raw + compressed"
                    );
                    assert_eq!(
                        st.stored_bytes,
                        st.raw_bytes + st.compressed_bytes,
                        "Invariant: stored_bytes == raw_bytes + compressed_bytes"
                    );
                    assert!(
                        st.page_count <= st.capacity_pages,
                        "Invariant: page_count <= capacity_pages"
                    );
                    assert!(
                        st.accounted_bytes <= st.budget_bytes,
                        "Invariant: accounted_bytes <= budget_bytes"
                    );
                    assert_eq!(
                        st.page_count,
                        oracle.len(),
                        "Invariant: page_count == oracle.len"
                    );

                    for (id, data) in &oracle {
                        assert!(s.contains(*id));
                        assert_eq!(s.lifecycle_state(*id), PageLifecycleState::Active);
                        assert_eq!(&s.peek(*id).unwrap(), data);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tiering_tests {
    use super::*;

    #[test]
    fn test_tier_query_and_roundtrip_migration() {
        let mut s = PageStore::adaptive(4096, 4, 65536).unwrap();
        // Insert a page with compressible repetitive data
        let mut data = vec![0x42u8; 4096];
        data[0] = 0xAA;
        data[4095] = 0xBB;
        let id = s.insert(&data).unwrap();

        // Compressible data starts in Compressed tier under adaptive store
        assert_eq!(s.page_tier(id).unwrap(), StorageTier::Compressed);

        // Migrate to Memory tier (raw)
        let migrated = s.migrate_page(id, StorageTier::Memory).unwrap();
        assert!(migrated);
        assert_eq!(s.page_tier(id).unwrap(), StorageTier::Memory);

        // Verify content integrity
        let read_back = s.get(id).unwrap();
        assert_eq!(read_back, data);

        // No-op migration to the same tier
        let noop = s.migrate_page(id, StorageTier::Memory).unwrap();
        assert!(!noop);
        assert_eq!(s.page_tier(id).unwrap(), StorageTier::Memory);

        // Migrate back to Compressed tier
        let migrated_back = s.migrate_page(id, StorageTier::Compressed).unwrap();
        assert!(migrated_back);
        assert_eq!(s.page_tier(id).unwrap(), StorageTier::Compressed);

        // Verify content integrity again
        let mut buf = vec![0u8; 4096];
        let bytes_read = s.get_into(id, &mut buf).unwrap();
        assert_eq!(bytes_read, 4096);
        assert_eq!(buf, data);
    }

    #[test]
    fn test_migration_accounting_delta() {
        let mut s = PageStore::adaptive(4096, 4, 65536).unwrap();
        let data = vec![0x55u8; 4096];
        let id = s.insert(&data).unwrap();

        let initial_accounted = s.stats().accounted_bytes;
        assert_eq!(s.page_tier(id).unwrap(), StorageTier::Compressed);

        // Decompress to Memory tier: accounted bytes must increase
        s.migrate_page(id, StorageTier::Memory).unwrap();
        let memory_accounted = s.stats().accounted_bytes;
        assert!(memory_accounted > initial_accounted);

        // Re-compress: accounted bytes must decrease back to initial_accounted
        s.migrate_page(id, StorageTier::Compressed).unwrap();
        assert_eq!(s.stats().accounted_bytes, initial_accounted);
    }

    #[test]
    fn test_migration_budget_protection() {
        // Budget only allows compressed entry (~100 bytes), cannot fit 4096 raw bytes
        let mut s = PageStore::adaptive(4096, 4, 1500).unwrap();
        let data = vec![0x33u8; 4096];
        let id = s.insert(&data).unwrap();
        let initial_accounted = s.stats().accounted_bytes;

        // Attempt migration to Memory: must fail with BudgetExceeded
        let res = s.migrate_page(id, StorageTier::Memory);
        assert_eq!(res, Err(Error::BudgetExceeded));

        // State remains strictly unchanged and readable
        assert_eq!(s.page_tier(id).unwrap(), StorageTier::Compressed);
        assert_eq!(s.stats().accounted_bytes, initial_accounted);
        assert_eq!(s.get(id).unwrap(), data);
    }

    #[test]
    fn test_apply_tiering_deterministic() {
        let mut s = PageStore::adaptive(4096, 8, 131072).unwrap();
        let mut ids = Vec::new();
        for i in 0..5 {
            let data = vec![i as u8; 4096];
            ids.push(s.insert(&data).unwrap());
        }

        // Apply tiering: migrate all to Memory
        let report1 = s
            .apply_tiering(|_id, _tier, _cost| StorageTier::Memory)
            .unwrap();
        assert_eq!(report1.inspected_pages, 5);
        assert_eq!(report1.migrated_to_memory, 5);
        assert_eq!(report1.no_change, 0);

        // Re-apply tiering to Memory: no changes
        let report2 = s
            .apply_tiering(|_id, _tier, _cost| StorageTier::Memory)
            .unwrap();
        assert_eq!(report2.inspected_pages, 5);
        assert_eq!(report2.migrated_to_memory, 0);
        assert_eq!(report2.no_change, 5);

        // Migrate all to Compressed
        let report3 = s
            .apply_tiering(|_id, _tier, _cost| StorageTier::Compressed)
            .unwrap();
        assert_eq!(report3.inspected_pages, 5);
        assert_eq!(report3.migrated_to_compressed, 5);
        assert_eq!(report3.no_change, 0);
        assert!(report3.net_accounted_delta < 0);
    }

    #[test]
    fn test_migration_budget_rate_limiting() {
        let mut s = PageStore::adaptive(4096, 8, 131072).unwrap();
        let mut ids = Vec::new();
        for i in 0..4 {
            let data = vec![i as u8; 4096];
            ids.push(s.insert(&data).unwrap());
        }

        // Initially all 4 pages are Compressed.
        // Apply tiering to Memory with bounded budget: max 2 pages per cycle.
        let budget = MigrationBudget::bounded(2, usize::MAX);
        let rep1 = s
            .apply_tiering_with_budget(budget, |_id, _tier, _cost| StorageTier::Memory)
            .unwrap();

        assert_eq!(rep1.inspected_pages, 4);
        assert_eq!(rep1.migrated_to_memory, 2);
        assert_eq!(rep1.deferred_by_migration_budget, 2);
        assert_eq!(rep1.no_change, 2);

        // First 2 pages are now Memory, last 2 are still Compressed
        assert_eq!(s.page_tier(ids[0]).unwrap(), StorageTier::Memory);
        assert_eq!(s.page_tier(ids[1]).unwrap(), StorageTier::Memory);
        assert_eq!(s.page_tier(ids[2]).unwrap(), StorageTier::Compressed);
        assert_eq!(s.page_tier(ids[3]).unwrap(), StorageTier::Compressed);

        // Second cycle: migrate remaining pages
        let rep2 = s
            .apply_tiering_with_budget(budget, |_id, _tier, _cost| StorageTier::Memory)
            .unwrap();

        assert_eq!(rep2.inspected_pages, 4);
        assert_eq!(rep2.migrated_to_memory, 2);
        assert_eq!(rep2.already_in_tier, 2);
        assert_eq!(rep2.deferred_by_migration_budget, 0);
        assert_eq!(rep2.no_change, 2);

        // Now all 4 pages are in Memory
        for id in &ids {
            assert_eq!(s.page_tier(*id).unwrap(), StorageTier::Memory);
        }
    }

    #[test]
    fn test_incompressible_page_protection() {
        let mut s = PageStore::adaptive(4096, 4, 65536).unwrap();

        // High entropy pseudo-random 4096 bytes
        let mut rng_seed = 0xDEAD_BEEF_CAFE_BABE_u64;
        let mut random_page = vec![0u8; 4096];
        for byte in random_page.iter_mut() {
            rng_seed ^= rng_seed << 13;
            rng_seed ^= rng_seed >> 7;
            rng_seed ^= rng_seed << 17;
            *byte = rng_seed as u8;
        }

        let id = s.insert(&random_page).unwrap();
        // Since random data cannot be compressed, adaptive store kept it in Memory (raw)
        assert_eq!(s.page_tier(id).unwrap(), StorageTier::Memory);

        // Attempting to force migration to Compressed tier must be rejected
        // because compression would expand memory rather than shrink it
        let migrated = s.migrate_page(id, StorageTier::Compressed).unwrap();
        assert!(
            !migrated,
            "Incompressible page must NOT be migrated to compressed tier"
        );
        assert_eq!(s.page_tier(id).unwrap(), StorageTier::Memory);

        // Verify data remains 100% intact
        assert_eq!(s.get(id).unwrap(), random_page);
    }
}
