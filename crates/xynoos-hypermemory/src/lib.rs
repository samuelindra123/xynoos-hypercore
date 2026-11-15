#![forbid(unsafe_code)]
use std::{collections::HashMap, fmt, mem::size_of};
use xynoos_core::PageId;
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
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
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
/// Batas ukuran simulasi untuk membatasi alokasi setiap pengambilan.
pub const MAX_PAGE_SIZE: usize = 1024 * 1024;
struct Entry {
    payload: Vec<u8>,
    payload_crc: u32,
    original_crc: u32,
    raw: bool,
}
pub struct PageStore {
    page_size: usize,
    capacity: usize,
    next: u64,
    adaptive: bool,
    budget: usize,
    accounted: usize,
    pages: HashMap<PageId, Entry>,
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
    pub fn insert(&mut self, data: &[u8]) -> Result<PageId, Error> {
        if data.len() != self.page_size {
            return Err(Error::InvalidLength {
                expected: self.page_size,
                actual: data.len(),
            });
        }
        if self.pages.len() >= self.capacity {
            return Err(Error::Full);
        }
        let next = self.next.checked_add(1).ok_or(Error::IdExhausted)?;
        let id = PageId(self.next);
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
        let accounted = self
            .accounted
            .checked_add(cost)
            .ok_or(Error::BudgetExceeded)?;
        if accounted > self.budget {
            return Err(Error::BudgetExceeded);
        }
        self.pages.try_reserve(1).map_err(|_| Error::Allocation)?;
        let entry = Entry {
            payload_crc: crc32fast::hash(&payload),
            original_crc: crc32fast::hash(data),
            payload,
            raw,
        };
        self.pages.insert(id, entry);
        self.next = next;
        self.accounted = accounted;
        Ok(id)
    }
    pub fn get(&self, id: PageId) -> Result<Vec<u8>, Error> {
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
    pub fn remove(&mut self, id: PageId) -> Result<(), Error> {
        let entry = self.pages.remove(&id).ok_or(Error::NotFound)?;
        self.accounted = self
            .accounted
            .checked_sub(
                entry
                    .payload
                    .capacity()
                    .checked_add(size_of::<(PageId, Entry)>())
                    .expect("validated entry cost"),
            )
            .expect("accounting invariant");
        Ok(())
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
