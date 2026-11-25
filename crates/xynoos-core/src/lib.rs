#![forbid(unsafe_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClusterId(pub u64);
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum MemoryPressure {
    Low,
    Moderate,
    High,
    Critical,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTier {
    Memory,
    Compressed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostEstimate {
    /// Heuristic access score incorporating base tier access penalty and recency penalty.
    pub latency_ns: u64,
    pub bytes: usize,
}

impl CostEstimate {
    pub const fn new(latency_ns: u64, bytes: usize) -> Self {
        Self { latency_ns, bytes }
    }

    /// Explicit alias separating heuristic score semantics from physical wall-clock latency.
    pub const fn heuristic_score(&self) -> u64 {
        self.latency_ns
    }
}

/// Explicit budget constraints for adaptive page tier migrations in a single evaluation cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationBudget {
    /// Maximum number of pages permitted to migrate in a single cycle.
    pub max_pages: usize,
    /// Maximum number of bytes permitted to be transferred/mutated during migrations in a cycle.
    pub max_bytes: usize,
}

impl Default for MigrationBudget {
    fn default() -> Self {
        Self::UNLIMITED
    }
}

impl MigrationBudget {
    /// An unbounded migration budget (no limit on page count or bytes per cycle).
    pub const UNLIMITED: Self = Self {
        max_pages: usize::MAX,
        max_bytes: usize::MAX,
    };

    /// Creates a strictly bounded migration budget.
    pub const fn bounded(max_pages: usize, max_bytes: usize) -> Self {
        Self {
            max_pages,
            max_bytes,
        }
    }
}

/// Explicit lifecycle state of a page stored in a memory engine or cache tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLifecycleState {
    /// Page is currently active and stored in the memory engine.
    Active,
    /// Page was issued and was evicted by a capacity or budget policy.
    Evicted,
    /// Page was issued and was explicitly removed by caller request.
    Removed,
    /// PageId was never issued by this store instance.
    NeverCreated,
}

/// Deterministic eviction policies for bounded memory storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvictionPolicy {
    /// Reject new allocations when capacity or budget is exhausted.
    #[default]
    Reject,
    /// Evict least recently accessed page (ties broken by PageId).
    Lru,
    /// Evict oldest inserted page (First-In, First-Out, ties broken by PageId).
    Fifo,
}

/// Explicit codec identifier stored with every compressed or raw object.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CodecId {
    /// Uncompressed raw byte storage.
    Raw,
    /// Standard fast LZ4 block compression.
    #[default]
    Lz4Standard,
    /// Pure-safe-Rust LZ4HC hash-chain + lazy-match block compression.
    Lz4Hc { level: u8 },
}

impl CodecId {
    pub const fn is_compressed(self) -> bool {
        !matches!(self, CodecId::Raw)
    }

    pub const fn tier(self) -> StorageTier {
        match self {
            CodecId::Raw => StorageTier::Memory,
            CodecId::Lz4Standard | CodecId::Lz4Hc { .. } => StorageTier::Compressed,
        }
    }

    pub const fn canonical_name(self) -> &'static str {
        match self {
            CodecId::Raw => "raw",
            CodecId::Lz4Standard => "lz4_standard",
            CodecId::Lz4Hc { .. } => "lz4_hc",
        }
    }
}

/// Explicit compression configuration with minimum savings and scratch bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompressionConfig {
    pub codec: CodecId,
    pub min_savings_bytes: usize,
    pub max_scratch_bytes: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            codec: CodecId::Lz4Standard,
            min_savings_bytes: 64,
            max_scratch_bytes: 512 * 1024,
        }
    }
}

/// Multi-resource pressure levels for CPU, memory, and I/O contention.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    Default,
)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePressureLevel {
    #[default]
    Low,
    Moderate,
    High,
    Critical,
}

/// Auditable action selected by the adaptive compression decision engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionAction {
    PreserveRaw,
    ApplyStandardLz4,
    ApplyLz4Hc { level: u8 },
    RetainExisting,
    DeferCompression,
    DeferMigration,
    PromoteToRaw,
    FallbackSafe,
}

/// Auditable reason code explaining a compression or migration decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    InvalidOrUnreliableTelemetry,
    ElevatedCpuPressureBlocksHighCompression,
    HighIoOrMemoryContentionDefersWork,
    IncompressibleDataProtected,
    NegligibleSavingsAvoidsRecompression,
    HotDataProtectedFromCompressionLatency,
    CooldownActivePreventsOscillation,
    MigrationBudgetExhausted,
    InsufficientAvailableMemoryForScratch,
    BeneficialStandardCompression,
    BeneficialHighCompression,
    HotPromotionUnderLowPressure,
    AlreadyOptimal,
}

/// Overhead-inclusive memory accounting snapshot for HyperMemory stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct MemoryAccounting {
    pub original_logical_bytes: usize,
    pub stored_payload_bytes: usize,
    pub metadata_bytes: usize,
    pub allocated_capacity_bytes: usize,
    pub scratch_buffer_bytes: usize,
    pub peak_memory_bytes: usize,
    pub total_accounted_bytes: usize,
    pub net_savings_bytes: isize,
}

impl MemoryAccounting {
    pub fn net_savings_ratio(&self) -> f64 {
        if self.original_logical_bytes == 0 {
            0.0
        } else {
            self.net_savings_bytes as f64 / self.original_logical_bytes as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_types_and_defaults() {
        let est = CostEstimate::new(350, 4096);
        assert_eq!(est.latency_ns, 350);
        assert_eq!(est.heuristic_score(), 350);
        assert_eq!(est.bytes, 4096);

        let def_budget = MigrationBudget::default();
        assert_eq!(def_budget, MigrationBudget::UNLIMITED);
        assert_eq!(def_budget.max_pages, usize::MAX);

        let bounded = MigrationBudget::bounded(16, 65536);
        assert_eq!(bounded.max_pages, 16);
        assert_eq!(bounded.max_bytes, 65536);
    }
}
