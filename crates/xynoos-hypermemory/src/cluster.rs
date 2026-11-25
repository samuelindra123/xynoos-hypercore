#![forbid(unsafe_code)]
use crate::{
    codec::{compress_adaptive, compress_exact, decompress_with_codec, CodecScratch},
    Error, Telemetry, MAX_PAGE_SIZE,
};
use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    mem::size_of,
};
pub use xynoos_core::{
    ClusterId, CodecId, CompressionConfig, EvictionPolicy, MemoryAccounting, StorageTier,
};

struct ClusterEntry {
    payload_crc: u32,
    payload: Vec<u8>,
    original_cluster_crc: u32,
    page_crcs: Vec<u32>,
    codec: CodecId,
    last_accessed_tick: Cell<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClusterStats {
    pub cluster_count: usize,
    pub page_count: usize,
    pub capacity_clusters: usize,
    pub pages_per_cluster: usize,
    pub raw_clusters: usize,
    pub compressed_clusters: usize,
    pub original_bytes: usize,
    pub stored_bytes: usize,
    pub metadata_bytes: usize,
    pub buffer_capacity_bytes: usize,
    pub scratch_buffer_bytes: usize,
    pub peak_memory_bytes: usize,
    pub accounted_bytes: usize,
    pub budget_bytes: usize,
}

impl ClusterStats {
    pub fn compression_ratio(&self) -> Option<f64> {
        (self.original_bytes != 0).then(|| self.stored_bytes as f64 / self.original_bytes as f64)
    }

    pub fn space_savings_pct(&self) -> Option<f64> {
        (self.original_bytes != 0)
            .then(|| (1.0 - (self.stored_bytes as f64 / self.original_bytes as f64)) * 100.0)
    }

    pub fn net_savings_bytes(&self) -> isize {
        let total = self
            .buffer_capacity_bytes
            .saturating_add(self.metadata_bytes)
            .saturating_add(self.scratch_buffer_bytes);
        (self.original_bytes as isize).saturating_sub(total as isize)
    }
}

/// Penyimpanan multi-halaman terkelompok (*Page Clustering / Extent Storage*)
/// yang mengelompokkan beberapa halaman memori berdekatan untuk kompresi blok
/// LZ4/LZ4HC terpadu, meningkatkan rasio kompresi dan mengurangi overhead metadata.
pub struct ClusterStore {
    page_size: usize,
    pages_per_cluster: usize,
    cluster_size: usize,
    capacity: usize,
    next_id: u64,
    default_codec: CodecId,
    budget: usize,
    accounted: usize,
    peak_memory_bytes: usize,
    clusters: HashMap<ClusterId, ClusterEntry>,
    evicted_history: VecDeque<ClusterId>,
    scratch: CodecScratch,
    tick: Cell<u64>,
    hit_count: Cell<u64>,
    miss_count: Cell<u64>,
    eviction_count: Cell<u64>,
    insert_count: Cell<u64>,
    remove_count: Cell<u64>,
    corruption_count: Cell<u64>,
}

impl ClusterStore {
    pub fn new(
        page_size: usize,
        pages_per_cluster: usize,
        capacity: usize,
        budget: usize,
    ) -> Result<Self, Error> {
        if page_size == 0 || page_size > MAX_PAGE_SIZE {
            return Err(Error::InvalidPageSize);
        }
        if pages_per_cluster == 0 || pages_per_cluster > 64 {
            return Err(Error::InvalidPageSize);
        }
        let cluster_size = page_size
            .checked_mul(pages_per_cluster)
            .ok_or(Error::InvalidCapacity)?;
        if capacity
            .checked_mul(lz4_flex::block::get_maximum_output_size(cluster_size))
            .is_none()
            || capacity
                .checked_mul(size_of::<(ClusterId, ClusterEntry)>())
                .is_none()
        {
            return Err(Error::InvalidCapacity);
        }

        Ok(Self {
            page_size,
            pages_per_cluster,
            cluster_size,
            capacity,
            next_id: 0,
            default_codec: CodecId::Lz4Standard,
            budget,
            accounted: 0,
            peak_memory_bytes: 0,
            clusters: HashMap::new(),
            evicted_history: VecDeque::new(),
            scratch: CodecScratch::with_limit(
                (512 * 1024).max(lz4_flex::block::get_maximum_output_size(cluster_size)),
            ),
            tick: Cell::new(0),
            hit_count: Cell::new(0),
            miss_count: Cell::new(0),
            eviction_count: Cell::new(0),
            insert_count: Cell::new(0),
            remove_count: Cell::new(0),
            corruption_count: Cell::new(0),
        })
    }

