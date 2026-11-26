//! Universal Hardware Intelligence, Kernel Capability Discovery & Existing Configuration Intelligence.
#![forbid(unsafe_code)]

pub mod config;
pub mod conflict;
pub mod cpu;
pub mod gpu;
pub mod inventory;
pub mod memory;
pub mod plan;
pub mod platform;
pub mod power;
pub mod storage;
pub mod types;

pub use config::{discover_configuration, ConfigItem, ConfigurationInventory};
pub use conflict::{analyze_conflicts, ConflictFinding};
pub use cpu::{discover_cpu, CpuInventory, CpuPolicy};
pub use gpu::{discover_gpu, GpuDevice, GpuInventory, GpuType, GpuVendor};
pub use inventory::{CachedSystemInventory, SystemInventory, INVENTORY_SCHEMA_VERSION};
pub use memory::{discover_memory, MemoryInventory, SwapDevice, ZramDevice, ZswapConfiguration};
pub use plan::{
    OptimizationPlan, ProposedAction, ProposedActionType, Subsystem, PLAN_SCHEMA_VERSION,
};
pub use platform::{
    assess_compatibility, discover_platform, CgroupHierarchy, CompatibilityTier,
    ContainerEnvironment, DistributionFamily, InitSystem, LibcKind, PackageBackendKind,
    PlatformCompatibilityAssessment, PlatformInventory, VirtualizationEnvironment,
};
pub use power::{discover_power, PowerDaemons, PowerInventory, ServiceState, ThermalZoneInfo};
pub use storage::{discover_storage, BlockDeviceInfo, StorageInventory, StorageMediaType};
pub use types::{
    ConfidenceLevel, ConfigurationOwnership, ConflictSeverity, DiscoveryStatus, PrivilegeLevel,
    ReversibilityGuarantee,
};
