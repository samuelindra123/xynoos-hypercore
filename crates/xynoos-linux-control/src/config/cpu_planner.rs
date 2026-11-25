//! Hardware-aware universal CPU optimization planning engine.
#![forbid(unsafe_code)]

use super::action::{
    ActionClassification, AuthorizationScope, ConfigSubsystem, ConfigurationAction,
    PersistenceRequirement, ReversalProcedure, TypedMutation, VerificationProcedure,
    CONFIG_ACTION_SCHEMA_VERSION,
};
use xynoos_pressureiq::discovery::{
    inventory::SystemInventory,
    types::{ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee},
};

/// Hardware profile categorization for CPU tuning heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuProfile {
    IntelLowPower,   // e.g. Intel Pentium Silver N5000, Celeron, Atom
    IntelMainstream, // Core i3/i5/i7/i9 desktop & mobile
    AmdModern,       // Zen 2/3/4/5
    Arm64Platform,   // Generic ARM64
    VirtualMachine,  // KVM, QEMU, VMware, Xen
    UnknownPlatform,
}

/// Plan generator for universal CPU optimizations.
pub struct CpuPlanner;

impl CpuPlanner {
    /// Categorize CPU into a capability and architecture profile.
    pub fn determine_profile(inv: &SystemInventory) -> CpuProfile {
        if inv.platform.virtualization
            != xynoos_pressureiq::discovery::platform::VirtualizationEnvironment::Physical
        {
            return CpuProfile::VirtualMachine;
        }

        let model = &inv.cpu.model_name;
        let vendor = &inv.cpu.vendor_id;

        if model.contains("N5000")
            || model.contains("Celeron")
            || model.contains("Pentium Silver")
            || model.contains("Atom")
        {
            CpuProfile::IntelLowPower
        } else if vendor.contains("Intel") || model.contains("Core") {
            CpuProfile::IntelMainstream
        } else if vendor.contains("AMD") || model.contains("Ryzen") || model.contains("EPYC") {
            CpuProfile::AmdModern
        } else if inv.platform.architecture == "aarch64" || inv.platform.architecture == "arm64" {
            CpuProfile::Arm64Platform
        } else {
            CpuProfile::UnknownPlatform
        }
    }