    /// Membuat ClusterStore dengan konfigurasi default: 4 KiB per halaman, 4 halaman per cluster (16 KiB extent).
    pub fn with_default_clustering(capacity: usize) -> Result<Self, Error> {
        Self::new(4096, 4, capacity, usize::MAX)
    }

    /// Membuat ClusterStore adaptif dengan batasan budget memori fisik terakuntansi.
    pub fn adaptive(
        page_size: usize,
        pages_per_cluster: usize,
        capacity: usize,
        budget: usize,
    ) -> Result<Self, Error> {
        Self::new(page_size, pages_per_cluster, capacity, budget)
    }

    pub fn set_default_codec(&mut self, codec: CodecId) {
        self.default_codec = codec;
    }

    pub fn default_codec(&self) -> CodecId {
        self.default_codec
    }

    pub fn page_size(&self) -> usize {
        self.page_size
    }

    pub fn pages_per_cluster(&self) -> usize {
        self.pages_per_cluster
    }

    pub fn cluster_size(&self) -> usize {
        self.cluster_size
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn contains(&self, id: ClusterId) -> bool {
        self.clusters.contains_key(&id)
    }

    fn advance_tick(&self) -> Result<u64, Error> {
        let current = self.tick.get();
        let next = current.checked_add(1).ok_or(Error::IdExhausted)?;
        self.tick.set(next);
        Ok(current)
    }

    fn entry_cost(entry: &ClusterEntry) -> Option<usize> {
        entry
            .payload
            .capacity()
            .checked_add(size_of::<(ClusterId, ClusterEntry)>())?
            .checked_add(entry.page_crcs.capacity().checked_mul(size_of::<u32>())?)
    }

    fn update_peak_memory(&mut self) {
        let total = self.overhead_accounting().total_accounted_bytes;
        if total > self.peak_memory_bytes {
            self.peak_memory_bytes = total;
        }
    }

    /// Menyisipkan sekumpulan halaman sebagai satu cluster terpadu menggunakan codec eksplisit.
    pub fn insert_cluster_with_codec(
        &mut self,
        pages: &[&[u8]],
        codec: CodecId,
    ) -> Result<ClusterId, Error> {
        if pages.len() != self.pages_per_cluster {
            return Err(Error::InvalidLength {
                expected: self.pages_per_cluster,
                actual: pages.len(),
            });
        }
        for p in pages.iter() {
            if p.len() != self.page_size {
                return Err(Error::InvalidLength {
                    expected: self.page_size,
                    actual: p.len(),
                });
            }
        }
        if self.capacity == 0 || self.clusters.len() >= self.capacity {
            return Err(Error::Full);
        }

        // Pre-validasi ID dan tick sebelum alokasi/mutasi
        let next_id = self.next_id.checked_add(1).ok_or(Error::IdExhausted)?;
        let id = ClusterId(self.next_id);
        let tick = self.tick.get();
        let next_tick = tick.checked_add(1).ok_or(Error::IdExhausted)?;

        // Susun buffer kontigu sementara
        let mut raw_cluster = Vec::new();
        raw_cluster
            .try_reserve_exact(self.cluster_size)
            .map_err(|_| Error::Allocation)?;
        let mut page_crcs = Vec::new();
        page_crcs
            .try_reserve_exact(pages.len())
            .map_err(|_| Error::Allocation)?;
        for p in pages {
            page_crcs.push(crc32fast::hash(p));
            raw_cluster.extend_from_slice(p);
        }
        let original_cluster_crc = crc32fast::hash(&raw_cluster);

        let (payload, chosen_codec, _) = compress_adaptive(
            &raw_cluster,
            CompressionConfig {
                codec,
                min_savings_bytes: 1,
                max_scratch_bytes: self.scratch.max_scratch_bytes(),
            },
            &mut self.scratch,
        )?;

        let cost = payload
            .capacity()
            .checked_add(size_of::<(ClusterId, ClusterEntry)>())
            .and_then(|v| v.checked_add(page_crcs.capacity().checked_mul(size_of::<u32>())?))
            .ok_or(Error::BudgetExceeded)?;
        let accounted = self
            .accounted
            .checked_add(cost)
            .ok_or(Error::BudgetExceeded)?;
        if accounted > self.budget {
            return Err(Error::BudgetExceeded);
        }

        self.clusters
            .try_reserve(1)
            .map_err(|_| Error::Allocation)?;

        let entry = ClusterEntry {
            payload_crc: crc32fast::hash(&payload),
            payload,
            original_cluster_crc,
            page_crcs,
            codec: chosen_codec,
            last_accessed_tick: Cell::new(tick),
        };

        self.clusters.insert(id, entry);
        self.next_id = next_id;
        self.tick.set(next_tick);
        self.accounted = accounted;
        self.insert_count
            .set(self.insert_count.get().saturating_add(1));
        self.update_peak_memory();
        Ok(id)
    }

    /// Menyisipkan sekumpulan halaman sebagai satu cluster terpadu.
    /// Memvalidasi integritas setiap sub-halaman dan menerapkan kompresi blok
    /// pada tingkat kluster untuk rasio kompresi optimal.
    pub fn insert_cluster(&mut self, pages: &[&[u8]]) -> Result<ClusterId, Error> {
        self.insert_cluster_with_codec(pages, self.default_codec)
    }

    fn decompress_cluster_verified<'a>(
        &'a self,
        id: ClusterId,
        out: &mut [u8],
    ) -> Result<&'a ClusterEntry, Error> {
        let entry = match self.clusters.get(&id) {
            Some(e) => e,
            None => {
                self.miss_count.set(self.miss_count.get().saturating_add(1));
                return Err(Error::NotFound);
            }
        };

