#![forbid(unsafe_code)]
//! Supported opt-in application-owned memory buffer pool built on top of `PageStore`.
//!
//! Applications explicitly store owned byte buffers inside a bounded, CRC-verified,
//! adaptively compressed pool. HyperCore does not intercept arbitrary process memory;
//! only buffers explicitly placed into `HyperMemoryPool` are managed here.

use crate::{Error, PageId, PageStore, Stats, Telemetry};
use std::collections::HashMap;
use xynoos_core::{CodecId, EvictionPolicy, MemoryAccounting};

/// Opaque handle to an explicitly stored buffer in `HyperMemoryPool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferHandle(pub u64);

#[derive(Debug, Clone)]
struct BufferMeta {
    len: usize,
    pages: Vec<PageId>,
}

/// Explicit opt-in bounded memory pool for application-owned buffers.
/// Splits variable-sized application buffers across fixed-size `PageStore` pages
/// with atomic insertion rollback if capacity or memory budget is exceeded.
pub struct HyperMemoryPool {
    store: PageStore,
    next_handle: u64,
    buffers: HashMap<BufferHandle, BufferMeta>,
    logical_bytes: usize,
}

impl HyperMemoryPool {
    /// Creates a bounded adaptive pool with `page_size`, `max_pages`, and `budget_bytes`.
    pub fn new(page_size: usize, max_pages: usize, budget_bytes: usize) -> Result<Self, Error> {
        let store = PageStore::adaptive(page_size, max_pages, budget_bytes)?;
        Ok(Self {
            store,
            next_handle: 1,
            buffers: HashMap::new(),
            logical_bytes: 0,
        })
    }

    /// Creates a 4 KiB-paged pool with explicit page capacity and byte budget.
    pub fn with_budget(max_pages: usize, budget_bytes: usize) -> Result<Self, Error> {
        Self::new(4096, max_pages, budget_bytes)
    }

    pub fn page_size(&self) -> usize {
        self.store.page_size()
    }

    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    pub fn logical_bytes(&self) -> usize {
        self.logical_bytes
    }

    /// Stores an application buffer explicitly using `codec`.
    /// Atomic on failure: if any chunk exceeds capacity or budget, previously inserted
    /// chunks for this call are removed cleanly before returning an error.
    pub fn put_with_codec(&mut self, data: &[u8], codec: CodecId) -> Result<BufferHandle, Error> {
        let handle_id = self.next_handle;
        let next_handle = handle_id.checked_add(1).ok_or(Error::IdExhausted)?;
        let handle = BufferHandle(handle_id);

        let page_size = self.store.page_size();
        let num_chunks = if data.is_empty() {
            0
        } else {
            data.len().div_ceil(page_size)
        };
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(num_chunks)
            .map_err(|_| Error::Allocation)?;
        self.buffers.try_reserve(1).map_err(|_| Error::Allocation)?;

        let mut scratch_page = Vec::new();
        if num_chunks > 0 {
            scratch_page
                .try_reserve_exact(page_size)
                .map_err(|_| Error::Allocation)?;
        }

        for chunk in data.chunks(page_size) {
            let page_res = if chunk.len() == page_size {
                self.store
                    .insert_with_codec(chunk, codec, EvictionPolicy::Reject)
            } else {
                scratch_page.clear();
                scratch_page.extend_from_slice(chunk);
                scratch_page.resize(page_size, 0);
                self.store
                    .insert_with_codec(&scratch_page, codec, EvictionPolicy::Reject)
            };

            match page_res {
                Ok(pid) => pages.push(pid),
                Err(e) => {
                    for pid in pages {
                        let _ = self.store.remove(pid);
                    }
                    return Err(e);
                }
            }
        }

        self.logical_bytes = self.logical_bytes.saturating_add(data.len());
        self.next_handle = next_handle;
        self.buffers.insert(
            handle,
            BufferMeta {
                len: data.len(),
                pages,
            },
        );
        Ok(handle)
    }

    /// Stores an application buffer using the store's default codec.
    pub fn put(&mut self, data: &[u8]) -> Result<BufferHandle, Error> {
        let codec = self.store.default_codec();
        self.put_with_codec(data, codec)
    }

