#![forbid(unsafe_code)]
//! Centralized runtime safety guard and circuit breaker for AURA.
//!
//! Prevents unsafe, destabilizing, or counterproductive kernel mutations by enforcing:
//! - Circuit breaker with configurable consecutive failure limits.
//! - Telemetry pressure bounds (CPU, I/O, and memory stall limits).
//! - Policy oscillation detection (rapid toggling between protective and throttling states).
//! - Rolling intervention rate limits.
//! - Automatic cooldown and half-open recovery probes.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Operational state of the safety guard circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    /// Circuit is closed: normal operation; transactions permitted.
    Closed,
    /// Circuit is open: tripped due to pressure, failures, or thrashing; transactions suspended.
    Open,
    /// Circuit is half-open: probing recovery after cooldown period.
    HalfOpen,
}

/// Explicit reason for rejecting an intrusive transaction or tripping the circuit breaker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyRejection {
    CircuitBreakerOpen {
        reason: String,
        cooldown_cycles_remaining: u64,
    },
    ExcessiveCpuPressure {
        avg10_pct: f64,
        threshold_pct: f64,
    },
    ExcessiveIoPressure {
        avg10_pct: f64,
        threshold_pct: f64,
    },
    ExcessiveMemoryPressure {
        avg10_pct: f64,
        threshold_pct: f64,
    },
    InterventionRateExceeded {
        interventions_in_window: u32,
        max_allowed: u32,
    },
    ConsecutiveFailuresExceeded {
        failures: u32,
        limit: u32,
    },
    PolicyOscillationDetected {
        transitions: u32,
    },
    RollbackFailureTripped {
        detail: String,
    },
}

impl std::fmt::Display for SafetyRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CircuitBreakerOpen {
                reason,
                cooldown_cycles_remaining,
            } => {
                write!(
                    f,
                    "circuit breaker is OPEN ({reason}); cooldown remaining: {cooldown_cycles_remaining} cycles"
                )
            }
            Self::ExcessiveCpuPressure {
                avg10_pct,
                threshold_pct,
            } => {
                write!(
                    f,
                    "excessive CPU pressure: {avg10_pct:.1}% exceeds safety threshold {threshold_pct:.1}%"
                )
            }
            Self::ExcessiveIoPressure {
                avg10_pct,
                threshold_pct,
            } => {
                write!(
                    f,
                    "excessive I/O pressure: {avg10_pct:.1}% exceeds safety threshold {threshold_pct:.1}%"
                )
            }
            Self::ExcessiveMemoryPressure {
                avg10_pct,
                threshold_pct,
            } => {
                write!(
                    f,
                    "excessive memory pressure: {avg10_pct:.1}% exceeds safety threshold {threshold_pct:.1}%"
                )
            }
            Self::InterventionRateExceeded {
                interventions_in_window,
                max_allowed,
            } => {
                write!(
                    f,
                    "intervention rate limit exceeded: {interventions_in_window} in window (max {max_allowed})"
                )
            }
            Self::ConsecutiveFailuresExceeded { failures, limit } => {
                write!(
                    f,
                    "consecutive failure limit exceeded: {failures} failures (limit {limit})"
                )
            }
            Self::PolicyOscillationDetected { transitions } => {
                write!(
                    f,
                    "rapid policy oscillation detected: {transitions} direction changes in recent cycles"
                )
            }
            Self::RollbackFailureTripped { detail } => {
                write!(f, "circuit breaker tripped by rollback failure: {detail}")
            }
        }
    }
}

/// Configuration parameters for `RuntimeSafetyGuard`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SafetyGuardConfig {
    pub max_consecutive_failures: u32,
    pub cooldown_cycles: u64,
    pub max_cpu_pressure_pct: f64,
    pub max_io_pressure_pct: f64,
    pub max_memory_pressure_pct: f64,
    pub max_interventions_per_window: u32,
    pub window_duration_secs: u64,
    pub oscillation_limit: u32,
}

impl Default for SafetyGuardConfig {
    fn default() -> Self {
        Self {
            max_consecutive_failures: 3,
            cooldown_cycles: 6,
            max_cpu_pressure_pct: 60.0,
            max_io_pressure_pct: 60.0,
            max_memory_pressure_pct: 75.0,
            max_interventions_per_window: 5,
            window_duration_secs: 60,
            oscillation_limit: 4,
        }
    }
}

/// Status summary of the runtime safety guard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyGuardStatus {
    pub state: CircuitState,
    pub consecutive_failures: u32,
    pub total_trips: u64,
    pub interventions_in_window: u32,
    pub cooldown_cycles_remaining: u64,
    pub last_trip_reason: Option<String>,
}

