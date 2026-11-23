#![forbid(unsafe_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    Low,
    Moderate,
    High,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTier {
    Memory,
    Compressed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostEstimate {
    pub latency_ns: u64,
    pub bytes: usize,
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
