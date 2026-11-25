//! Hardware-aware universal memory optimization planning engine.
#![forbid(unsafe_code)]

use super::action::{
    ActionClassification, AuthorizationScope, ConfigSubsystem, ConfigurationAction,
    PersistenceRequirement, ReversalProcedure, TypedMutation, VerificationProcedure,
    CONFIG_ACTION_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use xynoos_pressureiq::discovery::{
    inventory::SystemInventory,
    types::{ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee},
};

/// Hardware profile categorization for memory tuning heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProfile {
    VeryLowMemory,           // <= 2 GiB
    LowMemoryLaptop,         // > 2 GiB and <= 8 GiB (e.g. N5000 4 GiB)
    MainstreamDesktop,       // > 8 GiB and <= 32 GiB
    HighMemoryWorkstation,   // > 32 GiB
    VirtualMachineContainer, // Virtualized / restricted container
}

/// Plan generator for universal memory optimizations.
pub struct MemoryPlanner;

impl MemoryPlanner {
    /// Categorize host hardware into an evidence-based memory profile.
    pub fn determine_profile(inv: &SystemInventory) -> MemoryProfile {
        if inv.platform.virtualization
            != xynoos_pressureiq::discovery::platform::VirtualizationEnvironment::Physical
            || inv.platform.container_environment
                != xynoos_pressureiq::discovery::platform::ContainerEnvironment::None
        {
            return MemoryProfile::VirtualMachineContainer;
        }

        let ram = inv.memory.total_bytes;
        let gib = 1024 * 1024 * 1024;

        if ram == 0 {
            MemoryProfile::LowMemoryLaptop
        } else if ram <= 2 * gib {
            MemoryProfile::VeryLowMemory
        } else if ram <= 8 * gib {
            MemoryProfile::LowMemoryLaptop
        } else if ram <= 32 * gib {
            MemoryProfile::MainstreamDesktop
        } else {
            MemoryProfile::HighMemoryWorkstation
        }
    }

    /// Select optimal kernel compression algorithm based on CPU capabilities.
    pub fn select_compression_algorithm(inv: &SystemInventory) -> &'static str {
        let is_low_power_cpu = inv.cpu.model_name.contains("N5000")
            || inv.cpu.model_name.contains("Celeron")
            || inv.cpu.model_name.contains("Atom")
            || inv.cpu.model_name.contains("Silver");

