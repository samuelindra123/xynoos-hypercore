#![forbid(unsafe_code)]
use crate::Error;
use std::time::Instant;
pub use xynoos_core::{CodecId, CompressionConfig};

const MIN_MATCH: usize = 4;
const LAST_LITERALS: usize = 5;
const MF_LIMIT: usize = 12;
const MAX_DISTANCE: usize = 65_535;
const MAX_HASH_BITS: u32 = 15;
const CHAIN_SIZE: usize = 65_536;

/// Maximum supported input block size (16 MiB) to strictly bound scratch allocation.
pub const MAX_BLOCK_SIZE: usize = 16 * 1024 * 1024;

/// Metadata stored alongside every compressed or raw object in HyperMemory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressedObjectMeta {
    pub codec: CodecId,
    pub original_size: usize,
    pub compressed_size: usize,
    pub allocated_capacity: usize,
    pub payload_crc: u32,
    pub original_crc: u32,
    pub compress_cost_ns: u64,
    pub last_decompress_cost_ns: u64,
}

/// Reusable bounded scratch buffers for compression and decompression to avoid
/// per-operation heap churn.
#[derive(Debug, Clone)]
pub struct CodecScratch {
    pub encode_buf: Vec<u8>,
    pub decode_buf: Vec<u8>,
    hc_head: Vec<i32>,
    hc_chain: Vec<i32>,
    max_scratch_bytes: usize,
}

impl Default for CodecScratch {
    fn default() -> Self {
        Self::with_limit(512 * 1024)
    }
}

impl CodecScratch {
    pub fn with_limit(max_scratch_bytes: usize) -> Self {
        Self {
            encode_buf: Vec::new(),
            decode_buf: Vec::new(),
            hc_head: Vec::new(),
            hc_chain: Vec::new(),
            max_scratch_bytes,
        }
    }

    /// Total allocated scratch buffer bytes currently held by this scratch instance.
    pub fn allocated_bytes(&self) -> usize {
        self.encode_buf
            .capacity()
            .saturating_add(self.decode_buf.capacity())
            .saturating_add(self.hc_head.capacity().saturating_mul(4))
            .saturating_add(self.hc_chain.capacity().saturating_mul(4))
    }

    pub fn max_scratch_bytes(&self) -> usize {
        self.max_scratch_bytes
    }

    fn ensure_encode_capacity(&mut self, needed: usize) -> Result<(), Error> {
        if needed > self.max_scratch_bytes {
            return Err(Error::BudgetExceeded);
        }
        if self.encode_buf.capacity() < needed {
            let additional = needed - self.encode_buf.capacity();
            self.encode_buf
                .try_reserve(additional)
                .map_err(|_| Error::Allocation)?;
        }
        self.encode_buf.clear();
        Ok(())
    }

    pub fn ensure_decode_capacity(&mut self, needed: usize) -> Result<&mut [u8], Error> {
        if needed > self.max_scratch_bytes {
            return Err(Error::BudgetExceeded);
        }
        if self.decode_buf.capacity() < needed {
            let additional = needed - self.decode_buf.capacity();
            self.decode_buf
                .try_reserve(additional)
                .map_err(|_| Error::Allocation)?;
        }
        self.decode_buf.resize(needed, 0);
        Ok(&mut self.decode_buf[..needed])
    }

    fn prepare_hc_tables(&mut self, input_len: usize) -> Result<u32, Error> {
        let hash_bits = if input_len <= 4096 {
            12
        } else if input_len <= 16_384 {
            13
        } else {
            MAX_HASH_BITS
        };
        let hash_size = 1usize << hash_bits;
        let chain_len = input_len.min(CHAIN_SIZE);
        let needed_bytes = hash_size
            .saturating_mul(4)
            .saturating_add(chain_len.saturating_mul(4));
        if needed_bytes > self.max_scratch_bytes {
            return Err(Error::BudgetExceeded);
        }
        if self.hc_head.len() < hash_size {
            self.hc_head
                .try_reserve_exact(hash_size - self.hc_head.len())
                .map_err(|_| Error::Allocation)?;
            self.hc_head.resize(hash_size, -1);
        } else {
            self.hc_head[..hash_size].fill(-1);
        }
        if self.hc_chain.len() < chain_len {
            self.hc_chain
                .try_reserve_exact(chain_len - self.hc_chain.len())
                .map_err(|_| Error::Allocation)?;
            self.hc_chain.resize(chain_len, -1);
        } else {
            self.hc_chain[..chain_len].fill(-1);
        }
        Ok(hash_bits)
    }
}

