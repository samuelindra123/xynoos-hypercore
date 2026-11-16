//! Guarded controller activation and restoration. Checks and writes are not atomic;
//! recorded state is in memory only and cannot provide crash recovery.
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentIdentity {
    pub device: u64,
    pub inode: u64,
}

/// All reads and writes must be relative to the same verified directory.
/// A backend must reject replacement or loss of the designated unit binding.
trait ControllerBackend {
    fn verify_unit(&mut self) -> Result<ParentIdentity, String>;
    fn processes(&mut self) -> Result<String, String>;
    fn available(&mut self) -> Result<String, String>;
    fn enabled(&mut self) -> Result<String, String>;
    fn enable_memory(&mut self) -> Result<(), String>;
    fn disable_memory(&mut self) -> Result<(), String>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Activation {
    /// The verified pre-activation state, retained for a future guarded teardown.
    Enabled { original_controllers: Vec<String> },
}

fn controllers(value: &str) -> Result<BTreeSet<&str>, String> {
    let set: BTreeSet<_> = value.split_whitespace().collect();
    if set.iter().any(|c| !["cpu", "memory", "pids"].contains(c)) {
        return Err("unexpected enabled controllers".into());
    }
    Ok(set)
}

/// Enables memory only on a backend which independently verifies its unit.
/// A failed write or read-back is reported as ambiguous; there is no blind retry.
#[cfg(test)]
fn activate_memory<B: ControllerBackend>(
    backend: &mut B,
    authorized: bool,
) -> Result<Activation, String> {
    activate_memory_expected(backend, authorized, None)
}
fn activate_memory_expected<B: ControllerBackend>(
    backend: &mut B,
    authorized: bool,
    expected_original: Option<&[String]>,
) -> Result<Activation, String> {
    if !authorized {
        return Err("explicit authorization required".into());
    }
    let identity = backend.verify_unit()?;
    if !backend.processes()?.trim().is_empty() {
        return Err("parent populated".into());
    }
    if !backend
        .available()?
        .split_whitespace()
        .any(|c| c == "memory")
    {
        return Err("memory controller unavailable".into());
    }
    let before = backend.enabled()?;
    let before_set = controllers(&before)?;
    if let Some(expected) = expected_original {
        if before_set
            .iter()
            .copied()
            .ne(expected.iter().map(String::as_str))
        {
            return Err("controller state changed since inspection".into());
        }
    }
    if before_set.contains("memory") {
        return Err("memory already enabled; unexpected initial state".into());
    }
    if backend.verify_unit()? != identity || !backend.processes()?.trim().is_empty() {
        return Err("unit identity or population changed".into());
    }
    if controllers(&backend.enabled()?)? != before_set {
        return Err("enabled controllers changed before activation".into());
    }
    let original_description = before_set.iter().copied().collect::<Vec<_>>().join(" ");
    if let Err(e) = backend.enable_memory() {
        return Err(format!("activation write failed; state ambiguous; original controllers [{original_description}]: {e}"));
    }
    if backend
        .verify_unit()
        .map_err(|e| format!("unit verification after activation failed; state ambiguous; original controllers [{original_description}]: {e}"))?
        != identity
    {
        return Err(format!("unit changed after activation; state ambiguous; original controllers [{original_description}]"));
    }
    let after = backend
        .enabled()
        .map_err(|e| format!("activation read-back failed; state ambiguous; original controllers [{original_description}]: {e}"))?;
    let after_set = controllers(&after)
        .map_err(|e| format!("invalid activation read-back; state ambiguous; original controllers [{original_description}]: {e}"))?;
    let original_controllers = before_set.iter().map(|s| (*s).to_owned()).collect();
    let mut expected = before_set;
    expected.insert("memory");
    if after_set != expected {
        return Err(format!("activation read-back mismatch; state ambiguous; original controllers [{original_description}]"));
    }
    Ok(Activation::Enabled {
        original_controllers,
    })
}

/// Restore only the state recorded by a successful activation. No retry is made on failure.
fn restore_memory<B: ControllerBackend>(
    backend: &mut B,
    activation: &Activation,
    authorized: bool,
) -> Result<(), String> {
    if !authorized {
        return Err("explicit authorization required".into());
    }
    let Activation::Enabled {
        original_controllers,
    } = activation;
    let original_text = original_controllers.join(" ");
    let original = controllers(&original_text)?;
    if original.contains("memory") {
        return Err("invalid original controller state".into());
    }
    let mut expected = original.clone();
    expected.insert("memory");
    let id = backend.verify_unit()?;
    if !backend.processes()?.trim().is_empty() || controllers(&backend.enabled()?)? != expected {
        return Err("cleanup conflict: population or controllers changed".into());
    }
    if backend.verify_unit()? != id
        || !backend.processes()?.trim().is_empty()
        || controllers(&backend.enabled()?)? != expected
    {
        return Err("cleanup identity or state changed".into());
    }
    backend
        .disable_memory()
        .map_err(|e| format!("rollback write failed; state ambiguous: {e}"))?;
    if backend
        .verify_unit()
        .map_err(|e| format!("rollback identity failed; state ambiguous: {e}"))?
        != id
        || controllers(
            &backend
                .enabled()
                .map_err(|e| format!("rollback read failed; state ambiguous: {e}"))?,
        )
        .map_err(|e| format!("rollback read invalid; state ambiguous: {e}"))?
            != original
    {
        return Err("rollback verification failed; state ambiguous".into());
    }
    Ok(())
}

// Private substitution boundary: production constructors always install DescriptorIo.
// Neither observation evidence nor a test implementation can authorize a live binding.
mod session_io;
use session_io::{DescriptorIo, SessionIo};
mod session_adapter;
pub use session_adapter::ControllerSessionAdapter;

/// Linux implementation bound to the live transient service that contains this process.
/// The caller must explicitly authorize each activation and restoration call as well.
struct LinuxController {
    io: Box<dyn SessionIo>,
    authorized: bool,
    child_unresolved: bool,
}

#[cfg(test)]
mod session_io_tests;
impl LinuxController {
    fn new(binding: crate::systemd_identity::Binding, authorized: bool) -> Result<Self, String> {
        if !authorized {
            return Err("explicit authorization required".into());
        }
        binding.revalidate()?;
        Ok(Self {
            io: Box::new(DescriptorIo::new(binding)),
            authorized,
            child_unresolved: false,
        })
    }
    fn write(&mut self, command: &[u8]) -> Result<(), String> {
        if !self.authorized {
            return Err("explicit authorization required".into());
        }
        self.io.revalidate()?;
        self.io.write_controller(command)
    }
    fn read(&self, name: &std::ffi::CStr) -> Result<String, String> {
        self.io.revalidate()?;
        self.io.read_parent(name)
    }
}
/// Read-only initialization of the same bound controller backend used by a session.
/// It does not grant authority or expose mutation methods.
pub struct ControllerInspectionSession {
    backend: LinuxController,
}
#[derive(serde::Serialize)]
pub struct ControllerReadiness {
    pub parent_device: u64,
    pub parent_inode: u64,
    pub available_controllers: Vec<String>,
    pub enabled_controllers: Vec<String>,
    pub parent_empty: bool,
    pub ready: bool,
}
fn inspect_readiness<B: ControllerBackend>(backend: &mut B) -> Result<ControllerReadiness, String> {
    let id = backend.verify_unit()?;
    if !backend.processes()?.trim().is_empty() {
        return Err("parent populated".into());
    }
    let available_text = backend.available()?;
    let available: BTreeSet<_> = available_text.split_whitespace().collect();
    if !available.contains("memory") {
        return Err("memory controller unavailable".into());
    }
    let enabled_text = backend.enabled()?;
    let enabled = controllers(&enabled_text)?;
    if enabled.contains("memory") {
        return Err("memory already enabled; unexpected initial state".into());
    }
    if backend.verify_unit()? != id || !backend.processes()?.trim().is_empty() {
        return Err("unit identity or population changed".into());
    }
    let available_again = backend.available()?;
    let enabled_again = backend.enabled()?;
    if available_again.split_whitespace().collect::<BTreeSet<_>>() != available
        || controllers(&enabled_again)? != enabled
    {
        return Err("controller state changed during inspection".into());
    }
    backend.verify_unit().and_then(|current| {
        if current == id {
            Ok(())
        } else {
            Err("unit identity changed".into())
        }
    })?;
    Ok(ControllerReadiness {
        parent_device: id.device,
        parent_inode: id.inode,
        available_controllers: available.iter().map(|s| (*s).to_owned()).collect(),
        enabled_controllers: enabled.iter().map(|s| (*s).to_owned()).collect(),
        parent_empty: true,
        ready: true,
    })
}
impl ControllerInspectionSession {
    pub fn new(binding: crate::systemd_identity::Binding) -> Result<Self, String> {
        binding.revalidate()?;
        Ok(Self {
            backend: LinuxController {
                io: Box::new(DescriptorIo::new(binding)),
                authorized: false,
                child_unresolved: false,
            },
        })
    }
    pub fn inspect(&mut self) -> Result<ControllerReadiness, String> {
        inspect_readiness(&mut self.backend)
    }
    pub fn binding_diagnostic(&self) -> serde_json::Value {
        self.backend.io.diagnostic()
    }
}
/// The session operation whose effect is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOperation {
    Activate,
    RestoreControllers,
    CreateChild,
    RemoveChild,
}

