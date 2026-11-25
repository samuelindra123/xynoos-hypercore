//! Storage topology and block I/O scheduler planning engine.
#![forbid(unsafe_code)]

use super::action::{
    ActionClassification, AuthorizationScope, ConfigSubsystem, ConfigurationAction,
    PersistenceRequirement, ReversalProcedure, TypedMutation, VerificationProcedure,
    CONFIG_ACTION_SCHEMA_VERSION,
};
use xynoos_pressureiq::discovery::{
    inventory::SystemInventory,
    storage::StorageMediaType,
    types::{ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee},
};

/// Plan generator for block I/O scheduler and read-ahead optimizations.
pub struct StoragePlanner;

impl StoragePlanner {
    /// Select optimal block I/O scheduler for physical media type.
    pub fn select_scheduler(
        media_type: StorageMediaType,
        available: &[String],
    ) -> Option<&'static str> {
        let candidate = match media_type {
            StorageMediaType::NvmeSsd => {
                if available.iter().any(|s| s == "none") {
                    "none"
                } else if available.iter().any(|s| s == "mq-deadline") {
                    "mq-deadline"
                } else {
                    return None;
                }
            }
            StorageMediaType::EmmcFlash => {
                if available.iter().any(|s| s == "mq-deadline") {
                    "mq-deadline"
                } else if available.iter().any(|s| s == "bfq") {
                    "bfq"
                } else {
                    return None;
                }
            }
            StorageMediaType::SataSsd => {
                if available.iter().any(|s| s == "mq-deadline") {
                    "mq-deadline"
                } else if available.iter().any(|s| s == "bfq") {
                    "bfq"
                } else {
                    return None;
                }
            }
            StorageMediaType::RotationalHdd => {
                if available.iter().any(|s| s == "bfq") {
                    "bfq"
                } else if available.iter().any(|s| s == "mq-deadline") {
                    "mq-deadline"
                } else {
                    return None;
                }
            }
            StorageMediaType::VirtualDisk => {
                if available.iter().any(|s| s == "none") {
                    "none"
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        Some(candidate)
    }

    /// Plan storage I/O scheduler and read-ahead optimizations.
    pub fn plan(inv: &SystemInventory) -> Vec<ConfigurationAction> {
        let mut actions = Vec::new();

        for dev in &inv.storage.block_devices {
            // Skip volatile zram block devices, loop devices, or removable drives
            if dev.media_type == StorageMediaType::ZramDisk
                || dev.name.starts_with("loop")
                || dev.is_removable
            {
                continue;
            }

            let active_sched = dev
                .active_scheduler
                .as_deref()
                .unwrap_or("unknown")
                .to_string();

            if let Some(target_sched) =
                Self::select_scheduler(dev.media_type, &dev.available_schedulers)
            {
                let sched_matches = active_sched == target_sched;
                let is_custom_tuned = !sched_matches
                    && active_sched != "none"
                    && (active_sched == "bfq" || active_sched == "kyber");

                let (classification, desired_val, mutation) = if sched_matches || is_custom_tuned {
                    (ActionClassification::Preserve, active_sched.clone(), None)
                } else {
                    (
                        ActionClassification::Improve,
                        target_sched.to_string(),
                        Some(TypedMutation::ApplyBlockIoScheduler {
                            device_name: dev.name.clone(),
                            scheduler: target_sched.to_string(),
                            original_scheduler: Some(active_sched.clone()),
                        }),
                    )
                };

                actions.push(ConfigurationAction {
                    schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                    action_id: format!("STORAGE-SCHED-{}", dev.name.to_uppercase()),
                    target_subsystem: ConfigSubsystem::Storage,
                    target_setting: format!("block.{}.scheduler", dev.name),
                    current_observed_value: active_sched.clone(),
                    desired_value: desired_val,
                    current_ownership: if is_custom_tuned {
                        ConfigurationOwnership::AdministratorManaged
                    } else if sched_matches {
                        ConfigurationOwnership::KernelDefault
                    } else {
                        ConfigurationOwnership::KernelDefault
                    },
                    required_privileges: PrivilegeLevel::RequiresRoot,
                    required_capability: "block.io_scheduler".into(),
                    expected_benefit: format!(
                        "Configure '{}' scheduler on {} ({:?}) to optimize queue latency and throughput",
                        target_sched, dev.name, dev.media_type
                    ),
                    potential_side_effects: "Changes I/O queue dispatch ordering; fully reversible".into(),
                    confidence: ConfidenceLevel::High,
                    reversibility: ReversibilityGuarantee::FullyReversible,
                    classification,
                    authorization_scope: AuthorizationScope::StorageOptimization,
                    preconditions: vec![format!("sysfs block/{}/queue/scheduler writable", dev.name)],
                    dependencies: vec![],
                    conflict_status: None,
                    verification_procedure: VerificationProcedure::BlockIoSchedulerReadback {
                        device_name: dev.name.clone(),
                        expected_scheduler: target_sched.to_string(),
                    },
                    reversal_procedure: ReversalProcedure::RestoreBlockIoScheduler {
                        device_name: dev.name.clone(),
                        original_scheduler: active_sched,
                    },
                    persistence_requirements: PersistenceRequirement::RuntimeOnly,
                    mutation,
                });
            }

            // Read-ahead check for eMMC or SATA SSD (e.g. 128 KiB)
            if matches!(
                dev.media_type,
                StorageMediaType::EmmcFlash | StorageMediaType::SataSsd
            ) {
                if let Some(current_ra) = dev.read_ahead_kb {
                    let (ra_class, desired_ra, ra_mut) =
                        (ActionClassification::Preserve, current_ra.to_string(), None);

                    actions.push(ConfigurationAction {
                        schema_version: CONFIG_ACTION_SCHEMA_VERSION,
                        action_id: format!("STORAGE-RA-{}", dev.name.to_uppercase()),
                        target_subsystem: ConfigSubsystem::Storage,
                        target_setting: format!("block.{}.read_ahead_kb", dev.name),
                        current_observed_value: current_ra.to_string(),
                        desired_value: desired_ra,
                        current_ownership: ConfigurationOwnership::KernelDefault,
                        required_privileges: PrivilegeLevel::RequiresRoot,
                        required_capability: "block.read_ahead".into(),
                        expected_benefit: format!("Maintain balanced read-ahead on {}", dev.name),
                        potential_side_effects: "None; preserved".into(),
                        confidence: ConfidenceLevel::High,
                        reversibility: ReversibilityGuarantee::FullyReversible,
                        classification: ra_class,
                        authorization_scope: AuthorizationScope::StorageOptimization,
                        preconditions: vec![],
                        dependencies: vec![],
                        conflict_status: None,
                        verification_procedure: VerificationProcedure::NoOpPreserved,
                        reversal_procedure: ReversalProcedure::NoOpPreserved,
                        persistence_requirements: PersistenceRequirement::RuntimeOnly,
                        mutation: ra_mut,
                    });
                }
            }
        }

        actions
    }
}