/// Centralized safety monitor and circuit breaker governing transactional optimization.
#[derive(Debug, Clone)]
pub struct RuntimeSafetyGuard {
    config: SafetyGuardConfig,
    state: CircuitState,
    consecutive_failures: u32,
    total_trips: u64,
    cooldown_remaining: u64,
    last_trip_reason: Option<String>,
    intervention_timestamps: Vec<Instant>,
    recent_directions: Vec<i8>, // +1 for protect, -1 for throttle, 0 for neutral
}

impl RuntimeSafetyGuard {
    /// Creates a new safety guard with explicit `config`.
    pub fn new(config: SafetyGuardConfig) -> Self {
        Self {
            config,
            state: CircuitState::Closed,
            consecutive_failures: 0,
            total_trips: 0,
            cooldown_remaining: 0,
            last_trip_reason: None,
            intervention_timestamps: Vec::new(),
            recent_directions: Vec::new(),
        }
    }

    pub fn config(&self) -> &SafetyGuardConfig {
        &self.config
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }

    /// Evaluates current telemetry and breaker state to determine if an intrusive
    /// transaction is permitted.
    pub fn can_intervene(
        &self,
        cpu_psi_avg10: Option<f64>,
        io_psi_avg10: Option<f64>,
        mem_psi_avg10: Option<f64>,
    ) -> Result<(), SafetyRejection> {
        // 1. Check Circuit Breaker State
        if self.state == CircuitState::Open {
            return Err(SafetyRejection::CircuitBreakerOpen {
                reason: self
                    .last_trip_reason
                    .clone()
                    .unwrap_or_else(|| "unspecified circuit breaker trip".into()),
                cooldown_cycles_remaining: self.cooldown_remaining,
            });
        }

        // 2. Telemetry bounds checking
        if let Some(cpu) = cpu_psi_avg10 {
            if cpu > self.config.max_cpu_pressure_pct {
                return Err(SafetyRejection::ExcessiveCpuPressure {
                    avg10_pct: cpu,
                    threshold_pct: self.config.max_cpu_pressure_pct,
                });
            }
        }
        if let Some(io) = io_psi_avg10 {
            if io > self.config.max_io_pressure_pct {
                return Err(SafetyRejection::ExcessiveIoPressure {
                    avg10_pct: io,
                    threshold_pct: self.config.max_io_pressure_pct,
                });
            }
        }
        if let Some(mem) = mem_psi_avg10 {
            if mem > self.config.max_memory_pressure_pct {
                return Err(SafetyRejection::ExcessiveMemoryPressure {
                    avg10_pct: mem,
                    threshold_pct: self.config.max_memory_pressure_pct,
                });
            }
        }

        // 3. Rate limiting within sliding window
        let now = Instant::now();
        let window = Duration::from_secs(self.config.window_duration_secs);
        let active_interventions = self
            .intervention_timestamps
            .iter()
            .filter(|&&t| now.duration_since(t) <= window)
            .count() as u32;

        if active_interventions >= self.config.max_interventions_per_window {
            return Err(SafetyRejection::InterventionRateExceeded {
                interventions_in_window: active_interventions,
                max_allowed: self.config.max_interventions_per_window,
            });
        }

        // 4. Policy oscillation check
        let transitions = count_direction_changes(&self.recent_directions);
        if transitions >= self.config.oscillation_limit {
            return Err(SafetyRejection::PolicyOscillationDetected { transitions });
        }

        Ok(())
    }

    /// Records a successfully applied and verified transaction.
    /// Resets consecutive failures and transitions HalfOpen state back to Closed.
    pub fn record_success(&mut self, direction: i8) {
        self.consecutive_failures = 0;
        self.intervention_timestamps.push(Instant::now());
        self.record_direction(direction);

        if self.state == CircuitState::HalfOpen {
            self.state = CircuitState::Closed;
            self.last_trip_reason = None;
        }
        self.prune_old_records();
    }

