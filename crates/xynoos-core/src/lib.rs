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