/// Certainty of a session mutation. An ambiguous result must never be retried blindly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationStatus {
    RefusedBeforeMutation,
    AppliedAndVerified,
    Conflict,
    AmbiguousMutation,
    CleanupRequired,
    RestorationFailure,
}

/// Values verified or expected by a session operation. Missing values are unknown.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MutationState {
    pub controllers: Option<Vec<String>>,
    pub child: Option<crate::child_memory::ChildIdentity>,
    /// Whether the registered child exists, if verified.
    pub child_present: Option<bool>,
}

/// Underlying diagnostic supplied by the operation that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationError {
    pub operation: MutationOperation,
    pub detail: String,
}

/// A mutation report. `mutation_attempted` is None when the underlying API cannot
/// determine whether it reached the syscall. `verified` refers to the expected state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationOutcome {
    pub operation: MutationOperation,
    pub status: MutationStatus,
    pub last_verified: Option<Box<MutationState>>,
    pub expected: Option<Box<MutationState>>,
    pub observed: Option<Box<MutationState>>,
    pub mutation_attempted: Option<bool>,
    pub verified: bool,
    pub error: Option<MutationError>,
}
impl std::fmt::Display for MutationOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {:?}", self.operation, self.status)?;
        if let Some(error) = &self.error {
            write!(f, ": {}", error.detail)?;
        }
        Ok(())
    }
}
impl std::error::Error for MutationOutcome {}

fn mutation_result(
    operation: MutationOperation,
    status: MutationStatus,
    attempted: Option<bool>,
    last_verified: Option<MutationState>,
    expected: Option<MutationState>,
    observed: Option<MutationState>,
    error: Option<String>,
) -> MutationOutcome {
    MutationOutcome {
        operation,
        status,
        last_verified: last_verified.map(Box::new),
        expected: expected.map(Box::new),
        observed: observed.map(Box::new),
        mutation_attempted: attempted,
        verified: status == MutationStatus::AppliedAndVerified,
        error: error.map(|detail| MutationError { operation, detail }),
    }
}

/// An activation held in memory until one explicit restoration attempt.
/// Dropping this value never writes to the kernel.
/// Production consumers cannot invoke the private mutation operations:
/// ```compile_fail
/// use xynoos_linux_control::{controller::ControllerSession, child_memory::ChildIdentity};
/// fn bypass(s: &mut ControllerSession, child: ChildIdentity) { s.activate_prepared(); }
/// ```
/// ```compile_fail
/// use xynoos_linux_control::{controller::ControllerSession, child_memory::ChildIdentity};
/// fn bypass(s: &mut ControllerSession, child: ChildIdentity) { s.restore(true); }
/// ```
/// ```compile_fail
/// use xynoos_linux_control::{controller::ControllerSession, child_memory::ChildIdentity};
/// fn bypass(s: &mut ControllerSession, child: ChildIdentity) { s.create_empty_child(); }
/// ```
/// ```compile_fail
/// use xynoos_linux_control::{controller::ControllerSession, child_memory::ChildIdentity};
/// fn bypass(s: &mut ControllerSession, child: ChildIdentity) { s.remove_empty_child(); }
/// ```
/// ```compile_fail
/// use xynoos_linux_control::{controller::ControllerSession, child_memory::ChildIdentity};
/// fn bypass(s: &mut ControllerSession, child: ChildIdentity) { s.change_child_memory_low(child, 0, 4096, true, false); }
/// ```
/// Nor construct the private permit or fixture capability:
/// ```compile_fail
/// use xynoos_linux_control::controller::session_adapter::MutationPermit;
/// ```
/// ```compile_fail
/// use xynoos_linux_control::controller::session_adapter::fixture_permit;
/// ```
/// The former convenience activation entry point is absent:
/// ```compile_fail
/// use xynoos_linux_control::controller::ControllerSession;
/// let _ = ControllerSession::activate;
/// ```
pub struct ControllerSession {
    backend: LinuxController,
    activation: Option<Activation>,
    original_before_attempt: Option<Vec<String>>,
    activation_unresolved: bool,
    low_unresolved: bool,
}