#[inline]
fn hash4(input: &[u8], pos: usize, hash_bits: u32) -> usize {
    let val = u32::from_le_bytes([input[pos], input[pos + 1], input[pos + 2], input[pos + 3]]);
    (val.wrapping_mul(2_654_435_761) >> (32 - hash_bits)) as usize
}

#[inline]
fn write_length_ext(out: &mut Vec<u8>, mut len: usize) {
    while len >= 255 {
        out.push(255);
        len -= 255;
    }
    out.push(len as u8);
}

fn emit_sequence(
    out: &mut Vec<u8>,
    literals: &[u8],
    offset: u16,
    match_len: usize,
) -> Result<(), Error> {
    let lit_len = literals.len();
    let ml_code = match_len.saturating_sub(MIN_MATCH);
    let token = ((lit_len.min(15) as u8) << 4) | (ml_code.min(15) as u8);
    out.push(token);
    if lit_len >= 15 {
        write_length_ext(out, lit_len - 15);
    }
    out.extend_from_slice(literals);
    out.extend_from_slice(&offset.to_le_bytes());
    if ml_code >= 15 {
        write_length_ext(out, ml_code - 15);
    }
    Ok(())
}

fn emit_last_literals(out: &mut Vec<u8>, literals: &[u8]) {
    let lit_len = literals.len();
    let token = (lit_len.min(15) as u8) << 4;
    out.push(token);
    if lit_len >= 15 {
        write_length_ext(out, lit_len - 15);
    }
    out.extend_from_slice(literals);
}

#[inline]
fn insert_hash(input: &[u8], pos: usize, hash_bits: u32, head: &mut [i32], chain: &mut [i32]) {
    let h = hash4(input, pos, hash_bits);
    let prev = head[h];
    head[h] = pos as i32;
    chain[pos & (CHAIN_SIZE - 1)] = prev;
}

fn find_best_match(
    input: &[u8],
    pos: usize,
    match_limit: usize,
    hash_bits: u32,
    head: &[i32],
    chain: &[i32],
    max_attempts: usize,
) -> Option<(usize, usize)> {
    if pos + MIN_MATCH > match_limit {
        return None;
    }
    let h = hash4(input, pos, hash_bits);
    let mut cand = head[h];
    let mut attempts = max_attempts;
    let mut best_len = MIN_MATCH - 1;
    let mut best_pos = 0usize;

    let target_first = &input[pos..pos + 4];
    while cand >= 0 && attempts > 0 {
        let c = cand as usize;
        if c >= pos || pos - c > MAX_DISTANCE {
            break;
        }
        attempts -= 1;
        if input[c..c + 4] == *target_first {
            // Quick check at current best_len boundary to skip shorter matches fast
            let can_beat = if best_len >= MIN_MATCH {
                pos + best_len < match_limit && input[c + best_len] == input[pos + best_len]
            } else {
                true
            };
            if can_beat {
                let mut mlen = 4usize;
                let max_mlen = match_limit - pos;
                while mlen < max_mlen && input[c + mlen] == input[pos + mlen] {
                    mlen += 1;
                }
                if mlen > best_len {
                    best_len = mlen;
                    best_pos = c;
                    if pos + mlen == match_limit {
                        break;
                    }
                }
            }
        }
        let next_cand = chain[c & (CHAIN_SIZE - 1)];
        if next_cand >= cand {
            break;
        }
        cand = next_cand;
    }

    if best_len >= MIN_MATCH {
        Some((best_pos, best_len))
    } else {
        None
    }
}

