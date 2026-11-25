#![forbid(unsafe_code)]
//! Deterministic Adaptive Compression Decision Engine for AURA (Milestone G).

use crate::cost_model::{CostBenefitBreakdown, PageTemperature, ResourceCostModel};
use serde::{Deserialize, Serialize};
use xynoos_core::{
    CodecId, CompressionAction, DecisionReason, MemoryPressure, MigrationBudget,
    ResourcePressureLevel,
};

/// Multi-resource state snapshot consumed by the compression decision engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiResourceState {
    pub memory_pressure: MemoryPressure,
    pub cpu_pressure: ResourcePressureLevel,
    pub io_pressure: ResourcePressureLevel,
    pub available_ram_bytes: Option<u64>,
    pub telemetry_valid: bool,
}

impl MultiResourceState {
    /// Conservative safe fallback state when telemetry is unreliable or invalid.
    pub const fn fallback_safe() -> Self {
        Self {
            memory_pressure: MemoryPressure::Moderate,
            cpu_pressure: ResourcePressureLevel::High,
            io_pressure: ResourcePressureLevel::Moderate,
            available_ram_bytes: None,
            telemetry_valid: false,
        }
    }

    pub fn from_psi_percentages(
        memory_pressure: MemoryPressure,
        cpu_psi_some_10: Option<f64>,
        io_psi_some_10: Option<f64>,
        available_ram_bytes: Option<u64>,
        telemetry_valid: bool,
    ) -> Self {
        if !telemetry_valid {
            return Self::fallback_safe();
        }
        let cpu_pressure = match cpu_psi_some_10 {
            Some(v) if v.is_finite() && v >= 50.0 => ResourcePressureLevel::Critical,
            Some(v) if v.is_finite() && v >= 20.0 => ResourcePressureLevel::High,
            Some(v) if v.is_finite() && v >= 5.0 => ResourcePressureLevel::Moderate,
            Some(v) if v.is_finite() && v >= 0.0 => ResourcePressureLevel::Low,
            _ => ResourcePressureLevel::Moderate, // Conservative if CPU PSI unavailable
        };
        let io_pressure = match io_psi_some_10 {
            Some(v) if v.is_finite() && v >= 40.0 => ResourcePressureLevel::Critical,
            Some(v) if v.is_finite() && v >= 15.0 => ResourcePressureLevel::High,
            Some(v) if v.is_finite() && v >= 3.0 => ResourcePressureLevel::Moderate,
            Some(v) if v.is_finite() && v >= 0.0 => ResourcePressureLevel::Low,
            _ => ResourcePressureLevel::Moderate,
        };
        Self {
            memory_pressure,
            cpu_pressure,
            io_pressure,
            available_ram_bytes,
            telemetry_valid: true,
        }
    }
}

/// Per-object metadata evaluated by the compression decision engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectCompressionInput {
    pub current_codec: CodecId,
    pub original_bytes: usize,
    pub stored_bytes: usize,
    pub metadata_bytes: usize,
    pub access_count: u64,
    pub last_accessed_tick: u64,
    pub last_transition_tick: u64,
    pub current_tick: u64,
    pub incompressible_flagged: bool,
    pub estimated_compressibility_permille: Option<u64>,
}

/// Auditable decision output including target codec and explicit reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressionDecision {
    pub action: CompressionAction,
    pub target_codec: CodecId,
    pub reason: DecisionReason,
    pub temperature: PageTemperature,
    pub breakdown: CostBenefitBreakdown,
}

/// Configuration bounds for the deterministic compression decision policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveCompressionConfig {
    pub enable_lz4hc: bool,
    pub lz4hc_level: u8,
    pub min_savings_bytes: usize,
    pub min_recompression_savings_bytes: usize,
    pub min_scratch_ram_bytes: u64,
    pub selective_promotion: bool,
}

impl Default for AdaptiveCompressionConfig {
    fn default() -> Self {
        Self {
            enable_lz4hc: true,
            lz4hc_level: 6,
            min_savings_bytes: 128,
            min_recompression_savings_bytes: 256,
            min_scratch_ram_bytes: 8 * 1024 * 1024,
            selective_promotion: true,
        }
    }
}

/// Deterministic compression & tiering decision engine.
#[derive(Debug, Clone, Default)]
pub struct CompressionDecisionEngine {
    pub config: AdaptiveCompressionConfig,
}

impl CompressionDecisionEngine {
    pub fn new(config: AdaptiveCompressionConfig) -> Self {
        Self { config }
    }