        if crc32fast::hash(&entry.payload) != entry.payload_crc {
            self.corruption_count
                .set(self.corruption_count.get().saturating_add(1));
            return Err(Error::PayloadChecksumMismatch);
        }

        let (written, _) =
            decompress_with_codec(entry.codec, &entry.payload, &mut out[..self.cluster_size])?;
        if written != self.cluster_size {
            return Err(Error::Decompression);
        }

        if crc32fast::hash(&out[..self.cluster_size]) != entry.original_cluster_crc {
            self.corruption_count
                .set(self.corruption_count.get().saturating_add(1));
            return Err(Error::ChecksumMismatch);
        }

        Ok(entry)
    }

    /// Mengambil dekompresi seluruh kluster ke buffer output pemanggil.
    pub fn get_cluster_into(&self, id: ClusterId, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() < self.cluster_size {
            return Err(Error::OutputBufferTooSmall {
                min_required: self.cluster_size,
                actual: out.len(),
            });
        }
        let entry = self.decompress_cluster_verified(id, out)?;
        let tick = self.advance_tick()?;
        entry.last_accessed_tick.set(tick);
        self.hit_count.set(self.hit_count.get().saturating_add(1));
        Ok(self.cluster_size)
    }

    /// Mengambil satu sub-halaman menggunakan buffer scratch yang dapat dipakai ulang
    /// untuk menghindari alokasi heap pada akses berulang.
    pub fn get_page_with_scratch(
        &self,
        id: ClusterId,
        page_index: usize,
        out: &mut [u8],
        scratch: &mut CodecScratch,
    ) -> Result<usize, Error> {
        if page_index >= self.pages_per_cluster {
            return Err(Error::NotFound);
        }
        if out.len() < self.page_size {
            return Err(Error::OutputBufferTooSmall {
                min_required: self.page_size,
                actual: out.len(),
            });
        }
        let entry = match self.clusters.get(&id) {
            Some(e) => e,
            None => {
                self.miss_count.set(self.miss_count.get().saturating_add(1));
                return Err(Error::NotFound);
            }
        };

        let start = page_index * self.page_size;
        let end = start + self.page_size;

        if entry.codec == CodecId::Raw {
            if crc32fast::hash(&entry.payload) != entry.payload_crc {
                self.corruption_count
                    .set(self.corruption_count.get().saturating_add(1));
                return Err(Error::PayloadChecksumMismatch);
            }
            if entry.payload.len() != self.cluster_size {
                return Err(Error::Decompression);
            }
            out[..self.page_size].copy_from_slice(&entry.payload[start..end]);
        } else {
            let cluster_buf = scratch.ensure_decode_capacity(self.cluster_size)?;
            self.decompress_cluster_verified(id, cluster_buf)?;
            out[..self.page_size].copy_from_slice(&cluster_buf[start..end]);
        }

        let expected_page_crc = entry.page_crcs[page_index];
        if crc32fast::hash(&out[..self.page_size]) != expected_page_crc {
            self.corruption_count
                .set(self.corruption_count.get().saturating_add(1));
            return Err(Error::ChecksumMismatch);
        }

        let tick = self.advance_tick()?;
        entry.last_accessed_tick.set(tick);
        self.hit_count.set(self.hit_count.get().saturating_add(1));
        Ok(self.page_size)
    }