/// Pure-safe-Rust LZ4HC block compressor using bounded hash chains and lazy matching.
/// Produces standard LZ4 block bytes compatible with `lz4_flex::block::decompress_into`.
pub fn compress_lz4hc_into(
    input: &[u8],
    level: u8,
    scratch: &mut CodecScratch,
) -> Result<usize, Error> {
    if input.len() > MAX_BLOCK_SIZE {
        return Err(Error::InvalidLength {
            expected: MAX_BLOCK_SIZE,
            actual: input.len(),
        });
    }
    let max_out = lz4_flex::block::get_maximum_output_size(input.len());
    scratch.ensure_encode_capacity(max_out)?;

    if input.is_empty() {
        scratch.encode_buf.push(0);
        return Ok(1);
    }
    if input.len() < MF_LIMIT + 1 {
        emit_last_literals(&mut scratch.encode_buf, input);
        return Ok(scratch.encode_buf.len());
    }

    let clamped_level = level.clamp(1, 12);
    let max_attempts = match clamped_level {
        1..=3 => 16usize,
        4..=6 => 64usize,
        7..=9 => 256usize,
        _ => 512usize,
    };
    let lazy_depth = if clamped_level >= 7 {
        2usize
    } else if clamped_level >= 4 {
        1usize
    } else {
        0usize
    };

    let hash_bits = scratch.prepare_hc_tables(input.len())?;

    let input_limit = input.len() - MF_LIMIT;
    let match_limit = input.len() - LAST_LITERALS;
    let mut ip = 0usize;
    let mut anchor = 0usize;
    let mut next_hash_pos = 0usize;

    let head = &mut scratch.hc_head;
    let chain = &mut scratch.hc_chain;
    let out = &mut scratch.encode_buf;

    while ip <= input_limit {
        while next_hash_pos < ip {
            if next_hash_pos <= input_limit {
                insert_hash(input, next_hash_pos, hash_bits, head, chain);
            }
            next_hash_pos += 1;
        }

        let Some((mut best_pos, mut best_len)) =
            find_best_match(input, ip, match_limit, hash_bits, head, chain, max_attempts)
        else {
            insert_hash(input, ip, hash_bits, head, chain);
            next_hash_pos = ip + 1;
            ip += 1;
            continue;
        };

        insert_hash(input, ip, hash_bits, head, chain);
        next_hash_pos = ip + 1;

        // Lazy matching: inspect ip + 1 (and ip + 2 for higher levels) to find longer matches
        for _ in 0..lazy_depth {
            if ip + 1 > input_limit {
                break;
            }
            if let Some((next_pos, next_len)) = find_best_match(
                input,
                ip + 1,
                match_limit,
                hash_bits,
                head,
                chain,
                max_attempts,
            ) {
                if next_len > best_len {
                    insert_hash(input, ip + 1, hash_bits, head, chain);
                    ip += 1;
                    next_hash_pos = ip + 1;
                    best_pos = next_pos;
                    best_len = next_len;
                    continue;
                }
            }
            break;
        }

        let offset = (ip - best_pos) as u16;
        emit_sequence(out, &input[anchor..ip], offset, best_len)?;

        let match_end = ip + best_len;
        let hash_end = match_end.min(input_limit + 1);
        while next_hash_pos < hash_end {
            insert_hash(input, next_hash_pos, hash_bits, head, chain);
            next_hash_pos += 1;
        }
        ip = match_end;
        anchor = ip;
    }

    emit_last_literals(out, &input[anchor..]);
    Ok(out.len())
}

