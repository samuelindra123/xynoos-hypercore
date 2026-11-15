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