    pub fn decide(
        &self,
        resources: &MultiResourceState,
        obj: &ObjectCompressionInput,
        cost_model: &ResourceCostModel,
        budget_remaining: MigrationBudget,
    ) -> CompressionDecision {
        let recency_ticks = obj.current_tick.saturating_sub(obj.last_accessed_tick);
        let temperature = cost_model.classify_temperature(recency_ticks, obj.access_count);

        let zero_breakdown = cost_model.evaluate_transition(
            obj.current_codec,
            obj.current_codec,
            obj.original_bytes,
            obj.stored_bytes,
            obj.metadata_bytes,
            obj.estimated_compressibility_permille,
            temperature,
            resources.memory_pressure,
            resources.cpu_pressure,
            resources.available_ram_bytes,
            resources.telemetry_valid,
        );

        // 1. Fail-safe fallback when telemetry is invalid or unreliable
        if !resources.telemetry_valid {
            return CompressionDecision {
                action: CompressionAction::FallbackSafe,
                target_codec: obj.current_codec,
                reason: DecisionReason::InvalidOrUnreliableTelemetry,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        // 2. Cooldown guard to prevent policy oscillation
        let ticks_since_transition = obj.current_tick.saturating_sub(obj.last_transition_tick);
        if obj.current_tick > 0
            && ticks_since_transition < cost_model.cooldown_ticks
            && resources.memory_pressure != MemoryPressure::Critical
        {
            return CompressionDecision {
                action: CompressionAction::RetainExisting,
                target_codec: obj.current_codec,
                reason: DecisionReason::CooldownActivePreventsOscillation,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        // 3. Incompressible data protection
        if obj.incompressible_flagged && obj.current_codec == CodecId::Raw {
            return CompressionDecision {
                action: CompressionAction::PreserveRaw,
                target_codec: CodecId::Raw,
                reason: DecisionReason::IncompressibleDataProtected,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        // 4. Low memory pressure: promote hot compressed objects to Raw (if budget allows)
        if resources.memory_pressure == MemoryPressure::Low {
            if obj.current_codec.is_compressed() {
                if !self.config.selective_promotion || temperature == PageTemperature::Hot {
                    if budget_remaining.max_pages == 0
                        || budget_remaining.max_bytes < obj.original_bytes
                    {
                        return CompressionDecision {
                            action: CompressionAction::DeferMigration,
                            target_codec: obj.current_codec,
                            reason: DecisionReason::MigrationBudgetExhausted,
                            temperature,
                            breakdown: zero_breakdown,
                        };
                    }
                    let promo_breakdown = cost_model.evaluate_transition(
                        obj.current_codec,
                        CodecId::Raw,
                        obj.original_bytes,
                        obj.stored_bytes,
                        obj.metadata_bytes,
                        obj.estimated_compressibility_permille,
                        temperature,
                        resources.memory_pressure,
                        resources.cpu_pressure,
                        resources.available_ram_bytes,
                        true,
                    );
                    return CompressionDecision {
                        action: CompressionAction::PromoteToRaw,
                        target_codec: CodecId::Raw,
                        reason: DecisionReason::HotPromotionUnderLowPressure,
                        temperature,
                        breakdown: promo_breakdown,
                    };
                }
                return CompressionDecision {
                    action: CompressionAction::RetainExisting,
                    target_codec: obj.current_codec,
                    reason: DecisionReason::AlreadyOptimal,
                    temperature,
                    breakdown: zero_breakdown,
                };
            }
            return CompressionDecision {
                action: CompressionAction::PreserveRaw,
                target_codec: CodecId::Raw,
                reason: DecisionReason::AlreadyOptimal,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        // 5. Moderate pressure protects Hot pages from compression latency
        if resources.memory_pressure == MemoryPressure::Moderate
            && temperature == PageTemperature::Hot
        {
            if obj.current_codec == CodecId::Raw {
                return CompressionDecision {
                    action: CompressionAction::PreserveRaw,
                    target_codec: CodecId::Raw,
                    reason: DecisionReason::HotDataProtectedFromCompressionLatency,
                    temperature,
                    breakdown: zero_breakdown,
                };
            } else {
                return CompressionDecision {
                    action: CompressionAction::RetainExisting,
                    target_codec: obj.current_codec,
                    reason: DecisionReason::AlreadyOptimal,
                    temperature,
                    breakdown: zero_breakdown,
                };
            }
        }

        // 6. Check migration budget before compressing or upgrading compression
        if budget_remaining.max_pages == 0 || budget_remaining.max_bytes < obj.original_bytes {
            return CompressionDecision {
                action: CompressionAction::DeferMigration,
                target_codec: obj.current_codec,
                reason: DecisionReason::MigrationBudgetExhausted,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        // 7. Evaluate candidate codecs (Standard LZ4 vs LZ4HC)
        let std_breakdown = cost_model.evaluate_transition(
            obj.current_codec,
            CodecId::Lz4Standard,
            obj.original_bytes,
            obj.stored_bytes,
            obj.metadata_bytes,
            obj.estimated_compressibility_permille,
            temperature,
            resources.memory_pressure,
            resources.cpu_pressure,
            resources.available_ram_bytes,
            true,
        );

        let hc_codec = CodecId::Lz4Hc {
            level: self.config.lz4hc_level.clamp(1, 12),
        };
        let hc_allowed_by_cpu = self.config.enable_lz4hc
            && resources.cpu_pressure < ResourcePressureLevel::High
            && temperature != PageTemperature::Hot;
        let hc_allowed_by_ram = resources
            .available_ram_bytes
            .is_none_or(|avail| avail >= self.config.min_scratch_ram_bytes);

        let hc_breakdown = cost_model.evaluate_transition(
            obj.current_codec,
            hc_codec,
            obj.original_bytes,
            obj.stored_bytes,
            obj.metadata_bytes,
            obj.estimated_compressibility_permille,
            temperature,
            resources.memory_pressure,
            resources.cpu_pressure,
            resources.available_ram_bytes,
            true,
        );

        // Already in Lz4Hc: retain existing to avoid pointless recompression
        if matches!(obj.current_codec, CodecId::Lz4Hc { .. }) {
            return CompressionDecision {
                action: CompressionAction::RetainExisting,
                target_codec: obj.current_codec,
                reason: DecisionReason::AlreadyOptimal,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        // Already in Lz4Standard: only upgrade to Lz4Hc under High/Critical memory pressure
        // when CPU pressure allows and extra savings are material.
        if obj.current_codec == CodecId::Lz4Standard {
            if !hc_allowed_by_cpu && resources.cpu_pressure >= ResourcePressureLevel::High {
                return CompressionDecision {
                    action: CompressionAction::RetainExisting,
                    target_codec: obj.current_codec,
                    reason: DecisionReason::ElevatedCpuPressureBlocksHighCompression,
                    temperature,
                    breakdown: zero_breakdown,
                };
            }
            if hc_allowed_by_cpu
                && hc_allowed_by_ram
                && matches!(
                    resources.memory_pressure,
                    MemoryPressure::High | MemoryPressure::Critical
                )
                && hc_breakdown.actual_bytes_saved_after_overhead
                    >= self.config.min_recompression_savings_bytes as isize
                && hc_breakdown.net_benefit_score > 0
            {
                return CompressionDecision {
                    action: CompressionAction::ApplyLz4Hc {
                        level: self.config.lz4hc_level.clamp(1, 12),
                    },
                    target_codec: hc_codec,
                    reason: DecisionReason::BeneficialHighCompression,
                    temperature,
                    breakdown: hc_breakdown,
                };
            }
            return CompressionDecision {
                action: CompressionAction::RetainExisting,
                target_codec: obj.current_codec,
                reason: DecisionReason::NegligibleSavingsAvoidsRecompression,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        // Currently Raw -> decide between DeferCompression, ApplyStandardLz4, ApplyLz4Hc, or PreserveRaw
        if resources.cpu_pressure == ResourcePressureLevel::Critical
            && resources.memory_pressure != MemoryPressure::Critical
        {
            return CompressionDecision {
                action: CompressionAction::DeferCompression,
                target_codec: CodecId::Raw,
                reason: DecisionReason::ElevatedCpuPressureBlocksHighCompression,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        if hc_allowed_by_cpu
            && hc_allowed_by_ram
            && matches!(
                resources.memory_pressure,
                MemoryPressure::High | MemoryPressure::Critical
            )
            && temperature == PageTemperature::Cold
            && hc_breakdown.net_benefit_score > std_breakdown.net_benefit_score
            && hc_breakdown.actual_bytes_saved_after_overhead
                >= self.config.min_savings_bytes as isize
        {
            return CompressionDecision {
                action: CompressionAction::ApplyLz4Hc {
                    level: self.config.lz4hc_level.clamp(1, 12),
                },
                target_codec: hc_codec,
                reason: DecisionReason::BeneficialHighCompression,
                temperature,
                breakdown: hc_breakdown,
            };
        }

        if std_breakdown.actual_bytes_saved_after_overhead >= self.config.min_savings_bytes as isize
            && (std_breakdown.net_benefit_score > 0
                || resources.memory_pressure == MemoryPressure::Critical)
        {
            return CompressionDecision {
                action: CompressionAction::ApplyStandardLz4,
                target_codec: CodecId::Lz4Standard,
                reason: DecisionReason::BeneficialStandardCompression,
                temperature,
                breakdown: std_breakdown,
            };
        }

        if !hc_allowed_by_cpu && resources.cpu_pressure >= ResourcePressureLevel::High {
            return CompressionDecision {
                action: CompressionAction::DeferCompression,
                target_codec: CodecId::Raw,
                reason: DecisionReason::ElevatedCpuPressureBlocksHighCompression,
                temperature,
                breakdown: zero_breakdown,
            };
        }

        CompressionDecision {
            action: CompressionAction::PreserveRaw,
            target_codec: CodecId::Raw,
            reason: DecisionReason::NegligibleSavingsAvoidsRecompression,
            temperature,
            breakdown: std_breakdown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cold_raw_page() -> ObjectCompressionInput {
        ObjectCompressionInput {
            current_codec: CodecId::Raw,
            original_bytes: 4096,
            stored_bytes: 4096,
            metadata_bytes: 96,
            access_count: 0,
            last_accessed_tick: 0,
            last_transition_tick: 0,
            current_tick: 200,
            incompressible_flagged: false,
            estimated_compressibility_permille: Some(400),
        }
    }

    #[test]
    fn high_cpu_pressure_blocks_lz4hc_and_falls_back_or_defers() {
        let engine = CompressionDecisionEngine::default();
        let cost = ResourceCostModel::default();
        let obj = cold_raw_page();

        let high_cpu = MultiResourceState {
            memory_pressure: MemoryPressure::High,
            cpu_pressure: ResourcePressureLevel::High,
            io_pressure: ResourcePressureLevel::Low,
            available_ram_bytes: Some(512 * 1024 * 1024),
            telemetry_valid: true,
        };
        let dec = engine.decide(&high_cpu, &obj, &cost, MigrationBudget::UNLIMITED);
        assert!(!matches!(dec.action, CompressionAction::ApplyLz4Hc { .. }));

        let low_cpu = MultiResourceState {
            cpu_pressure: ResourcePressureLevel::Low,
            ..high_cpu
        };
        let dec_hc = engine.decide(&low_cpu, &obj, &cost, MigrationBudget::UNLIMITED);
        assert!(matches!(
            dec_hc.action,
            CompressionAction::ApplyLz4Hc { .. }
        ));
        assert_eq!(dec_hc.reason, DecisionReason::BeneficialHighCompression);
    }

    #[test]
    fn incompressible_and_invalid_telemetry_are_handled_safely() {
        let engine = CompressionDecisionEngine::default();
        let cost = ResourceCostModel::default();

        let mut incomp = cold_raw_page();
        incomp.incompressible_flagged = true;
        let state = MultiResourceState {
            memory_pressure: MemoryPressure::Critical,
            cpu_pressure: ResourcePressureLevel::Low,
            io_pressure: ResourcePressureLevel::Low,
            available_ram_bytes: Some(128 * 1024 * 1024),
            telemetry_valid: true,
        };
        let dec = engine.decide(&state, &incomp, &cost, MigrationBudget::UNLIMITED);
        assert_eq!(dec.action, CompressionAction::PreserveRaw);
        assert_eq!(dec.reason, DecisionReason::IncompressibleDataProtected);

        let bad_state = MultiResourceState::fallback_safe();
        let dec_bad = engine.decide(
            &bad_state,
            &cold_raw_page(),
            &cost,
            MigrationBudget::UNLIMITED,
        );
        assert_eq!(dec_bad.action, CompressionAction::FallbackSafe);
        assert_eq!(dec_bad.reason, DecisionReason::InvalidOrUnreliableTelemetry);
    }

    #[test]
    fn cooldown_prevents_oscillation_and_avoids_negligible_recompression() {
        let engine = CompressionDecisionEngine::default();
        let cost = ResourceCostModel::default();
        let state = MultiResourceState {
            memory_pressure: MemoryPressure::High,
            cpu_pressure: ResourcePressureLevel::Low,
            io_pressure: ResourcePressureLevel::Low,
            available_ram_bytes: Some(512 * 1024 * 1024),
            telemetry_valid: true,
        };

        let mut recently_moved = cold_raw_page();
        recently_moved.last_transition_tick = 199;
        recently_moved.current_tick = 200;
        let dec_cd = engine.decide(&state, &recently_moved, &cost, MigrationBudget::UNLIMITED);
        assert_eq!(dec_cd.action, CompressionAction::RetainExisting);
        assert_eq!(
            dec_cd.reason,
            DecisionReason::CooldownActivePreventsOscillation
        );

        // Already compressed with Lz4Standard with tiny additional savings -> retain
        let already_lz4 = ObjectCompressionInput {
            current_codec: CodecId::Lz4Standard,
            original_bytes: 4096,
            stored_bytes: 400,
            metadata_bytes: 96,
            access_count: 0,
            last_accessed_tick: 0,
            last_transition_tick: 0,
            current_tick: 200,
            incompressible_flagged: false,
            estimated_compressibility_permille: Some(98),
        };
        let dec_negl = engine.decide(&state, &already_lz4, &cost, MigrationBudget::UNLIMITED);
        assert_eq!(dec_negl.action, CompressionAction::RetainExisting);
        assert_eq!(
            dec_negl.reason,
            DecisionReason::NegligibleSavingsAvoidsRecompression
        );
    }
}