    /// Select optimal CPU governor for the discovered CPU profile and scaling driver.
    pub fn select_governor(profile: CpuProfile, scaling_driver: Option<&str>) -> &'static str {
        match scaling_driver {
            Some("intel_pstate") => {
                // intel_pstate in active mode operates its own internal governor with "powersave"
                "powersave"
            }
            Some("amd-pstate") | Some("amd-pstate-epp") => "powersave",
            _ => match profile {
                CpuProfile::IntelLowPower => "schedutil",
                CpuProfile::IntelMainstream => "schedutil",
                CpuProfile::AmdModern => "schedutil",
                CpuProfile::Arm64Platform => "schedutil",
                CpuProfile::VirtualMachine => "schedutil",
                CpuProfile::UnknownPlatform => "schedutil",
            },
        }
    }

    /// Select optimal Energy Performance Preference (EPP) if driver supports it.
    pub fn select_epp(profile: CpuProfile, is_battery: bool) -> &'static str {
        if is_battery {
            "power"
        } else {
            match profile {
                CpuProfile::IntelLowPower => "balance_performance",
                CpuProfile::IntelMainstream => "balance_performance",
                CpuProfile::AmdModern => "balance_performance",
                _ => "default",
            }
        }
    }

    /// Plan CPU optimizations based on hardware discovery and existing configuration.
    pub fn plan(inv: &SystemInventory) -> Vec<ConfigurationAction> {
        let mut actions = Vec::new();
        let profile = Self::determine_profile(inv);
        let scaling_driver = inv.cpu.scaling_driver.as_deref();
        let is_battery = inv.power.ac_online == Some(false);

        // 1. CPU Scaling Governor
        let target_gov = Self::select_governor(profile, scaling_driver);
        let current_gov = inv
            .cpu
            .active_governors
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".into());

        // Check if governor already matches desired target
        let gov_matches = current_gov == target_gov;
        let is_admin_managed = inv.cpu.active_governors.len() == 1
            && !gov_matches
            && current_gov != "ondemand"
            && current_gov != "powersave"
            && current_gov != "schedutil";

        let (classification, desired_val, mutation) = if gov_matches || is_admin_managed {
            (ActionClassification::Preserve, current_gov.clone(), None)
        } else {
            (
                ActionClassification::Improve,
                target_gov.to_string(),
                Some(TypedMutation::ApplyCpuGovernor {
                    cpu_range: "all".into(),
                    governor: target_gov.to_string(),
                    original_governor: Some(current_gov.clone()),
                }),
            )
        };

        actions.push(ConfigurationAction {
            schema_version: CONFIG_ACTION_SCHEMA_VERSION,
            action_id: "CPU-GOV-001".into(),
            target_subsystem: ConfigSubsystem::Cpu,
            target_setting: "cpufreq.scaling_governor".into(),
            current_observed_value: current_gov.clone(),
            desired_value: desired_val,
            current_ownership: if is_admin_managed {
                ConfigurationOwnership::AdministratorManaged
            } else if gov_matches {
                ConfigurationOwnership::DistributionManaged
            } else {
                ConfigurationOwnership::KernelDefault
            },
            required_privileges: PrivilegeLevel::RequiresRoot,
            required_capability: "cpu.frequency_governor".into(),
            expected_benefit: format!(
                "Ensure balanced frequency scaling using '{target_gov}' without locking cores to maximum clock"
            ),
            potential_side_effects: "Minimal; transitions between frequencies adapt dynamically to workload demand".into(),
            confidence: ConfidenceLevel::High,
            reversibility: ReversibilityGuarantee::FullyReversible,
            classification,
            authorization_scope: AuthorizationScope::CpuOptimization,
            preconditions: vec!["sysfs cpufreq scaling_governor writable".into()],
            dependencies: vec![],
            conflict_status: None,
            verification_procedure: VerificationProcedure::CpuGovernorReadback {
                cpu_range: "all".into(),
                expected_governor: target_gov.to_string(),
            },
            reversal_procedure: ReversalProcedure::RestoreCpuGovernor {
                cpu_range: "all".into(),
                original_governor: current_gov,
            },
            persistence_requirements: PersistenceRequirement::RuntimeOnly,
            mutation,
        });

        // 2. CPU Energy Performance Preference (EPP) if supported
        let has_epp = inv
            .cpu
            .policies
            .iter()
            .any(|p| p.energy_perf_preference.is_some() || !p.available_epp.is_empty());

        if has_epp {
            let target_epp = Self::select_epp(profile, is_battery);
            let current_epp = inv
                .cpu
                .policies
                .first()
                .and_then(|p| p.energy_perf_preference.clone())
                .unwrap_or_else(|| "default".into());

            let epp_matches = current_epp == target_epp;
            let (epp_class, desired_epp, epp_mut) = if epp_matches {
                (ActionClassification::Preserve, current_epp.clone(), None)
            } else {
                (
                    ActionClassification::Improve,
                    target_epp.to_string(),
                    Some(TypedMutation::ApplyCpuEpp {
                        cpu_range: "all".into(),
                        epp: target_epp.to_string(),
                        original_epp: Some(current_epp.clone()),
                    }),
                )
            };

            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "CPU-EPP-001".into(),
                target_subsystem: ConfigSubsystem::Cpu,
                target_setting: "cpufreq.energy_performance_preference".into(),
                current_observed_value: current_epp.clone(),
                desired_value: desired_epp,
                current_ownership: ConfigurationOwnership::KernelDefault,
                required_privileges: PrivilegeLevel::RequiresRoot,
                required_capability: "cpu.energy_performance_preference".into(),
                expected_benefit: format!(
                    "Align energy-performance preference to '{target_epp}' based on hardware profile"
                ),
                potential_side_effects: "Changes latency to peak frequency ramp-up".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: epp_class,
                authorization_scope: AuthorizationScope::CpuOptimization,
                preconditions: vec!["sysfs cpufreq energy_performance_preference writable".into()],
                dependencies: vec!["CPU-GOV-001".into()],
                conflict_status: None,
                verification_procedure: VerificationProcedure::CpuEppReadback {
                    cpu_range: "all".into(),
                    expected_epp: target_epp.to_string(),
                },
                reversal_procedure: ReversalProcedure::RestoreCpuEpp {
                    cpu_range: "all".into(),
                    original_epp: current_epp,
                },
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation: epp_mut,
            });
        }

        actions
    }
}
