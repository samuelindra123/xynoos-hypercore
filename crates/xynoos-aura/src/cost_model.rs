#![forbid(unsafe_code)]
//! Deterministic Resource Cost Intelligence for AURA (Milestone G).
//!
//! Uses bounded rolling statistics and fixed-point EWMA feedback (no ML runtime or external weights).
//! All arithmetic is checked or saturating fixed-point integer arithmetic for cross-host determinism.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use xynoos_core::{CodecId, MemoryPressure, ResourcePressureLevel};

/// EWMA smoothing factor alpha = 250 / 1000 = 0.25.
const EWMA_ALPHA_PERMILLE: u64 = 250;
const PERMILLE_BASE: u64 = 1000;
const ROLLING_WINDOW_CAP: usize = 32;

/// Workload access temperature classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageTemperature {
    Hot,
    Warm,
    Cold,
}

/// Measured cost-benefit breakdown for a candidate codec transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostBenefitBreakdown {
    pub bytes_potentially_saved: usize,
    pub actual_bytes_saved_after_overhead: isize,
    pub metadata_overhead_bytes: usize,
    pub temp_scratch_overhead_bytes: usize,
    pub estimated_cpu_cost_ns: u64,
    pub estimated_access_latency_penalty_ns: u64,
    pub recompression_cost_ns: u64,
    pub net_benefit_score: i64,
    pub confidence_permille: u16,
}

/// Bounded rolling statistics window (fixed capacity 32 samples).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingCostStats {
    samples: VecDeque<u64>,
    sum: u128,
}

impl Default for RollingCostStats {
    fn default() -> Self {
        Self::new()
    }
}

impl RollingCostStats {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(ROLLING_WINDOW_CAP),
            sum: 0,
        }
    }

    pub fn push(&mut self, value: u64) {
        if self.samples.len() == ROLLING_WINDOW_CAP {
            if let Some(old) = self.samples.pop_front() {
                self.sum = self.sum.saturating_sub(old as u128);
            }
        }
        self.samples.push_back(value);
        self.sum = self.sum.saturating_add(value as u128);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn mean(&self) -> Option<u64> {
        if self.samples.is_empty() {
            None
        } else {
            Some((self.sum / self.samples.len() as u128) as u64)
        }
    }
}

/// Deterministic resource cost model combining EWMA codec performance estimates,
/// rolling compression ratios, and pressure-weighted cost-benefit scoring.
///
/// Mathematical Cost Function:
/// Let:
///   - $\Delta B_{\text{net}}$ = expected net bytes saved after metadata and recompression overhead
///   - $W_{\text{mem}}(P_{\text{mem}})$ = memory value weight in ns/byte (Low=4, Moderate=64, High=512, Critical=1024)
///   - $C_{\text{cpu}}$ = estimated compression CPU cost (ns) + recompression penalty (ns)
///   - $C_{\text{lat}}$ = expected additional decompression latency over expected future reads (ns)
///   - $M_{\text{cpu}}(P_{\text{cpu}})$ = CPU contention multiplier in permille (Low=1000, Mod=2500, High=8000, Crit=32000)
///   - $P_{\text{scratch}}$ = temporary scratch buffer penalty if available RAM is constrained
///
/// Then:
///   $$\text{Score} = \Delta B_{\text{net}} \cdot W_{\text{mem}}(P_{\text{mem}}) - \frac{(C_{\text{cpu}} + C_{\text{lat}}) \cdot M_{\text{cpu}}(P_{\text{cpu}})}{1000} - P_{\text{scratch}}$$
///
/// Limitations:
///   - Linear weighting approximates non-linear kernel reclaim latency.
///   - Future read count is heuristically inferred from recent access recency & count.
///   - Measurements are observed in userspace `PageStore`/`ClusterStore`, not kernel zswap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceCostModel {
    pub lz4_compress_ns_per_kib_ewma: u64,
    pub lz4_decompress_ns_per_kib_ewma: u64,
    pub lz4hc_compress_ns_per_kib_ewma: u64,
    pub lz4hc_decompress_ns_per_kib_ewma: u64,
    pub lz4_ratio_permille_ewma: u64,
    pub lz4hc_ratio_permille_ewma: u64,
    pub observation_count: u64,
    pub recent_compress_ns: RollingCostStats,
    pub recent_savings_permille: RollingCostStats,
    pub hot_recency_ticks: u64,
    pub warm_recency_ticks: u64,
    pub cooldown_ticks: u64,
}

