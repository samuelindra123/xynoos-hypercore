#![forbid(unsafe_code)]
use xynoos_core::{CostEstimate, MemoryPressure, StorageTier};
pub trait DecisionPolicy {
    fn decide(&self, pressure: MemoryPressure, cost: CostEstimate) -> StorageTier;
}
pub struct Baseline;
impl DecisionPolicy for Baseline {
    fn decide(&self, pressure: MemoryPressure, _cost: CostEstimate) -> StorageTier {
        match pressure {
            MemoryPressure::Low => StorageTier::Memory,
            MemoryPressure::Moderate | MemoryPressure::High | MemoryPressure::Critical => {
                StorageTier::Compressed
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic() {
        let c = CostEstimate {
            latency_ns: 1,
            bytes: 4,
        };
        for _ in 0..10 {
            assert_eq!(
                Baseline.decide(MemoryPressure::Moderate, c),
                StorageTier::Compressed
            );
            assert_eq!(
                Baseline.decide(MemoryPressure::High, c),
                StorageTier::Compressed
            );
            assert_eq!(
                Baseline.decide(MemoryPressure::Critical, c),
                StorageTier::Compressed
            );
            assert_eq!(Baseline.decide(MemoryPressure::Low, c), StorageTier::Memory);
        }
    }
}

pub mod placement;

pub mod physical;
pub mod selector;

pub mod memory;

pub mod resource;

pub mod cost_model;
pub use cost_model::{CostBenefitBreakdown, PageTemperature, ResourceCostModel, RollingCostStats};

pub mod compression_policy;
pub use compression_policy::{
    AdaptiveCompressionConfig, CompressionDecision, CompressionDecisionEngine, MultiResourceState,
    ObjectCompressionInput,
};

pub mod tiering;
pub use tiering::{
    AdaptiveTieringPolicy, AdaptiveTieringReport, AuraAdaptiveCoordinator, AuraTieringCoordinator,
    PressureBridge, PressureStabilizer, TelemetryValidation,
};

pub mod interplay;
pub use interplay::{AuraKernelCoordinator, InterplayReport, KernelTuningRecommendation};

pub mod daemon;
pub use daemon::{
    AuraDaemon, DaemonConfig, DaemonDiagnostics, DaemonMode, PidLock, TransactionJournal,
};

pub mod profiler;
pub use profiler::{
    CalibrationReport, PolicyBaselineComparison, PolicyConfig, ShadowCalibrator,
    ShadowEvaluationReport, ShadowPlayer, TraceRecord, WorkloadTrace,
};