/// Evidence is read sequentially and is never a mutation authorization token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionObservation {
    /// Identity captured at binding time; current only when both binding checks pass.
    pub identity: crate::systemd_identity::BoundIdentity,
    pub binding_before: Result<(), String>,
    pub binding_after: Result<(), String>,
    pub parent_population: Result<Vec<u32>, String>,
    pub manager_population: Result<Vec<u32>, String>,
    pub registered_child: Option<ChildObservation>,
    pub available_controllers: Result<BTreeSet<String>, String>,
    pub enabled_controllers: Result<BTreeSet<String>, String>,
    pub activation: ObservedActivation,
    pub activation_unresolved: bool,
    pub child_unresolved: bool,
    pub memory_low_unresolved: bool,
    pub completeness: ObservationCompleteness,
    pub validity: ObservationValidity,
    pub issues: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildObservation {
    pub identity: crate::child_memory::ChildIdentity,
    pub population: Result<Vec<u32>, String>,
    pub memory_low: Result<u64, String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationCompleteness {
    Complete,
    Incomplete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationValidity {
    Valid,
    Invalid,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedActivation {
    NotActive,
    Active,
    Unresolved,
}

fn population(text: String) -> Result<Vec<u32>, String> {
    let mut pids = BTreeSet::new();
    for word in text.split_whitespace() {
        let pid = word
            .parse::<u32>()
            .map_err(|_| "invalid process population")?;
        if pid == 0 || !pids.insert(pid) {
            return Err("invalid process population".into());
        }
    }
    Ok(pids.into_iter().collect())
}
fn controller_set(text: String) -> Result<BTreeSet<String>, String> {
    let mut set = BTreeSet::new();
    for word in text.split_whitespace() {
        if !word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || !set.insert(word.to_owned())
        {
            return Err("invalid controller list".into());
        }
    }
    Ok(set)
}
fn activate_with_tracking<B: ControllerBackend>(
    backend: &mut B,
    original: &mut Option<Vec<String>>,
    unresolved: &mut bool,
) -> Result<Activation, String> {
    if *unresolved || original.is_some() {
        return Err("activation already attempted".into());
    }
    let readiness = inspect_readiness(backend)?;
    *original = Some(readiness.enabled_controllers);
    *unresolved = true;
    let activation = activate_memory_expected(backend, true, original.as_deref())?;
    let Activation::Enabled {
        original_controllers,
    } = &activation;
    if Some(original_controllers) != original.as_ref() {
        return Err(
            "controller state changed between inspection and activation; state unresolved".into(),
        );
    }
    *unresolved = false;
    Ok(activation)
}
// Records entry to the backend mutation method; backend errors after entry may have
// changed kernel state, even when they report an error.
struct TrackedController<'a, B> {
    backend: &'a mut B,
    attempted: bool,
    observed_after_write: Option<Vec<String>>,
}
impl<B: ControllerBackend> ControllerBackend for TrackedController<'_, B> {
    fn verify_unit(&mut self) -> Result<ParentIdentity, String> {
        self.backend.verify_unit()
    }
    fn processes(&mut self) -> Result<String, String> {
        self.backend.processes()
    }
    fn available(&mut self) -> Result<String, String> {
        self.backend.available()
    }
    fn enabled(&mut self) -> Result<String, String> {
        let value = self.backend.enabled()?;
        if self.attempted {
            let mut observed: Vec<String> = value.split_whitespace().map(str::to_owned).collect();
            observed.sort();
            self.observed_after_write = Some(observed);
        }
        Ok(value)
    }
    fn enable_memory(&mut self) -> Result<(), String> {
        self.attempted = true;
        self.backend.enable_memory()
    }
    fn disable_memory(&mut self) -> Result<(), String> {
        self.attempted = true;
        self.backend.disable_memory()
    }
}
fn activate_typed<B: ControllerBackend>(
    backend: &mut B,
    activation: &mut Option<Activation>,
    original: &mut Option<Vec<String>>,
    unresolved: &mut bool,
) -> Result<MutationOutcome, MutationOutcome> {
    let op = MutationOperation::Activate;
    if activation.is_some() || *unresolved || original.is_some() {
        return Err(mutation_result(
            op,
            MutationStatus::RefusedBeforeMutation,
            Some(false),
            None,
            None,
            None,
            Some("activation already attempted or unresolved".into()),
        ));
    }
    let mut tracked = TrackedController {
        backend,
        attempted: false,
        observed_after_write: None,
    };
    let result = activate_with_tracking(&mut tracked, original, unresolved);
    let attempted = tracked.attempted;
    let observed = tracked
        .observed_after_write
        .map(|controllers| MutationState {
            controllers: Some(controllers),
            ..Default::default()
        });
    let before = original.clone().map(|controllers| MutationState {
        controllers: Some(controllers),
        ..Default::default()
    });
    let expected = before.clone().map(|mut state| {
        state.controllers.as_mut().unwrap().push("memory".into());
        state.controllers.as_mut().unwrap().sort();
        state
    });
    match result {
        Ok(result_activation) => {
            *activation = Some(result_activation);
            Ok(mutation_result(
                op,
                MutationStatus::AppliedAndVerified,
                Some(true),
                before,
                expected.clone(),
                expected,
                None,
            ))
        }
        Err(error) => Err(mutation_result(
            op,
            if attempted {
                MutationStatus::AmbiguousMutation
            } else {
                MutationStatus::RefusedBeforeMutation
            },
            Some(attempted),
            before,
            expected,
            observed,
            Some(error),
        )),
    }
}
fn restore_typed<B: ControllerBackend>(
    backend: &mut B,
    activation: &mut Option<Activation>,
    unresolved: &mut bool,
    low_unresolved: bool,
    child_present: bool,
    child_unresolved: bool,
    authorized: bool,
) -> Result<MutationOutcome, MutationOutcome> {
    let op = MutationOperation::RestoreControllers;
    let expected = activation.as_ref().map(|activation| {
        let Activation::Enabled {
            original_controllers,
        } = activation;
        MutationState {
            controllers: Some(original_controllers.clone()),
            child_present: Some(false),
            ..Default::default()
        }
    });
    if !authorized {
        return Err(mutation_result(
            op,
            MutationStatus::RefusedBeforeMutation,
            Some(false),
            None,
            expected,
            None,
            Some("explicit authorization required".into()),
        ));
    }
    if *unresolved || low_unresolved || child_present || child_unresolved {
        return Err(mutation_result(
            op,
            MutationStatus::CleanupRequired,
            Some(false),
            None,
            expected,
            None,
            Some("child or controller state unresolved; restoration refused".into()),
        ));
    }
    let Some(activation) = activation.take() else {
        return Err(mutation_result(
            op,
            MutationStatus::RestorationFailure,
            Some(false),
            None,
            expected,
            None,
            Some("restoration already attempted".into()),
        ));
    };
    *unresolved = true;
    let mut tracked = TrackedController {
        backend,
        attempted: false,
        observed_after_write: None,
    };
    let result = restore_memory(&mut tracked, &activation, true);
    let attempted = tracked.attempted;
    let observed = tracked
        .observed_after_write
        .map(|controllers| MutationState {
            controllers: Some(controllers),
            ..Default::default()
        });
    match result {
        Ok(()) => {
            *unresolved = false;
            Ok(mutation_result(
                op,
                MutationStatus::AppliedAndVerified,
                Some(true),
                None,
                expected.clone(),
                expected,
                None,
            ))
        }
        Err(error) => Err(mutation_result(
            op,
            if attempted {
                MutationStatus::AmbiguousMutation
            } else {
                MutationStatus::RestorationFailure
            },
            Some(attempted),
            None,
            expected,
            observed,
            Some(error),
        )),
    }
}
// The same orchestration is used by the descriptor-backed implementation and fixtures.
trait RegisteredChildBackend: ControllerBackend {
    fn registered_child(&self) -> Option<crate::child_memory::ChildIdentity>;
    fn child_unresolved(&self) -> bool;
    fn child_identity_matches(&self) -> Result<bool, String>;
    fn set_child_unresolved(&mut self, value: bool);
    // Errors may follow mutation; callers latch unresolved before invoking these methods.
    fn create_registered_child(
        &mut self,
        parent: ParentIdentity,
    ) -> Result<crate::child_memory::ChildIdentity, String>;
    fn remove_registered_child(&mut self, parent: ParentIdentity) -> Result<(), String>;
}
impl RegisteredChildBackend for LinuxController {
    fn registered_child(&self) -> Option<crate::child_memory::ChildIdentity> {
        self.io.registered_child()
    }
    fn child_unresolved(&self) -> bool {
        self.child_unresolved
    }
    fn child_identity_matches(&self) -> Result<bool, String> {
        self.io.child_identity_matches()
    }
    fn set_child_unresolved(&mut self, value: bool) {
        self.child_unresolved = value;
    }
    fn create_registered_child(
        &mut self,
        parent: ParentIdentity,
    ) -> Result<crate::child_memory::ChildIdentity, String> {
        self.io.create_child(parent)
    }
    fn remove_registered_child(&mut self, parent: ParentIdentity) -> Result<(), String> {
        self.io.remove_child(parent)
    }
}
fn create_child_typed<B: RegisteredChildBackend>(
    backend: &mut B,
    activated: bool,
) -> Result<MutationOutcome, MutationOutcome> {
    let op = MutationOperation::CreateChild;
    let expected = Some(MutationState {
        child_present: Some(true),
        ..Default::default()
    });
    if !activated || backend.registered_child().is_some() || backend.child_unresolved() {
        return Err(mutation_result(
            op,
            MutationStatus::RefusedBeforeMutation,
            Some(false),
            None,
            expected,
            None,
            Some("verified activation and no registered or unresolved child required".into()),
        ));
    }
    let id = backend.verify_unit().map_err(|e| {
        mutation_result(
            op,
            MutationStatus::RefusedBeforeMutation,
            Some(false),
            None,
            expected.clone(),
            None,
            Some(e),
        )
    })?;
    if !backend
        .processes()
        .map_err(|e| {
            mutation_result(
                op,
                MutationStatus::RefusedBeforeMutation,
                Some(false),
                None,
                expected.clone(),
                None,
                Some(e),
            )
        })?
        .trim()
        .is_empty()
    {
        return Err(mutation_result(
            op,
            MutationStatus::Conflict,
            Some(false),
            None,
            expected,
            None,
            Some("parent populated".into()),
        ));
    }
    let again = backend.verify_unit().map_err(|e| {
        mutation_result(
            op,
            MutationStatus::RefusedBeforeMutation,
            Some(false),
            None,
            expected.clone(),
            None,
            Some(e),
        )
    })?;
    if id != again {
        return Err(mutation_result(
            op,
            MutationStatus::Conflict,
            Some(false),
            None,
            expected,
            None,
            Some("parent identity changed".into()),
        ));
    }
    let before = Some(MutationState {
        child_present: Some(false),
        ..Default::default()
    });
    backend.set_child_unresolved(true);
    let identity = backend.create_registered_child(id).map_err(|e| {
        mutation_result(
            op,
            MutationStatus::AmbiguousMutation,
            None,
            before.clone(),
            expected.clone(),
            None,
            Some(e),
        )
    })?;
    backend.set_child_unresolved(false);
    if let Err(e) = backend.verify_unit() {
        backend.set_child_unresolved(true);
        return Err(mutation_result(
            op,
            MutationStatus::AmbiguousMutation,
            Some(true),
            before,
            expected,
            None,
            Some(e),
        ));
    }
    let observed = Some(MutationState {
        child: Some(identity),
        child_present: Some(true),
        ..Default::default()
    });
    Ok(mutation_result(
        op,
        MutationStatus::AppliedAndVerified,
        Some(true),
        before,
        observed.clone(),
        observed,
        None,
    ))
}
fn remove_child_typed<B: RegisteredChildBackend>(
    backend: &mut B,
    low_unresolved: bool,
) -> Result<MutationOutcome, MutationOutcome> {
    let op = MutationOperation::RemoveChild;
    let expected = Some(MutationState {
        child_present: Some(false),
        ..Default::default()
    });
    if low_unresolved || backend.child_unresolved() {
        return Err(mutation_result(
            op,
            MutationStatus::CleanupRequired,
            Some(false),
            None,
            expected,
            None,
            Some("child or memory.low state unresolved; child preserved".into()),
        ));
    }
    if backend.registered_child().is_some() {
        let matches = backend.child_identity_matches().map_err(|e| {
            mutation_result(
                op,
                MutationStatus::RefusedBeforeMutation,
                Some(false),
                None,
                expected.clone(),
                None,
                Some(e),
            )
        })?;
        if !matches {
            return Err(mutation_result(
                op,
                MutationStatus::Conflict,
                Some(false),
                None,
                expected,
                None,
                Some("registered child identity changed".into()),
            ));
        }
    }
    let id = backend.verify_unit().map_err(|e| {
        mutation_result(
            op,
            MutationStatus::RefusedBeforeMutation,
            Some(false),
            None,
            expected.clone(),
            None,
            Some(e),
        )
    })?;
    let child = backend.registered_child().ok_or_else(|| {
        mutation_result(
            op,
            MutationStatus::RefusedBeforeMutation,
            Some(false),
            None,
            expected.clone(),
            None,
            Some("no registered child".into()),
        )
    })?;
    let before = Some(MutationState {
        child: Some(child),
        child_present: Some(true),
        ..Default::default()
    });
    backend.set_child_unresolved(true);
    backend.remove_registered_child(id).map_err(|e| {
        mutation_result(
            op,
            MutationStatus::AmbiguousMutation,
            None,
            before.clone(),
            expected.clone(),
            None,
            Some(e),
        )
    })?;
    backend.set_child_unresolved(false);
    if let Err(e) = backend.verify_unit() {
        backend.set_child_unresolved(true);
        return Err(mutation_result(
            op,
            MutationStatus::AmbiguousMutation,
            Some(true),
            before,
            expected,
            None,
            Some(e),
        ));
    }
    Ok(mutation_result(
        op,
        MutationStatus::AppliedAndVerified,
        Some(true),
        before,
        expected.clone(),
        expected,
        None,
    ))
}
impl ControllerSession {
    /// Collect diagnostic evidence without authorizing, attempting, or retrying a mutation.
    /// Individual reads are sequential; a complete snapshot is not an atomic kernel view.
    pub fn observe(&mut self) -> SessionObservation {
        let io = &self.backend.io;
        let identity = io.bound_identity();
        let binding_before = io.revalidate();
        let parent_population = io.read_parent(c"cgroup.procs").and_then(population);
        let manager_population = io.read_manager(c"cgroup.procs").and_then(population);
        let available_controllers = io
            .read_parent(c"cgroup.controllers")
            .and_then(controller_set);
        let enabled_controllers = io
            .read_parent(c"cgroup.subtree_control")
            .and_then(controller_set);
        let parent_stat = io.read_parent(c"cgroup.stat");
        let manager_stat = io.read_manager(c"cgroup.stat");
        let registered_child = io.registered_child().map(|identity| ChildObservation {
            identity,
            population: io.child_population().and_then(population),
            memory_low: io.read_low(),
        });
        let available_again = io
            .read_parent(c"cgroup.controllers")
            .and_then(controller_set);
        let enabled_again = io
            .read_parent(c"cgroup.subtree_control")
            .and_then(controller_set);
        let binding_after = io.revalidate();
        let mut issues = Vec::new();
        let mut incomplete = false;
        let mut invalid = false;
        for (label, result) in [
            ("binding before", &binding_before),
            ("binding after", &binding_after),
        ] {
            if let Err(e) = result {
                issues.push(format!("{label}: {e}"));
                invalid = true;
                incomplete = true;
            }
        }
        for (label, error) in [
            ("parent population", parent_population.as_ref().err()),
            ("manager population", manager_population.as_ref().err()),
            (
                "available controllers",
                available_controllers.as_ref().err(),
            ),
            ("enabled controllers", enabled_controllers.as_ref().err()),
            ("parent hierarchy", parent_stat.as_ref().err()),
            ("manager hierarchy", manager_stat.as_ref().err()),
            (
                "available controllers reread",
                available_again.as_ref().err(),
            ),
            ("enabled controllers reread", enabled_again.as_ref().err()),
        ] {
            if let Some(e) = error {
                issues.push(format!("{label}: {e}"));
                incomplete = true;
            }
        }
        if parent_population.as_ref().is_ok_and(|p| !p.is_empty())
            || manager_population
                .as_ref()
                .is_ok_and(|p| p.as_slice() != [identity.manager_pid])
        {
            issues.push("unexpected process population".into());
            invalid = true;
        }
        if let Some(child) = &registered_child {
            for (label, error) in [
                ("child population", child.population.as_ref().err()),
                ("memory.low", child.memory_low.as_ref().err()),
            ] {
                if let Some(e) = error {
                    issues.push(format!("{label}: {e}"));
                    incomplete = true;
                }
            }
            if child.population.as_ref().is_ok_and(|p| !p.is_empty()) {
                issues.push("child populated".into());
                invalid = true;
            }
        }
        if let (Ok(parent), Ok(manager), Ok(procs)) =
            (&parent_stat, &manager_stat, &manager_population)
        {
            let procs = procs
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            if let Err(e) = validate_hierarchy(
                parent,
                manager,
                &procs,
                identity.manager_pid,
                registered_child.is_some(),
            ) {
                issues.push(e);
                invalid = true;
            }
        }
        if let (Ok(a), Ok(b)) = (&available_controllers, &available_again) {
            if a != b {
                issues.push("available controllers changed during observation".into());
                invalid = true;
            }
        }
        if let (Ok(a), Ok(b)) = (&enabled_controllers, &enabled_again) {
            if a != b {
                issues.push("enabled controllers changed during observation".into());
                invalid = true;
            }
        }
        if let (
            Some(Activation::Enabled {
                original_controllers,
            }),
            Ok(enabled),
        ) = (&self.activation, &enabled_controllers)
        {
            let mut expected: BTreeSet<_> = original_controllers.iter().cloned().collect();
            expected.insert("memory".into());
            if *enabled != expected {
                issues.push("controller state conflicts with activation".into());
                invalid = true;
            }
        }
        if !self.activation_unresolved
            && self.activation.is_none()
            && enabled_controllers
                .as_ref()
                .is_ok_and(|set| set.contains("memory"))
        {
            issues.push("memory enabled without a verified session activation".into());
            invalid = true;
        }
        if let (Ok(available), Ok(enabled)) = (&available_controllers, &enabled_controllers) {
            if !enabled.is_subset(available) || !available.contains("memory") {
                issues.push("controller availability conflicts with session state".into());
                invalid = true;
            }
        }
        if self.activation_unresolved || self.backend.child_unresolved || self.low_unresolved {
            issues.push("mutation outcome unresolved; observation does not permit retry".into());
            incomplete = true;
        }
        SessionObservation {
            identity,
            binding_before,
            binding_after,
            parent_population,
            manager_population,
            registered_child,
            available_controllers,
            enabled_controllers,
            activation: if self.activation_unresolved {
                ObservedActivation::Unresolved
            } else if self.activation.is_some() {
                ObservedActivation::Active
            } else {
                ObservedActivation::NotActive
            },
            activation_unresolved: self.activation_unresolved,
            child_unresolved: self.backend.child_unresolved,
            memory_low_unresolved: self.low_unresolved,
            completeness: if incomplete {
                ObservationCompleteness::Incomplete
            } else {
                ObservationCompleteness::Complete
            },
            validity: if invalid {
                ObservationValidity::Invalid
            } else if incomplete {
                ObservationValidity::Unknown
            } else {
                ObservationValidity::Valid
            },
            issues,
        }
    }

    /// Check the shared mutation preconditions independently of any observation.
    /// Operation-specific checks and explicit authorization still apply.
    pub fn verify_for_mutation(&mut self) -> Result<ParentIdentity, String> {
        if self.activation_unresolved || self.low_unresolved {
            return Err("mutation state unresolved".into());
        }
        self.backend.verify_unit()
    }
    /// Hold the validated binding before any controller write is attempted.
    pub fn prepare(
        binding: crate::systemd_identity::Binding,
        authorized: bool,
    ) -> Result<Self, String> {
        Ok(Self {
            backend: LinuxController::new(binding, authorized)?,
            activation: None,
            original_before_attempt: None,
            activation_unresolved: false,
            low_unresolved: false,
        })
    }
    /// Capture initial state before activation. A failed attempt remains unresolved;
    /// no automatic retry or rollback is performed on an ambiguous write.
    fn activate_prepared(
        &mut self,
        _permit: session_adapter::MutationPermit,
    ) -> Result<MutationOutcome, MutationOutcome> {
        activate_typed(
            &mut self.backend,
            &mut self.activation,
            &mut self.original_before_attempt,
            &mut self.activation_unresolved,
        )
    }
    /// The observed pre-attempt state does not by itself authorize recovery.
    pub fn activation_diagnostic(&self) -> serde_json::Value {
        serde_json::json!({
            "binding": self.backend.io.diagnostic(),
            "original_controllers": self.original_before_attempt,
            "activation_unresolved": self.activation_unresolved,
            "child_unresolved": self.backend.child_unresolved,
            "child_registered": self.backend.registered_child().is_some(),
            "memory_low_unresolved": self.low_unresolved,
        })
    }
    pub fn original_controllers(&self) -> &[String] {
        let Some(Activation::Enabled {
            original_controllers,
        }) = &self.activation
        else {
            return &[];
        };
        original_controllers
    }
    /// Return the identity of this session's registered, validated child.
    pub fn registered_child_identity(
        &mut self,
    ) -> Result<crate::child_memory::ChildIdentity, String> {
        self.backend.verify_unit()?;
        self.backend
            .registered_child()
            .ok_or("no registered child".into())
    }
    /// Read only through the held registered child descriptor.
    pub fn read_child_memory_low(
        &mut self,
        child: crate::child_memory::ChildIdentity,
    ) -> Result<u64, crate::child_memory::MemoryLowError> {
        crate::child_memory::read(&mut SessionLow(self), child)
    }
    /// Validate a requested change. Physical memory.low writes are disabled even when authorized.
    fn change_child_memory_low(
        &mut self,
        _permit: session_adapter::MutationPermit,
        child: crate::child_memory::ChildIdentity,
        expected: u64,
        value: u64,
        authorized: bool,
        restoring: bool,
    ) -> Result<u64, crate::child_memory::MemoryLowError> {
        let mut unresolved = self.low_unresolved;
        let result = crate::child_memory::change(
            &mut SessionLow(self),
            &mut unresolved,
            child,
            expected,
            value,
            authorized,
            restoring,
        );
        self.low_unresolved = unresolved;
        result
    }
    /// A failed authorized attempt is never retried automatically.
    fn restore(
        &mut self,
        _permit: session_adapter::MutationPermit,
        authorized: bool,
    ) -> Result<MutationOutcome, MutationOutcome> {
        let child_present = self.backend.registered_child().is_some();
        let child_unresolved = self.backend.child_unresolved;
        restore_typed(
            &mut self.backend,
            &mut self.activation,
            &mut self.activation_unresolved,
            self.low_unresolved,
            child_present,
            child_unresolved,
            authorized,
        )
    }
    /// Create one empty, session-registered child. Unknown effects remain latched.
    fn create_empty_child(
        &mut self,
        _permit: session_adapter::MutationPermit,
    ) -> Result<MutationOutcome, MutationOutcome> {
        create_child_typed(
            &mut self.backend,
            self.activation.is_some() && !self.activation_unresolved,
        )
    }
    /// Inspect the registered child's inherited controller list without control writes.
    pub fn child_controllers(&mut self) -> Result<String, String> {
        let id = self.backend.verify_unit()?;
        self.backend.io.child_controllers(id)
    }
    /// Remove only the registered, still-empty child; failed cleanup blocks restoration.
    fn remove_empty_child(
        &mut self,
        _permit: session_adapter::MutationPermit,
    ) -> Result<MutationOutcome, MutationOutcome> {
        remove_child_typed(&mut self.backend, self.low_unresolved)
    }
}

struct SessionLow<'a>(&'a mut ControllerSession);
impl crate::child_memory::Access for SessionLow<'_> {
    fn verify(&mut self, child: crate::child_memory::ChildIdentity) -> Result<(), String> {
        if self.0.activation_unresolved || self.0.activation.is_none() {
            return Err("verified activation required".into());
        }
        self.0.backend.verify_unit()?;
        if self
            .0
            .backend
            .registered_child()
            .ok_or("no registered child")?
            != child
        {
            return Err("registered child identity mismatch".into());
        }
        let mut expected: BTreeSet<_> = self.0.original_controllers().iter().cloned().collect();
        expected.insert("memory".into());
        let enabled = self.0.backend.enabled()?;
        if controllers(&enabled)?
            .iter()
            .copied()
            .ne(expected.iter().map(String::as_str))
        {
            return Err("controller state changed".into());
        }
        self.0.backend.verify_unit()?;
        Ok(())
    }
    fn read(&mut self) -> Result<u64, String> {
        self.0.backend.io.read_low()
    }
    fn write(&mut self, _bytes: &[u8]) -> Result<usize, String> {
        #[cfg(test)]
        if self.0.backend.io.fixture_write_enabled() {
            return self.0.backend.io.fixture_write_low(_bytes);
        }
        Err("physical memory.low writes disabled".into())
    }
    fn prepare_write(&mut self) -> Result<(), String> {
        #[cfg(test)]
        if self.0.backend.io.fixture_write_enabled() {
            return self.0.backend.io.fixture_prepare_low();
        }
        Err("physical memory.low writes disabled".into())
    }
    fn write_enabled(&self) -> bool {
        #[cfg(test)]
        {
            self.0.backend.authorized && self.0.backend.io.fixture_write_enabled()
        }
        #[cfg(not(test))]
        {
            false
        }
    }
}
fn validate_hierarchy(
    stat: &str,
    manager_stat: &str,
    manager_procs: &str,
    pid: u32,
    child_registered: bool,
) -> Result<(), String> {
    let expected = if child_registered {
        "nr_descendants 2"
    } else {
        "nr_descendants 1"
    };
    if stat
        .lines()
        .filter(|line| line.split_whitespace().next() == Some("nr_descendants"))
        .count()
        != 1
        || !stat.lines().any(|line| line.trim() == expected)
        || manager_stat
            .lines()
            .filter(|line| line.split_whitespace().next() == Some("nr_descendants"))
            .collect::<Vec<_>>()
            != ["nr_descendants 0"]
    {
        return Err("unexpected delegated hierarchy descendants".into());
    }
    if manager_procs.trim() != pid.to_string() {
        return Err("unexpected manager subgroup population".into());
    }
    Ok(())
}
impl ControllerBackend for LinuxController {
    fn verify_unit(&mut self) -> Result<ParentIdentity, String> {
        if self.child_unresolved {
            return Err("child state unresolved".into());
        }
        self.io.revalidate()?;
        if !self.io.read_parent(c"cgroup.procs")?.trim().is_empty() {
            return Err("parent populated".into());
        }
        let stat = self.io.read_parent(c"cgroup.stat")?;
        let manager_stat = self.io.read_manager(c"cgroup.stat")?;
        let manager_procs = self.io.read_manager(c"cgroup.procs")?;
        let (device, inode) = self.io.bound_identity().parent;
        if self.registered_child().is_some() {
            self.io.verify_child(ParentIdentity { device, inode })?;
        }
        validate_hierarchy(
            &stat,
            &manager_stat,
            &manager_procs,
            std::process::id(),
            self.registered_child().is_some(),
        )?;
        Ok(ParentIdentity { device, inode })
    }
    fn processes(&mut self) -> Result<String, String> {
        self.read(c"cgroup.procs")
    }
    fn available(&mut self) -> Result<String, String> {
        self.read(c"cgroup.controllers")
    }
    fn enabled(&mut self) -> Result<String, String> {
        self.read(c"cgroup.subtree_control")
    }
    fn enable_memory(&mut self) -> Result<(), String> {
        self.write(b"+memory")
    }
    fn disable_memory(&mut self) -> Result<(), String> {
        self.write(b"-memory")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        calls: usize,
        enabled: String,
        procs: String,
        available: String,
        fail_write: bool,
        mismatch: bool,
        change_id: bool,
        writes: usize,
        enabled_reads: usize,
        competing_change: bool,
        change_after_inspection: bool,
        fail_unit_at: Option<usize>,
        populate_after_first_read: bool,
        process_reads: usize,
        child: Option<crate::child_memory::ChildIdentity>,
        child_unresolved: bool,
        fail_create: bool,
        fail_remove: bool,
        restore_mismatch: bool,
        child_replaced: bool,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                calls: 0,
                enabled: "cpu pids".into(),
                procs: String::new(),
                available: "cpu memory pids".into(),
                fail_write: false,
                mismatch: false,
                change_id: false,
                writes: 0,
                enabled_reads: 0,
                competing_change: false,
                change_after_inspection: false,
                fail_unit_at: None,
                populate_after_first_read: false,
                process_reads: 0,
                child: None,
                child_unresolved: false,
                fail_create: false,
                fail_remove: false,
                restore_mismatch: false,
                child_replaced: false,
            }
        }
    }
    impl ControllerBackend for Fixture {
        fn verify_unit(&mut self) -> Result<ParentIdentity, String> {
            self.calls += 1;
            if self.fail_unit_at == Some(self.calls) {
                return Err("injected unit or manager identity mismatch".into());
            }
            Ok(ParentIdentity {
                device: 1,
                inode: if self.change_id && self.calls > 1 {
                    2
                } else {
                    1
                },
            })
        }
        fn processes(&mut self) -> Result<String, String> {
            self.process_reads += 1;
            if self.populate_after_first_read && self.process_reads > 1 {
                return Ok("123".into());
            }
            Ok(self.procs.clone())
        }
        fn available(&mut self) -> Result<String, String> {
            Ok(self.available.clone())
        }
        fn enabled(&mut self) -> Result<String, String> {
            self.enabled_reads += 1;
            if self.competing_change && self.enabled_reads == 2 {
                self.enabled.push_str(" memory");
            }
            if self.change_after_inspection && self.enabled_reads == 3 {
                self.enabled = "cpu".into();
            }
            Ok(self.enabled.clone())
        }
        fn disable_memory(&mut self) -> Result<(), String> {
            self.writes += 1;
            if self.fail_write {
                return Err("denied".into());
            }
            if self.restore_mismatch {
                return Ok(());
            }
            self.enabled = self
                .enabled
                .split_whitespace()
                .filter(|v| *v != "memory")
                .collect::<Vec<_>>()
                .join(" ");
            Ok(())
        }
        fn enable_memory(&mut self) -> Result<(), String> {
            self.writes += 1;
            if self.fail_write {
                return Err("denied".into());
            }
            if !self.mismatch {
                self.enabled.push_str(" memory");
            }
            Ok(())
        }
    }
    impl RegisteredChildBackend for Fixture {
        fn registered_child(&self) -> Option<crate::child_memory::ChildIdentity> {
            self.child
        }
        fn child_unresolved(&self) -> bool {
            self.child_unresolved
        }
        fn child_identity_matches(&self) -> Result<bool, String> {
            Ok(!self.child_replaced)
        }
        fn set_child_unresolved(&mut self, value: bool) {
            self.child_unresolved = value;
        }
        fn create_registered_child(
            &mut self,
            _: ParentIdentity,
        ) -> Result<crate::child_memory::ChildIdentity, String> {
            let child = crate::child_memory::ChildIdentity {
                device: 1,
                inode: 9,
            };
            self.child = Some(child);
            if self.fail_create {
                return Err("injected after creation".into());
            }
            Ok(child)
        }
        fn remove_registered_child(&mut self, _: ParentIdentity) -> Result<(), String> {
            self.child = None;
            if self.fail_remove {
                return Err("injected failure after possible removal".into());
            }
            Ok(())
        }
    }
    fn child_state(
        identity: Option<crate::child_memory::ChildIdentity>,
        present: bool,
    ) -> Option<Box<MutationState>> {
        Some(Box::new(MutationState {
            child: identity,
            child_present: Some(present),
            ..Default::default()
        }))
    }
    #[test]
    fn typed_child_lifecycle_and_failures() {
        let identity = crate::child_memory::ChildIdentity {
            device: 1,
            inode: 9,
        };
        let mut f = Fixture::new();
        let refused = create_child_typed(&mut f, false).unwrap_err();
        assert_eq!(refused.status, MutationStatus::RefusedBeforeMutation);
        assert_eq!(refused.expected, child_state(None, true));
        let created = create_child_typed(&mut f, true).unwrap();
        assert_eq!(created.status, MutationStatus::AppliedAndVerified);
        assert_eq!(created.last_verified, child_state(None, false));
        assert_eq!(created.observed, child_state(Some(identity), true));
        assert_eq!(created.expected, created.observed);
        let removed = remove_child_typed(&mut f, false).unwrap();
        assert_eq!(removed.status, MutationStatus::AppliedAndVerified);
        assert_eq!(removed.last_verified, child_state(Some(identity), true));
        assert_eq!(removed.expected, child_state(None, false));
        assert_eq!(removed.observed, removed.expected);
        let mut f = Fixture::new();
        f.fail_create = true;
        let failure = create_child_typed(&mut f, true).unwrap_err();
        assert_eq!(failure.status, MutationStatus::AmbiguousMutation);
        assert_eq!(failure.last_verified, child_state(None, false));
        assert_eq!(failure.expected, child_state(None, true));
        assert_eq!(failure.observed, None);
        assert!(f.child_unresolved);
        assert_eq!(
            create_child_typed(&mut f, true).unwrap_err().status,
            MutationStatus::RefusedBeforeMutation
        );
        assert_eq!(
            remove_child_typed(&mut f, false).unwrap_err().status,
            MutationStatus::CleanupRequired
        );
        let mut f = Fixture::new();
        create_child_typed(&mut f, true).unwrap();
        f.fail_remove = true;
        let failure = remove_child_typed(&mut f, false).unwrap_err();
        assert_eq!(failure.status, MutationStatus::AmbiguousMutation);
        assert_eq!(failure.last_verified, child_state(Some(identity), true));
        assert_eq!(failure.expected, child_state(None, false));
        assert_eq!(failure.observed, None);
        assert_eq!(
            remove_child_typed(&mut f, false).unwrap_err().status,
            MutationStatus::CleanupRequired
        );
        let mut f = Fixture::new();
        create_child_typed(&mut f, true).unwrap();
        f.child_replaced = true;
        let conflict = remove_child_typed(&mut f, false).unwrap_err();
        assert_eq!(conflict.status, MutationStatus::Conflict);
        assert_eq!(conflict.expected, child_state(None, false));
        assert_eq!(f.child, Some(identity));
        let mut f = Fixture::new();
        f.change_id = true;
        let conflict = create_child_typed(&mut f, true).unwrap_err();
        assert_eq!(conflict.status, MutationStatus::Conflict);
        assert_eq!(conflict.expected, child_state(None, true));
        assert!(f.child.is_none());
    }
    #[test]
    fn typed_restoration_results_and_retry_guard() {
        for case in 0..6 {
            let mut f = Fixture::new();
            f.enabled = "cpu memory pids".into();
            f.fail_write = case == 3;
            f.restore_mismatch = case == 4;
            if case == 5 {
                f.enabled = "cpu pids".into();
            }
            let mut activation = Some(Activation::Enabled {
                original_controllers: vec!["cpu".into(), "pids".into()],
            });
            let mut unresolved = false;
            let result = restore_typed(
                &mut f,
                &mut activation,
                &mut unresolved,
                case == 2,
                false,
                false,
                case != 1,
            );
            let expected = Some(Box::new(MutationState {
                controllers: Some(vec!["cpu".into(), "pids".into()]),
                child_present: Some(false),
                ..Default::default()
            }));
            if case == 0 {
                let report = result.unwrap();
                assert_eq!(report.status, MutationStatus::AppliedAndVerified);
                assert_eq!(report.expected, expected);
                assert_eq!(report.observed, expected);
            } else {
                let report = result.unwrap_err();
                assert_eq!(
                    report.status,
                    match case {
                        1 => MutationStatus::RefusedBeforeMutation,
                        2 => MutationStatus::CleanupRequired,
                        5 => MutationStatus::RestorationFailure,
                        _ => MutationStatus::AmbiguousMutation,
                    }
                );
                assert_eq!(report.expected, expected);
                if case == 4 {
                    assert_eq!(
                        report.observed,
                        Some(Box::new(MutationState {
                            controllers: Some(vec!["cpu".into(), "memory".into(), "pids".into()]),
                            ..Default::default()
                        }))
                    );
                } else {
                    assert_eq!(report.observed, None);
                }
                if case >= 3 {
                    assert!(unresolved);
                    let writes = f.writes;
                    assert_eq!(
                        restore_typed(
                            &mut f,
                            &mut activation,
                            &mut unresolved,
                            false,
                            false,
                            false,
                            true
                        )
                        .unwrap_err()
                        .status,
                        MutationStatus::CleanupRequired
                    );
                    assert_eq!(f.writes, writes);
                }
            }
        }
    }
    #[test]
    fn typed_activation_reports_verified_refusal_and_ambiguity_without_retry() {
        for mode in 0..5 {
            let mut f = Fixture::new();
            match mode {
                1 => f.procs = "123".into(),
                2 => f.fail_write = true,
                3 => f.mismatch = true,
                4 => f.change_after_inspection = true,
                _ => (),
            }
            let mut activation = None;
            let mut original = None;
            let mut unresolved = false;
            let result = activate_typed(&mut f, &mut activation, &mut original, &mut unresolved);
            if mode == 0 {
                let report = result.unwrap();
                assert_eq!(report.status, MutationStatus::AppliedAndVerified);
                assert!(report.verified);
                assert_eq!(report.mutation_attempted, Some(true));
                assert_eq!(report.expected, report.observed);
            } else {
                let report = result.unwrap_err();
                assert!(!report.verified);
                assert_eq!(report.operation, MutationOperation::Activate);
                assert_eq!(
                    report.status,
                    if mode == 1 || mode == 4 {
                        MutationStatus::RefusedBeforeMutation
                    } else {
                        MutationStatus::AmbiguousMutation
                    }
                );
                assert!(report.error.is_some());
                let writes = f.writes;
                if unresolved {
                    let retry =
                        activate_typed(&mut f, &mut activation, &mut original, &mut unresolved)
                            .unwrap_err();
                    assert_eq!(retry.status, MutationStatus::RefusedBeforeMutation);
                    assert_eq!(f.writes, writes);
                }
            }
        }
    }
    #[test]
    fn authorized_write_and_readback() {
        let mut f = Fixture::new();
        assert_eq!(
            activate_memory(&mut f, true),
            Ok(Activation::Enabled {
                original_controllers: vec!["cpu".into(), "pids".into()],
            })
        );
        assert_eq!(f.writes, 1);
        assert!(activate_memory(&mut f, true)
            .unwrap_err()
            .contains("already enabled"));
        assert_eq!(f.writes, 1);
    }
    #[test]
    fn preconditions_never_write() {
        for case in 0..5 {
            let mut f = Fixture::new();
            let authorized = case != 0;
            match case {
                1 => f.procs = "123".into(),
                2 => f.available = "cpu".into(),
                3 => f.enabled = "io".into(),
                4 => f.change_id = true,
                _ => (),
            }
            assert!(activate_memory(&mut f, authorized).is_err());
            assert_eq!(f.writes, 0);
        }
    }
    #[test]
    fn write_and_readback_fail_closed() {
        let mut f = Fixture::new();
        f.fail_write = true;
        assert!(activate_memory(&mut f, true)
            .unwrap_err()
            .contains("ambiguous"));
        assert_eq!(f.writes, 1);
        let mut f = Fixture::new();
        f.mismatch = true;
        assert!(activate_memory(&mut f, true)
            .unwrap_err()
            .contains("mismatch"));
        assert_eq!(f.writes, 1);
    }
    #[test]
    fn competing_change_before_write_is_not_overwritten() {
        let mut f = Fixture::new();
        f.competing_change = true;
        assert!(activate_memory(&mut f, true)
            .unwrap_err()
            .contains("changed"));
        assert_eq!(f.writes, 0);
    }
    #[test]
    fn unit_or_manager_verification_failure_never_writes() {
        for call in [1, 2] {
            let mut f = Fixture::new();
            f.fail_unit_at = Some(call);
            assert!(activate_memory(&mut f, true).is_err());
            assert_eq!(f.writes, 0);
        }
    }
    #[test]
    fn newly_populated_parent_never_writes() {
        let mut f = Fixture::new();
        f.populate_after_first_read = true;
        assert!(activate_memory(&mut f, true).is_err());
        assert_eq!(f.writes, 0);
    }
    #[test]
    fn post_activation_identity_failure_is_ambiguous() {
        let mut f = Fixture::new();
        f.fail_unit_at = Some(3);
        assert!(activate_memory(&mut f, true)
            .unwrap_err()
            .contains("ambiguous"));
        assert_eq!(f.writes, 1);
    }
    #[test]
    fn guarded_restoration_and_failures() {
        let mut f = Fixture::new();
        let activation = activate_memory(&mut f, true).unwrap();
        assert!(restore_memory(&mut f, &activation, false).is_err());
        assert_eq!(f.writes, 1);
        f.enabled.push_str(" io");
        assert!(restore_memory(&mut f, &activation, true).is_err());
        assert_eq!(f.writes, 1);
        f.enabled = "cpu pids memory".into();
        f.fail_unit_at = Some(f.calls + 1);
        assert!(restore_memory(&mut f, &activation, true).is_err());
        assert_eq!(f.writes, 1);
        f.fail_unit_at = None;
        f.fail_write = true;
        assert!(restore_memory(&mut f, &activation, true)
            .unwrap_err()
            .contains("ambiguous"));
        assert_eq!(f.writes, 2);
        f.fail_write = false;
        restore_memory(&mut f, &activation, true).unwrap();
        assert_eq!(f.enabled, "cpu pids");
    }
    #[test]
    fn tracked_activation_retains_original_and_refuses_retry_after_ambiguity() {
        for mismatch in [false, true] {
            let mut f = Fixture::new();
            f.fail_write = !mismatch;
            f.mismatch = mismatch;
            let mut original = None;
            let mut unresolved = false;
            assert!(activate_with_tracking(&mut f, &mut original, &mut unresolved).is_err());
            assert_eq!(original, Some(vec!["cpu".into(), "pids".into()]));
            assert!(unresolved);
            assert_eq!(f.writes, 1);
            assert!(activate_with_tracking(&mut f, &mut original, &mut unresolved).is_err());
            assert_eq!(f.writes, 1);
        }
        let mut f = Fixture::new();
        let mut original = None;
        let mut unresolved = false;
        let activation = activate_with_tracking(&mut f, &mut original, &mut unresolved).unwrap();
        assert!(!unresolved);
        assert_eq!(original, Some(vec!["cpu".into(), "pids".into()]));
        restore_memory(&mut f, &activation, true).unwrap();
        assert_eq!(f.enabled, "cpu pids");

        let mut f = Fixture::new();
        f.change_after_inspection = true;
        let mut original = None;
        let mut unresolved = false;
        assert!(activate_with_tracking(&mut f, &mut original, &mut unresolved).is_err());
        assert!(unresolved);
        assert_eq!(f.writes, 0);
        assert_eq!(original, Some(vec!["cpu".into(), "pids".into()]));
    }
    #[test]
    fn read_only_session_readiness_refuses_changed_state_without_writes() {
        let mut f = Fixture::new();
        let readiness = inspect_readiness(&mut f).unwrap();
        assert!(readiness.ready && readiness.parent_empty);
        assert_eq!(f.writes, 0);
        for case in 0..7 {
            let mut f = Fixture::new();
            match case {
                0 => f.procs = "123".into(),
                1 => f.available = "cpu".into(),
                2 => f.enabled = "memory".into(),
                3 => f.enabled = "io".into(),
                4 => f.change_id = true,
                5 => f.populate_after_first_read = true,
                6 => f.competing_change = true,
                _ => unreachable!(),
            }
            assert!(inspect_readiness(&mut f).is_err(), "case {case}");
            assert_eq!(f.writes, 0);
        }
        for call in 1..=3 {
            let mut f = Fixture::new();
            f.fail_unit_at = Some(call);
            assert!(inspect_readiness(&mut f).is_err());
            assert_eq!(f.writes, 0);
        }
    }
    #[test]
    fn hierarchy_requires_only_manager_and_no_other_descendants() {
        assert!(
            validate_hierarchy("nr_descendants 1\n", "nr_descendants 0", "42\n", 42, false).is_ok()
        );
        assert!(
            validate_hierarchy("nr_descendants 2\n", "nr_descendants 0", "42\n", 42, true).is_ok()
        );
        assert!(
            validate_hierarchy("nr_descendants 2", "nr_descendants 0", "42", 42, false).is_err()
        );
        assert!(
            validate_hierarchy("nr_descendants 3", "nr_descendants 0", "42", 42, true).is_err()
        );
        assert!(
            validate_hierarchy("nr_descendants 2", "nr_descendants 1", "42", 42, true).is_err()
        );
        for (stat, procs) in [
            ("nr_descendants 2", "42"),
            ("nr_descendants 1", "42\n43"),
            ("nr_descendants 1\nnr_descendants 1", "42"),
            ("", "42"),
        ] {
            assert!(validate_hierarchy(stat, "nr_descendants 0", procs, 42, false).is_err());
        }
    }
}

