//! GPU capability intelligence and safe resource planning engine.
#![forbid(unsafe_code)]

use super::action::{
    ActionClassification, AuthorizationScope, ConfigSubsystem, ConfigurationAction,
    PersistenceRequirement, ReversalProcedure, VerificationProcedure, CONFIG_ACTION_SCHEMA_VERSION,
};
use xynoos_pressureiq::discovery::{
    inventory::SystemInventory,
    types::{ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee},
};

/// Discovered GPU architecture profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuProfile {
    IntelIntegratedUhd, // e.g. Intel UHD Graphics 605 (Gemini Lake)
    IntelIntegratedGen, // Other Intel HD/Iris/Xe integrated
    IntelDiscreteXe,    // Intel Arc discrete
    AmdIntegratedRadeon,
    AmdDiscreteRadeon,
    NvidiaDiscrete,
    NouveauOpenSource,
    GenericDrm,
    NoGpuDetected,
}

/// Plan generator for GPU-aware resource intelligence.
pub struct GpuPlanner;

impl GpuPlanner {
    /// Classify GPU architecture and driver profile.
    pub fn determine_profile(inv: &SystemInventory) -> GpuProfile {
        if inv.gpu.devices.is_empty() {
            return GpuProfile::NoGpuDetected;
        }

        let first = &inv.gpu.devices[0];
        let card = &first.card_name;
        let driver = first.driver.as_deref().unwrap_or("unknown");
        let is_discrete = first.gpu_type == xynoos_pressureiq::discovery::gpu::GpuType::Discrete;

        if card.contains("UHD Graphics 605")
            || card.contains("Gemini Lake")
            || first.vendor == xynoos_pressureiq::discovery::gpu::GpuVendor::Intel && !is_discrete
        {
            GpuProfile::IntelIntegratedUhd
        } else if driver == "i915" || driver == "xe" {
            if is_discrete {
                GpuProfile::IntelDiscreteXe
            } else {
                GpuProfile::IntelIntegratedGen
            }
        } else if driver == "amdgpu" {
            if is_discrete {
                GpuProfile::AmdDiscreteRadeon
            } else {
                GpuProfile::AmdIntegratedRadeon
            }
        } else if driver.contains("nvidia") {
            GpuProfile::NvidiaDiscrete
        } else if driver == "nouveau" {
            GpuProfile::NouveauOpenSource
        } else {
            GpuProfile::GenericDrm
        }
    }

    /// Plan GPU-aware resource observations and safe configuration actions.
    ///
    /// Per HyperCore safety principles, no invasive hardware register writes,
    /// clock adjustments, firmware replacements, or unsupported power overrides are performed.
    pub fn plan(inv: &SystemInventory) -> Vec<ConfigurationAction> {
        let mut actions = Vec::new();
        let profile = Self::determine_profile(inv);

        if profile == GpuProfile::NoGpuDetected {
            return actions;
        }

        let dev = &inv.gpu.devices[0];
        let is_discrete = dev.gpu_type == xynoos_pressureiq::discovery::gpu::GpuType::Discrete;
        let observed_summary = format!(
            "Device: {}, Driver: {}, Memory: {}",
            dev.card_name,
            dev.driver.as_deref().unwrap_or("unknown"),
            if is_discrete {
                "Dedicated VRAM"
            } else {
                "Shared System RAM"
            }
        );

        let benefit = match profile {
            GpuProfile::IntelIntegratedUhd => {
                "Track shared system memory contention between CPU and Intel UHD 605; preserve driver power management".into()
            }
            GpuProfile::IntelIntegratedGen | GpuProfile::AmdIntegratedRadeon => {
                "Preserve integrated GPU driver runtime power management and observe shared memory footprint".into()
            }
            GpuProfile::IntelDiscreteXe | GpuProfile::AmdDiscreteRadeon | GpuProfile::NvidiaDiscrete => {
                "Preserve discrete GPU driver power management; observe PCI bus power states without invasive register writes".into()
            }
            _ => "Observe standard DRM device telemetry without modifying hardware state".into(),
        };

        actions.push(ConfigurationAction {
            schema_version: CONFIG_ACTION_SCHEMA_VERSION,
            action_id: "GPU-RESOURCE-001".into(),
            target_subsystem: ConfigSubsystem::Power,
            target_setting: "gpu.power_and_memory_state".into(),
            current_observed_value: observed_summary.clone(),
            desired_value: observed_summary,
            current_ownership: ConfigurationOwnership::DistributionManaged,
            required_privileges: PrivilegeLevel::Unprivileged,
            required_capability: "gpu.telemetry_observe".into(),
            expected_benefit: benefit,
            potential_side_effects: "None; strictly observe-only and non-mutating".into(),
            confidence: ConfidenceLevel::High,
            reversibility: ReversibilityGuarantee::FullyReversible,
            classification: ActionClassification::Preserve,
            authorization_scope: AuthorizationScope::PowerOptimization,
            preconditions: vec!["DRM subsystem accessible".into()],
            dependencies: vec![],
            conflict_status: None,
            verification_procedure: VerificationProcedure::NoOpPreserved,
            reversal_procedure: ReversalProcedure::NoOpPreserved,
            persistence_requirements: PersistenceRequirement::RuntimeOnly,
            mutation: None,
        });

        actions
    }
}
