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
        s.removes += 1;
        if s.fail_remove {
            return Err("injected child cleanup failure".into());
        }
        s.child = None;
        Ok(())
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
