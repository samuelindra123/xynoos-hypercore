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
            MemoryPressure::Moderate | MemoryPressure::High => StorageTier::Compressed,
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
            assert_eq!(Baseline.decide(MemoryPressure::Low, c), StorageTier::Memory);
        }
    }
}

pub mod placement;

pub mod physical;
pub mod selector;

pub mod memory;