    /// Mengambil satu sub-halaman spesifik dari dalam kluster berdasarkan indeks halaman.
    /// Melakukan validasi CRC32 spesifik terhadap sub-halaman tersebut.
    pub fn get_page_into(
        &self,
        id: ClusterId,
        page_index: usize,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let mut local_scratch = CodecScratch::with_limit(self.cluster_size);
        self.get_page_with_scratch(id, page_index, out, &mut local_scratch)
    }

    /// Mengetahui tingkat penyimpanan (StorageTier) dari kluster saat ini.
    pub fn cluster_tier(&self, id: ClusterId) -> Result<StorageTier, Error> {
        match self.clusters.get(&id) {
            Some(entry) => Ok(entry.codec.tier()),
            None => Err(Error::NotFound),
        }
    }

    /// Mengetahui codec yang menyimpan kluster saat ini.
    pub fn cluster_codec(&self, id: ClusterId) -> Result<CodecId, Error> {
        self.clusters
            .get(&id)
            .map(|e| e.codec)
            .ok_or(Error::NotFound)
    }

    /// Memigrasikan kluster secara in-place ke codec target.
    pub fn migrate_cluster_with_codec(
        &mut self,
        id: ClusterId,
        target_codec: CodecId,
    ) -> Result<bool, Error> {
        let entry = self.clusters.get(&id).ok_or(Error::NotFound)?;
        if entry.codec == target_codec {
            return Ok(false);
        }

        let old_cost = Self::entry_cost(entry).expect("validated cost");
        if crc32fast::hash(&entry.payload) != entry.payload_crc {
            self.corruption_count
                .set(self.corruption_count.get().saturating_add(1));
            return Err(Error::PayloadChecksumMismatch);
        }

        let current_codec = entry.codec;
        let original_cluster_crc = entry.original_cluster_crc;
        let page_crcs_cap = entry.page_crcs.capacity();

        match target_codec {
            CodecId::Raw => {
                let mut raw_buf = Vec::new();
                raw_buf
                    .try_reserve_exact(self.cluster_size)
                    .map_err(|_| Error::Allocation)?;
                raw_buf.resize(self.cluster_size, 0);
                let (n, _) = decompress_with_codec(current_codec, &entry.payload, &mut raw_buf)?;
                if n != self.cluster_size {
                    return Err(Error::Decompression);
                }
                if crc32fast::hash(&raw_buf) != original_cluster_crc {
                    self.corruption_count
                        .set(self.corruption_count.get().saturating_add(1));
                    return Err(Error::ChecksumMismatch);
                }

                let new_cost = raw_buf
                    .capacity()
                    .checked_add(size_of::<(ClusterId, ClusterEntry)>())
                    .and_then(|v| v.checked_add(page_crcs_cap.checked_mul(size_of::<u32>())?))
                    .ok_or(Error::BudgetExceeded)?;

                let new_accounted = self.checked_recompute_accounted(old_cost, new_cost)?;
                let entry = self.clusters.get_mut(&id).expect("checked existence");
                entry.payload = raw_buf;
                entry.codec = CodecId::Raw;
                entry.payload_crc = crc32fast::hash(&entry.payload);
                self.accounted = new_accounted;
                self.update_peak_memory();
                Ok(true)
            }
            CodecId::Lz4Standard | CodecId::Lz4Hc { .. } => {
                let (compressed, old_cap) = if current_codec == CodecId::Raw {
                    let old_cap = entry.payload.capacity();
                    let raw_slice = entry.payload.clone();
                    let (comp, _) = compress_exact(&raw_slice, target_codec, &mut self.scratch)?;
                    (comp, old_cap)
                } else {
                    let old_cap = entry.payload.capacity();
                    let mut raw_buf = Vec::new();
                    raw_buf
                        .try_reserve_exact(self.cluster_size)
                        .map_err(|_| Error::Allocation)?;
                    raw_buf.resize(self.cluster_size, 0);
                    let (n, _) =
                        decompress_with_codec(current_codec, &entry.payload, &mut raw_buf)?;
                    if n != self.cluster_size {
                        return Err(Error::Decompression);
                    }
                    if crc32fast::hash(&raw_buf) != original_cluster_crc {
                        self.corruption_count
                            .set(self.corruption_count.get().saturating_add(1));
                        return Err(Error::ChecksumMismatch);
                    }
                    let (comp, _) = compress_exact(&raw_buf, target_codec, &mut self.scratch)?;
                    (comp, old_cap)
                };

                if compressed.capacity() >= old_cap {
                    return Ok(false);
                }

                let new_cost = compressed
                    .capacity()
                    .checked_add(size_of::<(ClusterId, ClusterEntry)>())
                    .and_then(|v| v.checked_add(page_crcs_cap.checked_mul(size_of::<u32>())?))
                    .ok_or(Error::BudgetExceeded)?;
                let new_accounted = self.checked_recompute_accounted(old_cost, new_cost)?;

                let entry = self.clusters.get_mut(&id).expect("checked existence");
                entry.payload = compressed;
                entry.codec = target_codec;
                entry.payload_crc = crc32fast::hash(&entry.payload);
                self.accounted = new_accounted;
                self.update_peak_memory();
                Ok(true)
            }
        }
    }