impl Default for ResourceCostModel {
    fn default() -> Self {
        Self {
            // Conservative initial priors for low-power x86_64 (e.g. Pentium N5000 class):
            // ~2.5 us/KiB LZ4 compress, ~0.8 us/KiB decompress, ~28 us/KiB LZ4HC compress
            lz4_compress_ns_per_kib_ewma: 2_500,
            lz4_decompress_ns_per_kib_ewma: 800,
            lz4hc_compress_ns_per_kib_ewma: 28_000,
            lz4hc_decompress_ns_per_kib_ewma: 850,
            lz4_ratio_permille_ewma: 550,
            lz4hc_ratio_permille_ewma: 430,
            observation_count: 0,
            recent_compress_ns: RollingCostStats::new(),
            recent_savings_permille: RollingCostStats::new(),
            hot_recency_ticks: 16,
            warm_recency_ticks: 64,
            cooldown_ticks: 4,
        }
    }
}

#[inline]
fn ewma_update(prev: u64, sample: u64) -> u64 {
    let weighted_sample = EWMA_ALPHA_PERMILLE.saturating_mul(sample);
    let weighted_prev = (PERMILLE_BASE - EWMA_ALPHA_PERMILLE).saturating_mul(prev);
    weighted_sample.saturating_add(weighted_prev) / PERMILLE_BASE
}

impl ResourceCostModel {
    /// Update EWMA telemetry feedback after observing a real compression/decompression operation.
    pub fn record_compression_feedback(
        &mut self,
        codec: CodecId,
        original_bytes: usize,
        stored_bytes: usize,
        compress_ns: u64,
    ) {
        if original_bytes == 0 {
            return;
        }
        let kib = (original_bytes as u64).div_ceil(1024).max(1);
        let ns_per_kib = compress_ns / kib;
        let ratio_permille = ((stored_bytes as u128).saturating_mul(PERMILLE_BASE as u128)
            / original_bytes as u128)
            .min(2000) as u64;

        match codec {
            CodecId::Raw => {}
            CodecId::Lz4Standard => {
                self.lz4_compress_ns_per_kib_ewma =
                    ewma_update(self.lz4_compress_ns_per_kib_ewma, ns_per_kib);
                self.lz4_ratio_permille_ewma =
                    ewma_update(self.lz4_ratio_permille_ewma, ratio_permille);
            }
            CodecId::Lz4Hc { .. } => {
                self.lz4hc_compress_ns_per_kib_ewma =
                    ewma_update(self.lz4hc_compress_ns_per_kib_ewma, ns_per_kib);
                self.lz4hc_ratio_permille_ewma =
                    ewma_update(self.lz4hc_ratio_permille_ewma, ratio_permille);
            }
        }
        self.recent_compress_ns.push(compress_ns);
        let savings_permille = PERMILLE_BASE.saturating_sub(ratio_permille);
        self.recent_savings_permille.push(savings_permille);
        self.observation_count = self.observation_count.saturating_add(1);
    }

    /// Update decompression latency EWMA.
    pub fn record_decompression_feedback(
        &mut self,
        codec: CodecId,
        original_bytes: usize,
        decompress_ns: u64,
    ) {
        if original_bytes == 0 || codec == CodecId::Raw {
            return;
        }
        let kib = (original_bytes as u64).div_ceil(1024).max(1);
        let ns_per_kib = decompress_ns / kib;
        match codec {
            CodecId::Raw => {}
            CodecId::Lz4Standard => {
                self.lz4_decompress_ns_per_kib_ewma =
                    ewma_update(self.lz4_decompress_ns_per_kib_ewma, ns_per_kib);
            }
            CodecId::Lz4Hc { .. } => {
                self.lz4hc_decompress_ns_per_kib_ewma =
                    ewma_update(self.lz4hc_decompress_ns_per_kib_ewma, ns_per_kib);
            }
        }
    }

