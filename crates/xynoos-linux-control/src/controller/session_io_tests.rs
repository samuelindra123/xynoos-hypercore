//! These tests inject only I/O: ControllerSession, LinuxController::verify_unit,
//! SessionLow and the typed mutation orchestration are the production code paths.
//! No Binding, filesystem, systemd call, worker or kernel mutation is used here.
use super::*;
use crate::{child_memory::ChildIdentity, systemd_identity::BoundIdentity};
use std::{
    ffi::CStr,
    sync::{Arc, Mutex},
};

const PARENT: ParentIdentity = ParentIdentity {
    device: 7,
    inode: 11,
};
const CHILD: ChildIdentity = ChildIdentity {
    device: 7,
    inode: 13,
};
#[derive(Clone, Copy, Default)]
enum WriteMode {
    #[default]
    Apply,
    ErrorAfterApply,
    Mismatch,
    ReadBackError,
}
struct State {
    enable_low: bool,
    low_writes: usize,
    low_mode: Option<(usize, &'static str)>,
    compete_low: bool,
    trace: Vec<String>,
    parent: ParentIdentity,
    enabled: String,
    available: String,
    parent_procs: String,
    manager_procs: String,
    extra_descendants: bool,
    manager_descendants: bool,
    child: Option<ChildIdentity>,
    child_entry: ChildIdentity,
    child_populated: bool,
    low: Result<u64, String>,
    controller_read_error: bool,
    change_on_reread: bool,
    enabled_reads: usize,
    revalidations: usize,
    identity_failure_after_write: bool,
    write_mode: WriteMode,
    readback_failed: bool,
    fail_create: bool,
    fail_remove: bool,
    writes: Vec<Vec<u8>>,
    creates: usize,
    removes: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            enable_low: false,
            low_writes: 0,
            low_mode: None,
            compete_low: false,
            trace: Vec::new(),
            parent: PARENT,
            enabled: "cpu pids".into(),
            available: "cpu memory pids".into(),
            parent_procs: String::new(),
            manager_procs: std::process::id().to_string(),
            extra_descendants: false,
            manager_descendants: false,
            child: None,
            child_entry: CHILD,
            child_populated: false,
            low: Ok(4096),
            controller_read_error: false,
            change_on_reread: false,
            enabled_reads: 0,
            revalidations: 0,
            identity_failure_after_write: false,
            write_mode: WriteMode::Apply,
            readback_failed: false,
            fail_create: false,
            fail_remove: false,
            writes: Vec::new(),
            creates: 0,
            removes: 0,
        }
    }
}
struct FixtureIo(Arc<Mutex<State>>);
impl FixtureIo {
    fn check_child(s: &State) -> Result<(), String> {
        if s.parent != PARENT || s.child != Some(s.child_entry) || s.child_populated {
            return Err("injected child identity, ownership or population failure".into());
        }
        Ok(())
    }
}
impl SessionIo for FixtureIo {
    fn bound_identity(&self) -> BoundIdentity {
        BoundIdentity {
            unit_id: "in-memory-fixture.service".into(),
            invocation_id: vec![1; 16],
            manager_pid: std::process::id(),
            manager_start_ticks: 19,
            parent: (PARENT.device, PARENT.inode),
            manager: (7, 12),
        }
    }
    fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"fixture": true})
    }
    fn revalidate(&self) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        s.revalidations += 1;
        if s.parent != PARENT || (s.identity_failure_after_write && !s.writes.is_empty()) {
            return Err("injected binding identity failure".into());
        }
        Ok(())
    }
    fn read_parent(&self, name: &CStr) -> Result<String, String> {
        let mut s = self.0.lock().unwrap();
        match name.to_bytes() {
            b"cgroup.procs" => Ok(s.parent_procs.clone()),
            b"cgroup.stat" => Ok(format!(
                "nr_descendants {}",
                1 + usize::from(s.child.is_some()) + usize::from(s.extra_descendants)
            )),
            b"cgroup.controllers" => Ok(s.available.clone()),
            b"cgroup.subtree_control" => {
                s.enabled_reads += 1;
                if s.controller_read_error || s.readback_failed {
                    return Err("injected controller read failure".into());
                }
                if s.change_on_reread && s.enabled_reads == 2 {
                    s.enabled = "cpu".into();
                }
                Ok(s.enabled.clone())
            }
            _ => panic!("unexpected parent read {name:?}"),
        }
    }
    fn read_manager(&self, name: &CStr) -> Result<String, String> {
        let s = self.0.lock().unwrap();
        match name.to_bytes() {
            b"cgroup.procs" => Ok(s.manager_procs.clone()),
            b"cgroup.stat" => Ok(format!(
                "nr_descendants {}",
                usize::from(s.manager_descendants)
            )),
            _ => panic!("unexpected manager read {name:?}"),
        }
    }
    fn write_controller(&mut self, command: &[u8]) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        s.trace.push(String::from_utf8(command.to_vec()).unwrap());
        s.writes.push(command.to_vec());
        s.readback_failed = matches!(s.write_mode, WriteMode::ReadBackError);
        if !matches!(s.write_mode, WriteMode::Mismatch) {
            s.enabled = match command {
                b"+memory" => "cpu memory pids".into(),
                b"-memory" => "cpu pids".into(),
                _ => panic!("unexpected controller command"),
            };
        }
        if matches!(s.write_mode, WriteMode::ErrorAfterApply) {
            return Err("injected error after possible write".into());
        }
        Ok(())
    }
    fn registered_child(&self) -> Option<ChildIdentity> {
        self.0.lock().unwrap().child
    }
    fn child_identity_matches(&self) -> Result<bool, String> {
        let s = self.0.lock().unwrap();
        Ok(s.child == Some(s.child_entry))
    }
    fn verify_child(&self, parent: ParentIdentity) -> Result<(), String> {
        assert_eq!(parent, PARENT);
        Self::check_child(&self.0.lock().unwrap())
    }
    fn child_population(&self) -> Result<String, String> {
        let s = self.0.lock().unwrap();
        if s.child != Some(s.child_entry) {
            return Err("injected child inode mismatch".into());
        }
        Ok(if s.child_populated {
            "99".into()
        } else {
            String::new()
        })
    }
    fn read_low(&self) -> Result<u64, String> {
        let s = self.0.lock().unwrap();
        Self::check_child(&s)?;
        s.low.clone()
    }
    fn child_controllers(&self, parent: ParentIdentity) -> Result<String, String> {
        self.verify_child(parent)?;
        Ok(self.0.lock().unwrap().available.clone())
    }
    fn create_child(&mut self, parent: ParentIdentity) -> Result<ChildIdentity, String> {
        assert_eq!(parent, PARENT);
        let mut s = self.0.lock().unwrap();
        assert!(s.child.is_none());
        s.trace.push("create".into());
        s.creates += 1;
        s.child = Some(CHILD);
        if s.fail_create {
            return Err("injected child creation failure after possible mkdir".into());
        }
        Ok(CHILD)
    }
    fn remove_child(&mut self, parent: ParentIdentity) -> Result<(), String> {
        self.verify_child(parent)?;
        let mut s = self.0.lock().unwrap();
        s.trace.push("remove".into());
        s.removes += 1;
        if s.fail_remove {
            return Err("injected child cleanup failure".into());
        }
        s.child = None;
        Ok(())
    }
    fn fixture_write_enabled(&self) -> bool {
        self.0.lock().unwrap().enable_low
    }
    fn fixture_prepare_low(&mut self) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        if s.compete_low {
            s.low = Ok(12288);
            s.compete_low = false;
        }
        if s.low_mode == Some((s.low_writes + 1, "before")) {
            return Err("injected prepare refusal".into());
        }
        Ok(())
    }
    fn fixture_write_low(&mut self, bytes: &[u8]) -> Result<usize, String> {
        let mut s = self.0.lock().unwrap();
        s.low_writes += 1;
        let value = std::str::from_utf8(bytes).unwrap().parse::<u64>().unwrap();
        s.trace.push(format!("low:{value}"));
        s.low = Ok(value);
        let mode = s
            .low_mode
            .filter(|(n, _)| *n == s.low_writes)
            .map(|(_, m)| m);
        match mode {
            Some("error") => Err("injected low write error after effect".into()),
            Some("short") => Ok(bytes.len() - 1),
            Some("mismatch") => {
                s.low = Ok(12288);
                Ok(bytes.len())
            }
            Some("readback") => {
                s.low = Err("injected readback error".into());
                Ok(bytes.len())
            }
            _ => Ok(bytes.len()),
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
struct Fixture {
    session: ControllerSession,
    state: Arc<Mutex<State>>,
}
impl Fixture {
    fn new() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        Self {
            session: ControllerSession {
                backend: LinuxController {
                    io: Box::new(FixtureIo(state.clone())),
                    // Test-only authority over in-memory state; no live Binding exists.
                    authorized: true,
                    child_unresolved: false,
                },
                activation: None,
                original_before_attempt: None,
                activation_unresolved: false,
                low_unresolved: false,
            },
            state,
        }
    }
    fn activate(&mut self) {
        applied(
            self.session.activate_prepared().unwrap(),
            MutationOperation::Activate,
        );
    }
    fn child(&mut self) {
        self.activate();
        applied(
            self.session.create_empty_child().unwrap(),
            MutationOperation::CreateChild,
        );
        assert_eq!(self.session.registered_child_identity(), Ok(CHILD));
    }
}
fn applied(outcome: MutationOutcome, operation: MutationOperation) {
    assert_eq!(outcome.operation, operation);
    assert_eq!(outcome.status, MutationStatus::AppliedAndVerified);
    assert_eq!(outcome.mutation_attempted, Some(true));
    assert!(outcome.verified);
    assert_eq!(outcome.expected, outcome.observed);
    assert!(outcome.error.is_none());
}
fn failed(
    outcome: &MutationOutcome,
    op: MutationOperation,
    status: MutationStatus,
    attempted: Option<bool>,
) {
    assert_eq!(outcome.operation, op);
    assert_eq!(outcome.status, status);
    assert_eq!(outcome.mutation_attempted, attempted);
    assert!(!outcome.verified);
    assert_eq!(outcome.error.as_ref().unwrap().operation, op);
}
fn observation(
    o: &SessionObservation,
    validity: ObservationValidity,
    completeness: ObservationCompleteness,
) {
    assert_eq!(o.validity, validity);
    assert_eq!(o.completeness, completeness);
}

#[test]
fn valid_session_observation_and_verified_lifecycle() {
    let mut f = Fixture::new();
    let o = f.session.observe();
    observation(
        &o,
        ObservationValidity::Valid,
        ObservationCompleteness::Complete,
    );
    assert_eq!(o.activation, ObservedActivation::NotActive);
    assert_eq!(o.parent_population, Ok(vec![]));
    assert_eq!(o.manager_population, Ok(vec![std::process::id()]));
    assert_eq!(
        o.enabled_controllers,
        Ok(BTreeSet::from(["cpu".into(), "pids".into()]))
    );
    assert!(o.registered_child.is_none() && o.issues.is_empty());
    assert_eq!(f.state.lock().unwrap().revalidations, 2);
    assert!(f.state.lock().unwrap().writes.is_empty());
    assert_eq!(f.session.verify_for_mutation(), Ok(PARENT));
    f.child();
    let o = f.session.observe();
    observation(
        &o,
        ObservationValidity::Valid,
        ObservationCompleteness::Complete,
    );
    assert_eq!(o.activation, ObservedActivation::Active);
    assert_eq!(o.registered_child.unwrap().memory_low, Ok(4096));
    assert_eq!(f.session.child_controllers(), Ok("cpu memory pids".into()));
    assert_eq!(f.session.read_child_memory_low(CHILD), Ok(4096));
    applied(
        f.session.remove_empty_child().unwrap(),
        MutationOperation::RemoveChild,
    );
    applied(
        f.session.restore(true).unwrap(),
        MutationOperation::RestoreControllers,
    );
    observation(
        &f.session.observe(),
        ObservationValidity::Valid,
        ObservationCompleteness::Complete,
    );
    assert_eq!(
        f.state.lock().unwrap().writes,
        [b"+memory".to_vec(), b"-memory".to_vec()]
    );
}
#[test]
fn controller_read_failure_is_unknown_and_refuses_activation() {
    let mut f = Fixture::new();
    f.state.lock().unwrap().controller_read_error = true;
    let o = f.session.observe();
    observation(
        &o,
        ObservationValidity::Unknown,
        ObservationCompleteness::Incomplete,
    );
    assert!(o.enabled_controllers.is_err());
    failed(
        &f.session.activate_prepared().unwrap_err(),
        MutationOperation::Activate,
        MutationStatus::RefusedBeforeMutation,
        Some(false),
    );
    assert!(f.state.lock().unwrap().writes.is_empty());
}
#[test]
fn controller_change_between_observations_is_not_cached_authority() {
    let mut f = Fixture::new();
    observation(
        &f.session.observe(),
        ObservationValidity::Valid,
        ObservationCompleteness::Complete,
    );
    f.state.lock().unwrap().enabled = "cpu memory pids".into();
    let o = f.session.observe();
    observation(
        &o,
        ObservationValidity::Invalid,
        ObservationCompleteness::Complete,
    );
    assert_eq!(o.activation, ObservedActivation::NotActive);
    assert!(o.enabled_controllers.unwrap().contains("memory"));
    failed(
        &f.session.activate_prepared().unwrap_err(),
        MutationOperation::Activate,
        MutationStatus::RefusedBeforeMutation,
        Some(false),
    );
    assert!(f.state.lock().unwrap().writes.is_empty());
}
#[test]
fn controller_change_during_observation_is_invalid() {
    let mut f = Fixture::new();
    f.state.lock().unwrap().change_on_reread = true;
    observation(
        &f.session.observe(),
        ObservationValidity::Invalid,
        ObservationCompleteness::Complete,
    );
    assert_eq!(f.state.lock().unwrap().enabled_reads, 2);
    assert!(f.state.lock().unwrap().writes.is_empty());
}
#[test]
fn parent_identity_mismatch_invalidates_and_refuses() {
    let mut f = Fixture::new();
    f.state.lock().unwrap().parent.inode += 1;
    let o = f.session.observe();
    observation(
        &o,
        ObservationValidity::Invalid,
        ObservationCompleteness::Incomplete,
    );
    assert!(o.binding_before.is_err() && o.binding_after.is_err());
    assert_eq!(o.identity.parent, (PARENT.device, PARENT.inode));
    assert!(f.session.verify_for_mutation().is_err());
    failed(
        &f.session.activate_prepared().unwrap_err(),
        MutationOperation::Activate,
        MutationStatus::RefusedBeforeMutation,
        Some(false),
    );
    assert!(f.state.lock().unwrap().writes.is_empty());
}
#[test]
fn registered_child_inode_mismatch_preserves_child() {
    let mut f = Fixture::new();
    f.child();
    f.state.lock().unwrap().child_entry.inode += 1;
    let o = f.session.observe();
    observation(
        &o,
        ObservationValidity::Unknown,
        ObservationCompleteness::Incomplete,
    );
    let child = o.registered_child.unwrap();
    assert_eq!(child.identity, CHILD);
    assert!(child.population.is_err() && child.memory_low.is_err());
    assert!(f.session.verify_for_mutation().is_err());
    assert_eq!(
        f.session.read_child_memory_low(CHILD).unwrap_err().kind,
        crate::child_memory::ErrorKind::IdentityConflict
    );
    failed(
        &f.session.remove_empty_child().unwrap_err(),
        MutationOperation::RemoveChild,
        MutationStatus::Conflict,
        Some(false),
    );
    failed(
        &f.session.restore(true).unwrap_err(),
        MutationOperation::RestoreControllers,
        MutationStatus::CleanupRequired,
        Some(false),
    );
    assert_eq!(f.state.lock().unwrap().removes, 0);
    assert_eq!(f.state.lock().unwrap().child, Some(CHILD));
    assert_eq!(f.state.lock().unwrap().writes.len(), 1);
}
#[test]
fn memory_low_read_failure_is_typed_and_incomplete() {
    let mut f = Fixture::new();
    f.child();
    f.state.lock().unwrap().low = Err("injected memory.low read failure".into());
    let o = f.session.observe();
    observation(
        &o,
        ObservationValidity::Unknown,
        ObservationCompleteness::Incomplete,
    );
    assert!(o.registered_child.unwrap().memory_low.is_err());
    let e = f.session.read_child_memory_low(CHILD).unwrap_err();
    assert_eq!(e.kind, crate::child_memory::ErrorKind::RefusedBeforeWrite);
    assert_eq!(e.observed, None);
    assert!(!f.session.observe().memory_low_unresolved);
    assert_eq!(f.state.lock().unwrap().writes.len(), 1);
}
#[test]
fn pre_write_refusal_uses_real_hierarchy_and_population_checks() {
    for case in 0..4 {
        let mut f = Fixture::new();
        {
            let mut s = f.state.lock().unwrap();
            match case {
                0 => s.parent_procs = "99".into(),
                1 => s.manager_procs = "0".into(),
                2 => s.extra_descendants = true,
                3 => s.manager_descendants = true,
                _ => unreachable!(),
            }
        }
        assert!(f.session.verify_for_mutation().is_err(), "case {case}");
        failed(
            &f.session.activate_prepared().unwrap_err(),
            MutationOperation::Activate,
            MutationStatus::RefusedBeforeMutation,
            Some(false),
        );
        assert!(f.state.lock().unwrap().writes.is_empty());
    }
}
#[test]
fn ambiguous_controller_write_retains_expected_and_observed_certainty() {
    let mut f = Fixture::new();
    f.state.lock().unwrap().write_mode = WriteMode::ErrorAfterApply;
    let e = f.session.activate_prepared().unwrap_err();
    failed(
        &e,
        MutationOperation::Activate,
        MutationStatus::AmbiguousMutation,
        Some(true),
    );
    assert_eq!(
        e.last_verified.unwrap().controllers,
        Some(vec!["cpu".into(), "pids".into()])
    );
    assert_eq!(
        e.expected.unwrap().controllers,
        Some(vec!["cpu".into(), "memory".into(), "pids".into()])
    );
    assert_eq!(e.observed, None);
    let o = f.session.observe();
    assert_eq!(o.activation, ObservedActivation::Unresolved);
    assert!(o.activation_unresolved);
    observation(
        &o,
        ObservationValidity::Unknown,
        ObservationCompleteness::Incomplete,
    );
    assert_eq!(f.state.lock().unwrap().writes.len(), 1);
}
#[test]
fn controller_write_readback_mismatch_is_ambiguous_with_observed_state() {
    let mut f = Fixture::new();
    f.state.lock().unwrap().write_mode = WriteMode::Mismatch;
    let e = f.session.activate_prepared().unwrap_err();
    failed(
        &e,
        MutationOperation::Activate,
        MutationStatus::AmbiguousMutation,
        Some(true),
    );
    assert_eq!(
        e.observed.as_ref().unwrap().controllers,
        Some(vec!["cpu".into(), "pids".into()])
    );
    assert_ne!(e.observed, e.expected);
    assert!(f.session.observe().activation_unresolved);
}
#[test]
fn controller_readback_error_is_ambiguous_with_unknown_observed_state() {
    let mut f = Fixture::new();
    f.state.lock().unwrap().write_mode = WriteMode::ReadBackError;
    let e = f.session.activate_prepared().unwrap_err();
    failed(
        &e,
        MutationOperation::Activate,
        MutationStatus::AmbiguousMutation,
        Some(true),
    );
    assert_eq!(e.observed, None);
    assert!(f.session.observe().activation_unresolved);
}
#[test]
fn child_cleanup_failure_latches_and_blocks_controller_restoration() {
    let mut f = Fixture::new();
    f.child();
    f.state.lock().unwrap().fail_remove = true;
    let e = f.session.remove_empty_child().unwrap_err();
    failed(
        &e,
        MutationOperation::RemoveChild,
        MutationStatus::AmbiguousMutation,
        None,
    );
    assert_eq!(e.last_verified.unwrap().child, Some(CHILD));
    assert_eq!(e.expected.unwrap().child_present, Some(false));
    assert_eq!(e.observed, None);
    let o = f.session.observe();
    assert!(o.child_unresolved);
    observation(
        &o,
        ObservationValidity::Unknown,
        ObservationCompleteness::Incomplete,
    );
    assert!(f.session.verify_for_mutation().is_err());
    failed(
        &f.session.remove_empty_child().unwrap_err(),
        MutationOperation::RemoveChild,
        MutationStatus::CleanupRequired,
        Some(false),
    );
    failed(
        &f.session.restore(true).unwrap_err(),
        MutationOperation::RestoreControllers,
        MutationStatus::CleanupRequired,
        Some(false),
    );
    assert_eq!(f.state.lock().unwrap().removes, 1);
    assert_eq!(f.state.lock().unwrap().child, Some(CHILD));
    assert_eq!(f.state.lock().unwrap().writes.len(), 1);
}
#[test]
fn controller_restoration_failure_before_write_is_not_retried() {
    let mut f = Fixture::new();
    f.activate();
    f.state.lock().unwrap().parent.inode += 1;
    let e = f.session.restore(true).unwrap_err();
    failed(
        &e,
        MutationOperation::RestoreControllers,
        MutationStatus::RestorationFailure,
        Some(false),
    );
    assert_eq!(
        e.expected.unwrap().controllers,
        Some(vec!["cpu".into(), "pids".into()])
    );
    f.state.lock().unwrap().parent = PARENT;
    assert!(f.session.observe().activation_unresolved);
    failed(
        &f.session.restore(true).unwrap_err(),
        MutationOperation::RestoreControllers,
        MutationStatus::CleanupRequired,
        Some(false),
    );
    assert_eq!(f.state.lock().unwrap().writes.len(), 1);
}
#[test]
fn controller_restoration_write_failure_or_mismatch_stays_ambiguous() {
    for mode in [
        WriteMode::ErrorAfterApply,
        WriteMode::Mismatch,
        WriteMode::ReadBackError,
    ] {
        let mut f = Fixture::new();
        f.activate();
        f.state.lock().unwrap().write_mode = mode;
        let e = f.session.restore(true).unwrap_err();
        failed(
            &e,
            MutationOperation::RestoreControllers,
            MutationStatus::AmbiguousMutation,
            Some(true),
        );
        assert!(f.session.observe().activation_unresolved);
        failed(
            &f.session.restore(true).unwrap_err(),
            MutationOperation::RestoreControllers,
            MutationStatus::CleanupRequired,
            Some(false),
        );
        assert_eq!(f.state.lock().unwrap().writes.len(), 2);
    }
}
#[test]
fn unresolved_mutation_blocks_retry_even_after_valid_io_returns() {
    let mut f = Fixture::new();
    f.state.lock().unwrap().write_mode = WriteMode::ErrorAfterApply;
    failed(
        &f.session.activate_prepared().unwrap_err(),
        MutationOperation::Activate,
        MutationStatus::AmbiguousMutation,
        Some(true),
    );
    f.state.lock().unwrap().write_mode = WriteMode::Apply;
    let o = f.session.observe();
    assert!(o.binding_before.is_ok() && o.binding_after.is_ok());
    assert_eq!(o.activation, ObservedActivation::Unresolved);
    assert!(f.session.verify_for_mutation().is_err());
    failed(
        &f.session.activate_prepared().unwrap_err(),
        MutationOperation::Activate,
        MutationStatus::RefusedBeforeMutation,
        Some(false),
    );
    failed(
        &f.session.create_empty_child().unwrap_err(),
        MutationOperation::CreateChild,
        MutationStatus::RefusedBeforeMutation,
        Some(false),
    );
    failed(
        &f.session.restore(true).unwrap_err(),
        MutationOperation::RestoreControllers,
        MutationStatus::CleanupRequired,
        Some(false),
    );
    assert_eq!(f.state.lock().unwrap().writes.len(), 1);
    assert_eq!(f.state.lock().unwrap().creates, 0);
}
#[test]
fn child_creation_failure_retains_ownership_and_blocks_retry() {
    let mut f = Fixture::new();
    f.activate();
    f.state.lock().unwrap().fail_create = true;
    failed(
        &f.session.create_empty_child().unwrap_err(),
        MutationOperation::CreateChild,
        MutationStatus::AmbiguousMutation,
        None,
    );
    assert!(f.session.observe().child_unresolved);
    assert_eq!(f.state.lock().unwrap().child, Some(CHILD));
    failed(
        &f.session.create_empty_child().unwrap_err(),
        MutationOperation::CreateChild,
        MutationStatus::RefusedBeforeMutation,
        Some(false),
    );
    failed(
        &f.session.restore(true).unwrap_err(),
        MutationOperation::RestoreControllers,
        MutationStatus::CleanupRequired,
        Some(false),
    );
    assert_eq!(f.state.lock().unwrap().creates, 1);
}
#[test]
fn identity_failure_after_write_is_ambiguous() {
    let mut f = Fixture::new();
    f.state.lock().unwrap().identity_failure_after_write = true;
    let e = f.session.activate_prepared().unwrap_err();
    failed(
        &e,
        MutationOperation::Activate,
        MutationStatus::AmbiguousMutation,
        Some(true),
    );
    assert!(f.session.observe().activation_unresolved);
    assert_eq!(f.state.lock().unwrap().writes.len(), 1);
}
#[test]
fn observation_and_fixture_identity_cannot_enable_writes() {
    let mut f = Fixture::new();
    f.session.backend.authorized = false;
    observation(
        &f.session.observe(),
        ObservationValidity::Valid,
        ObservationCompleteness::Complete,
    );
    assert_eq!(f.session.verify_for_mutation(), Ok(PARENT));
    assert!(f.session.backend.enable_memory().is_err());
    assert!(f.state.lock().unwrap().writes.is_empty());
    f.session.backend.authorized = true;
    f.child();
    failed(
        &f.session.restore(false).unwrap_err(),
        MutationOperation::RestoreControllers,
        MutationStatus::RefusedBeforeMutation,
        Some(false),
    );
    for authorized in [false, true] {
        let e = f
            .session
            .change_child_memory_low(CHILD, 4096, 8192, authorized, false)
            .unwrap_err();
        assert_eq!(e.kind, crate::child_memory::ErrorKind::RefusedBeforeWrite);
        assert!(!f.session.observe().memory_low_unresolved);
        assert_eq!(f.state.lock().unwrap().low, Ok(4096));
    }
    assert_eq!(f.state.lock().unwrap().writes.len(), 1);
}
#[test]
fn populated_registered_child_refuses_cleanup_without_mutation() {
    let mut f = Fixture::new();
    f.child();
    f.state.lock().unwrap().child_populated = true;
    observation(
        &f.session.observe(),
        ObservationValidity::Invalid,
        ObservationCompleteness::Incomplete,
    );
    assert!(f.session.verify_for_mutation().is_err());
    failed(
        &f.session.remove_empty_child().unwrap_err(),
        MutationOperation::RemoveChild,
        MutationStatus::RefusedBeforeMutation,
        Some(false),
    );
    assert_eq!(f.state.lock().unwrap().removes, 0);
}

#[cfg(test)]
mod adapter_integration {
    use super::*;
    use crate::{
        empty_child_coordinator::{
            self as coordinator, Backend, Certainty, FaultKind, Outcome, Stage,
        },
        safety::{self, SafetyDecision},
    };
    type IntegratedFixture = (
        ControllerSessionAdapter,
        Arc<Mutex<State>>,
        Arc<Mutex<(u64, bool)>>,
    );
    fn adapter() -> IntegratedFixture {
        let f = Fixture::new();
        f.state.lock().unwrap().enable_low = true;
        let telemetry = Arc::new(Mutex::new((u64::MAX, false)));
        let readings = telemetry.clone();
        let a = ControllerSessionAdapter::fixture(f.session, move |thresholds| {
            let (available, stale) = *readings.lock().unwrap();
            safety::fixture_boundary(thresholds, available, stale)
        });
        (a, f.state, telemetry)
    }
    fn run(a: &mut ControllerSessionAdapter) -> coordinator::Report {
        coordinator::run(a, true, 8192, |_| false)
    }
    fn clean(s: &Arc<Mutex<State>>) {
        let s = s.lock().unwrap();
        assert!(s.child.is_none());
        assert_eq!(s.enabled, "cpu pids");
        assert_eq!(s.low, Ok(4096));
    }
    #[test]
    fn integrated_complete_lifecycle() {
        let (mut a, s, _) = adapter();
        let r = run(&mut a);
        assert_eq!(r.outcome, Outcome::Complete);
        assert_eq!(r.original_low, Some(4096));
        assert_eq!(r.low_restoration_verified, Certainty::Yes);
        assert_eq!(r.child_remains, Certainty::No);
        assert_eq!(r.controller_restoration_verified, Certainty::Yes);
        clean(&s);
        assert_eq!(
            s.lock().unwrap().trace,
            ["+memory", "create", "low:8192", "low:4096", "remove", "-memory"]
        );
        assert_eq!(a.mutation_evidence.len(), 4);
        assert!(a
            .mutation_evidence
            .iter()
            .all(|(_, o)| o.status == MutationStatus::AppliedAndVerified));
        for stage in [
            Stage::Activate,
            Stage::CreateChild,
            Stage::ApplyLow,
            Stage::RestoreLow,
            Stage::RemoveChild,
            Stage::RestoreControllers,
        ] {
            assert!(a
                .safety_evidence
                .iter()
                .any(|(at, o)| *at == stage && o.decision == SafetyDecision::Eligible));
        }
        assert!(a
            .session_evidence
            .iter()
            .all(|(_, o)| o.validity == ObservationValidity::Valid));
    }
    #[test]
    fn integrated_authorization_and_live_gate_stay_closed() {
        let (mut a, s, _) = adapter();
        let r = coordinator::run(&mut a, false, 8192, |_| false);
        assert_eq!(r.failed_transition, Some(Stage::Authorization));
        assert!(s.lock().unwrap().trace.is_empty());
        let (mut a, s, _) = adapter();
        a.session_observation();
        // The production constructor cannot use the test-only mutation capability.
        let f = Fixture::new();
        let mut production =
            ControllerSessionAdapter::new(f.session, crate::safety::SafetyThresholds::default());
        assert_eq!(
            production.activate(&BTreeSet::new()).unwrap_err().kind,
            FaultKind::Before
        );
        assert!(f.state.lock().unwrap().trace.is_empty());
        assert!(s.lock().unwrap().trace.is_empty());
    }
    #[test]
    fn integrated_telemetry_refusal_blocks_all_later_mutations() {
        for stale in [false, true] {
            let (mut a, s, t) = adapter();
            *t.lock().unwrap() = (0, stale);
            let r = run(&mut a);
            assert_ne!(r.outcome, Outcome::Complete);
            assert!(s.lock().unwrap().trace.is_empty());
            assert!(a
                .safety_evidence
                .iter()
                .any(|(_, o)| matches!(o.decision, SafetyDecision::Refused(_))));
        }
        for failed in [
            Stage::Activate,
            Stage::CreateChild,
            Stage::ApplyLow,
            Stage::RestoreLow,
            Stage::RemoveChild,
            Stage::RestoreControllers,
        ] {
            let (mut a, s, t) = adapter();
            let mut snapshot = None;
            let r = coordinator::run(&mut a, true, 8192, |stage| {
                if stage == failed {
                    *t.lock().unwrap() = (0, false);
                    snapshot = Some(s.lock().unwrap().trace.clone());
                }
                false
            });
            assert_ne!(r.outcome, Outcome::Complete);
            assert_eq!(s.lock().unwrap().trace, snapshot.unwrap(), "{failed:?}");
        }
    }
    #[test]
    fn integrated_stale_binding_refuses_without_mutation() {
        let (mut a, s, _) = adapter();
        s.lock().unwrap().parent.inode += 1;
        let r = run(&mut a);
        assert_eq!(r.outcome, Outcome::Conflict);
        assert!(s.lock().unwrap().trace.is_empty());
        assert!(a.session_evidence[0].1.binding_before.is_err());
    }
    #[test]
    fn integrated_controller_activation_failure_latches() {
        let (mut a, s, _) = adapter();
        s.lock().unwrap().write_mode = WriteMode::ErrorAfterApply;
        let r = run(&mut a);
        assert_eq!(r.outcome, Outcome::Ambiguous);
        assert_eq!(r.failed_transition, Some(Stage::Activate));
        assert!(a.session_observation().activation_unresolved);
        assert_eq!(s.lock().unwrap().trace, ["+memory"]);
        assert_eq!(
            a.mutation_evidence[0].1.status,
            MutationStatus::AmbiguousMutation
        );
    }
    #[test]
    fn integrated_child_creation_failure_preserves_ownership() {
        let (mut a, s, _) = adapter();
        s.lock().unwrap().fail_create = true;
        let r = run(&mut a);
        assert_eq!(r.outcome, Outcome::Ambiguous);
        assert_eq!(r.failed_transition, Some(Stage::CreateChild));
        assert!(a.session_observation().child_unresolved);
        assert_eq!(s.lock().unwrap().trace, ["+memory", "create"]);
        assert_eq!(s.lock().unwrap().child, Some(CHILD));
    }
    #[test]
    fn integrated_low_ambiguity_and_readback_mismatch_never_retry() {
        for mode in ["error", "short", "mismatch", "readback"] {
            let (mut a, s, _) = adapter();
            s.lock().unwrap().low_mode = Some((1, mode));
            let r = run(&mut a);
            assert_eq!(r.outcome, Outcome::Ambiguous, "{mode}");
            assert_eq!(r.failed_transition, Some(Stage::ApplyLow));
            assert!(a.session_observation().memory_low_unresolved);
            assert_eq!(s.lock().unwrap().trace, ["+memory", "create", "low:8192"]);
            assert_eq!(
                a.low_errors[0].1.kind,
                if mode == "mismatch" {
                    crate::child_memory::ErrorKind::ReadBackMismatch
                } else {
                    crate::child_memory::ErrorKind::WriteOutcomeAmbiguous
                }
            );
        }
    }
    #[test]
    fn integrated_low_prewrite_refusal_recovers_in_order() {
        let (mut a, s, _) = adapter();
        s.lock().unwrap().low_mode = Some((1, "before"));
        let r = run(&mut a);
        assert_eq!(r.outcome, Outcome::RestoredAfterFailure);
        assert_eq!(
            a.low_errors[0].1.kind,
            crate::child_memory::ErrorKind::RefusedBeforeWrite
        );
        clean(&s);
        assert_eq!(
            s.lock().unwrap().trace,
            ["+memory", "create", "remove", "-memory"]
        );
    }
    #[test]
    fn integrated_competing_low_is_not_overwritten() {
        let (mut a, s, _) = adapter();
        s.lock().unwrap().compete_low = true;
        let r = run(&mut a);
        assert_eq!(r.outcome, Outcome::Conflict);
        assert_eq!(
            a.low_errors[0].1.kind,
            crate::child_memory::ErrorKind::CompetingValue
        );
        assert!(!a.session_observation().memory_low_unresolved);
        assert_eq!(s.lock().unwrap().low, Ok(12288));
        assert_eq!(s.lock().unwrap().trace, ["+memory", "create"]);
    }
    #[test]
    fn integrated_low_restoration_failure_retains_cause_and_child() {
        let (mut a, s, _) = adapter();
        s.lock().unwrap().low_mode = Some((2, "error"));
        let r = run(&mut a);
        assert_eq!(r.outcome, Outcome::Ambiguous);
        assert_eq!(r.failed_transition, Some(Stage::RestoreLow));
        let e = &a.low_errors[0].1;
        assert_eq!(e.kind, crate::child_memory::ErrorKind::RestorationFailure);
        assert_eq!(
            e.cause.as_ref().unwrap().kind,
            crate::child_memory::ErrorKind::WriteOutcomeAmbiguous
        );
        assert!(a.session_observation().memory_low_unresolved);
        assert_eq!(
            s.lock().unwrap().trace,
            ["+memory", "create", "low:8192", "low:4096"]
        );
    }
    #[test]
    fn integrated_child_cleanup_failure_blocks_parent_restoration() {
        let (mut a, s, _) = adapter();
        s.lock().unwrap().fail_remove = true;
        let r = run(&mut a);
        assert_eq!(r.outcome, Outcome::Ambiguous);
        assert_eq!(r.failed_transition, Some(Stage::RemoveChild));
        assert!(a.session_observation().child_unresolved);
        assert_eq!(s.lock().unwrap().child, Some(CHILD));
        assert_eq!(
            s.lock().unwrap().trace,
            ["+memory", "create", "low:8192", "low:4096", "remove"]
        );
    }
    #[test]
    fn integrated_controller_restoration_refuses_with_child() {
        let (mut a, s, _) = adapter();
        let r = coordinator::run(&mut a, true, 8192, |stage| stage == Stage::RemoveChild);
        assert_eq!(r.outcome, Outcome::RestoredAfterFailure);
        clean(&s);
        let (mut a, s, _) = adapter();
        let r = coordinator::run(&mut a, true, 8192, |stage| {
            if stage == Stage::RemoveChild {
                s.lock().unwrap().fail_remove = true;
            }
            false
        });
        assert_eq!(r.outcome, Outcome::Ambiguous);
        let original = BTreeSet::from(["cpu".into(), "pids".into()]);
        assert!(a
            .restore_controllers(Stage::RecoveryRestoreControllers, &original)
            .is_err());
        assert!(!s.lock().unwrap().trace.iter().any(|x| x == "-memory"));
    }
    #[test]
    fn integrated_cancellation_runs_existing_reverse_recovery() {
        for cancelled in [
            Stage::Authorization,
            Stage::Binding,
            Stage::CaptureControllers,
            Stage::Activate,
            Stage::VerifyActivation,
            Stage::CreateChild,
            Stage::VerifyChild,
            Stage::CaptureLow,
            Stage::ApplyLow,
            Stage::VerifyApplied,
            Stage::RestoreLow,
            Stage::VerifyRestored,
            Stage::VerifyEmpty,
            Stage::RemoveChild,
            Stage::RestoreControllers,
            Stage::VerifyFinal,
        ] {
            let (mut a, s, _) = adapter();
            let r = coordinator::run(&mut a, true, 8192, |stage| stage == cancelled);
            assert_eq!(r.failed_transition, Some(cancelled));
            assert_ne!(r.outcome, Outcome::Complete);
            clean(&s);
            if matches!(cancelled, Stage::VerifyApplied | Stage::RestoreLow) {
                assert_eq!(r.outcome, Outcome::RestoredAfterFailure);
                assert_eq!(r.low_restoration_verified, Certainty::Yes);
                assert_eq!(r.child_remains, Certainty::No);
                assert_eq!(
                    s.lock().unwrap().trace,
                    ["+memory", "create", "low:8192", "low:4096", "remove", "-memory"]
                );
                let stages: Vec<_> = a.mutation_evidence.iter().map(|(s, _)| *s).collect();
                assert!(stages.contains(&Stage::RecoveryRemoveChild));
                assert!(stages.contains(&Stage::RecoveryRestoreControllers));
            }
        }
    }
    #[test]
    fn integrated_controller_restoration_write_failure_is_ambiguous() {
        let (mut a, s, _) = adapter();
        let r = coordinator::run(&mut a, true, 8192, |stage| {
            if stage == Stage::RestoreControllers {
                s.lock().unwrap().write_mode = WriteMode::ErrorAfterApply;
            }
            false
        });
        assert_eq!(r.outcome, Outcome::Ambiguous);
        assert_eq!(r.failed_transition, Some(Stage::RestoreControllers));
        assert!(a.session_observation().activation_unresolved);
        assert_eq!(
            a.mutation_evidence.last().unwrap().1.status,
            MutationStatus::AmbiguousMutation
        );
        assert_eq!(
            s.lock().unwrap().trace,
            ["+memory", "create", "low:8192", "low:4096", "remove", "-memory"]
        );
    }
    #[test]
    fn integrated_typed_controller_refusal_and_child_cleanup_guard() {
        let (mut a, s, _) = adapter();
        let original = BTreeSet::from(["cpu".into(), "pids".into()]);
        a.activate(&original).unwrap();
        a.create_child().unwrap();
        assert_eq!(
            a.restore_controllers(Stage::RestoreControllers, &original)
                .unwrap_err()
                .kind,
            FaultKind::Before
        );
        assert_eq!(
            a.mutation_evidence.last().unwrap().1.status,
            MutationStatus::CleanupRequired
        );
        assert_eq!(s.lock().unwrap().trace, ["+memory", "create"]);
        let (mut a, s, _) = adapter();
        let r = coordinator::run(&mut a, true, 8192, |stage| {
            if stage == Stage::RestoreControllers {
                s.lock().unwrap().enabled = "cpu memory".into();
            }
            false
        });
        assert_ne!(r.outcome, Outcome::Complete);
        let outcome = &a.mutation_evidence.last().unwrap().1;
        assert_eq!(outcome.status, MutationStatus::RestorationFailure);
        assert_eq!(outcome.mutation_attempted, Some(false));
        assert!(a.session_observation().activation_unresolved);
        assert_eq!(
            s.lock().unwrap().trace,
            ["+memory", "create", "low:8192", "low:4096", "remove"]
        );
    }
    #[test]
    fn integrated_recovery_failures_stop_reverse_cleanup() {
        for mode in ["low", "child", "controller"] {
            let (mut a, s, _) = adapter();
            let r = coordinator::run(&mut a, true, 8192, |stage| {
                if stage == Stage::VerifyApplied {
                    let mut state = s.lock().unwrap();
                    match mode {
                        "low" => state.low_mode = Some((2, "error")),
                        "child" => state.fail_remove = true,
                        _ => state.write_mode = WriteMode::ErrorAfterApply,
                    };
                    return true;
                }
                false
            });
            assert_eq!(r.outcome, Outcome::Ambiguous);
            assert_eq!(
                r.recovery_failed_transition,
                Some(match mode {
                    "low" => Stage::RecoveryRestoreLow,
                    "child" => Stage::RecoveryRemoveChild,
                    _ => Stage::RecoveryRestoreControllers,
                })
            );
            let state = s.lock().unwrap();
            if mode != "controller" {
                assert!(!state.trace.iter().any(|x| x == "-memory"));
                assert_eq!(state.child, Some(CHILD));
            }
            if mode == "low" {
                assert_eq!(state.trace, ["+memory", "create", "low:8192", "low:4096"]);
            }
        }
    }
}
