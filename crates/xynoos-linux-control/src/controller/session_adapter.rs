//! Session-backed coordinator bridge. Live mutations remain disabled.
use super::*;
use crate::{
    child_memory::{ChildIdentity, ErrorKind, MemoryLowError},
    empty_child_coordinator::{
        Backend, ChildObservation as CoordinatorChild, Fault, FaultKind, Observation, Stage,
    },
    safety::{self, SafetyDecision, SafetyObservation, SafetyThresholds},
};

/// Uses the existing coordinator and session protocols; no second state machine.
/// Evidence remains owned by the adapter even if a coordinator Fault is coarser.
/// Production construction never enables mutation, even for an authorized session.
pub struct ControllerSessionAdapter {
    session: ControllerSession,
    thresholds: SafetyThresholds,
    telemetry: Box<dyn FnMut(SafetyThresholds) -> SafetyObservation>,
    safety_refused: bool,
    pub safety_evidence: Vec<(Stage, SafetyObservation)>,
    pub session_evidence: Vec<(Stage, SessionObservation)>,
    pub mutation_evidence: Vec<(Stage, MutationOutcome)>,
    pub low_errors: Vec<(Stage, MemoryLowError)>,
    #[cfg(test)]
    fixture_mutations: bool,
}
fn fault(kind: FaultKind, message: impl Into<String>) -> Fault {
    Fault::new(kind, message)
}
fn child_key(c: ChildIdentity) -> String {
    format!("{}:{}", c.device, c.inode)
}
fn low_kind(e: &MemoryLowError) -> FaultKind {
    match e.kind {
        ErrorKind::RefusedBeforeWrite => FaultKind::Before,
        ErrorKind::IdentityConflict | ErrorKind::CompetingValue => FaultKind::Conflict,
        ErrorKind::WriteOutcomeAmbiguous | ErrorKind::ReadBackMismatch => FaultKind::Ambiguous,
        ErrorKind::RestorationFailure => e
            .cause
            .as_deref()
            .map(low_kind)
            .unwrap_or(FaultKind::Unavailable),
    }
}
impl ControllerSessionAdapter {
    pub fn new(session: ControllerSession, thresholds: SafetyThresholds) -> Self {
        Self {
            session,
            thresholds,
            telemetry: Box::new(safety::check_boundary),
            safety_refused: false,
            safety_evidence: Vec::new(),
            session_evidence: Vec::new(),
            mutation_evidence: Vec::new(),
            low_errors: Vec::new(),
            #[cfg(test)]
            fixture_mutations: false,
        }
    }
    /// Read retained diagnostic/session state; no mutation authority is returned.
    pub fn session_observation(&mut self) -> SessionObservation {
        self.session.observe()
    }
    #[cfg(test)]
    pub(super) fn fixture(
        session: ControllerSession,
        telemetry: impl FnMut(SafetyThresholds) -> SafetyObservation + 'static,
    ) -> Self {
        let mut adapter = Self::new(session, SafetyThresholds::default());
        adapter.telemetry = Box::new(telemetry);
        adapter.fixture_mutations = true;
        adapter
    }
    fn safety(&mut self, stage: Stage) -> Result<(), Fault> {
        let o = (self.telemetry)(self.thresholds);
        let eligible = o.decision == SafetyDecision::Eligible;
        self.safety_evidence.push((stage, o));
        if !eligible {
            self.safety_refused = true;
        }
        if self.safety_refused {
            return Err(fault(
                FaultKind::Before,
                "safety telemetry refused; mutation blocked for this adapter",
            ));
        }
        Ok(())
    }
    fn guard(&mut self, stage: Stage) -> Result<(), Fault> {
        if !self.session.backend.authorized {
            return Err(fault(
                FaultKind::Before,
                "explicit session authorization required",
            ));
        }
        let enabled = {
            #[cfg(test)]
            {
                self.fixture_mutations && self.session.backend.io.fixture_write_enabled()
            }
            #[cfg(not(test))]
            {
                false
            }
        };
        if !enabled {
            return Err(fault(FaultKind::Before, "live adapter mutations disabled"));
        }
        self.safety(stage)?;
        self.session
            .verify_for_mutation()
            .map_err(|e| fault(FaultKind::Unavailable, e))?;
        Ok(())
    }
    fn mutation(
        &mut self,
        stage: Stage,
        result: Result<MutationOutcome, MutationOutcome>,
    ) -> Result<(), Fault> {
        let (ok, outcome) = match result {
            Ok(o) => (true, o),
            Err(o) => (false, o),
        };
        let kind = match outcome.status {
            MutationStatus::AppliedAndVerified => FaultKind::Unavailable,
            MutationStatus::RefusedBeforeMutation
            | MutationStatus::CleanupRequired
            | MutationStatus::RestorationFailure => FaultKind::Before,
            MutationStatus::Conflict => FaultKind::Conflict,
            MutationStatus::AmbiguousMutation => FaultKind::Ambiguous,
        };
        let message = outcome.to_string();
        self.mutation_evidence.push((stage, outcome));
        if ok {
            Ok(())
        } else {
            Err(fault(kind, message))
        }
    }
    fn child(&mut self, identity: &str) -> Result<ChildIdentity, Fault> {
        let c = self
            .session
            .registered_child_identity()
            .map_err(|e| fault(FaultKind::Unavailable, e))?;
        if child_key(c) != identity {
            return Err(fault(FaultKind::Conflict, "registered child mismatch"));
        }
        Ok(c)
    }
    fn low_error(&mut self, stage: Stage, error: MemoryLowError) -> Fault {
        // A post-write identity failure is ambiguous even though its typed cause
        // is IdentityConflict. Never let the coordinator recover through a latch.
        let kind = if self.session.low_unresolved {
            FaultKind::Ambiguous
        } else {
            low_kind(&error)
        };
        let message = error.message.clone();
        self.low_errors.push((stage, error));
        fault(kind, message)
    }
}
impl Backend for ControllerSessionAdapter {
    fn inspect(&mut self, stage: Stage) -> Result<Observation, Fault> {
        let o = self.session.observe();
        self.session_evidence.push((stage, o.clone()));
        if o.activation_unresolved || o.child_unresolved || o.memory_low_unresolved {
            return Err(fault(FaultKind::Ambiguous, "session mutation unresolved"));
        }
        if o.validity != ObservationValidity::Valid
            || o.completeness != ObservationCompleteness::Complete
        {
            return Err(fault(
                if o.validity == ObservationValidity::Invalid {
                    FaultKind::Conflict
                } else {
                    FaultKind::Unavailable
                },
                "session observation invalid or incomplete",
            ));
        }
        self.session
            .verify_for_mutation()
            .map_err(|e| fault(FaultKind::Unavailable, e))?;
        // Inspect still collects evidence after a refusal; it never clears the latch.
        let safety_satisfied = self.safety(stage).is_ok();
        let child = if let Some(c) = o.registered_child {
            let inherited = self
                .session
                .child_controllers()
                .map_err(|e| fault(FaultKind::Unavailable, e))?
                .split_whitespace()
                .any(|c| c == "memory");
            let low = self
                .session
                .read_child_memory_low(c.identity)
                .map_err(|e| self.low_error(stage, e))?;
            Some(CoordinatorChild {
                identity: child_key(c.identity),
                empty: c.population.as_ref().is_ok_and(Vec::is_empty),
                // The mandatory strict verifier above checks zero child descendants.
                descendants: 0,
                memory_inherited: inherited,
                low: Some(low),
            })
        } else {
            None
        };
        let id = o.identity;
        // Include the complete historical binding identity, not just a pathname.
        let binding = format!("{:?}", id);
        Ok(Observation {
            binding,
            parent_empty: o.parent_population.as_ref().is_ok_and(Vec::is_empty),
            safety_satisfied,
            available_controllers: o
                .available_controllers
                .map_err(|e| fault(FaultKind::Unavailable, e))?,
            controllers: o
                .enabled_controllers
                .map_err(|e| fault(FaultKind::Unavailable, e))?,
            child,
        })
    }
    fn activate(&mut self, expected: &BTreeSet<String>) -> Result<(), Fault> {
        self.guard(Stage::Activate)?;
        let o = self.session.observe();
        if o.enabled_controllers.as_ref().ok() != Some(expected) {
            return Err(fault(FaultKind::Conflict, "controller capture changed"));
        }
        let result = self.session.activate_prepared();
        self.mutation(Stage::Activate, result)
    }
    fn create_child(&mut self) -> Result<String, Fault> {
        self.guard(Stage::CreateChild)?;
        let result = self.session.create_empty_child();
        self.mutation(Stage::CreateChild, result)?;
        self.session
            .registered_child_identity()
            .map(child_key)
            .map_err(|e| fault(FaultKind::Ambiguous, e))
    }
    fn write_low(
        &mut self,
        stage: Stage,
        identity: &str,
        expected: u64,
        value: u64,
    ) -> Result<(), Fault> {
        self.guard(stage)?;
        let child = self.child(identity)?;
        let restoring = matches!(stage, Stage::RestoreLow | Stage::RecoveryRestoreLow);
        self.session
            .change_child_memory_low(child, expected, value, true, restoring)
            .map(|_| ())
            .map_err(|e| self.low_error(stage, e))
    }
    fn remove_child(&mut self, stage: Stage, identity: &str) -> Result<(), Fault> {
        self.guard(stage)?;
        self.child(identity)?;
        let result = self.session.remove_empty_child();
        self.mutation(stage, result)
    }
    fn restore_controllers(
        &mut self,
        stage: Stage,
        original: &BTreeSet<String>,
    ) -> Result<(), Fault> {
        self.guard(stage)?;
        if self
            .session
            .original_controllers()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            != *original
        {
            return Err(fault(
                FaultKind::Conflict,
                "original controller record mismatch",
            ));
        }
        let result = self.session.restore(true);
        self.mutation(stage, result)
    }
}