    /// Classify page temperature deterministically from recency and access count.
    pub fn classify_temperature(&self, recency_ticks: u64, access_count: u64) -> PageTemperature {
        if recency_ticks <= self.hot_recency_ticks && access_count >= 2 {
            PageTemperature::Hot
        } else if recency_ticks <= self.warm_recency_ticks || access_count >= 1 {
            PageTemperature::Warm
        } else {
            PageTemperature::Cold
        }
    }

    /// Confidence indicator in `[100, 1000]` permille based on feedback count and telemetry health.
    pub fn confidence_permille(&self, telemetry_reliable: bool) -> u16 {
        if !telemetry_reliable {
            return 150;
        }
        let base = 400u64;
        let bonus = self.observation_count.min(30).saturating_mul(20);
        base.saturating_add(bonus).min(1000) as u16
    }

    pub const fn memory_value_ns_per_byte(pressure: MemoryPressure) -> i64 {
        match pressure {
            MemoryPressure::Low => 4,
            MemoryPressure::Moderate => 64,
            MemoryPressure::High => 512,
            MemoryPressure::Critical => 1024,
        }
    }

    pub const fn cpu_penalty_multiplier_permille(cpu: ResourcePressureLevel) -> i64 {
        match cpu {
            ResourcePressureLevel::Low => 1_000,
            ResourcePressureLevel::Moderate => 2_500,
            ResourcePressureLevel::High => 8_000,
            ResourcePressureLevel::Critical => 32_000,
        }
    }

