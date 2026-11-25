//! Read-only optimization planning engine.
#![forbid(unsafe_code)]

use super::{
    inventory::SystemInventory,
    storage::StorageMediaType,
    types::{ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee},
};
use serde::{Deserialize, Serialize};

pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// Target subsystem of an optimization recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subsystem {
    Cpu,
    Memory,
    Compression,
    Storage,
    Power,
    Cgroup,
    Sysctl,
}

/// Action classification for a proposed optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposedActionType {
    /// Beneficial existing configuration detected; preserve without mutation.
    Preserve,
    /// Beneficial optimization missing; propose configuring safely.
    Configure,
    /// Suboptimal configuration detected; propose improving setting.
    Improve,
    /// Action deferred due to current workload conditions or power profile.
    Defer,
    /// Feature unsupported on current kernel, architecture, or hardware.
    Unsupported,
    /// Competing daemon or policy conflict detected; avoid uncoordinated changes.
    Conflict,
    /// Potentially intrusive action requiring explicit operator authorization.
    RequiresAuthorization,
    /// Insufficient evidence to justify a safe recommendation.
    InsufficientEvidence,
}

/// A structured, auditable proposed optimization action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedAction {
    pub subsystem: Subsystem,
    pub action_type: ProposedActionType,
    pub target_parameter: String,
    pub current_state: String,
    pub detected_ownership: ConfigurationOwnership,
    pub required_capability: String,
    pub proposed_setting: Option<String>,
    pub rationale: String,
    pub expected_benefit: String,
    pub potential_trade_offs: String,
    pub required_privileges: PrivilegeLevel,
    pub reversibility: ReversibilityGuarantee,
    pub confidence: ConfidenceLevel,
    pub evidence_references: Vec<String>,
}

/// Complete read-only optimization plan generated from a SystemInventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationPlan {
    pub schema_version: u32,
    pub generated_at_unix_ms: u64,
    pub actions: Vec<ProposedAction>,
    pub summary_preserve_count: usize,
    pub summary_configure_count: usize,
    pub summary_improve_count: usize,
    pub summary_conflict_count: usize,
    pub summary_unsupported_count: usize,
}

