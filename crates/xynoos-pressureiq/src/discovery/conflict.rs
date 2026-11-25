//! Non-mutating configuration conflict analysis engine.
#![forbid(unsafe_code)]

use super::{
    config::ConfigurationInventory,
    cpu::CpuInventory,
    memory::MemoryInventory,
    power::{PowerInventory, ServiceState},
    storage::StorageInventory,
    types::ConflictSeverity,
};
use serde::{Deserialize, Serialize};

/// Detailed finding describing a detected configuration or daemon conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictFinding {
    pub rule_id: String,
    pub title: String,
    pub severity: ConflictSeverity,
    pub primary_subsystem: String,
    pub conflicting_entities: Vec<String>,
    pub explanation: String,
    pub recommendation: String,
    pub evidence: Vec<String>,
}

/// Run comprehensive, read-only conflict analysis across all discovered subsystems.
pub fn analyze_conflicts(
    cpu: &CpuInventory,
    memory: &MemoryInventory,
    storage: &StorageInventory,
    power: &PowerInventory,
    config: &ConfigurationInventory,
) -> Vec<ConflictFinding> {
    let mut findings = Vec::new();

    // 1. Multiple Zram Managers
    if memory.zram_tuning_services.len() > 1 {
        findings.push(ConflictFinding {
            rule_id: "CONF-001".into(),
            title: "Multiple ZRAM Configuration Daemons Detected".into(),
            severity: ConflictSeverity::ConfirmedConflict,
            primary_subsystem: "memory_compression".into(),
            conflicting_entities: memory.zram_tuning_services.clone(),
            explanation: format!(
                "Multiple active or installed ZRAM managers ({}) can race during boot, causing unpredictable device sizing or priority collisions.",
                memory.zram_tuning_services.join(", ")
            ),
            recommendation: "Retain one authoritative ZRAM service (or delegate zram management to HyperCore) and disable redundant packages.".into(),
            evidence: memory.zram_tuning_services.iter().map(|s| format!("service:{s}")).collect(),
        });
    }

    // 2. Competing CPU Governor and Power Daemons
    let mut running_power_daemons = Vec::new();
    if power.power_daemons.tlp == ServiceState::Running {
        running_power_daemons.push("tlp");
    }
    if power.power_daemons.power_profiles_daemon == ServiceState::Running {
        running_power_daemons.push("power-profiles-daemon");
    }
    if power.power_daemons.tuned == ServiceState::Running {
        running_power_daemons.push("tuned");
    }
    if power.power_daemons.auto_cpufreq == ServiceState::Running {
        running_power_daemons.push("auto-cpufreq");
    }
    if power.power_daemons.laptop_mode_tools == ServiceState::Running {
        running_power_daemons.push("laptop-mode-tools");
    }

    if running_power_daemons.len() > 1 {
        findings.push(ConflictFinding {
            rule_id: "CONF-002".into(),
            title: "Competing CPU Frequency and Power Management Daemons Running".into(),
            severity: ConflictSeverity::ConfirmedConflict,
            primary_subsystem: "power_and_cpu".into(),
            conflicting_entities: running_power_daemons.iter().map(|s| s.to_string()).collect(),
            explanation: format!(
                "Multiple power management daemons ({}) are actively running simultaneously. This causes governor oscillation, conflicting energy-performance policies, and unpredictable battery drain.",
                running_power_daemons.join(", ")
            ),
            recommendation: "Disable or mask all but one power management daemon to prevent conflicting sysfs writes.".into(),
            evidence: running_power_daemons.iter().map(|s| format!("running_process:{s}")).collect(),
        });
    }

    // 3. Multiple Userspace OOM Managers (Distinguish actively running from dormant packages)
    let mut running_oom_daemons = Vec::new();
    if memory.oom_managers.earlyoom_active {
        running_oom_daemons.push("earlyoom");
    }
    if memory.oom_managers.systemd_oomd_active {
        running_oom_daemons.push("systemd-oomd");
    }
    if memory.oom_managers.nohang_active {
        running_oom_daemons.push("nohang");
    }
    if memory.oom_managers.oomd_active {
        running_oom_daemons.push("oomd");
    }

    if running_oom_daemons.len() > 1 {
        findings.push(ConflictFinding {
            rule_id: "CONF-003".into(),
            title: "Multiple Active Userspace OOM Killer Daemons Detected".into(),
            severity: ConflictSeverity::ConfirmedConflict,
            primary_subsystem: "oom_management".into(),
            conflicting_entities: running_oom_daemons.iter().map(|s| s.to_string()).collect(),
            explanation: format!(
                "Multiple out-of-memory daemons ({}) are actively running simultaneously. When memory pressure spikes, both may kill processes simultaneously, leading to unnecessary application loss.",
                running_oom_daemons.join(", ")
            ),
            recommendation: "Standardize on a single active OOM daemon to avoid double-killing workloads.".into(),
            evidence: running_oom_daemons.iter().map(|s| format!("active_oom_process:{s}")).collect(),
        });
    } else {
        let mut installed_oom = Vec::new();
        if memory.oom_managers.earlyoom_present {
            installed_oom.push("earlyoom");
        }
        if memory.oom_managers.systemd_oomd_present {
            installed_oom.push("systemd-oomd");
        }
        if memory.oom_managers.nohang_present {
            installed_oom.push("nohang");
        }
        if memory.oom_managers.oomd_present {
            installed_oom.push("oomd");
        }
        if installed_oom.len() > 1 && running_oom_daemons.len() <= 1 {
            findings.push(ConflictFinding {
                rule_id: "CONF-003-ADV".into(),
                title: "Multiple OOM Management Packages Installed (Dormant)".into(),
                severity: ConflictSeverity::CompatibleCoexistence,
                primary_subsystem: "oom_management".into(),
                conflicting_entities: installed_oom.iter().map(|s| s.to_string()).collect(),
                explanation: format!(
                    "Multiple OOM packages ({}) are installed, but only {} are actively running. Inactive packages do not conflict with running workloads.",
                    installed_oom.join(", "),
                    running_oom_daemons.len()
                ),
                recommendation: "Retain current active manager; no remediation required unless inactive services are started.".into(),
                evidence: installed_oom.iter().map(|s| format!("installed_package:{s}")).collect(),
            });
        }
    }

    // 4. Zswap and Zram Coexistence Analysis (Context-aware swap tiering)
    let zram_active = !memory.zram_devices.is_empty();
    let zswap_active = memory.zswap.available && memory.zswap.enabled;
    let has_disk_swap = memory
        .swap_devices
        .iter()
        .any(|s| !s.is_zram && s.total_bytes > 0);

    if zram_active && zswap_active {
        let zram_algo = memory
            .zram_devices
            .first()
            .and_then(|z| z.active_algorithm.as_deref())
            .unwrap_or("unknown");
        let zswap_algo = memory.zswap.compressor.as_deref().unwrap_or("unknown");

        if has_disk_swap {
            findings.push(ConflictFinding {
                rule_id: "CONF-004".into(),
                title: "Tiered ZRAM and ZSWAP Coordination Detected".into(),
                severity: ConflictSeverity::CompatibleCoexistence,
                primary_subsystem: "memory_compression".into(),
                conflicting_entities: vec![
                    format!("zram(algo={zram_algo})"),
                    format!("zswap(algo={zswap_algo})"),
                    "disk_backed_swap".into(),
                ],
                explanation: format!(
                    "System features active ZRAM swap alongside disk-backed swap and kernel ZSWAP. ZSWAP accelerates cold page evictions to disk storage while ZRAM absorbs transient memory pressure.",
                    zswap_algo = zswap_algo,
                    zram_algo = zram_algo,
                ),
                recommendation: "Preserve tiered hierarchy; ensure ZRAM swap priority exceeds disk swap priority.".into(),
                evidence: vec![
                    format!("zram_devices_count:{}", memory.zram_devices.len()),
                    format!("zswap_enabled:{}", memory.zswap.enabled),
                    "disk_swap_present:true".into(),
                ],
            });
        } else {
            findings.push(ConflictFinding {
                rule_id: "CONF-004".into(),
                title: "Redundant ZSWAP with Exclusive ZRAM Swap".into(),
                severity: ConflictSeverity::PotentialConflict,
                primary_subsystem: "memory_compression".into(),
                conflicting_entities: vec![
                    format!("zram(algo={zram_algo})"),
                    format!("zswap(algo={zswap_algo})"),
                ],
                explanation: format!(
                    "Both kernel ZSWAP and ZRAM are active without a disk swap partition. ZSWAP may compress pages before handing them to ZRAM, adding redundant CPU compression overhead.",
                    zswap_algo = zswap_algo,
                    zram_algo = zram_algo,
                ),
                recommendation: "When ZRAM is the sole swap device, consider disabling zswap (`echo 0 > /sys/module/zswap/parameters/enabled`) to eliminate double compression overhead.".into(),
                evidence: vec![
                    format!("zram_devices_count:{}", memory.zram_devices.len()),
                    format!("zswap_enabled:{}", memory.zswap.enabled),
                    "disk_swap_present:false".into(),
                ],
            });
        }
    }

    // 5. Runtime vs Persistent Sysctl Discrepancies
    for (key, item) in &config.sysctl_items {
        if item.has_discrepancy {
            findings.push(ConflictFinding {
                rule_id: "CONF-005".into(),
                title: format!("Sysctl Value Discrepancy for {}", key),
                severity: ConflictSeverity::PotentialConflict,
                primary_subsystem: "sysctl".into(),
                conflicting_entities: vec![
                    format!("runtime:{}", item.runtime_value.as_deref().unwrap_or("none")),
                    format!("persistent:{}", item.persistent_value.as_deref().unwrap_or("none")),
                ],
                explanation: format!(
                    "Parameter {} runtime value ({}) does not match declaration in {} ({}). A service or manual write mutated the kernel without persisting it.",
                    key,
                    item.runtime_value.as_deref().unwrap_or("none"),
                    item.source_file.as_deref().unwrap_or("unknown file"),
                    item.persistent_value.as_deref().unwrap_or("none"),
                ),
                recommendation: "Align persistent sysctl files with intended runtime state to prevent configuration loss upon reboot.".into(),
                evidence: vec![
                    format!("key:{key}"),
                    format!("runtime:{}", item.runtime_value.as_deref().unwrap_or("")),
                    format!("source:{}", item.source_file.as_deref().unwrap_or("")),
                ],
            });
        }
    }

    // 6. Rotational Storage with aggressive swappiness without zram
    if storage.has_rotational_storage
        && !zram_active
        && memory.memory_sysctl.swappiness.unwrap_or(60) > 60
    {
        findings.push(ConflictFinding {
            rule_id: "CONF-006".into(),
            title: "Elevated Swappiness on Rotational HDD Storage Without ZRAM".into(),
            severity: ConflictSeverity::PotentialConflict,
            primary_subsystem: "storage_and_swap".into(),
            conflicting_entities: vec![
                "rotational_hdd".into(),
                format!("vm.swappiness={}", memory.memory_sysctl.swappiness.unwrap_or(60)),
            ],
            explanation: "High swappiness on mechanical disk storage induces severe random I/O latency spikes during anonymous memory paging.".into(),
            recommendation: "Configure ZRAM compressed swap or lower vm.swappiness to 10-20 to reduce disk head thrashing.".into(),
            evidence: vec![
                "storage:rotational_hdd".into(),
                format!("swappiness:{}", memory.memory_sysctl.swappiness.unwrap_or(60)),
            ],
        });
    }

    // 7. Heterogeneous CPU without autogrouping or scheduler awareness
    if cpu.is_heterogeneous && cpu.policies.len() > 1 {
        findings.push(ConflictFinding {
            rule_id: "CONF-007".into(),
            title: "Heterogeneous CPU Topology Detected".into(),
            severity: ConflictSeverity::CompatibleCoexistence,
            primary_subsystem: "cpu_scheduling".into(),
            conflicting_entities: cpu.policies.iter().map(|p| format!("policy{}", p.policy_id)).collect(),
            explanation: "System features heterogeneous CPU cores with differing maximum frequencies or capacities (e.g. big.LITTLE or performance/efficient cores).".into(),
            recommendation: "Ensure Energy Performance Preference (EPP) and CFS autogroup are tuned appropriately for asymmetric core allocation.".into(),
            evidence: cpu.policies.iter().map(|p| format!("policy{}:max_freq={:?}", p.policy_id, p.max_freq_khz)).collect(),
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_empty() {
        let cpu = CpuInventory::default();
        let mem = MemoryInventory::default();
        let sto = StorageInventory::default();
        let pwr = PowerInventory::default();
        let cfg = ConfigurationInventory::default();

        let findings = analyze_conflicts(&cpu, &mem, &sto, &pwr, &cfg);
        assert!(findings.is_empty());
    }
}