#[cfg(test)]
mod observation_tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        path: PathBuf,
        session: ControllerSession,
    }
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "xynoos-observe-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            fs::create_dir(path.join("manager")).unwrap();
            for (name, value) in [
                ("cgroup.procs", ""),
                ("cgroup.stat", "nr_descendants 1\n"),
                ("cgroup.controllers", "cpu memory pids"),
                ("cgroup.subtree_control", "cpu pids"),
                ("manager/cgroup.procs", "42\n"),
                ("manager/cgroup.stat", "nr_descendants 0\n"),
            ] {
                fs::write(path.join(name), value).unwrap();
            }
            let binding = crate::systemd_identity::Binding::fixture(path.clone(), 42);
            let session = ControllerSession {
                backend: LinuxController {
                    io: Box::new(DescriptorIo::new(binding)),
                    authorized: false,
                    child_unresolved: false,
                },
                activation: None,
                original_before_attempt: None,
                activation_unresolved: false,
                low_unresolved: false,
            };
            Self { path, session }
        }
        fn activate(&mut self) {
            fs::write(self.path.join("cgroup.subtree_control"), "cpu memory pids").unwrap();
            self.session.activation = Some(Activation::Enabled {
                original_controllers: vec!["cpu".into(), "pids".into()],
            });
        }
        fn child(&mut self) {
            let path = self.path.join("child");
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            for (name, value) in [
                ("cgroup.procs", ""),
                ("cgroup.stat", "nr_descendants 0"),
                ("memory.low", "4096"),
            ] {
                fs::write(path.join(name), value).unwrap();
            }
            fs::write(self.path.join("cgroup.stat"), "nr_descendants 2").unwrap();
            let io = self
                .session
                .backend
                .io
                .as_any_mut()
                .downcast_mut::<DescriptorIo>()
                .unwrap();
            io.child = Some(crate::bound_child::BoundChild::fixture(
                io.binding.parent(),
                c"child",
            ));
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
    #[test]
    fn observes_lifecycle_and_child() {
        let mut f = Fixture::new();
        let before = f.session.observe();
        assert_eq!(before.validity, ObservationValidity::Valid);
        assert_eq!(before.completeness, ObservationCompleteness::Complete);
        assert_eq!(before.identity.unit_id, "fixture.service");
        assert_eq!(before.manager_population, Ok(vec![42]));
        f.activate();
        assert_eq!(f.session.observe().validity, ObservationValidity::Valid);
        f.child();
        let after = f.session.observe();
        assert_eq!(after.validity, ObservationValidity::Valid);
        assert_eq!(after.registered_child.unwrap().memory_low, Ok(4096));
        fs::write(f.path.join("cgroup.subtree_control"), "cpu pids").unwrap();
        assert_eq!(f.session.observe().validity, ObservationValidity::Invalid);
    }
    #[test]
    fn unresolved_evidence_never_authorizes_retry() {
        let mut f = Fixture::new();
        f.session.activation_unresolved = true;
        let o = f.session.observe();
        assert!(o.activation_unresolved);
        assert_eq!(o.validity, ObservationValidity::Unknown);
        assert!(f.session.verify_for_mutation().is_err());
        assert!(f
            .session
            .activate_prepared(session_adapter::fixture_permit())
            .is_err());
        f.session.activation_unresolved = false;
        f.activate();
        f.child();
        f.session.backend.child_unresolved = true;
        f.session.low_unresolved = true;
        let o = f.session.observe();
        assert!(o.child_unresolved && o.memory_low_unresolved);
        assert_eq!(o.validity, ObservationValidity::Unknown);
        assert!(f.session.verify_for_mutation().is_err());
        let child = f.session.backend.registered_child().unwrap();
        assert!(f
            .session
            .change_child_memory_low(
                session_adapter::fixture_permit(),
                child,
                4096,
                8192,
                true,
                false
            )
            .is_err());
        assert_eq!(
            fs::read_to_string(f.path.join("child/memory.low")).unwrap(),
            "4096"
        );
        assert!(f
            .session
            .remove_empty_child(session_adapter::fixture_permit())
            .is_err());
        assert!(f
            .session
            .restore(session_adapter::fixture_permit(), true)
            .is_err());
    }
    #[test]
    fn stale_hierarchy_and_failed_reads_are_visible() {
        let mut f = Fixture::new();
        fs::write(f.path.join("stale"), "").unwrap();
        let o = f.session.observe();
        assert!(o.binding_before.is_err());
        assert_eq!(o.validity, ObservationValidity::Invalid);
        fs::remove_file(f.path.join("stale")).unwrap();
        fs::write(f.path.join("cgroup.stat"), "nr_descendants 2").unwrap();
        assert_eq!(f.session.observe().validity, ObservationValidity::Invalid);
        fs::write(f.path.join("cgroup.stat"), "nr_descendants 1").unwrap();
        fs::remove_file(f.path.join("cgroup.controllers")).unwrap();
        let o = f.session.observe();
        assert!(o.available_controllers.is_err());
        assert_eq!(o.completeness, ObservationCompleteness::Incomplete);
        assert_eq!(o.validity, ObservationValidity::Unknown);
    }
    #[test]
    fn missing_low_and_populated_child_are_visible() {
        let mut f = Fixture::new();
        f.activate();
        f.child();
        fs::remove_file(f.path.join("child/memory.low")).unwrap();
        let o = f.session.observe();
        assert!(o.registered_child.unwrap().memory_low.is_err());
        assert_eq!(o.validity, ObservationValidity::Unknown);
        fs::write(f.path.join("child/cgroup.procs"), "17").unwrap();
        assert_eq!(f.session.observe().validity, ObservationValidity::Invalid);
    }
}
