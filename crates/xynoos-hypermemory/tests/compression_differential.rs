#![forbid(unsafe_code)]

use std::time::Instant;
use xynoos_core::{CodecId, EvictionPolicy, StorageTier};
use xynoos_hypermemory::{
    compress_exact, compress_lz4hc_into, decompress_with_codec, ClusterStore, CodecScratch, Error,
    PageStore,
};

struct Xorshift64(u64);

impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn fill_mixed(&mut self, len: usize) -> Vec<u8> {
        let mode = self.next() % 5;
        match mode {
            0 => vec![0u8; len],
            1 => {
                let pat = b"XYNOOS-LZ4HC-BOUNDARY-PATTERN-0123456789ABCDEF";
                (0..len).map(|i| pat[i % pat.len()]).collect()
            }
            2 => (0..len)
                .map(|i| {
                    if i % 32 < 24 {
                        ((i / 32) % 16) as u8
                    } else {
                        self.next() as u8
                    }
                })
                .collect(),
            3 => (0..len).map(|_| self.next() as u8).collect(),
            _ => {
                // Periodic long match with small perturbations
                let mut v = Vec::with_capacity(len);
                let mut i = 0;
                while i < len {
                    let run = ((self.next() % 300) + 1) as usize;
                    let byte = (self.next() % 4) as u8;
                    for _ in 0..run.min(len - i) {
                        v.push(byte);
                        i += 1;
                    }
                }
                v
            }
        }
    }
}

fn verify_roundtrip_both_codecs(input: &[u8], level: u8, scratch: &mut CodecScratch) {
    let written = compress_lz4hc_into(input, level, scratch).expect("lz4hc encode");
    let hc_block = scratch.encode_buf[..written].to_vec();

    // Cross-decode HyperCore LZ4HC output using independent lz4_flex block decoder
    if input.is_empty() {
        let mut out = Vec::new();
        let n = lz4_flex::block::decompress_into(&hc_block, &mut out).expect("empty decode");
        assert_eq!(n, 0);
    } else {
        let mut out = vec![0u8; input.len()];
        let n = lz4_flex::block::decompress_into(&hc_block, &mut out)
            .expect("lz4_flex decode of lz4hc");
        assert_eq!(n, input.len());
        assert_eq!(out, input);

        // Also decode via HyperCore decompress_with_codec
        let mut out2 = vec![0u8; input.len()];
        let (n2, _) =
            decompress_with_codec(CodecId::Lz4Hc { level }, &hc_block, &mut out2).unwrap();
        assert_eq!(n2, input.len());
        assert_eq!(out2, input);
    }

    // Cross-encode with lz4_flex::block::compress and decode with HyperCore decompress_with_codec
    let std_block = lz4_flex::block::compress(input);
    let mut out_std = vec![0u8; input.len()];
    let (n_std, _) = decompress_with_codec(CodecId::Lz4Standard, &std_block, &mut out_std).unwrap();
    assert_eq!(n_std, input.len());
    assert_eq!(out_std, input);
}

