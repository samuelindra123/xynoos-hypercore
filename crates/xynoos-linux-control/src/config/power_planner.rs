//! Power management, thermal state, and external daemon coexistence planning engine.
#![forbid(unsafe_code)]

use super::action::{
    ActionClassification, AuthorizationScope, ConfigSubsystem, ConfigurationAction,
    PersistenceRequirement, ReversalProcedure, TypedMutation, VerificationProcedure,
    CONFIG_ACTION_SCHEMA_VERSION,
};
use xynoos_pressureiq::discovery::{
    inventory::SystemInventory,
    power::ServiceState,
    types::{ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee},
};

/// Plan generator for power and thermal management.
pub struct PowerPlanner;

impl PowerPlanner {
    /// Check whether an external power or thermal daemon is active.
    pub fn detect_competing_daemon(inv: &SystemInventory) -> Option<(&'static str, &'static str)> {
        let daemons = &inv.power.power_daemons;
        if daemons.power_profiles_daemon == ServiceState::Running {
            Some(("power-profiles-daemon", "running daemon"))
        } else if daemons.tlp == ServiceState::Running {
            Some(("tlp", "running daemon"))
        } else if daemons.tuned == ServiceState::Running {
            Some(("tuned", "running daemon"))
        } else if daemons.auto_cpufreq == ServiceState::Running {
            Some(("auto-cpufreq", "running daemon"))
        } else if daemons.thermald == ServiceState::Running {
            Some(("thermald", "running daemon"))
        } else {
            None
        }
    }

    /// Plan power and thermal configuration actions.
    pub fn plan(inv: &SystemInventory) -> Vec<ConfigurationAction> {
        let mut actions = Vec::new();
        let competing = Self::detect_competing_daemon(inv);
        let is_throttled = inv.cpu.core_throttle_events > 0 || inv.cpu.package_throttle_events > 0;
        let is_battery = inv.power.ac_online == Some(false);

        if let Some((daemon_name, status)) = competing {
            // Respect external power management ownership (TLP, power-profiles-daemon, tuned, etc.)
            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "POWER-COEXIST-001".into(),
                target_subsystem: ConfigSubsystem::Power,
                target_setting: "power.platform_profile_ownership".into(),
                current_observed_value: format!("{daemon_name} ({status})"),
                desired_value: format!("Preserve {daemon_name} ownership"),
                current_ownership: ConfigurationOwnership::ThirdPartyManaged,
                required_privileges: PrivilegeLevel::Unprivileged,
                required_capability: "power.service_coexistence".into(),
                expected_benefit: format!(
                    "Prevent conflicting profile mutations with actively running '{daemon_name}'"
                ),
                potential_side_effects:
                    "None; HyperCore defers power profile ownership to external daemon".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification: ActionClassification::Preserve,
                authorization_scope: AuthorizationScope::PowerOptimization,
                preconditions: vec![],
                dependencies: vec![],
                conflict_status: Some(format!(
                    "External power manager '{daemon_name}' is {status}"
                )),
                verification_procedure: VerificationProcedure::NoOpPreserved,
                reversal_procedure: ReversalProcedure::NoOpPreserved,
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation: None,
            });
            return actions;
        }

        // If platform profile is supported and unmanaged
        if let Some(active_profile) = &inv.power.platform_power_profile {
            let target_profile = if is_battery {
                "power-saver"
            } else {
                "balanced"
            };

            let profile_matches = active_profile == target_profile;
            let (classification, desired_val, mutation) = if profile_matches {
                (ActionClassification::Preserve, active_profile.clone(), None)
            } else if is_throttled {
                (ActionClassification::Defer, active_profile.clone(), None)
            } else {
                (
                    ActionClassification::Improve,
                    target_profile.to_string(),
                    Some(TypedMutation::ApplyPowerProfile {
                        profile: target_profile.to_string(),
                        original_profile: Some(active_profile.clone()),
                    }),
                )
            };

            actions.push(ConfigurationAction {
                schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                action_id: "POWER-PROFILE-001".into(),
                target_subsystem: ConfigSubsystem::Power,
                target_setting: "power.platform_profile".into(),
                current_observed_value: active_profile.clone(),
                desired_value: desired_val,
                current_ownership: ConfigurationOwnership::KernelDefault,
                required_privileges: PrivilegeLevel::RequiresRoot,
                required_capability: "power.platform_profile".into(),
                expected_benefit: format!(
                    "Set conservative platform profile to '{target_profile}' without disabling hardware thermal trip points"
                ),
                potential_side_effects: "Slight adjustment to thermal and fan budget".into(),
                confidence: ConfidenceLevel::High,
                reversibility: ReversibilityGuarantee::FullyReversible,
                classification,
                authorization_scope: AuthorizationScope::PowerOptimization,
                preconditions: vec!["/sys/firmware/acpi/platform_profile writable".into()],
                dependencies: vec![],
                conflict_status: None,
                verification_procedure: VerificationProcedure::PowerProfileReadback {
                    expected_profile: target_profile.to_string(),
                },
                reversal_procedure: ReversalProcedure::RestorePowerProfile {
                    original_profile: active_profile.clone(),
                },
                persistence_requirements: PersistenceRequirement::RuntimeOnly,
                mutation,
            });
        }

        actions
    }
}