    /// Reads and verifies an explicitly stored buffer, returning an exact-length `Vec<u8>`.
    pub fn get(&self, handle: BufferHandle) -> Result<Vec<u8>, Error> {
        let meta = self.buffers.get(&handle).ok_or(Error::NotFound)?;
        let mut out = Vec::new();
        out.try_reserve_exact(meta.len)
            .map_err(|_| Error::Allocation)?;
        if meta.len == 0 {
            return Ok(out);
        }

        let page_size = self.store.page_size();
        let mut page_buf = vec![0u8; page_size];
        let mut remaining = meta.len;
        for &pid in &meta.pages {
            self.store.get_into(pid, &mut page_buf)?;
            let take = remaining.min(page_size);
            out.extend_from_slice(&page_buf[..take]);
            remaining -= take;
        }
        Ok(out)
    }

    /// Explicitly removes a buffer and frees all backing pages.
    pub fn remove(&mut self, handle: BufferHandle) -> Result<(), Error> {
        let meta = self.buffers.remove(&handle).ok_or(Error::NotFound)?;
        for pid in meta.pages {
            let _ = self.store.remove(pid);
        }
        self.logical_bytes = self.logical_bytes.saturating_sub(meta.len);
        Ok(())
    }

    /// Migrates all backing pages of a buffer to a target codec (`Raw`, `Lz4Standard`, `Lz4Hc`).
    pub fn set_buffer_codec(
        &mut self,
        handle: BufferHandle,
        target: CodecId,
    ) -> Result<usize, Error> {
        let pages = self
            .buffers
            .get(&handle)
            .ok_or(Error::NotFound)?
            .pages
            .clone();
        let mut changed = 0usize;
        for pid in pages {
            if self.store.migrate_page_with_codec(pid, target)? {
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Exposes mutable reference to underlying `PageStore` for AURA adaptive coordination.
    pub fn page_store_mut(&mut self) -> &mut PageStore {
        &mut self.store
    }

    pub fn page_store(&self) -> &PageStore {
        &self.store
    }

    pub fn stats(&self) -> Stats {
        self.store.stats()
    }

    pub fn telemetry(&self) -> Telemetry {
        self.store.telemetry()
    }

    pub fn overhead_accounting(&self) -> MemoryAccounting {
        self.store.overhead_accounting()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_round_trips_arbitrary_lengths_and_recovers_on_budget_failure() {
        let mut pool = HyperMemoryPool::new(4096, 8, 24 * 1024).unwrap();

        let empty = pool.put(&[]).unwrap();
        assert_eq!(pool.get(empty).unwrap(), Vec::<u8>::new());

        let unaligned: Vec<u8> = (0..10_000).map(|i| (i % 7) as u8).collect();
        let h1 = pool
            .put_with_codec(&unaligned, CodecId::Lz4Standard)
            .unwrap();
        assert_eq!(pool.get(h1).unwrap(), unaligned);
        assert_eq!(pool.logical_bytes(), 10_000);

        // Request an oversized high-entropy buffer that exceeds max_pages=8 -> must fail cleanly
        let mut seed = 0x1234_5678u64;
        let big: Vec<u8> = (0..40_000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as u8
            })
            .collect();
        let before_pages = pool.stats().page_count;
        assert!(pool.put(&big).is_err());
        assert_eq!(pool.stats().page_count, before_pages);

        // Remove and verify reclaimed pages
        pool.remove(h1).unwrap();
        assert_eq!(pool.logical_bytes(), 0);
        assert_eq!(pool.stats().page_count, 0);
    }

    #[test]
    fn pool_supports_codec_migration_without_corruption() {
        let mut pool = HyperMemoryPool::with_budget(32, usize::MAX).unwrap();
        // 16 pages (64 KiB) so net savings exceed even the 32 KiB scaled LZ4HC scratch
        let payload = vec![b'X'; 16 * 4096];
        let h = pool.put_with_codec(&payload, CodecId::Raw).unwrap();
        let moved = pool
            .set_buffer_codec(h, CodecId::Lz4Hc { level: 6 })
            .unwrap();
        assert_eq!(moved, 16);
        assert_eq!(pool.get(h).unwrap(), payload);
        let acct = pool.overhead_accounting();
        assert!(acct.stored_payload_bytes < acct.original_logical_bytes);
        assert!(acct.net_savings_bytes > 0);
    }
}