#[test]
fn differential_boundary_offsets_lengths_and_max_blocks() {
    let mut scratch = CodecScratch::with_limit(2 * 1024 * 1024);

    // 1. Empty and minimal sizes 0..=32
    for size in 0..=32usize {
        let data: Vec<u8> = (0..size).map(|i| (i * 11) as u8).collect();
        for level in [1u8, 4, 6, 9, 12] {
            verify_roundtrip_both_codecs(&data, level, &mut scratch);
        }
    }

    // 2. Match-offset boundaries around 4096 and MAX_DISTANCE (65535 / 65536)
    for gap in [1usize, 64, 4094, 4095, 4096, 65_500, 65_535, 65_536, 70_000] {
        let mut data = vec![0xAAu8; gap + 128];
        let mut rng = Xorshift64(0xABCD_0001 + gap as u64);
        for b in &mut data[32..gap] {
            *b = rng.next() as u8;
        }
        // Duplicate 32-byte anchor at start and after gap
        let anchor = b"XYNOOS_OFFSET_BOUNDARY_ANCHOR_32";
        data[..32].copy_from_slice(anchor);
        data[gap..gap + 32].copy_from_slice(anchor);
        for level in [1u8, 6, 12] {
            verify_roundtrip_both_codecs(&data, level, &mut scratch);
        }
    }

    // 3. Length-extension boundaries for literals (14, 15, 269, 270, 525) & matches (4, 18, 19, 273, 274)
    for lit_len in [14usize, 15, 16, 269, 270, 271, 525] {
        for match_len in [4usize, 18, 19, 20, 273, 274] {
            let mut rng = Xorshift64((lit_len as u64) << 16 | match_len as u64);
            let mut data = Vec::with_capacity(lit_len + match_len + 64);
            let pattern = vec![0x5Au8; match_len];
            data.extend_from_slice(&pattern);
            for _ in 0..lit_len {
                data.push((rng.next() as u8) | 0x80);
            }
            data.extend_from_slice(&pattern);
            data.extend_from_slice(b"TAIL_LITERALS_123456");
            verify_roundtrip_both_codecs(&data, 6, &mut scratch);
        }
    }

    // 4. Maximum supported PageStore block size (1 MiB)
    let mut rng = Xorshift64(0x1048_5760);
    let one_mib = rng.fill_mixed(1024 * 1024);
    verify_roundtrip_both_codecs(&one_mib, 6, &mut scratch);
}

#[test]
fn corrupted_truncated_and_invalid_size_inputs_never_panic() {
    let mut scratch = CodecScratch::with_limit(512 * 1024);
    let sample: Vec<u8> = (0..4096).map(|i| (i % 19) as u8).collect();
    let (valid_hc, _) = compress_exact(&sample, CodecId::Lz4Hc { level: 6 }, &mut scratch).unwrap();

    // Truncated streams
    for trunc in 0..valid_hc.len() {
        let mut out = vec![0u8; sample.len()];
        let res = decompress_with_codec(CodecId::Lz4Hc { level: 6 }, &valid_hc[..trunc], &mut out);
        assert!(
            res.is_err(),
            "truncated payload len={trunc} unexpectedly succeeded"
        );
    }

    // Output buffer too small / wrong declared original size
    if sample.len() > 64 {
        let mut small = vec![0u8; 64];
        assert!(decompress_with_codec(CodecId::Lz4Hc { level: 6 }, &valid_hc, &mut small).is_err());
        let mut large = vec![0u8; sample.len() + 64];
        assert!(decompress_with_codec(CodecId::Lz4Hc { level: 6 }, &valid_hc, &mut large).is_err());
    }

    // Deterministic bounded fuzzing against decoder robustness (2000 mutated / arbitrary inputs)
    let mut rng = Xorshift64(0xF022_DECD_1234_5678);
    for case in 0..2000usize {
        let mut payload = if case % 2 == 0 {
            let mut mutated = valid_hc.clone();
            if !mutated.is_empty() {
                let idx = (rng.next() as usize) % mutated.len();
                mutated[idx] ^= ((rng.next() % 255) + 1) as u8;
            }
            mutated
        } else {
            let len = (rng.next() as usize) % 512;
            (0..len).map(|_| rng.next() as u8).collect()
        };
        if case % 50 == 0 && !payload.is_empty() {
            payload[0] = 0xFF; // Maximal literal + match extension nibble
        }
        let mut out = vec![0u8; 4096];
        let _ = decompress_with_codec(CodecId::Lz4Hc { level: 6 }, &payload, &mut out);
    }
}

