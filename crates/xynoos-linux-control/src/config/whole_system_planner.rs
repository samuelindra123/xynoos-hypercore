//! Unified whole-system adaptive optimization planner coordinating Memory, CPU, GPU, Storage, and Power.
#![forbid(unsafe_code)]

use super::{
    action::{ActionClassification, AuthorizationScope, ConfigurationAction},
    cpu_planner::CpuPlanner,
    gpu_planner::GpuPlanner,
    memory_planner::MemoryPlanner,
    power_planner::PowerPlanner,
    storage_planner::StoragePlanner,
};
use xynoos_pressureiq::discovery::inventory::SystemInventory;

/// Summary metrics for a whole-system optimization plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WholeSystemPlanSummary {
    pub total_actions: usize,
    pub mutating_actions: usize,
    pub preserved_actions: usize,
    pub deferred_actions: usize,
    pub conflict_actions: usize,
    pub required_scopes: Vec<AuthorizationScope>,
}

/// Unified whole-system planner coordinating all subsystem engines.
pub struct WholeSystemPlanner;

impl WholeSystemPlanner {
    /// Generate a cohesive whole-system configuration plan.
    pub fn plan(inv: &SystemInventory) -> Vec<ConfigurationAction> {
        let mut actions = Vec::new();

        // 1. Memory Subsystem (ZRAM, ZSWAP, swappiness, THP, cache pressure)
        let memory_actions = MemoryPlanner::plan(inv);
        actions.extend(memory_actions);

        // 2. CPU Subsystem (governor, EPP, bounds)
        let cpu_actions = CpuPlanner::plan(inv);
        actions.extend(cpu_actions);

        // 3. GPU Intelligence (shared RAM awareness, observe-only)
        let gpu_actions = GpuPlanner::plan(inv);
        actions.extend(gpu_actions);

        // 4. Storage & Block I/O Subsystem (schedulers, read-ahead)
        let storage_actions = StoragePlanner::plan(inv);
        actions.extend(storage_actions);

        // 5. Power & Thermal Subsystem (platform profile, coexistence)
        let power_actions = PowerPlanner::plan(inv);
        actions.extend(power_actions);

        // Multi-resource arbitration:
        // If thermal throttling is active, ensure any CPU or power boost is deferred.
        let is_throttled = inv.cpu.core_throttle_events > 0 || inv.cpu.package_throttle_events > 0;
        if is_throttled {
            for action in &mut actions {
                if action.action_id.starts_with("CPU-") || action.action_id.starts_with("POWER-") {
                    if action.classification == ActionClassification::Improve {
                        action.classification = ActionClassification::Defer;
                        action.potential_side_effects = format!(
                            "Deferred due to active thermal throttling events (core: {}, pkg: {})",
                            inv.cpu.core_throttle_events, inv.cpu.package_throttle_events
                        );
                    }
                }
            }
        }

        actions
    }

    /// Compute summary statistics for a whole-system plan.
    pub fn summarize(actions: &[ConfigurationAction]) -> WholeSystemPlanSummary {
        let mut mutating = 0;
        let mut preserved = 0;
        let mut deferred = 0;
        let mut conflicts = 0;
        let mut scopes = std::collections::BTreeSet::new();

        for a in actions {
            match a.classification {
                ActionClassification::Configure | ActionClassification::Improve => mutating += 1,
                ActionClassification::Preserve => preserved += 1,
                ActionClassification::Defer => deferred += 1,
                ActionClassification::Conflict => conflicts += 1,
                _ => {}
            }
            if a.is_mutation() {
                scopes.insert(a.authorization_scope);
            }
        }

        WholeSystemPlanSummary {
            total_actions: actions.len(),
            mutating_actions: mutating,
            preserved_actions: preserved,
            deferred_actions: deferred,
            conflict_actions: conflicts,
            required_scopes: scopes.into_iter().collect(),
        }
    }
}
