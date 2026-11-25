//! Unified, versioned, structured SystemInventory schema.
#![forbid(unsafe_code)]

use super::{
    config::{discover_configuration, ConfigurationInventory},
    conflict::{analyze_conflicts, ConflictFinding},
    cpu::{discover_cpu, CpuInventory},
    gpu::{discover_gpu, GpuInventory},
    memory::{discover_memory, MemoryInventory},
    platform::{discover_platform, PlatformInventory},
    power::{discover_power, PowerInventory},
    storage::{discover_storage, StorageInventory},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const INVENTORY_SCHEMA_VERSION: u32 = 1;

/// Versioned, privacy-preserving, holistic system inventory representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInventory {
    pub schema_version: u32,
    pub collected_at_unix_ms: u64,
    pub collection_duration_ns: u64,
    pub platform: PlatformInventory,
    pub cpu: CpuInventory,
    pub memory: MemoryInventory,
    pub gpu: GpuInventory,
    pub storage: StorageInventory,
    pub power: PowerInventory,
    pub tuning: ConfigurationInventory,
    pub conflicts: Vec<ConflictFinding>,
    pub metadata: BTreeMap<String, String>,
}

impl SystemInventory {
    /// Collect comprehensive system inventory across given filesystem roots (for live system or fixtures).
    pub fn collect_from(sys_root: &Path, proc_root: &Path, etc_root: &Path) -> Self {
        let t0 = Instant::now();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let platform = discover_platform(sys_root, proc_root, etc_root);
        let cpu = discover_cpu(sys_root, proc_root);
        let memory = discover_memory(sys_root, proc_root, etc_root);
        let gpu = discover_gpu(sys_root);
        let storage = discover_storage(sys_root, proc_root);
        let power = discover_power(sys_root, proc_root, etc_root);
        let tuning = discover_configuration(proc_root, etc_root);

        let conflicts = analyze_conflicts(&cpu, &memory, &storage, &power, &tuning);

        let mut metadata = BTreeMap::new();
        metadata.insert("engine".into(), "XYNOOS HyperCore".into());
        metadata.insert("milestone".into(), "Milestone K".into());
        metadata.insert("sys_root".into(), sys_root.display().to_string());
        metadata.insert("proc_root".into(), proc_root.display().to_string());
        metadata.insert("etc_root".into(), etc_root.display().to_string());

        Self {
            schema_version: INVENTORY_SCHEMA_VERSION,
            collected_at_unix_ms: now_ms,
            collection_duration_ns: t0.elapsed().as_nanos() as u64,
            platform,
            cpu,
            memory,
            gpu,
            storage,
            power,
            tuning,
            conflicts,
            metadata,
        }
    }

    /// Collect comprehensive system inventory from live standard Linux paths.
    pub fn collect_live() -> Self {
        Self::collect_from(Path::new("/sys"), Path::new("/proc"), Path::new("/etc"))
    }
}

impl Default for SystemInventory {
    fn default() -> Self {
        Self::collect_from(
            Path::new("/nonexistent-sys"),
            Path::new("/nonexistent-proc"),
            Path::new("/nonexistent-etc"),
        )
    }
}

/// Bounded cached system inventory to prevent redundant expensive hardware/config probing.
#[derive(Debug, Clone)]
pub struct CachedSystemInventory {
    inventory: SystemInventory,
    cached_at: Instant,
    ttl: std::time::Duration,
}

impl CachedSystemInventory {
    pub fn new(ttl: std::time::Duration) -> Self {
        Self {
            inventory: SystemInventory::collect_live(),
            cached_at: Instant::now(),
            ttl,
        }
    }

    pub fn with_initial(inventory: SystemInventory, ttl: std::time::Duration) -> Self {
        Self {
            inventory,
            cached_at: Instant::now(),
            ttl,
        }
    }

    pub fn get_or_refresh(&mut self) -> &SystemInventory {
        if self.cached_at.elapsed() > self.ttl {
            self.inventory = SystemInventory::collect_live();
            self.cached_at = Instant::now();
        }
        &self.inventory
    }

    pub fn current(&self) -> &SystemInventory {
        &self.inventory
    }

    pub fn invalidate(&mut self) {
        self.cached_at = Instant::now()
            .checked_sub(self.ttl + std::time::Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_creation() {
        let inv = SystemInventory::collect_from(
            Path::new("/nonexistent-sys"),
            Path::new("/nonexistent-proc"),
            Path::new("/nonexistent-etc"),
        );
        assert_eq!(inv.schema_version, 1);
        assert_eq!(inv.platform.distribution_id, "unknown");
    }
}