#[test]
fn bounded_fuzz_roundtrips_and_store_invariants() {
    let start = Instant::now();
    let mut rng = Xorshift64(0x4859_5045_5243_4F52);
    let mut scratch = CodecScratch::with_limit(512 * 1024);

    // 1. 400 randomized variable-length LZ4HC round-trips
    for i in 0..400usize {
        let len = (rng.next() as usize) % 32_768;
        let data = rng.fill_mixed(len);
        let level = match i % 4 {
            0 => 1,
            1 => 4,
            2 => 6,
            _ => 9,
        };
        verify_roundtrip_both_codecs(&data, level, &mut scratch);
    }

    // 2. Randomized PageStore + ClusterStore operations verifying accounting & CRC invariants
    let mut page_store = PageStore::adaptive(4096, 64, 512 * 1024).unwrap();
    let mut active_pages: Vec<(xynoos_core::PageId, Vec<u8>)> = Vec::new();
    for step in 0..300usize {
        let op = rng.next() % 5;
        match op {
            0 | 1 => {
                let data = rng.fill_mixed(4096);
                let codec = match step % 3 {
                    0 => CodecId::Raw,
                    1 => CodecId::Lz4Standard,
                    _ => CodecId::Lz4Hc { level: 6 },
                };
                if let Ok(id) = page_store.insert_with_codec(&data, codec, EvictionPolicy::Lru) {
                    active_pages.retain(|(pid, _)| page_store.contains(*pid));
                    active_pages.push((id, data));
                }
            }
            2 if !active_pages.is_empty() => {
                let idx = (rng.next() as usize) % active_pages.len();
                let (id, expected) = &active_pages[idx];
                if page_store.contains(*id) {
                    let got = page_store.get(*id).unwrap();
                    assert_eq!(&got, expected);
                }
            }
            3 if !active_pages.is_empty() => {
                let idx = (rng.next() as usize) % active_pages.len();
                let id = active_pages[idx].0;
                if page_store.contains(id) {
                    let target = match step % 3 {
                        0 => CodecId::Raw,
                        1 => CodecId::Lz4Standard,
                        _ => CodecId::Lz4Hc { level: 6 },
                    };
                    let _ = page_store.migrate_page_with_codec(id, target);
                    assert_eq!(page_store.get(id).unwrap(), active_pages[idx].1);
                }
            }
            _ if !active_pages.is_empty() => {
                let idx = (rng.next() as usize) % active_pages.len();
                let (id, _) = active_pages.swap_remove(idx);
                if page_store.contains(id) {
                    page_store.remove(id).unwrap();
                }
            }
        }
        let acct = page_store.overhead_accounting();
        assert_eq!(
            acct.total_accounted_bytes,
            acct.allocated_capacity_bytes + acct.metadata_bytes + acct.scratch_buffer_bytes
        );
    }

    // 3. ClusterStore extraction & migration invariants
    let mut cstore = ClusterStore::new(4096, 4, 16, usize::MAX).unwrap();
    let p0 = rng.fill_mixed(4096);
    let p1 = rng.fill_mixed(4096);
    let p2 = rng.fill_mixed(4096);
    let p3 = rng.fill_mixed(4096);
    let cid = cstore.insert_cluster(&[&p0, &p1, &p2, &p3]).unwrap();
    let mut page_out = vec![0u8; 4096];
    cstore.get_page_into(cid, 0, &mut page_out).unwrap();
    assert_eq!(page_out, p0);
    cstore.get_page_into(cid, 2, &mut page_out).unwrap();
    assert_eq!(page_out, p2);
    assert!(cstore.get_page_into(cid, 4, &mut page_out).is_err());
    let _ = cstore.migrate_cluster(cid, StorageTier::Memory).unwrap();
    cstore.get_page_into(cid, 3, &mut page_out).unwrap();
    assert_eq!(page_out, p3);
    let _ = cstore
        .migrate_cluster_with_codec(cid, CodecId::Lz4Hc { level: 6 })
        .unwrap();
    cstore.get_page_into(cid, 1, &mut page_out).unwrap();
    assert_eq!(page_out, p1);

    // Ensure invalid length is rejected cleanly
    assert_eq!(
        cstore.insert_cluster(&[&p0, &p1]).unwrap_err(),
        Error::InvalidLength {
            expected: 4,
            actual: 2
        }
    );
    let _elapsed = start.elapsed();
}