    fn checked_recompute_accounted(
        &self,
        old_cost: usize,
        new_cost: usize,
    ) -> Result<usize, Error> {
        if new_cost >= old_cost {
            let diff = new_cost - old_cost;
            let acc = self
                .accounted
                .checked_add(diff)
                .ok_or(Error::BudgetExceeded)?;
            if acc > self.budget {
                return Err(Error::BudgetExceeded);
            }
            Ok(acc)
        } else {
            let diff = old_cost - new_cost;
            Ok(self
                .accounted
                .checked_sub(diff)
                .expect("accounting invariant"))
        }
    }

    /// Memigrasikan kluster secara in-place antar tingkatan Memory (raw) dan Compressed.
    pub fn migrate_cluster(&mut self, id: ClusterId, target: StorageTier) -> Result<bool, Error> {
        let target_codec = match target {
            StorageTier::Memory => CodecId::Raw,
            StorageTier::Compressed => {
                let current = self.cluster_codec(id)?;
                if current.is_compressed() {
                    return Ok(false);
                }
                if self.default_codec.is_compressed() {
                    self.default_codec
                } else {
                    CodecId::Lz4Standard
                }
            }
        };
        self.migrate_cluster_with_codec(id, target_codec)
    }

    fn remove_internal(&mut self, id: ClusterId, is_eviction: bool) -> Result<(), Error> {
        let entry = self.clusters.remove(&id).ok_or(Error::NotFound)?;
        let cost = Self::entry_cost(&entry).expect("validated cost");
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
            self.evicted_history.retain(|x| *x != id);
            self.remove_count
                .set(self.remove_count.get().saturating_add(1));
        }
        Ok(())
    }

    /// Menggusur kluster paling lama tidak diakses (LRU).
    pub fn evict_lru(&mut self) -> Result<Option<ClusterId>, Error> {
        let candidate_id = self
            .clusters
            .iter()
            .min_by_key(|(id, e)| (e.last_accessed_tick.get(), id.0))
            .map(|(id, _)| *id);

        if let Some(id) = candidate_id {
            self.remove_internal(id, true)?;
            Ok(Some(id))
        } else {
            Ok(None)
        }
    }

    /// Menghapus kluster dari store.
    pub fn remove(&mut self, id: ClusterId) -> Result<(), Error> {
        self.remove_internal(id, false)
    }

    /// Akuntansi memori ketat termasuk metadata, kapasitas teralokasi, dan scratch buffer.
    pub fn overhead_accounting(&self) -> MemoryAccounting {
        let mut stored_payload_bytes = 0usize;
        let mut allocated_capacity_bytes = 0usize;
        let mut entry_metadata_bytes = 0usize;

        for entry in self.clusters.values() {
            stored_payload_bytes = stored_payload_bytes
                .checked_add(entry.payload.len())
                .expect("accounting invariant");
            allocated_capacity_bytes = allocated_capacity_bytes
                .checked_add(entry.payload.capacity())
                .expect("accounting invariant");
            entry_metadata_bytes = entry_metadata_bytes
                .checked_add(size_of::<(ClusterId, ClusterEntry)>())
                .and_then(|v| {
                    v.checked_add(entry.page_crcs.capacity().saturating_mul(size_of::<u32>()))
                })
                .expect("accounting invariant");
        }

        let history_bytes = self
            .evicted_history
            .capacity()
            .saturating_mul(size_of::<ClusterId>());
        let metadata_bytes = entry_metadata_bytes
            .saturating_add(history_bytes)
            .saturating_add(size_of::<Self>());
        let scratch_buffer_bytes = self.scratch.allocated_bytes();
        let total_accounted_bytes = allocated_capacity_bytes
            .saturating_add(metadata_bytes)
            .saturating_add(scratch_buffer_bytes);
        let original_logical_bytes = self
            .clusters
            .len()
            .checked_mul(self.cluster_size)
            .expect("validated capacity");
        let net_savings_bytes =
            (original_logical_bytes as isize).saturating_sub(total_accounted_bytes as isize);

        MemoryAccounting {
            original_logical_bytes,
            stored_payload_bytes,
            metadata_bytes,
            allocated_capacity_bytes,
            scratch_buffer_bytes,
            peak_memory_bytes: self.peak_memory_bytes.max(total_accounted_bytes),
            total_accounted_bytes,
            net_savings_bytes,
        }
    }

    /// Statistik penggunaan memori dan efisiensi kluster.
    pub fn stats(&self) -> ClusterStats {
        let mut raw_clusters = 0usize;
        let mut compressed_clusters = 0usize;
        let mut stored_bytes = 0usize;
        let mut buffer_capacity_bytes = 0usize;
        let mut metadata_bytes = 0usize;

        for entry in self.clusters.values() {
            if entry.codec == CodecId::Raw {
                raw_clusters += 1;
            } else {
                compressed_clusters += 1;
            }
            stored_bytes = stored_bytes
                .checked_add(entry.payload.len())
                .expect("accounting invariant");
            buffer_capacity_bytes = buffer_capacity_bytes
                .checked_add(entry.payload.capacity())
                .expect("accounting invariant");
            metadata_bytes = metadata_bytes
                .checked_add(size_of::<(ClusterId, ClusterEntry)>())
                .and_then(|v| {
                    v.checked_add(entry.page_crcs.capacity().saturating_mul(size_of::<u32>()))
                })
                .expect("accounting invariant");
        }

        let cluster_count = self.clusters.len();
        let page_count = cluster_count
            .checked_mul(self.pages_per_cluster)
            .expect("validated capacity");
        let original_bytes = page_count
            .checked_mul(self.page_size)
            .expect("validated capacity");
        let scratch_buffer_bytes = self.scratch.allocated_bytes();
        let peak_memory_bytes = self.peak_memory_bytes.max(
            buffer_capacity_bytes
                .saturating_add(metadata_bytes)
                .saturating_add(scratch_buffer_bytes),
        );

        ClusterStats {
            cluster_count,
            page_count,
            capacity_clusters: self.capacity,
            pages_per_cluster: self.pages_per_cluster,
            raw_clusters,
            compressed_clusters,
            original_bytes,
            stored_bytes,
            metadata_bytes,
            buffer_capacity_bytes,
            scratch_buffer_bytes,
            peak_memory_bytes,
            accounted_bytes: self.accounted,
            budget_bytes: self.budget,
        }
    }

    pub fn telemetry(&self) -> Telemetry {
        Telemetry {
            hit_count: self.hit_count.get(),
            miss_count: self.miss_count.get(),
            eviction_count: self.eviction_count.get(),
            insert_count: self.insert_count.get(),
            update_count: 0,
            remove_count: self.remove_count.get(),
            corruption_count: self.corruption_count.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_page(pattern: u8, size: usize) -> Vec<u8> {
        vec![pattern; size]
    }

    #[test]
    fn test_cluster_store_creation_and_bounds() {
        assert!(ClusterStore::new(0, 4, 10, 1000).is_err());
        assert!(ClusterStore::new(4096, 0, 10, 1000).is_err());
        assert!(ClusterStore::new(4096, 65, 10, 1000).is_err());

        let store = ClusterStore::with_default_clustering(16).unwrap();
        assert_eq!(store.page_size(), 4096);
        assert_eq!(store.pages_per_cluster(), 4);
        assert_eq!(store.cluster_size(), 16384);
        assert_eq!(store.capacity(), 16);
    }

    #[test]
    fn test_cluster_insert_and_get_whole_cluster() {
        let mut store = ClusterStore::with_default_clustering(8).unwrap();
        let p1 = generate_page(0xAA, 4096);
        let p2 = generate_page(0xBB, 4096);
        let p3 = generate_page(0xCC, 4096);
        let p4 = generate_page(0xDD, 4096);

        let id = store.insert_cluster(&[&p1, &p2, &p3, &p4]).unwrap();
        assert!(store.contains(id));

        let mut out = vec![0u8; 16384];
        let n = store.get_cluster_into(id, &mut out).unwrap();
        assert_eq!(n, 16384);
        assert_eq!(&out[0..4096], &p1[..]);
        assert_eq!(&out[4096..8192], &p2[..]);
        assert_eq!(&out[8192..12288], &p3[..]);
        assert_eq!(&out[12288..16384], &p4[..]);
    }

    #[test]
    fn test_cluster_get_individual_page() {
        let mut store = ClusterStore::with_default_clustering(8).unwrap();
        let p1 = generate_page(0x11, 4096);
        let p2 = generate_page(0x22, 4096);
        let p3 = generate_page(0x33, 4096);
        let p4 = generate_page(0x44, 4096);

        let id = store.insert_cluster(&[&p1, &p2, &p3, &p4]).unwrap();

        let mut page_buf = vec![0u8; 4096];

        // Retrieve each page independently
        store.get_page_into(id, 0, &mut page_buf).unwrap();
        assert_eq!(&page_buf, &p1);

        store.get_page_into(id, 1, &mut page_buf).unwrap();
        assert_eq!(&page_buf, &p2);

        store.get_page_into(id, 2, &mut page_buf).unwrap();
        assert_eq!(&page_buf, &p3);

        store.get_page_into(id, 3, &mut page_buf).unwrap();
        assert_eq!(&page_buf, &p4);

        // Invalid page index
        assert!(store.get_page_into(id, 4, &mut page_buf).is_err());
    }

    #[test]
    fn test_cluster_compression_ratio_savings() {
        let mut store = ClusterStore::with_default_clustering(8).unwrap();
        // Structured data that compresses well across pages
        let mut p1 = vec![0u8; 4096];
        let mut p2 = vec![0u8; 4096];
        let mut p3 = vec![0u8; 4096];
        let mut p4 = vec![0u8; 4096];
        for i in 0..4096 {
            p1[i] = (i % 32) as u8;
            p2[i] = (i % 32) as u8;
            p3[i] = (i % 32) as u8;
            p4[i] = (i % 32) as u8;
        }

        let id = store.insert_cluster(&[&p1, &p2, &p3, &p4]).unwrap();
        assert_eq!(store.cluster_tier(id).unwrap(), StorageTier::Compressed);

        let stats = store.stats();
        assert_eq!(stats.original_bytes, 16384);
        assert!(
            stats.stored_bytes < 1000,
            "Stored bytes should be heavily compressed: {}",
            stats.stored_bytes
        );
        assert!(stats.space_savings_pct().unwrap() > 90.0);
    }

    #[test]
    fn test_cluster_incompressible_data_protection() {
        let mut store = ClusterStore::with_default_clustering(8).unwrap();
        // High-entropy pseudorandom data
        let mut seed = 0x12345678u64;
        let mut rand_page = || {
            let mut v = vec![0u8; 4096];
            for byte in v.iter_mut() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                *byte = (seed >> 33) as u8;
            }
            v
        };
        let p1 = rand_page();
        let p2 = rand_page();
        let p3 = rand_page();
        let p4 = rand_page();

        let id = store.insert_cluster(&[&p1, &p2, &p3, &p4]).unwrap();
        // Should remain in Memory (raw) tier because compressed size would exceed original
        assert_eq!(store.cluster_tier(id).unwrap(), StorageTier::Memory);
        let stats = store.stats();
        assert_eq!(stats.raw_clusters, 1);
        assert_eq!(stats.compressed_clusters, 0);
    }

    #[test]
    fn test_cluster_migration_memory_and_compressed() {
        let mut store = ClusterStore::with_default_clustering(8).unwrap();
        let p1 = generate_page(0x55, 4096);
        let p2 = generate_page(0x66, 4096);
        let p3 = generate_page(0x77, 4096);
        let p4 = generate_page(0x88, 4096);

        let id = store.insert_cluster(&[&p1, &p2, &p3, &p4]).unwrap();
        assert_eq!(store.cluster_tier(id).unwrap(), StorageTier::Compressed);

        // Migrate to Memory
        assert!(store.migrate_cluster(id, StorageTier::Memory).unwrap());
        assert_eq!(store.cluster_tier(id).unwrap(), StorageTier::Memory);

        // Re-read after migration to memory
        let mut out = vec![0u8; 16384];
        store.get_cluster_into(id, &mut out).unwrap();
        assert_eq!(&out[0..4096], &p1[..]);

        // Migrate back to Compressed
        assert!(store.migrate_cluster(id, StorageTier::Compressed).unwrap());
        assert_eq!(store.cluster_tier(id).unwrap(), StorageTier::Compressed);

        // Re-read after compression
        store.get_cluster_into(id, &mut out).unwrap();
        assert_eq!(&out[0..4096], &p1[..]);
    }

    #[test]
    fn test_cluster_eviction_lru() {
        let mut store = ClusterStore::with_default_clustering(2).unwrap();
        let p1 = generate_page(1, 4096);
        let p2 = generate_page(2, 4096);
        let p3 = generate_page(3, 4096);
        let p4 = generate_page(4, 4096);

        let id1 = store.insert_cluster(&[&p1, &p1, &p1, &p1]).unwrap();
        let id2 = store.insert_cluster(&[&p2, &p2, &p2, &p2]).unwrap();

        // Access id1 to make id2 LRU
        let mut buf = vec![0u8; 4096];
        store.get_page_into(id1, 0, &mut buf).unwrap();

        // Evict LRU -> should evict id2
        let evicted = store.evict_lru().unwrap();
        assert_eq!(evicted, Some(id2));
        assert!(store.contains(id1));
        assert!(!store.contains(id2));

        // Now capacity allows a new cluster
        let id3 = store.insert_cluster(&[&p3, &p3, &p4, &p4]).unwrap();
        assert!(store.contains(id3));
    }

    #[test]
    fn test_cluster_get_page_single_hit_and_bounded_eviction_history() {
        let mut store = ClusterStore::with_default_clustering(2).unwrap();
        let p = generate_page(0x2A, 4096);
        let id = store.insert_cluster(&[&p, &p, &p, &p]).unwrap();

        let mut buf = vec![0u8; 4096];
        let mut scratch = CodecScratch::with_limit(16384);
        store
            .get_page_with_scratch(id, 2, &mut buf, &mut scratch)
            .unwrap();
        assert_eq!(buf, p);
        assert_eq!(store.telemetry().hit_count, 1);

        // Repeatedly insert & evict to verify evicted_history stays strictly bounded
        for idx in 0..100u8 {
            let page = generate_page(idx, 4096);
            if store.clusters.len() >= store.capacity() {
                store.evict_lru().unwrap();
            }
            store.insert_cluster(&[&page, &page, &page, &page]).unwrap();
        }
        assert!(store.evicted_history.len() <= 16);

        let acct = store.overhead_accounting();
        assert_eq!(
            acct.total_accounted_bytes,
            acct.allocated_capacity_bytes + acct.metadata_bytes + acct.scratch_buffer_bytes
        );
    }

    #[test]
    fn test_cluster_lz4hc_codec_migration_and_accounting() {
        let mut store = ClusterStore::with_default_clustering(4).unwrap();
        let mut pages = Vec::new();
        for p in 0..4usize {
            let mut buf = Vec::with_capacity(4096);
            for i in 0..128 {
                let s = format!(
                    "{{\"k\":{},\"v\":\"cluster_data_{}\",\"n\":{}}}\n",
                    i,
                    p,
                    i * 3
                );
                buf.extend_from_slice(s.as_bytes());
            }
            buf.resize(4096, b' ');
            pages.push(buf);
        }
        let refs: Vec<&[u8]> = pages.iter().map(|v| v.as_slice()).collect();
        let id = store
            .insert_cluster_with_codec(&refs, CodecId::Lz4Standard)
            .unwrap();
        let std_bytes = store.stats().stored_bytes;
        assert_eq!(store.cluster_codec(id).unwrap(), CodecId::Lz4Standard);

        let migrated = store
            .migrate_cluster_with_codec(id, CodecId::Lz4Hc { level: 9 })
            .unwrap();
        if migrated {
            assert_eq!(
                store.cluster_codec(id).unwrap(),
                CodecId::Lz4Hc { level: 9 }
            );
            assert!(store.stats().stored_bytes < std_bytes);
        }

        let mut out = vec![0u8; 4096];
        for (idx, expected) in pages.iter().enumerate() {
            store.get_page_into(id, idx, &mut out).unwrap();
            assert_eq!(out, *expected);
        }
    }
}
