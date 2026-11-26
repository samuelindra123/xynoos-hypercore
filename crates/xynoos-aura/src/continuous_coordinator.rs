//! Multi-resource adaptive coordinator, oscillation prevention, and closed-loop effectiveness evaluator.
#![forbid(unsafe_code)]

use crate::telemetry_scheduler::TelemetrySample;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use xynoos_linux_control::config::RuntimeAuthorizationContract;

pub const COORDINATOR_SCHEMA_VERSION: u32 = 1;
const MAX_INTERVENTION_HISTORY: usize = 32;
const MIN_COOLDOWN_MS: u64 = 15_000; // 15s cooldown between interventions on same target
const CIRCUIT_BREAKER_WINDOW_MS: u64 = 60_000;
const MAX_FLIPS_BEFORE_CIRCUIT_BREAK: usize = 3;

/// Daemon runtime state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoordinatorState {
    #[default]
    Uninitialized,
    Discovering,
    Idle,
    ActiveMonitoring,
    Evaluating,
    Intervening,
    EvaluatingBenefit,
    CoolingDown,
    ShuttingDown,
}

/// Status of an active or historical intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionStatus {
    Applied,
    Beneficial,
    Ineffective,
    Regressed,
    Reverted,
    Expired,
}

/// Record of an active or evaluated temporary intervention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionRecord {
    pub target_id: String,
    pub control: String,
    pub original_value: u64,
    pub applied_value: u64,
    pub applied_at_epoch_ms: u64,
    pub lease_duration_ms: u64,
    pub pre_psi_memory: Option<f64>,
    pub pre_psi_cpu: Option<f64>,
    pub pre_psi_io: Option<f64>,
    pub post_psi_memory: Option<f64>,
    pub post_psi_cpu: Option<f64>,
    pub post_psi_io: Option<f64>,
    pub evaluated_cycles: u32,
    pub status: InterventionStatus,
    pub message: String,
}

/// Multi-resource adaptive coordinator.
pub struct ContinuousCoordinator {
    pub state: CoordinatorState,
    pub active_interventions: Vec<InterventionRecord>,
    pub history: VecDeque<InterventionRecord>,
    pub last_intervention_time_ms: u64,
    pub circuit_breaker_until_ms: u64,
    recent_flips: VecDeque<(u64, String)>,
    interventions_this_minute: u32,
    minute_window_start_ms: u64,
}

impl Default for ContinuousCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl ContinuousCoordinator {
    pub fn new() -> Self {
        Self {
            state: CoordinatorState::Uninitialized,
            active_interventions: Vec::new(),
            history: VecDeque::with_capacity(MAX_INTERVENTION_HISTORY),
            last_intervention_time_ms: 0,
            circuit_breaker_until_ms: 0,
            recent_flips: VecDeque::new(),
            interventions_this_minute: 0,
            minute_window_start_ms: 0,
        }
    }

    /// Check if circuit breaker is currently tripped.
    pub fn is_circuit_breaker_tripped(&self, now_ms: u64) -> bool {
        now_ms < self.circuit_breaker_until_ms
    }

    /// Clean up expired flips from sliding window.
    fn prune_flips(&mut self, now_ms: u64) {
        while let Some(&(ts, _)) = self.recent_flips.front() {
            if now_ms.saturating_sub(ts) > CIRCUIT_BREAKER_WINDOW_MS {
                self.recent_flips.pop_front();
            } else {
                break;
            }
        }
    }

    /// Record a directional policy change for oscillation detection.
    pub fn record_policy_flip(&mut self, target: &str, now_ms: u64) {
        self.prune_flips(now_ms);
        self.recent_flips.push_back((now_ms, target.to_string()));

        let target_flips = self
            .recent_flips
            .iter()
            .filter(|(_, t)| t == target)
            .count();

        if target_flips >= MAX_FLIPS_BEFORE_CIRCUIT_BREAK {
            // Trip circuit breaker for 60 seconds
            self.circuit_breaker_until_ms = now_ms.saturating_add(CIRCUIT_BREAKER_WINDOW_MS);
            self.state = CoordinatorState::CoolingDown;
        }
    }

    /// Multi-resource arbitration: evaluate whether an intervention is justified and safe.
    pub fn can_intervene(
        &mut self,
        target_id: &str,
        control: &str,
        sample: &TelemetrySample,
        auth: &RuntimeAuthorizationContract,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        // 1. Check circuit breaker
        if self.is_circuit_breaker_tripped(now_ms) {
            return Err("Circuit breaker active: policy oscillation detected");
        }

        // 2. Check cooldown
        if now_ms.saturating_sub(self.last_intervention_time_ms) < MIN_COOLDOWN_MS {
            return Err("Cooldown active: minimum interval between interventions not elapsed");
        }

        // 3. Check rate limit
        if now_ms.saturating_sub(self.minute_window_start_ms) >= 60_000 {
            self.minute_window_start_ms = now_ms;
            self.interventions_this_minute = 0;
        }
        if self.interventions_this_minute >= auth.max_interventions_per_minute {
            return Err("Intervention rate limit exceeded for current minute");
        }

        // 4. Multi-resource arbitration rules
        // Thermal constraint: if max temp >= 85°C or active throttle delta > 0, refuse performance ramps
        if (sample.max_cpu_temp_c.is_some_and(|t| t >= 85.0) || sample.cpu_throttle_delta > 0)
            && control == "cpu.weight"
        {
            return Err("Arbitration: thermal throttling active; CPU boosts prohibited");
        }

        // I/O constraint: if I/O PSI >= 25%, refuse interventions that trigger storage contention
        if sample.psi_io_some_avg10.is_some_and(|io| io >= 25.0) {
            return Err("Arbitration: severe I/O saturation detected; deferring adjustments");
        }

        // Battery constraint: if battery capacity < 20% and on battery, do not elevate CPU weights
        if sample.ac_online == Some(false)
            && sample.battery_capacity_pct.is_some_and(|b| b < 20)
            && control == "cpu.weight"
        {
            return Err("Arbitration: low battery power; preserving power-saver profile");
        }

        // 5. Scoped runtime authorization check
        auth.can_intervene(control, target_id, 120, now_ms)?;

        Ok(())
    }