        if is_low_power_cpu {
            "lz4"
        } else if inv.cpu.physical_cores >= 4 && inv.memory.total_bytes >= 8 * 1024 * 1024 * 1024 {
            "zstd"
        } else {
            "lz4"
        }
    }

    /// Generate a comprehensive, reversible memory optimization plan.
    pub fn plan(inv: &SystemInventory) -> Vec<ConfigurationAction> {
        let mut actions = Vec::new();
        let profile = Self::determine_profile(inv);
        let selected_algo = Self::select_compression_algorithm(inv);

        let has_competing_zram_mgr = !inv.memory.zram_tuning_services.is_empty();
        let has_zram_device = !inv.memory.zram_devices.is_empty();
        let has_disk_swap = inv
            .memory
            .swap_devices
            .iter()
            .any(|s| !s.is_zram && s.total_bytes > 0);

        // 1. ZRAM Block Device Planning
        if has_competing_zram_mgr {
            let mgrs = inv.memory.zram_tuning_services.join(", ");
            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "ACT-ZRAM-PRESERVE-EXT".into(),
                target_subsystem: ConfigSubsystem::Compression,
                target_setting: "zram.service".into(),
                current_observed_value: format!("Active external manager(s): {mgrs}"),
                desired_value: "Preserve external ZRAM manager".into(),
                current_ownership: ConfigurationOwnership::ThirdPartyManaged,
                required_privileges: PrivilegeLevel::Unprivileged,
                required_capability: "kernel.zram".into(),
                expected_benefit: "Prevents race conditions and duplicate ZRAM allocation.".into(),
                potential_side_effects: "None; avoids conflicting device configuration.".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: ActionClassification::Preserve,
                authorization_scope: AuthorizationScope::MemoryOptimization,
                preconditions: vec!["external_zram_manager_active".into()],
                dependencies: vec![],
                conflict_status: None,
                verification_procedure: VerificationProcedure::NoOpPreserved,
                reversal_procedure: ReversalProcedure::NoOpPreserved,
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation: None,
            });
        } else if has_zram_device {
            let active_algo = inv
                .memory
                .zram_devices
                .first()
                .and_then(|z| z.active_algorithm.as_deref())
                .unwrap_or("unknown");
            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "ACT-ZRAM-PRESERVE-DEV".into(),
                target_subsystem: ConfigSubsystem::Compression,
                target_setting: "zram0.state".into(),
                current_observed_value: format!("Active ZRAM device (algo: {active_algo})"),
                desired_value: "Preserve active ZRAM device".into(),
                current_ownership: ConfigurationOwnership::DistributionManaged,
                required_privileges: PrivilegeLevel::Unprivileged,
                required_capability: "kernel.zram".into(),
                expected_benefit: "Maintains running compressed swap without unneeded swapoff."
                    .into(),
                potential_side_effects: "None.".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: ActionClassification::Preserve,
                authorization_scope: AuthorizationScope::MemoryOptimization,
                preconditions: vec!["zram_device_present".into()],
                dependencies: vec![],
                conflict_status: None,
                verification_procedure: VerificationProcedure::NoOpPreserved,
                reversal_procedure: ReversalProcedure::NoOpPreserved,
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation: None,
            });
        } else if profile == MemoryProfile::VeryLowMemory
            || profile == MemoryProfile::LowMemoryLaptop
        {
            let target_disksize = if inv.memory.total_bytes > 0 {
                (inv.memory.total_bytes / 2).min(2 * 1024 * 1024 * 1024)
            } else {
                1024 * 1024 * 1024
            };

            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "ACT-ZRAM-CONFIGURE".into(),
                target_subsystem: ConfigSubsystem::Compression,
                target_setting: "zram0.allocation".into(),
                current_observed_value: "No ZRAM devices initialized".into(),
                desired_value: format!("{target_disksize} bytes ({selected_algo}, priority 100)"),
                current_ownership: ConfigurationOwnership::KernelDefault,
                required_privileges: PrivilegeLevel::RequiresRoot,
                required_capability: "kernel.zram".into(),
                expected_benefit:
                    "Expands usable memory pool and avoids disk thrashing on low-resource hardware."
                        .into(),
                potential_side_effects:
                    "Minimal CPU overhead for compression during peak memory pressure.".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: ActionClassification::Configure,
                authorization_scope: AuthorizationScope::MemoryOptimization,
                preconditions: vec!["no_competing_zram_service".into()],
                dependencies: vec![],
                conflict_status: None,
                verification_procedure: VerificationProcedure::SysfsReadback {
                    sysfs_path: "block/zram0/disksize".into(),
                    expected_value: target_disksize.to_string(),
                },
                reversal_procedure: ReversalProcedure::ResetZramDevice {
                    device_name: "zram0".into(),
                },
                persistence_requirements: PersistenceRequirement::RuntimeAndDropIn,
                mutation: Some(TypedMutation::ConfigureZramDevice {
                    device_name: "zram0".into(),
                    disksize_bytes: target_disksize,
                    algorithm: selected_algo.to_string(),
                    priority: 100,
                }),
            });
        }

        // 2. vm.swappiness Optimization
        let swappiness_item = inv.tuning.sysctl_items.get("vm.swappiness");
        let current_swappiness = inv.memory.memory_sysctl.swappiness.unwrap_or(60);
        let swappiness_owner = swappiness_item
            .map(|i| i.ownership)
            .unwrap_or(ConfigurationOwnership::KernelDefault);

        if swappiness_owner == ConfigurationOwnership::AdministratorManaged {
            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "ACT-SYSCTL-SWAPPINESS-PRESERVE".into(),
                target_subsystem: ConfigSubsystem::Sysctl,
                target_setting: "vm.swappiness".into(),
                current_observed_value: current_swappiness.to_string(),
                desired_value: current_swappiness.to_string(),
                current_ownership: ConfigurationOwnership::AdministratorManaged,
                required_privileges: PrivilegeLevel::Unprivileged,
                required_capability: "kernel.sysctl".into(),
                expected_benefit: "Respects explicit human administrator sysctl declaration."
                    .into(),
                potential_side_effects: "None.".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: ActionClassification::Preserve,
                authorization_scope: AuthorizationScope::MemoryOptimization,
                preconditions: vec!["admin_sysctl_declared".into()],
                dependencies: vec![],
                conflict_status: None,
                verification_procedure: VerificationProcedure::NoOpPreserved,
                reversal_procedure: ReversalProcedure::NoOpPreserved,
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation: None,
            });
        } else {
            let target_swappiness = if has_zram_device || !has_competing_zram_mgr {
                100u64
            } else if inv.storage.has_rotational_storage && !has_zram_device {
                10u64
            } else {
                60u64
            };

            if current_swappiness as u64 != target_swappiness {
                let dropin_content = format!(
                    "# XYNOOS HyperCore Managed Memory Optimization\n# Drop-in generated for vm.swappiness\nvm.swappiness = {target_swappiness}\n"
                );
                actions.push(ConfigurationAction {
                    schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                    action_id: "ACT-SYSCTL-SWAPPINESS-ADAPT".into(),
                    target_subsystem: ConfigSubsystem::Sysctl,
                    target_setting: "vm.swappiness".into(),
                    current_observed_value: current_swappiness.to_string(),
                    desired_value: target_swappiness.to_string(),
                    current_ownership: swappiness_owner,
                    required_privileges: PrivilegeLevel::RequiresRoot,
                    required_capability: "kernel.sysctl".into(),
                    expected_benefit: "Balances anonymous memory paging with file cache retention."
                        .into(),
                    potential_side_effects:
                        "Slightly increased swap activity under peak anonymous allocation.".into(),
                    confidence: ConfidenceLevel::High,
                    reversibility: ReversibilityGuarantee::FullyReversible,
                    classification: ActionClassification::Improve,
                    authorization_scope: AuthorizationScope::MemoryOptimization,
                    preconditions: vec!["not_admin_managed".into()],
                    dependencies: vec![],
                    conflict_status: None,
                    verification_procedure: VerificationProcedure::SysctlReadback {
                        key: "vm.swappiness".into(),
                        expected_value: target_swappiness,
                    },
                    reversal_procedure: ReversalProcedure::RestoreSysctl {
                        key: "vm.swappiness".into(),
                        original_value: current_swappiness as u64,
                    },
                    persistence_requirements: PersistenceRequirement::RuntimeAndDropIn,
                    mutation: Some(TypedMutation::ApplySysctl {
                        key: "vm.swappiness".into(),
                        target_value: target_swappiness,
                        original_value: Some(current_swappiness as u64),
                    }),
                });

                actions.push(ConfigurationAction {
                    schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                    action_id: "ACT-DROPIN-MEMORY-SYSCTL".into(),
                    target_subsystem: ConfigSubsystem::Sysctl,
                    target_setting: "etc/sysctl.d/99-xynoos-memory.conf".into(),
                    current_observed_value: "No drop-in file present".into(),
                    desired_value: format!("Drop-in with vm.swappiness = {target_swappiness}"),
                    current_ownership: ConfigurationOwnership::KernelDefault,
                    required_privileges: PrivilegeLevel::RequiresRoot,
                    required_capability: "fs.sysctl_d".into(),
                    expected_benefit: "Persists memory sysctl parameters across system reboots."
                        .into(),
                    potential_side_effects: "None; strictly scoped to dedicated HyperCore drop-in."
                        .into(),
                    confidence: ConfidenceLevel::High,
                    reversibility: ReversibilityGuarantee::FullyReversible,
                    classification: ActionClassification::Configure,
                    authorization_scope: AuthorizationScope::MemoryOptimization,
                    preconditions: vec!["sysctl_d_supported".into()],
                    dependencies: vec!["ACT-SYSCTL-SWAPPINESS-ADAPT".into()],
                    conflict_status: None,
                    verification_procedure: VerificationProcedure::DropInFileExists {
                        path: "etc/sysctl.d/99-xynoos-memory.conf".into(),
                        expected_sha256: "".into(),
                    },
                    reversal_procedure: ReversalProcedure::DeleteDropIn {
                        target_path: "etc/sysctl.d/99-xynoos-memory.conf".into(),
                        expected_sha256: "".into(),
                    },
                    persistence_requirements: PersistenceRequirement::DropInFile,
                    mutation: Some(TypedMutation::WriteSysctlDropIn {
                        dropin_filename: "99-xynoos-memory.conf".into(),
                        content: dropin_content,
                        key: "vm.swappiness".into(),
                        target_value: target_swappiness,
                    }),
                });
            }
        }

        // 3. vm.page-cluster Adaptation
        let current_page_cluster = inv.memory.memory_sysctl.page_cluster.unwrap_or(3);
        if (has_zram_device || !has_competing_zram_mgr) && current_page_cluster > 0 {
            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "ACT-SYSCTL-PAGE-CLUSTER".into(),
                target_subsystem: ConfigSubsystem::Sysctl,
                target_setting: "vm.page-cluster".into(),
                current_observed_value: current_page_cluster.to_string(),
                desired_value: "0".into(),
                current_ownership: ConfigurationOwnership::KernelDefault,
                required_privileges: PrivilegeLevel::RequiresRoot,
                required_capability: "kernel.sysctl".into(),
                expected_benefit: "Disables sequential swap readahead; eliminates wasted decompression for random zram reads.".into(),
                potential_side_effects: "None for RAM-backed zram swap.".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: ActionClassification::Improve,
                authorization_scope: AuthorizationScope::MemoryOptimization,
                preconditions: vec!["zram_swap_primary".into()],
                dependencies: vec![],
                conflict_status: None,
                verification_procedure: VerificationProcedure::SysctlReadback {
                    key: "vm.page-cluster".into(),
                    expected_value: 0,
                },
                reversal_procedure: ReversalProcedure::RestoreSysctl {
                    key: "vm.page-cluster".into(),
                    original_value: current_page_cluster as u64,
                },
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation: Some(TypedMutation::ApplySysctl {
                    key: "vm.page-cluster".into(),
                    target_value: 0,
                    original_value: Some(current_page_cluster as u64),
                }),
            });
        }

        // 4. ZSWAP Coordination
        if inv.memory.zswap.available
            && inv.memory.zswap.enabled
            && has_disk_swap
            && has_zram_device
        {
            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "ACT-ZSWAP-PRESERVE-TIERED".into(),
                target_subsystem: ConfigSubsystem::Swap,
                target_setting: "zswap.tiered_coexistence".into(),
                current_observed_value: "Enabled alongside disk swap and ZRAM".into(),
                desired_value: "Preserve tiered swap hierarchy".into(),
                current_ownership: ConfigurationOwnership::DistributionManaged,
                required_privileges: PrivilegeLevel::Unprivileged,
                required_capability: "kernel.zswap".into(),
                expected_benefit: "ZRAM handles hot swap while ZSWAP buffers writes to disk swap."
                    .into(),
                potential_side_effects: "None.".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: ActionClassification::Preserve,
                authorization_scope: AuthorizationScope::MemoryOptimization,
                preconditions: vec!["disk_swap_present".into()],
                dependencies: vec![],
                conflict_status: None,
                verification_procedure: VerificationProcedure::NoOpPreserved,
                reversal_procedure: ReversalProcedure::NoOpPreserved,
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation: None,
            });
        }

        // 5. Transparent Huge Pages (THP) Planning
        if let Some(thp) = &inv.memory.thp_enabled {
            if thp.contains("[always]")
                && (profile == MemoryProfile::VeryLowMemory
                    || profile == MemoryProfile::LowMemoryLaptop)
            {
                actions.push(ConfigurationAction {
                    schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                    action_id: "ACT-THP-MADVISE".into(),
                    target_subsystem: ConfigSubsystem::TransparentHugePages,
                    target_setting: "transparent_hugepage.enabled".into(),
                    current_observed_value: "always".into(),
                    desired_value: "madvise".into(),
                    current_ownership: ConfigurationOwnership::KernelDefault,
                    required_privileges: PrivilegeLevel::RequiresRoot,
                    required_capability: "kernel.mm.thp".into(),
                    expected_benefit:
                        "Avoids memory bloating and direct compaction latency on low-RAM systems."
                            .into(),
                    potential_side_effects:
                        "Applications wanting 2MB hugepages must issue madvise(MADV_HUGEPAGE)."
                            .into(),
                    confidence: ConfidenceLevel::Medium,
                    reversibility: ReversibilityGuarantee::FullyReversible,
                    classification: ActionClassification::Improve,
                    authorization_scope: AuthorizationScope::MemoryOptimization,
                    preconditions: vec!["thp_available".into()],
                    dependencies: vec![],
                    conflict_status: None,
                    verification_procedure: VerificationProcedure::SysfsReadback {
                        sysfs_path: "kernel/mm/transparent_hugepage/enabled".into(),
                        expected_value: "madvise".into(),
                    },
                    reversal_procedure: ReversalProcedure::RestoreThp {
                        original_mode: "always".into(),
                    },
                    persistence_requirements: PersistenceRequirement::RuntimeOnly,
                    mutation: Some(TypedMutation::ConfigureTransparentHugePages {
                        mode: "madvise".into(),
                        original_mode: Some("always".into()),
                    }),
                });
            } else {
                actions.push(ConfigurationAction {
                    schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                    action_id: "ACT-THP-PRESERVE".into(),
                    target_subsystem: ConfigSubsystem::TransparentHugePages,
                    target_setting: "transparent_hugepage.enabled".into(),
                    current_observed_value: thp.clone(),
                    desired_value: thp.clone(),
                    current_ownership: ConfigurationOwnership::KernelDefault,
                    required_privileges: PrivilegeLevel::Unprivileged,
                    required_capability: "kernel.mm.thp".into(),
                    expected_benefit: "Maintains current THP mode aligned with platform capacity."
                        .into(),
                    potential_side_effects: "None.".into(),
                    confidence: ConfidenceLevel::High,
                    reversibility: ReversibilityGuarantee::FullyReversible,
                    classification: ActionClassification::Preserve,
                    authorization_scope: AuthorizationScope::MemoryOptimization,
                    preconditions: vec![],
                    dependencies: vec![],
                    conflict_status: None,
                    verification_procedure: VerificationProcedure::NoOpPreserved,
                    reversal_procedure: ReversalProcedure::NoOpPreserved,
                    persistence_requirements: PersistenceRequirement::RuntimeOnly,
                    mutation: None,
                });
            }
        }

        // 6. Userspace OOM Coexistence
        let mut active_oom = Vec::new();
        if inv.memory.oom_managers.earlyoom_active {
            active_oom.push("earlyoom");
        }
        if inv.memory.oom_managers.systemd_oomd_active {
            active_oom.push("systemd-oomd");
        }
        if inv.memory.oom_managers.nohang_active {
            active_oom.push("nohang");
        }
        if inv.memory.oom_managers.oomd_active {
            active_oom.push("oomd");
        }

        if !active_oom.is_empty() {
            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "ACT-OOM-COEXIST".into(),
                target_subsystem: ConfigSubsystem::Memory,
                target_setting: "oom.coexistence".into(),
                current_observed_value: format!("Active OOM daemons: {}", active_oom.join(", ")),
                desired_value: "Preserve active OOM protection without interference".into(),
                current_ownership: ConfigurationOwnership::ThirdPartyManaged,
                required_privileges: PrivilegeLevel::Unprivileged,
                required_capability: "process.monitoring".into(),
                expected_benefit: "Ensures HyperCore optimization cooperates with existing emergency OOM safeguards.".into(),
                potential_side_effects: "None; avoids double-killing applications.".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: ActionClassification::Preserve,
                authorization_scope: AuthorizationScope::MemoryOptimization,
                preconditions: vec!["oom_manager_present".into()],
                dependencies: vec![],
                conflict_status: None,
                verification_procedure: VerificationProcedure::NoOpPreserved,
                reversal_procedure: ReversalProcedure::NoOpPreserved,
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation: None,
            });
        }

        actions
    }
}