/// Compresses `input` using `config.codec` into a reusable `scratch`, returning an exact-sized
/// payload `Vec<u8>`, the actual `CodecId` chosen (falling back to `CodecId::Raw` if compression
/// does not achieve at least `min_savings_bytes`), and measured compression duration in nanoseconds.
pub fn compress_adaptive(
    input: &[u8],
    config: CompressionConfig,
    scratch: &mut CodecScratch,
) -> Result<(Vec<u8>, CodecId, u64), Error> {
    if input.len() > MAX_BLOCK_SIZE {
        return Err(Error::InvalidLength {
            expected: MAX_BLOCK_SIZE,
            actual: input.len(),
        });
    }
    let start = Instant::now();
    match config.codec {
        CodecId::Raw => {
            let mut raw = Vec::new();
            raw.try_reserve_exact(input.len())
                .map_err(|_| Error::Allocation)?;
            raw.extend_from_slice(input);
            Ok((raw, CodecId::Raw, start.elapsed().as_nanos() as u64))
        }
        CodecId::Lz4Standard => {
            let max_out = lz4_flex::block::get_maximum_output_size(input.len());
            scratch.ensure_encode_capacity(max_out)?;
            scratch.encode_buf.resize(max_out, 0);
            let written = lz4_flex::block::compress_into(input, &mut scratch.encode_buf)
                .map_err(|_| Error::Allocation)?;
            scratch.encode_buf.truncate(written);
            finalize_compressed_or_raw(
                input,
                &scratch.encode_buf,
                CodecId::Lz4Standard,
                config.min_savings_bytes,
                start.elapsed().as_nanos() as u64,
            )
        }
        CodecId::Lz4Hc { level } => {
            let written = compress_lz4hc_into(input, level, scratch)?;
            finalize_compressed_or_raw(
                input,
                &scratch.encode_buf[..written],
                CodecId::Lz4Hc {
                    level: level.clamp(1, 12),
                },
                config.min_savings_bytes,
                start.elapsed().as_nanos() as u64,
            )
        }
    }
}

/// Compresses `input` strictly using the requested `codec` (without adaptive fallback to Raw).
pub fn compress_exact(
    input: &[u8],
    codec: CodecId,
    scratch: &mut CodecScratch,
) -> Result<(Vec<u8>, u64), Error> {
    if input.len() > MAX_BLOCK_SIZE {
        return Err(Error::InvalidLength {
            expected: MAX_BLOCK_SIZE,
            actual: input.len(),
        });
    }
    let start = Instant::now();
    match codec {
        CodecId::Raw => {
            let mut raw = Vec::new();
            raw.try_reserve_exact(input.len())
                .map_err(|_| Error::Allocation)?;
            raw.extend_from_slice(input);
            Ok((raw, start.elapsed().as_nanos() as u64))
        }
        CodecId::Lz4Standard => {
            let max_out = lz4_flex::block::get_maximum_output_size(input.len());
            scratch.ensure_encode_capacity(max_out)?;
            scratch.encode_buf.resize(max_out, 0);
            let written = lz4_flex::block::compress_into(input, &mut scratch.encode_buf)
                .map_err(|_| Error::Allocation)?;
            let mut out = Vec::new();
            out.try_reserve_exact(written)
                .map_err(|_| Error::Allocation)?;
            out.extend_from_slice(&scratch.encode_buf[..written]);
            Ok((out, start.elapsed().as_nanos() as u64))
        }
        CodecId::Lz4Hc { level } => {
            let written = compress_lz4hc_into(input, level, scratch)?;
            let mut out = Vec::new();
            out.try_reserve_exact(written)
                .map_err(|_| Error::Allocation)?;
            out.extend_from_slice(&scratch.encode_buf[..written]);
            Ok((out, start.elapsed().as_nanos() as u64))
        }
    }
}

fn finalize_compressed_or_raw(
    input: &[u8],
    compressed_slice: &[u8],
    codec: CodecId,
    min_savings_bytes: usize,
    elapsed_ns: u64,
) -> Result<(Vec<u8>, CodecId, u64), Error> {
    let saves_enough = compressed_slice.len().saturating_add(min_savings_bytes) <= input.len()
        && compressed_slice.len() < input.len();
    if saves_enough {
        let mut out = Vec::new();
        out.try_reserve_exact(compressed_slice.len())
            .map_err(|_| Error::Allocation)?;
        out.extend_from_slice(compressed_slice);
        Ok((out, codec, elapsed_ns))
    } else {
        let mut raw = Vec::new();
        raw.try_reserve_exact(input.len())
            .map_err(|_| Error::Allocation)?;
        raw.extend_from_slice(input);
        Ok((raw, CodecId::Raw, elapsed_ns))
    }
}

