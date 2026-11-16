//! Bounded PressureIQ → decision hysteresis → AURA → owned-buffer executor.
//! Live kernel mutation is unconditionally refused, including with authorization.
#![forbid(unsafe_code)]

pub mod recovery;
pub mod replay;

use serde::Serialize;
use std::time::{Duration, Instant};
use xynoos_aura::{Baseline, DecisionPolicy};
use xynoos_core::{CostEstimate, MemoryPressure, StorageTier};
use xynoos_decision::{Decision, Kind};
use xynoos_pressureiq::system::SystemSnapshot;

pub const MAX_STEPS: usize = 60;
pub const PAGE_SIZE: usize = 4096;
pub const PAGES: usize = 64;
pub const STORE_BUDGET: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Baseline,
    Observe,
    Simulate,
    Controlled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    KeepOwnedRaw,
    CompressOwnedPages,
    BackOff,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Baseline,
    NoSustainedMemoryPressure,
    SustainedMemoryPressure,
    CpuOrIoPressure,
    InsufficientAvailableMemory,
    TelemetryUnavailable,
    AuthorizationRequired,
    PhysicalMutationLocked,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Observed,
    Simulated,
    Baseline,
    Refused,
    Cancelled,
}
#[derive(Debug, Serialize)]
pub struct Plan {
    pub mode: Mode,
    pub action: Action,
    pub reason: Reason,
    pub outcome: Outcome,
    pub decision: Option<Decision>,
    pub kernel_write_available: bool,
    pub physical_admission: Option<xynoos_linux_control::Decision>,
    pub maximum_store_accounted_bytes: usize,
}
#[derive(Default)]
pub struct Engine {
    pressure: xynoos_decision::Engine,
    previous_swap: Option<(u64, u64)>,
    cancelled: bool,
}
impl Engine {
    /// No caller-supplied plan is accepted by the executor. A plan is diagnostic data.
    /// Simulation authorization permits only this process's own bounded page store.
    pub fn evaluate(
        &mut self,
        snapshot: &SystemSnapshot,
        now: Instant,
        mode: Mode,
        authorized: bool,
        cancelled: bool,
    ) -> Plan {
        let mut p = Plan {
            mode,
            action: Action::KeepOwnedRaw,
            reason: Reason::Baseline,
            outcome: Outcome::Baseline,
            decision: None,
            kernel_write_available: false,
            physical_admission: None,
            maximum_store_accounted_bytes: STORE_BUDGET,
        };
        self.cancelled |= cancelled;
        if self.cancelled {
            p.action = Action::BackOff;
            p.reason = Reason::Cancelled;
            p.outcome = Outcome::Cancelled;
            return p;
        }
        if mode == Mode::Baseline {
            return p;
        }
        let d = self.pressure.evaluate(snapshot, now, None);
        // Swap counters are required diagnostics; absent/reset input never means zero.
        let swap = snapshot.memory.vm.result.as_ref().ok().and_then(|vm| {
            vm.counters
                .get("pswpin")
                .copied()
                .zip(vm.counters.get("pswpout").copied())
        });
        let vm_valid =
            swap.is_some_and(|(i, o)| self.previous_swap.is_none_or(|(pi, po)| i >= pi && o >= po));
        let valid = d.kind != Kind::Decline
            && vm_valid
            && now
                .checked_duration_since(snapshot.memory.monotonic)
                .is_some_and(|age| age <= Duration::from_secs(5));
        p.outcome = Outcome::Observed;
        p.reason = Reason::NoSustainedMemoryPressure;
        if !valid {
            self.pressure = xynoos_decision::Engine::default();
            self.previous_swap = None;
            p.action = Action::BackOff;
            p.reason = Reason::TelemetryUnavailable;
            p.outcome = Outcome::Refused;
        } else if snapshot
            .memory
            .mem
            .result
            .as_ref()
            .ok()
            .and_then(|m| m.bytes.get("MemAvailable"))
            .is_none_or(|n| *n < 200 * 1024 * 1024)
        {
            p.action = Action::BackOff;
            p.reason = Reason::InsufficientAvailableMemory;
        } else if d.active_pressure[0]
            || d.active_pressure[2]
            || d.pressure_percent
                .is_some_and(|v| v[0] >= 10.0 || v[2] >= 10.0)
        {
            p.action = Action::BackOff;
            p.reason = Reason::CpuOrIoPressure;
        } else {
            let pressure = if d.active_pressure[1] {
                if d.pressure_percent.is_some_and(|v| v[1] >= 20.0) {
                    MemoryPressure::High
                } else {
                    MemoryPressure::Moderate
                }
            } else {
                MemoryPressure::Low
            };
            // Existing AURA policy is deliberately simple; no invented cost predictions.
            let tier = Baseline.decide(
                pressure,
                CostEstimate {
                    latency_ns: 0,
                    bytes: PAGE_SIZE,
                },
            );
            if tier == StorageTier::Compressed {
                p.action = Action::CompressOwnedPages;
                p.reason = Reason::SustainedMemoryPressure;
            }
        }
        if valid {
            self.previous_swap = swap;
        }
        p.decision = Some(d);
        if mode == Mode::Controlled {
            // Use the same Linux execution admission boundary as existing callers.
            p.physical_admission = Some(xynoos_linux_control::physical_admission(authorized));
            p.action = Action::BackOff;
            p.outcome = Outcome::Refused;
            p.reason = if authorized {
                Reason::PhysicalMutationLocked
            } else {
                Reason::AuthorizationRequired
            };
        } else if mode == Mode::Simulate && p.outcome != Outcome::Refused {
            if authorized {
                p.outcome = Outcome::Simulated;
            } else {
                p.action = Action::BackOff;
                p.reason = Reason::AuthorizationRequired;
                p.outcome = Outcome::Refused;
            }
        }
        p
    }
}