    /// Records a transaction failure. Increments failure count and trips the circuit breaker
    /// if the threshold is reached.
    pub fn record_failure(&mut self, reason: &str) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.config.max_consecutive_failures
            || self.state == CircuitState::HalfOpen
        {
            self.trip(format!(
                "consecutive failure threshold reached ({}): {}",
                self.consecutive_failures, reason
            ));
        }
    }

    /// Records a critical rollback failure, which immediately trips the circuit breaker.
    pub fn record_rollback_failure(&mut self, reason: &str) {
        self.trip(format!("CRITICAL rollback failure: {reason}"));
    }

    /// Trips the circuit breaker, suspending further transactions.
    pub fn trip(&mut self, reason: String) {
        self.state = CircuitState::Open;
        self.total_trips = self.total_trips.saturating_add(1);
        self.cooldown_remaining = self.config.cooldown_cycles;
        self.last_trip_reason = Some(reason);
    }

    /// Advances the safety guard clock by one daemon cycle.
    /// Handles cooldown decrements and transitions from Open to HalfOpen.
    pub fn tick_cycle(&mut self) {
        match self.state {
            CircuitState::Open => {
                if self.cooldown_remaining > 0 {
                    self.cooldown_remaining -= 1;
                }
                if self.cooldown_remaining == 0 {
                    // Transition to HalfOpen to allow a single probe transaction
                    self.state = CircuitState::HalfOpen;
                }
            }
            CircuitState::HalfOpen | CircuitState::Closed => {}
        }
        self.prune_old_records();
    }

    /// Returns a serializable status snapshot.
    pub fn status(&self) -> SafetyGuardStatus {
        let now = Instant::now();
        let window = Duration::from_secs(self.config.window_duration_secs);
        let active = self
            .intervention_timestamps
            .iter()
            .filter(|&&t| now.duration_since(t) <= window)
            .count() as u32;

        SafetyGuardStatus {
            state: self.state,
            consecutive_failures: self.consecutive_failures,
            total_trips: self.total_trips,
            interventions_in_window: active,
            cooldown_cycles_remaining: self.cooldown_remaining,
            last_trip_reason: self.last_trip_reason.clone(),
        }
    }

    /// Resets the circuit breaker and cleared failure counts.
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.consecutive_failures = 0;
        self.cooldown_remaining = 0;
        self.last_trip_reason = None;
        self.intervention_timestamps.clear();
        self.recent_directions.clear();
    }

    fn record_direction(&mut self, dir: i8) {
        self.recent_directions.push(dir);
        if self.recent_directions.len() > 10 {
            self.recent_directions.remove(0);
        }
    }

    fn prune_old_records(&mut self) {
        let now = Instant::now();
        let window = Duration::from_secs(self.config.window_duration_secs);
        self.intervention_timestamps
            .retain(|&t| now.duration_since(t) <= window);
    }
}

fn count_direction_changes(dirs: &[i8]) -> u32 {
    if dirs.len() < 2 {
        return 0;
    }
    let mut changes = 0u32;
    for i in 1..dirs.len() {
        if dirs[i] != 0 && dirs[i - 1] != 0 && dirs[i] != dirs[i - 1] {
            changes += 1;
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_breaker_trips_on_consecutive_failures_and_recovers_after_cooldown() {
        let config = SafetyGuardConfig {
            max_consecutive_failures: 2,
            cooldown_cycles: 2,
            ..Default::default()
        };
        let mut guard = RuntimeSafetyGuard::new(config);

        assert_eq!(guard.state(), CircuitState::Closed);
        assert!(guard
            .can_intervene(Some(10.0), Some(10.0), Some(20.0))
            .is_ok());

        // First failure: remains Closed
        guard.record_failure("timeout");
        assert_eq!(guard.state(), CircuitState::Closed);

        // Second failure: trips to Open
        guard.record_failure("permission denied");
        assert_eq!(guard.state(), CircuitState::Open);
        assert!(guard
            .can_intervene(Some(10.0), Some(10.0), Some(20.0))
            .is_err());

        // Tick cycle 1: still in cooldown
        guard.tick_cycle();
        assert_eq!(guard.state(), CircuitState::Open);

        // Tick cycle 2: cooldown expires -> transitions to HalfOpen
        guard.tick_cycle();
        assert_eq!(guard.state(), CircuitState::HalfOpen);
        assert!(guard
            .can_intervene(Some(10.0), Some(10.0), Some(20.0))
            .is_ok());

        // Successful probe resets to Closed
        guard.record_success(1);
        assert_eq!(guard.state(), CircuitState::Closed);
    }

    #[test]
    fn circuit_breaker_trips_immediately_on_critical_rollback_failure() {
        let mut guard = RuntimeSafetyGuard::new(SafetyGuardConfig::default());
        guard.record_rollback_failure("cgroup.subtree_control locked");
        assert_eq!(guard.state(), CircuitState::Open);
        assert_eq!(guard.status().total_trips, 1);
    }

    #[test]
    fn excessive_pressure_blocks_intervention_without_tripping() {
        let guard = RuntimeSafetyGuard::new(SafetyGuardConfig {
            max_cpu_pressure_pct: 50.0,
            ..Default::default()
        });

        // High CPU pressure rejected
        let res = guard.can_intervene(Some(55.0), Some(10.0), Some(10.0));
        assert!(matches!(
            res,
            Err(SafetyRejection::ExcessiveCpuPressure { .. })
        ));
        // Breaker itself remains Closed
        assert_eq!(guard.state(), CircuitState::Closed);
    }
}