    /// Record newly applied intervention and enter evaluation state.
    #[allow(clippy::too_many_arguments)]
    pub fn record_intervention_applied(
        &mut self,
        target_id: &str,
        control: &str,
        original_value: u64,
        applied_value: u64,
        lease_duration_ms: u64,
        sample: &TelemetrySample,
        now_ms: u64,
    ) {
        let record = InterventionRecord {
            target_id: target_id.to_string(),
            control: control.to_string(),
            original_value,
            applied_value,
            applied_at_epoch_ms: now_ms,
            lease_duration_ms,
            pre_psi_memory: sample.psi_memory_some_avg10,
            pre_psi_cpu: sample.psi_cpu_some_avg10,
            pre_psi_io: sample.psi_io_some_avg10,
            post_psi_memory: None,
            post_psi_cpu: None,
            post_psi_io: None,
            evaluated_cycles: 0,
            status: InterventionStatus::Applied,
            message: format!("Applied {control} = {applied_value} (lease: {lease_duration_ms} ms)"),
        };

        self.last_intervention_time_ms = now_ms;
        self.interventions_this_minute = self.interventions_this_minute.saturating_add(1);
        self.active_interventions.push(record);
        self.state = CoordinatorState::Intervening;
    }

    /// Closed-loop evaluation: inspect post-intervention telemetry and determine whether to retain, extend, or revert.
    pub fn evaluate_closed_loop(
        &mut self,
        sample: &TelemetrySample,
        now_ms: u64,
    ) -> Vec<InterventionRecord> {
        let mut actions_needed = Vec::new();

        self.active_interventions.retain_mut(|record| {
            record.evaluated_cycles = record.evaluated_cycles.saturating_add(1);
            let elapsed_ms = now_ms.saturating_sub(record.applied_at_epoch_ms);

            // 1. Check lease expiration
            if elapsed_ms >= record.lease_duration_ms {
                record.status = InterventionStatus::Expired;
                record.message = "Intervention lease expired; reverting to original value".into();
                actions_needed.push(record.clone());
                return false;
            }

            // 2. Closed-loop metric evaluation after at least 2 cycles
            if record.evaluated_cycles >= 2 {
                record.post_psi_memory = sample.psi_memory_some_avg10;
                record.post_psi_cpu = sample.psi_cpu_some_avg10;
                record.post_psi_io = sample.psi_io_some_avg10;

                // Evaluate whether intervention caused regression (handling zero/near-zero baselines & noise)
                let mut regressed = false;
                if let (Some(pre), Some(post)) =
                    (record.pre_psi_memory, sample.psi_memory_some_avg10)
                {
                    if (pre < 1.0 && post >= 10.0)
                        || (pre >= 1.0 && post > pre * 1.25 && (post - pre) >= 5.0)
                        || (post >= 30.0 && pre < 15.0)
                    {
                        regressed = true;
                    }
                }
                if let (Some(pre), Some(post)) = (record.pre_psi_cpu, sample.psi_cpu_some_avg10) {
                    if (pre < 1.0 && post >= 25.0)
                        || (pre >= 1.0 && post > pre * 1.25 && (post - pre) >= 15.0)
                    {
                        regressed = true;
                    }
                }

                if regressed {
                    record.status = InterventionStatus::Regressed;
                    record.message =
                        "Telemetry regression detected; aborting intervention immediately".into();
                    actions_needed.push(record.clone());
                    return false;
                }

                // Check if beneficial
                let mut beneficial = false;
                if let (Some(pre), Some(post)) =
                    (record.pre_psi_memory, sample.psi_memory_some_avg10)
                {
                    if post < pre {
                        beneficial = true;
                    }
                }
                if let (Some(pre), Some(post)) = (record.pre_psi_cpu, sample.psi_cpu_some_avg10) {
                    if post < pre {
                        beneficial = true;
                    }
                }

                if beneficial {
                    record.status = InterventionStatus::Beneficial;
                } else if record.evaluated_cycles >= 6 {
                    // Ineffective after 6 cycles -> revert
                    record.status = InterventionStatus::Ineffective;
                    record.message =
                        "Intervention produced no measurable telemetry improvement; reverting"
                            .into();
                    actions_needed.push(record.clone());
                    return false;
                }
            }

            true
        });

        // Archive completed interventions
        for item in &actions_needed {
            if self.history.len() >= MAX_INTERVENTION_HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(item.clone());
        }

        if self.active_interventions.is_empty() {
            if self.is_circuit_breaker_tripped(now_ms) {
                self.state = CoordinatorState::CoolingDown;
            } else {
                self.state = CoordinatorState::Idle;
            }
        } else {
            self.state = CoordinatorState::EvaluatingBenefit;
        }

        actions_needed
    }
}