/// Decompresses `payload` produced by `codec` directly into `out` (whose length must match
/// the expected original size), returning `(written_bytes, elapsed_ns)`.
pub fn decompress_with_codec(
    codec: CodecId,
    payload: &[u8],
    out: &mut [u8],
) -> Result<(usize, u64), Error> {
    let start = Instant::now();
    let written = match codec {
        CodecId::Raw => {
            if payload.len() != out.len() {
                return Err(Error::Decompression);
            }
            out.copy_from_slice(payload);
            out.len()
        }
        CodecId::Lz4Standard | CodecId::Lz4Hc { .. } => {
            let n =
                lz4_flex::block::decompress_into(payload, out).map_err(|_| Error::Decompression)?;
            if n != out.len() {
                return Err(Error::Decompression);
            }
            n
        }
    };
    Ok((written, start.elapsed().as_nanos() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz4hc_round_trip_all_boundary_sizes() {
        let mut scratch = CodecScratch::with_limit(512 * 1024);
        for size in [
            0usize, 1, 4, 11, 12, 13, 15, 16, 64, 512, 4096, 16384, 32768,
        ] {
            let input: Vec<u8> = (0..size)
                .map(|i| ((i % 29) as u8).wrapping_mul(3))
                .collect();
            let (payload, cost_ns) =
                compress_exact(&input, CodecId::Lz4Hc { level: 6 }, &mut scratch).unwrap();
            let _ = cost_ns;
            if size == 0 {
                // Empty block check
                let mut out = vec![];
                let n = lz4_flex::block::decompress_into(&payload, &mut out).unwrap();
                assert_eq!(n, 0);
            } else {
                let mut out = vec![0u8; size];
                let (n, _) =
                    decompress_with_codec(CodecId::Lz4Hc { level: 6 }, &payload, &mut out).unwrap();
                assert_eq!(n, size);
                assert_eq!(out, input, "mismatch for size={size}");
            }
        }
    }

    #[test]
    fn lz4hc_outperforms_standard_lz4_on_structured_payload() {
        // Structured JSON-like repeated log block with subtle variations where lazy/hash-chain
        // finds longer cross-window matches than single-slot fast LZ4.
        let mut input = Vec::with_capacity(16384);
        for idx in 0..256 {
            let line = format!(
                "{{\"service\":\"xynoos-aura\",\"pid\":{},\"psi_some\":{}.25,\"mem_avail_kib\":{}}}\n",
                1000 + (idx % 7),
                idx % 10,
                204800 + (idx * 17) % 1024
            );
            input.extend_from_slice(line.as_bytes());
        }
        input.truncate(16384);

        let mut scratch = CodecScratch::with_limit(512 * 1024);
        let (std_payload, _) = compress_exact(&input, CodecId::Lz4Standard, &mut scratch).unwrap();
        let (hc_payload, _) =
            compress_exact(&input, CodecId::Lz4Hc { level: 9 }, &mut scratch).unwrap();

        let mut decoded_std = vec![0u8; input.len()];
        let mut decoded_hc = vec![0u8; input.len()];
        decompress_with_codec(CodecId::Lz4Standard, &std_payload, &mut decoded_std).unwrap();
        decompress_with_codec(CodecId::Lz4Hc { level: 9 }, &hc_payload, &mut decoded_hc).unwrap();
        assert_eq!(decoded_std, input);
        assert_eq!(decoded_hc, input);
        assert!(
            hc_payload.len() < std_payload.len(),
            "Expected genuine LZ4HC ({}) < standard LZ4 ({})",
            hc_payload.len(),
            std_payload.len()
        );
    }

    #[test]
    fn adaptive_falls_back_to_raw_on_incompressible_data() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let random: Vec<u8> = (0..4096)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as u8
            })
            .collect();
        let mut scratch = CodecScratch::default();
        let (payload, chosen, _) = compress_adaptive(
            &random,
            CompressionConfig {
                codec: CodecId::Lz4Hc { level: 6 },
                min_savings_bytes: 64,
                max_scratch_bytes: 512 * 1024,
            },
            &mut scratch,
        )
        .unwrap();
        assert_eq!(chosen, CodecId::Raw);
        assert_eq!(payload, random);
    }

    #[test]
    fn scratch_limit_is_enforced() {
        let mut scratch = CodecScratch::with_limit(1024);
        let input = vec![0u8; 4096];
        let err = compress_exact(&input, CodecId::Lz4Hc { level: 6 }, &mut scratch).unwrap_err();
        assert_eq!(err, Error::BudgetExceeded);
    }
}
