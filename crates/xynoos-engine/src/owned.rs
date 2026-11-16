//! Experimental cooperative storage. Single owner; no kernel-control executor.
//! Raw blocks are replaced only after exact decode equality. No duplicate corpus is retained.
use crate::{Engine, Mode, Outcome, Reason};
use serde::Serialize;
use std::time::{Duration, Instant};
use xynoos_aura::cooperative::{choose, Choice};
use xynoos_pressureiq::system::SystemSnapshot;

pub const BLOCK: usize = 1024 * 1024;
pub const BLOCKS: usize = 16;
pub const RETENTION_BUDGET: usize = 8 * BLOCK;
pub const MAX_REQUESTS: usize = 256;
pub const MAX_AGE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CorpusKind {
    Mixed,
    Entropy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Kept,
    Observed,
    RefusedTelemetry,
    RefusedMemory,
    BackOff,
    Compressed,
    PartialFreshnessStop,
    NoBenefit,
    Cancelled,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub effect: Effect,
    pub aura_choice: &'static str,
    pub retained_before: usize,
    pub retained_after: usize,
    pub sustained_retention: bool,
    pub host_pressure: Option<[bool; 3]>,
    pub host_psi_percent: Option<[f64; 3]>,
    pub host_reason: Option<Reason>,
    pub action_ns: u128,
    pub changed_blocks: usize,
}
#[derive(Default)]
struct Retention {
    previous_high: bool,
    active: bool,
    previous_at: Option<Instant>,
    last_attempt: Option<Instant>,
}
impl Retention {
    fn update(&mut self, bytes: usize, at: Instant) -> bool {
        let continuous = self.previous_at.is_some_and(|p| {
            at.checked_duration_since(p)
                .is_some_and(|d| d > Duration::ZERO && d <= MAX_AGE)
        });
        if !continuous {
            self.previous_high = false;
            self.active = false;
        }
        let high = bytes > RETENTION_BUDGET;
        if continuous && high && self.previous_high {
            self.active = true;
        } else if !high {
            self.active = false;
        }
        self.previous_high = high;
        self.previous_at = Some(at);
        self.active
    }
    fn cooling(&self, now: Instant) -> bool {
        self.last_attempt.is_some_and(|t| {
            now.checked_duration_since(t)
                .is_none_or(|d| d < Duration::from_secs(10))
        })
    }
}
enum BlockData {
    Raw(Box<[u8]>),
    Packed(Box<[u8]>),
}
pub struct Corpus {
    blocks: Vec<BlockData>,
    checksums: Vec<u32>,
    scratch: Vec<u8>,
    kind: CorpusKind,
    engine: Engine,
    retention: Retention,
    cancelled: bool,
    requests: usize,
}
fn generated(index: usize, kind: CorpusKind) -> Vec<u8> {
    let mut seed = 0x12345678u32 ^ (index as u32 + 1);
    (0..BLOCK)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            if kind == CorpusKind::Mixed && i % 1024 >= 256 {
                let text = b"owned application record: lossless cold corpus; retain all data\n";
                text[i % text.len()]
            } else {
                seed as u8
            }
        })
        .collect()
}
impl Corpus {
    pub fn new(kind: CorpusKind) -> Self {
        let blocks: Vec<_> = (0..BLOCKS)
            .map(|i| BlockData::Raw(generated(i, kind).into_boxed_slice()))
            .collect();
        let checksums = blocks
            .iter()
            .map(|b| match b {
                BlockData::Raw(v) => crc32fast::hash(v),
                _ => unreachable!(),
            })
            .collect();
        Self {
            blocks,
            checksums,
            scratch: vec![0; BLOCK],
            kind,
            engine: Engine::default(),
            retention: Retention::default(),
            cancelled: false,
            requests: 0,
        }
    }
    /// Payload allocation, including the reusable decode/copy scratch. Metadata excluded.
    pub fn retained(&self) -> usize {
        self.scratch.capacity()
            + self
                .blocks
                .iter()
                .map(|b| match b {
                    BlockData::Raw(v) | BlockData::Packed(v) => v.len(),
                })
                .sum::<usize>()
    }
    pub fn identity(&self) -> &[u32] {
        &self.checksums
    }
    fn decode(&mut self, index: usize) -> Result<(), &'static str> {
        match &self.blocks[index] {
            BlockData::Raw(v) => self.scratch.copy_from_slice(v),
            BlockData::Packed(v) => {
                let n = lz4_flex::block::decompress_into(v, &mut self.scratch)
                    .map_err(|_| "decode failure")?;
                if n != BLOCK {
                    return Err("decoded length mismatch");
                }
            }
        }
        if crc32fast::hash(&self.scratch) != self.checksums[index] {
            return Err("checksum mismatch");
        }
        Ok(())
    }
    /// Same four block copies/decodes and CRCs for every arm. All blocks are read cyclically.
    pub fn work(&mut self) -> Result<u32, &'static str> {
        if self.cancelled || self.requests >= MAX_REQUESTS {
            return Err("cancelled or request limit");
        }
        let mut hash = crc32fast::Hasher::new();
        for j in 0..4 {
            let index = (self.requests * 4 + j) % BLOCKS;
            self.decode(index)?;
            hash.update(&self.scratch);
        }
        self.requests += 1;
        Ok(hash.finalize())
    }
    /// Private typed action is selected and admitted here; callers cannot supply executable plans.
    pub fn evaluate(
        &mut self,
        snapshot: &SystemSnapshot,
        now: Instant,
        active_opt_in: bool,
    ) -> Result<Report, &'static str> {
        let before = self.retained();
        let mut out = Report {
            effect: Effect::Observed,
            aura_choice: "not_evaluated",
            retained_before: before,
            retained_after: before,
            sustained_retention: false,
            host_pressure: None,
            host_psi_percent: None,
            host_reason: None,
            action_ns: 0,
            changed_blocks: 0,
        };
        if self.cancelled {
            out.effect = Effect::Cancelled;
            return Ok(out);
        }
        let plan = self
            .engine
            .evaluate(snapshot, now, Mode::Observe, false, false);
        out.host_reason = Some(plan.reason);
        if let Some(d) = &plan.decision {
            out.host_pressure = Some(d.active_pressure);
            out.host_psi_percent = d.pressure_percent;
        }
        if plan.outcome == Outcome::Refused
            || [snapshot.monotonic, snapshot.memory.monotonic]
                .iter()
                .any(|t| now.checked_duration_since(*t).is_none_or(|d| d > MAX_AGE))
        {
            self.retention = Retention {
                last_attempt: self.retention.last_attempt,
                ..Default::default()
            };
            out.effect = Effect::RefusedTelemetry;
            return Ok(out);
        }
        if snapshot
            .memory
            .mem
            .result
            .as_ref()
            .ok()
            .and_then(|m| m.bytes.get("MemAvailable"))
            .is_none_or(|n| *n < 256 * BLOCK as u64)
        {
            self.retention.previous_at = None;
            out.effect = Effect::RefusedMemory;
            return Ok(out);
        }
        out.sustained_retention = self.retention.update(before, snapshot.monotonic);
        let busy = plan.reason == Reason::CpuOrIoPressure;
        let action = choose(out.sustained_retention, busy, self.retention.cooling(now));
        out.aura_choice = match action {
            Choice::Keep => "keep",
            Choice::BackOff => "back_off",
            Choice::Compress => "compress",
        };
        if !active_opt_in {
            return Ok(out);
        }
        out.effect = match action {
            Choice::Keep => Effect::Kept,
            Choice::BackOff => Effect::BackOff,
            Choice::Compress => {
                // Check freshness again at the actual mutation boundary.
                if snapshot.memory.monotonic.elapsed() > MAX_AGE {
                    out.effect = Effect::RefusedTelemetry;
                    return Ok(out);
                }
                self.retention.last_attempt = Some(now);
                let t = Instant::now();
                let mut partial = false;
                for b in &mut self.blocks {
                    if snapshot.memory.monotonic.elapsed() > MAX_AGE {
                        partial = true;
                        break;
                    }
                    let BlockData::Raw(raw) = b else { continue };
                    let packed = lz4_flex::block::compress(raw).into_boxed_slice();
                    // Require >=12.5% saving; bounded expansion never replaces the original.
                    if packed.len() > raw.len() * 7 / 8 {
                        continue;
                    }
                    let n = lz4_flex::block::decompress_into(&packed, &mut self.scratch)
                        .map_err(|_| "compression verification decode")?;
                    if n != BLOCK || self.scratch != **raw {
                        return Err("compression verification equality");
                    }
                    // Exact equality precedes replacement; raw allocation is now freed.
                    *b = BlockData::Packed(packed);
                    out.changed_blocks += 1;
                }
                out.action_ns = t.elapsed().as_nanos();
                if partial {
                    Effect::PartialFreshnessStop
                } else if out.changed_blocks > 0 {
                    Effect::Compressed
                } else {
                    Effect::NoBenefit
                }
            }
        };
        out.retained_after = self.retained();
        Ok(out)
    }
    /// Reverse every completed replacement. Byte equality to independent generation is checked.
    pub fn restore(&mut self, cancelled: bool) -> Result<(), &'static str> {
        self.cancelled |= cancelled;
        for i in 0..BLOCKS {
            self.decode(i)?;
            let expected = generated(i, self.kind);
            if expected != self.scratch {
                return Err("restoration equality failure");
            }
            if matches!(self.blocks[i], BlockData::Packed(_)) {
                self.blocks[i] = BlockData::Raw(self.scratch.clone().into_boxed_slice());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(at: Instant, cpu: f64) -> SystemSnapshot {
        let mut s = crate::replay::default_trace().steps[0]
            .snapshot(at)
            .unwrap()
            .0;
        s.monotonic = at;
        s.memory.monotonic = at;
        s.cpu_psi = Ok(xynoos_pressureiq::parse_psi(&format!(
            "some avg10={cpu} avg60=0 avg300=0 total=1"
        ))
        .unwrap());
        s
    }
    #[test]
    fn state_transitions_and_cooldown() {
        let t = Instant::now();
        let mut s = Retention::default();
        assert!(!s.update(16 * BLOCK, t));
        assert!(s.update(16 * BLOCK, t + Duration::from_millis(1)));
        s.last_attempt = Some(t);
        assert!(s.cooling(t + Duration::from_secs(9)));
        assert!(!s.cooling(t + Duration::from_secs(10)));
        assert!(!s.update(BLOCK, t + Duration::from_millis(2)));
        assert!(!s.update(16 * BLOCK, t + Duration::from_secs(3)));
    }
    #[test]
    fn compression_is_real_equivalent_and_reversible() {
        let mut raw = Corpus::new(CorpusKind::Mixed);
        let mut active = Corpus::new(CorpusKind::Mixed);
        let t = Instant::now();
        assert_eq!(
            active.evaluate(&sample(t, 0.), t, true).unwrap().effect,
            Effect::Kept
        );
        let at = Instant::now();
        let r = active.evaluate(&sample(at, 0.), at, true).unwrap();
        assert_eq!(r.effect, Effect::Compressed);
        assert_eq!(r.changed_blocks, BLOCKS);
        assert!(r.retained_after < r.retained_before / 2);
        for _ in 0..8 {
            assert_eq!(raw.work(), active.work());
        }
        let at = Instant::now();
        assert_eq!(
            active.evaluate(&sample(at, 0.), at, true).unwrap().effect,
            Effect::BackOff
        );
        active.restore(false).unwrap();
        assert_eq!(raw.retained(), active.retained());
        active.restore(true).unwrap();
        assert!(active.work().is_err());
    }
    #[test]
    fn telemetry_authorization_backoff_and_entropy() {
        let mut c = Corpus::new(CorpusKind::Entropy);
        for _ in 0..2 {
            let at = Instant::now();
            assert_eq!(
                c.evaluate(&sample(at, 0.), at, false).unwrap().effect,
                Effect::Observed
            );
        }
        let at = Instant::now();
        let r = c.evaluate(&sample(at, 0.), at, true).unwrap();
        assert_eq!(r.effect, Effect::NoBenefit);
        assert_eq!(r.retained_before, r.retained_after);
        let at = Instant::now();
        let stale = at - Duration::from_secs(2);
        assert_eq!(
            c.evaluate(&sample(stale, 0.), at, true).unwrap().effect,
            Effect::RefusedTelemetry
        );
        let at = Instant::now();
        assert_eq!(
            c.evaluate(&sample(at, 99.), at, true).unwrap().effect,
            Effect::BackOff
        );
        c.restore(false).unwrap();
    }
    #[test]
    fn corruption_and_partial_restoration_failure_are_errors() {
        let mut c = Corpus::new(CorpusKind::Mixed);
        if let BlockData::Raw(v) = &mut c.blocks[3] {
            v[0] ^= 1;
        }
        assert_eq!(c.restore(false), Err("checksum mismatch"));
    }

    #[test]
    fn low_memory_missing_and_reset_telemetry_refuse_without_changes() {
        let mut c = Corpus::new(CorpusKind::Mixed);
        let before = c.retained();
        let at = Instant::now();
        let mut s = sample(at, 0.);
        s.memory.mem.result = Ok(xynoos_pressureiq::parse_meminfo(
            "MemTotal: 4194304 kB\nMemAvailable: 204800 kB\n",
        )
        .unwrap());
        assert_eq!(
            c.evaluate(&s, at, true).unwrap().effect,
            Effect::RefusedMemory
        );
        let at = Instant::now();
        let mut s = sample(at, 0.);
        s.io_psi = Err(xynoos_pressureiq::Error::Permission);
        assert_eq!(
            c.evaluate(&s, at, true).unwrap().effect,
            Effect::RefusedTelemetry
        );
        let at = Instant::now();
        let mut s = sample(at, 0.);
        s.memory
            .vm
            .result
            .as_mut()
            .unwrap()
            .counters
            .insert("pswpin".into(), 10);
        c.evaluate(&s, at, false).unwrap();
        let at = Instant::now();
        assert_eq!(
            c.evaluate(&sample(at, 0.), at, true).unwrap().effect,
            Effect::RefusedTelemetry
        );
        assert_eq!(c.retained(), before);
    }

    #[test]
    fn compressed_corruption_after_partial_reverse_is_not_success() {
        let mut c = Corpus::new(CorpusKind::Mixed);
        for _ in 0..2 {
            let at = Instant::now();
            c.evaluate(&sample(at, 0.), at, true).unwrap();
        }
        assert!(matches!(&c.blocks[0], BlockData::Packed(_)));
        if let BlockData::Packed(v) = &mut c.blocks[3] {
            v.fill(0);
        }
        assert!(c.restore(false).is_err());
        assert!(matches!(&c.blocks[0], BlockData::Raw(_)));
        assert!(matches!(&c.blocks[3], BlockData::Packed(_)));
    }
}