    /// Evaluate exact cost-benefit score for transitioning an object from `current_codec`
    /// to `candidate_codec`.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_transition(
        &self,
        current_codec: CodecId,
        candidate_codec: CodecId,
        original_bytes: usize,
        current_stored_bytes: usize,
        metadata_bytes: usize,
        estimated_ratio_permille: Option<u64>,
        temperature: PageTemperature,
        mem_pressure: MemoryPressure,
        cpu_pressure: ResourcePressureLevel,
        available_ram_bytes: Option<u64>,
        telemetry_reliable: bool,
    ) -> CostBenefitBreakdown {
        let kib = (original_bytes as u64).div_ceil(1024).max(1);
        let target_ratio_permille = match candidate_codec {
            CodecId::Raw => PERMILLE_BASE,
            CodecId::Lz4Standard => estimated_ratio_permille
                .unwrap_or(self.lz4_ratio_permille_ewma)
                .min(PERMILLE_BASE),
            CodecId::Lz4Hc { .. } => {
                let base = estimated_ratio_permille.unwrap_or(self.lz4hc_ratio_permille_ewma);
                // LZ4HC typically saves ~10-20% over standard ratio estimate when not explicitly given
                if estimated_ratio_permille.is_some() {
                    (base.saturating_mul(850) / 1000).min(base)
                } else {
                    base
                }
            }
        };

        let expected_stored_bytes = ((original_bytes as u128)
            .saturating_mul(target_ratio_permille as u128)
            / PERMILLE_BASE as u128) as usize;

        let bytes_potentially_saved = current_stored_bytes.saturating_sub(expected_stored_bytes);
        let actual_bytes_saved_after_overhead = if candidate_codec == current_codec {
            0
        } else {
            (current_stored_bytes as isize).saturating_sub(expected_stored_bytes as isize)
        };

        let temp_scratch_overhead_bytes = match candidate_codec {
            CodecId::Raw => 0,
            CodecId::Lz4Standard => original_bytes.saturating_add(1024),
            CodecId::Lz4Hc { .. } => {
                let hash_bytes: usize = if original_bytes <= 4096 {
                    4096 * 4
                } else if original_bytes <= 16_384 {
                    8192 * 4
                } else {
                    32_768 * 4
                };
                let chain_bytes = original_bytes.min(65_536).saturating_mul(4);
                hash_bytes
                    .saturating_add(chain_bytes)
                    .saturating_add(original_bytes)
            }
        };

        let estimated_cpu_cost_ns = match candidate_codec {
            CodecId::Raw => {
                if current_codec == CodecId::Raw {
                    0
                } else {
                    kib.saturating_mul(self.lz4_decompress_ns_per_kib_ewma)
                }
            }
            CodecId::Lz4Standard => {
                if current_codec == CodecId::Lz4Standard {
                    0
                } else {
                    kib.saturating_mul(self.lz4_compress_ns_per_kib_ewma)
                }
            }
            CodecId::Lz4Hc { .. } => {
                if matches!(current_codec, CodecId::Lz4Hc { .. }) {
                    0
                } else {
                    kib.saturating_mul(self.lz4hc_compress_ns_per_kib_ewma)
                }
            }
        };

        // Recompression from an already-compressed representation pays decode + encode
        let recompression_cost_ns = if current_codec.is_compressed()
            && candidate_codec.is_compressed()
            && current_codec != candidate_codec
        {
            kib.saturating_mul(self.lz4_decompress_ns_per_kib_ewma)
        } else {
            0
        };

        let expected_reads: u64 = match temperature {
            PageTemperature::Hot => 8,
            PageTemperature::Warm => 2,
            PageTemperature::Cold => 0,
        };
        let per_read_decomp_ns = match candidate_codec {
            CodecId::Raw => 0,
            CodecId::Lz4Standard => kib.saturating_mul(self.lz4_decompress_ns_per_kib_ewma),
            CodecId::Lz4Hc { .. } => kib.saturating_mul(self.lz4hc_decompress_ns_per_kib_ewma),
        };
        let estimated_access_latency_penalty_ns = expected_reads.saturating_mul(per_read_decomp_ns);

        let mem_weight = Self::memory_value_ns_per_byte(mem_pressure);
        let cpu_mult = Self::cpu_penalty_multiplier_permille(cpu_pressure);

        let memory_benefit_score =
            (actual_bytes_saved_after_overhead as i64).saturating_mul(mem_weight);
        let total_cpu_ns = estimated_cpu_cost_ns
            .saturating_add(recompression_cost_ns)
            .saturating_add(estimated_access_latency_penalty_ns)
            .min(i64::MAX as u64) as i64;
        let weighted_cpu_penalty = total_cpu_ns.saturating_mul(cpu_mult) / 1000;

        let scratch_penalty: i64 = match available_ram_bytes {
            Some(avail) if avail < (temp_scratch_overhead_bytes as u64).saturating_mul(8) => 50_000,
            _ => 0,
        };

        let net_benefit_score = memory_benefit_score
            .saturating_sub(weighted_cpu_penalty)
            .saturating_sub(scratch_penalty);

        CostBenefitBreakdown {
            bytes_potentially_saved,
            actual_bytes_saved_after_overhead,
            metadata_overhead_bytes: metadata_bytes,
            temp_scratch_overhead_bytes,
            estimated_cpu_cost_ns,
            estimated_access_latency_penalty_ns,
            recompression_cost_ns,
            net_benefit_score,
            confidence_permille: self.confidence_permille(telemetry_reliable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ewma_and_rolling_window_stay_bounded_and_deterministic() {
        let mut model = ResourceCostModel::default();
        for i in 0..100u64 {
            model.record_compression_feedback(CodecId::Lz4Standard, 4096, 1024, 5000 + i * 10);
        }
        assert_eq!(model.recent_compress_ns.len(), ROLLING_WINDOW_CAP);
        assert_eq!(model.observation_count, 100);
        assert_eq!(model.confidence_permille(true), 1000);
        assert_eq!(model.confidence_permille(false), 150);
    }

    #[test]
    fn high_cpu_pressure_penalizes_lz4hc_relative_to_low_cpu_pressure() {
        let model = ResourceCostModel::default();
        let cold_low_cpu = model.evaluate_transition(
            CodecId::Raw,
            CodecId::Lz4Hc { level: 6 },
            4096,
            4096,
            96,
            Some(400),
            PageTemperature::Cold,
            MemoryPressure::High,
            ResourcePressureLevel::Low,
            Some(512 * 1024 * 1024),
            true,
        );
        let cold_high_cpu = model.evaluate_transition(
            CodecId::Raw,
            CodecId::Lz4Hc { level: 6 },
            4096,
            4096,
            96,
            Some(400),
            PageTemperature::Cold,
            MemoryPressure::High,
            ResourcePressureLevel::Critical,
            Some(512 * 1024 * 1024),
            true,
        );
        assert!(cold_low_cpu.net_benefit_score > 0);
        assert!(cold_high_cpu.net_benefit_score < cold_low_cpu.net_benefit_score);
        assert!(cold_high_cpu.net_benefit_score < 0);
    }
}