impl OptimizationPlan {
    /// Generate a strictly read-only optimization plan based on discovered system inventory.
    pub fn generate(inv: &SystemInventory) -> Self {
        let mut actions = Vec::new();

        // 1. ZRAM Memory Compression Planning
        let has_competing_zram = !inv.memory.zram_tuning_services.is_empty();
        let has_zram_device = !inv.memory.zram_devices.is_empty();

        if has_competing_zram {
            let mgrs = inv.memory.zram_tuning_services.join(", ");
            actions.push(ProposedAction {
                subsystem: Subsystem::Compression,
                action_type: ProposedActionType::Preserve,
                target_parameter: "zram.management".into(),
                current_state: format!("Managed externally by {mgrs}"),
                detected_ownership: ConfigurationOwnership::ThirdPartyManaged,
                required_capability: "kernel.zram".into(),
                proposed_setting: None,
                rationale: "External ZRAM service is already configured; preserve existing devices to prevent race conditions.".into(),
                expected_benefit: "System stability; prevents conflicting device re-initialization.".into(),
                potential_trade_offs: "None; avoids double management.".into(),
                required_privileges: PrivilegeLevel::Unprivileged,
                reversibility: ReversibilityGuarantee::FullyReversible,
                confidence: ConfidenceLevel::High,
                evidence_references: inv.memory.zram_tuning_services.iter().map(|s| format!("service:{s}")).collect(),
            });
        } else if has_zram_device {
            let active_algo = inv
                .memory
                .zram_devices
                .first()
                .and_then(|z| z.active_algorithm.as_deref())
                .unwrap_or("unknown");
            actions.push(ProposedAction {
                subsystem: Subsystem::Compression,
                action_type: ProposedActionType::Preserve,
                target_parameter: "zram.algorithm".into(),
                current_state: format!("Active ZRAM algorithm: {active_algo}"),
                detected_ownership: ConfigurationOwnership::DistributionManaged,
                required_capability: "kernel.zram".into(),
                proposed_setting: None,
                rationale: "Existing ZRAM block device detected and active.".into(),
                expected_benefit: "Maintains existing compressed swap capacity.".into(),
                potential_trade_offs: "None.".into(),
                required_privileges: PrivilegeLevel::Unprivileged,
                reversibility: ReversibilityGuarantee::FullyReversible,
                confidence: ConfidenceLevel::High,
                evidence_references: vec![format!("zram_algorithm:{active_algo}")],
            });
        } else if inv.memory.total_bytes <= 8 * 1024 * 1024 * 1024 && inv.memory.total_bytes > 0 {
            // Low to moderate RAM without ZRAM
            actions.push(ProposedAction {
                subsystem: Subsystem::Compression,
                action_type: ProposedActionType::Configure,
                target_parameter: "zram.adaptive_swap".into(),
                current_state: "No ZRAM devices configured".into(),
                detected_ownership: ConfigurationOwnership::KernelDefault,
                required_capability: "kernel.zram".into(),
                proposed_setting: Some("zram-swap-50pct-lz4".into()),
                rationale: "System has <= 8 GiB RAM and no compressed swap. Configuring LZ4 ZRAM swap expands effective multitasking capacity with minimal CPU overhead.".into(),
                expected_benefit: "Reduces out-of-memory pressure and eliminates disk thrashing on low-memory workloads.".into(),
                potential_trade_offs: "Consumes CPU cycles for memory compression (typically < 1% on modern cores).".into(),
                required_privileges: PrivilegeLevel::RequiresRoot,
                reversibility: ReversibilityGuarantee::ReversibleConfig,
                confidence: ConfidenceLevel::High,
                evidence_references: vec![format!("mem_total_bytes:{}", inv.memory.total_bytes)],
            });
        }

        // 2. Swappiness Tuning Planning
        let swappiness_item = inv.tuning.sysctl_items.get("vm.swappiness");
        let current_swappiness = inv.memory.memory_sysctl.swappiness.unwrap_or(60);
        let swappiness_owner = swappiness_item
            .map(|i| i.ownership)
            .unwrap_or(ConfigurationOwnership::KernelDefault);

        if swappiness_owner == ConfigurationOwnership::AdministratorManaged {
            actions.push(ProposedAction {
                subsystem: Subsystem::Sysctl,
                action_type: ProposedActionType::Preserve,
                target_parameter: "vm.swappiness".into(),
                current_state: format!("{current_swappiness}"),
                detected_ownership: ConfigurationOwnership::AdministratorManaged,
                required_capability: "kernel.sysctl".into(),
                proposed_setting: None,
                rationale: "Administrator explicitly declared vm.swappiness in /etc/sysctl.conf; preserve human intent.".into(),
                expected_benefit: "Respects explicit system operator policy.".into(),
                potential_trade_offs: "None.".into(),
                required_privileges: PrivilegeLevel::Unprivileged,
                reversibility: ReversibilityGuarantee::VolatileRuntime,
                confidence: ConfidenceLevel::High,
                evidence_references: vec![format!("swappiness_owner:{swappiness_owner:?}")],
            });
        } else if has_zram_device && current_swappiness < 80 {
            actions.push(ProposedAction {
                subsystem: Subsystem::Sysctl,
                action_type: ProposedActionType::Improve,
                target_parameter: "vm.swappiness".into(),
                current_state: format!("{current_swappiness}"),
                detected_ownership: swappiness_owner,
                required_capability: "kernel.sysctl".into(),
                proposed_setting: Some("100".into()),
                rationale: "When ZRAM is active, higher swappiness (100-150) allows the kernel to page out anonymous memory to RAM-compressed swap before evicting valuable filesystem cache.".into(),
                expected_benefit: "Keeps file cache resident, significantly improving application launch and responsiveness.".into(),
                potential_trade_offs: "Increased ZRAM memory consumption.".into(),
                required_privileges: PrivilegeLevel::RequiresRoot,
                reversibility: ReversibilityGuarantee::FullyReversible,
                confidence: ConfidenceLevel::Medium,
                evidence_references: vec![format!("current_swappiness:{current_swappiness}")],
            });
        } else if inv.storage.has_rotational_storage && !has_zram_device && current_swappiness > 20
        {
            actions.push(ProposedAction {
                subsystem: Subsystem::Sysctl,
                action_type: ProposedActionType::Improve,
                target_parameter: "vm.swappiness".into(),
                current_state: format!("{current_swappiness}"),
                detected_ownership: swappiness_owner,
                required_capability: "kernel.sysctl".into(),
                proposed_setting: Some("10".into()),
                rationale: "Mechanical HDD detected as swap storage without ZRAM. Lowering swappiness prevents severe disk thrashing during routine memory allocations.".into(),
                expected_benefit: "Eliminates prolonged desktop freezes caused by rotational swap latency.".into(),
                potential_trade_offs: "Reduces file cache retention under high anonymous memory pressure.".into(),
                required_privileges: PrivilegeLevel::RequiresRoot,
                reversibility: ReversibilityGuarantee::FullyReversible,
                confidence: ConfidenceLevel::High,
                evidence_references: vec!["storage:rotational_hdd".into()],
            });
        } else {
            actions.push(ProposedAction {
                subsystem: Subsystem::Sysctl,
                action_type: ProposedActionType::Preserve,
                target_parameter: "vm.swappiness".into(),
                current_state: format!("{current_swappiness}"),
                detected_ownership: swappiness_owner,
                required_capability: "kernel.sysctl".into(),
                proposed_setting: None,
                rationale: "Current swappiness value is aligned with storage topology.".into(),
                expected_benefit: "Maintains balanced memory reclaim.".into(),
                potential_trade_offs: "None.".into(),
                required_privileges: PrivilegeLevel::Unprivileged,
                reversibility: ReversibilityGuarantee::VolatileRuntime,
                confidence: ConfidenceLevel::Medium,
                evidence_references: vec![format!("current_swappiness:{current_swappiness}")],
            });
        }

        // 3. CPU Governor and Frequency Scaling Planning
        let has_power_conflict = inv.conflicts.iter().any(|c| c.rule_id == "CONF-002");
        if has_power_conflict {
            actions.push(ProposedAction {
                subsystem: Subsystem::Cpu,
                action_type: ProposedActionType::Conflict,
                target_parameter: "cpufreq.governor".into(),
                current_state: "Multiple competing power daemons running".into(),
                detected_ownership: ConfigurationOwnership::ThirdPartyManaged,
                required_capability: "kernel.cpufreq".into(),
                proposed_setting: None,
                rationale: "Conflicting power management daemons are actively controlling CPU scaling. HyperCore must not apply competing governor changes.".into(),
                expected_benefit: "Avoids frequency governor oscillation.".into(),
                potential_trade_offs: "None.".into(),
                required_privileges: PrivilegeLevel::Unprivileged,
                reversibility: ReversibilityGuarantee::VolatileRuntime,
                confidence: ConfidenceLevel::High,
                evidence_references: vec!["conflict:CONF-002".into()],
            });
        } else if let Some(driver) = &inv.cpu.scaling_driver {
            actions.push(ProposedAction {
                subsystem: Subsystem::Cpu,
                action_type: ProposedActionType::Preserve,
                target_parameter: "cpufreq.driver".into(),
                current_state: format!("Driver: {driver}, Governors: {}", inv.cpu.active_governors.join(", ")),
                detected_ownership: ConfigurationOwnership::KernelDefault,
                required_capability: "kernel.cpufreq".into(),
                proposed_setting: None,
                rationale: "Native CPU frequency scaling driver is active and operating under distribution defaults.".into(),
                expected_benefit: "Maintains hardware power-efficient performance scaling.".into(),
                potential_trade_offs: "None.".into(),
                required_privileges: PrivilegeLevel::Unprivileged,
                reversibility: ReversibilityGuarantee::VolatileRuntime,
                confidence: ConfidenceLevel::High,
                evidence_references: vec![format!("driver:{driver}")],
            });
        }

        // 4. Transparent Huge Pages (THP) Planning
        if let Some(thp) = &inv.memory.thp_enabled {
            if thp.contains("[always]")
                && inv.memory.total_bytes <= 4 * 1024 * 1024 * 1024
                && inv.memory.total_bytes > 0
            {
                actions.push(ProposedAction {
                    subsystem: Subsystem::Memory,
                    action_type: ProposedActionType::Improve,
                    target_parameter: "transparent_hugepage.enabled".into(),
                    current_state: "always".into(),
                    detected_ownership: ConfigurationOwnership::KernelDefault,
                    required_capability: "kernel.mm.thp".into(),
                    proposed_setting: Some("madvise".into()),
                    rationale: "THP 'always' on general-purpose <= 4 GiB systems can induce memory bloat and compaction stalls under mixed multitasking; 'madvise' allows opt-in for high-throughput processes without system-wide overhead.".into(),
                    expected_benefit: "Reduces memory fragmentation risk and eliminates unexpected compaction latency spikes.".into(),
                    potential_trade_offs: "Applications requiring automatic 2MB pages must opt-in via madvise; compute/database workloads benefiting from system-wide 2MB TLB mappings may see slight TLB miss increases.".into(),
                    required_privileges: PrivilegeLevel::RequiresRoot,
                    reversibility: ReversibilityGuarantee::FullyReversible,
                    confidence: ConfidenceLevel::Medium,
                    evidence_references: vec!["thp:always".into(), format!("ram_bytes:{}", inv.memory.total_bytes)],
                });
            } else {
                actions.push(ProposedAction {
                    subsystem: Subsystem::Memory,
                    action_type: ProposedActionType::Preserve,
                    target_parameter: "transparent_hugepage.enabled".into(),
                    current_state: thp.clone(),
                    detected_ownership: ConfigurationOwnership::KernelDefault,
                    required_capability: "kernel.mm.thp".into(),
                    proposed_setting: None,
                    rationale: "THP configuration is suitable for current memory capacity.".into(),
                    expected_benefit: "Stable memory allocation behavior.".into(),
                    potential_trade_offs: "None.".into(),
                    required_privileges: PrivilegeLevel::Unprivileged,
                    reversibility: ReversibilityGuarantee::VolatileRuntime,
                    confidence: ConfidenceLevel::Medium,
                    evidence_references: vec![format!("thp:{thp}")],
                });
            }
        }

        // 5. Storage I/O Scheduler Planning
        if inv.storage.has_nvme {
            let nvme_dev = inv
                .storage
                .block_devices
                .iter()
                .find(|d| d.media_type == StorageMediaType::NvmeSsd);
            if let Some(dev) = nvme_dev {
                let active_sched = dev.active_scheduler.as_deref().unwrap_or("none");
                if active_sched != "none" {
                    actions.push(ProposedAction {
                        subsystem: Subsystem::Storage,
                        action_type: ProposedActionType::Improve,
                        target_parameter: format!("{}.scheduler", dev.name),
                        current_state: active_sched.to_string(),
                        detected_ownership: ConfigurationOwnership::KernelDefault,
                        required_capability: "kernel.block.scheduler".into(),
                        proposed_setting: Some("none".into()),
                        rationale: "NVMe solid-state storage has hardware multi-queue dispatching; legacy software schedulers add unnecessary CPU locking overhead.".into(),
                        expected_benefit: "Reduces NVMe I/O CPU overhead and maximizes random IOPS.".into(),
                        potential_trade_offs: "Disables software deadline queuing.".into(),
                        required_privileges: PrivilegeLevel::RequiresRoot,
                        reversibility: ReversibilityGuarantee::FullyReversible,
                        confidence: ConfidenceLevel::High,
                        evidence_references: vec![format!("nvme_device:{}", dev.name)],
                    });
                } else {
                    actions.push(ProposedAction {
                        subsystem: Subsystem::Storage,
                        action_type: ProposedActionType::Preserve,
                        target_parameter: format!("{}.scheduler", dev.name),
                        current_state: "none".into(),
                        detected_ownership: ConfigurationOwnership::KernelDefault,
                        required_capability: "kernel.block.scheduler".into(),
                        proposed_setting: None,
                        rationale: "NVMe drive is already operating with optimal 'none' scheduler."
                            .into(),
                        expected_benefit: "Optimal multi-queue NVMe performance.".into(),
                        potential_trade_offs: "None.".into(),
                        required_privileges: PrivilegeLevel::Unprivileged,
                        reversibility: ReversibilityGuarantee::VolatileRuntime,
                        confidence: ConfidenceLevel::High,
                        evidence_references: vec![format!("nvme_device:{}", dev.name)],
                    });
                }
            }
        }

        // 6. Cgroup v2 Isolation Planning
        if inv.platform.cgroup_hierarchy == super::platform::CgroupHierarchy::V2Unified {
            if inv.platform.cgroup_delegation_available {
                actions.push(ProposedAction {
                    subsystem: Subsystem::Cgroup,
                    action_type: ProposedActionType::Preserve,
                    target_parameter: "cgroup_v2.delegation".into(),
                    current_state: "Unified cgroup v2 with delegation enabled".into(),
                    detected_ownership: ConfigurationOwnership::DistributionManaged,
                    required_capability: "cgroup.v2".into(),
                    proposed_setting: None,
                    rationale: "Host exposes cgroup v2 with controller delegation; unprivileged container-like isolation is supported.".into(),
                    expected_benefit: "Enables safe, non-root resource bounding for transactional leases.".into(),
                    potential_trade_offs: "None.".into(),
                    required_privileges: PrivilegeLevel::Unprivileged,
                    reversibility: ReversibilityGuarantee::FullyReversible,
                    confidence: ConfidenceLevel::High,
                    evidence_references: vec!["cgroup:v2_unified".into()],
                });
            } else {
                actions.push(ProposedAction {
                    subsystem: Subsystem::Cgroup,
                    action_type: ProposedActionType::RequiresAuthorization,
                    target_parameter: "cgroup_v2.subtree_control".into(),
                    current_state: "Unified cgroup v2 without user memory controller delegation".into(),
                    detected_ownership: ConfigurationOwnership::DistributionManaged,
                    required_capability: "cgroup.v2".into(),
                    proposed_setting: Some("+memory +cpu in user.slice".into()),
                    rationale: "Non-root cgroup memory delegation is missing; requires systemd or admin configuration to enable memory controller in user session slices.".into(),
                    expected_benefit: "Allows unprivileged HyperCore daemon to manage user workload memory limits safely.".into(),
                    potential_trade_offs: "Requires one-time administrator authorization.".into(),
                    required_privileges: PrivilegeLevel::RequiresRoot,
                    reversibility: ReversibilityGuarantee::ReversibleConfig,
                    confidence: ConfidenceLevel::High,
                    evidence_references: vec!["cgroup:missing_delegation".into()],
                });
            }
        } else {
            actions.push(ProposedAction {
                subsystem: Subsystem::Cgroup,
                action_type: ProposedActionType::Unsupported,
                target_parameter: "cgroup_v2".into(),
                current_state: format!("{:?}", inv.platform.cgroup_hierarchy),
                detected_ownership: ConfigurationOwnership::KernelDefault,
                required_capability: "cgroup.v2".into(),
                proposed_setting: None,
                rationale: "System is not running unified cgroup v2. Cgroup v1 or hybrid hierarchies cannot safely support transactional cgroup operations.".into(),
                expected_benefit: "Informs operator of limitation.".into(),
                potential_trade_offs: "Limits advanced cgroup resource bounding features.".into(),
                required_privileges: PrivilegeLevel::Unprivileged,
                reversibility: ReversibilityGuarantee::IrreversibleWithoutReboot,
                confidence: ConfidenceLevel::High,
                evidence_references: vec![format!("cgroup_hierarchy:{:?}", inv.platform.cgroup_hierarchy)],
            });
        }

        let preserve_count = actions
            .iter()
            .filter(|a| a.action_type == ProposedActionType::Preserve)
            .count();
        let config_count = actions
            .iter()
            .filter(|a| a.action_type == ProposedActionType::Configure)
            .count();
        let improve_count = actions
            .iter()
            .filter(|a| a.action_type == ProposedActionType::Improve)
            .count();
        let conflict_count = actions
            .iter()
            .filter(|a| a.action_type == ProposedActionType::Conflict)
            .count();
        let unsupported_count = actions
            .iter()
            .filter(|a| a.action_type == ProposedActionType::Unsupported)
            .count();

        Self {
            schema_version: PLAN_SCHEMA_VERSION,
            generated_at_unix_ms: inv.collected_at_unix_ms,
            actions,
            summary_preserve_count: preserve_count,
            summary_configure_count: config_count,
            summary_improve_count: improve_count,
            summary_conflict_count: conflict_count,
            summary_unsupported_count: unsupported_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_generation_empty() {
        let inv = SystemInventory::collect_from(
            std::path::Path::new("/dummy-sys"),
            std::path::Path::new("/dummy-proc"),
            std::path::Path::new("/dummy-etc"),
        );
        let plan = OptimizationPlan::generate(&inv);
        assert_eq!(plan.schema_version, 1);
        assert!(!plan.actions.is_empty());
    }
}
