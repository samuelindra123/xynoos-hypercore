//! Transaction orchestration with deterministic fixtures and a session adapter.
//! `controller::ControllerSessionAdapter` keeps live mutation disabled.
//! Backend observations and writes must each independently validate the held binding,
//! identities, population and exclusive delegation; this interface is not authorization.
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Stage {
    Authorization,
    Binding,
    CaptureControllers,
    Activate,
    VerifyActivation,
    CreateChild,
    VerifyChild,
    CaptureLow,
    ApplyLow,
    VerifyApplied,
    RestoreLow,
    VerifyRestored,
    VerifyEmpty,
    RemoveChild,
    RestoreControllers,
    VerifyFinal,
    RecoveryInspect,
    RecoveryRestoreLow,
    RecoveryVerifyLow,
    RecoveryRemoveChild,
    RecoveryVerifyRemoval,
    RecoveryRestoreControllers,
    RecoveryVerifyControllers,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Outcome {
    Complete,
    RestoredAfterFailure,
    Conflict,
    Ambiguous,
    CleanupRequired,
    RecoveryUnavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Certainty {
    Yes,
    No,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChildObservation {
    pub identity: String,
    pub empty: bool,
    pub descendants: u64,
    pub memory_inherited: bool,
    pub low: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Observation {
    /// Opaque identity of the validated unit invocation and held parent/manager descriptors.
    pub binding: String,
    pub parent_empty: bool,
    pub safety_satisfied: bool,
    pub available_controllers: BTreeSet<String>,
    pub controllers: BTreeSet<String>,
    pub child: Option<ChildObservation>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// The operation is known to have made no mutation.
    Before,
    /// A write or removal may have happened. No automatic retry is safe.
    Ambiguous,
    Conflict,
    Unavailable,
}
#[derive(Debug, Clone)]
pub struct Fault {
    pub kind: FaultKind,
    pub message: String,
}
impl Fault {
    pub fn new(kind: FaultKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub outcome: Outcome,
    pub last_completed: Option<Stage>,
    pub failed_transition: Option<Stage>,
    pub recovery_failed_transition: Option<Stage>,
    pub original: Option<Observation>,
    pub original_low: Option<u64>,
    pub registered_child_identity: Option<String>,
    pub current: Option<Observation>,
    pub low_restoration_verified: Certainty,
    pub child_remains: Certainty,
    pub controller_restoration_verified: Certainty,
    pub investigation: Vec<String>,
}
/// Contract for a deterministic backend. Mutation methods must fail with Ambiguous
/// if their effect is not known, and must revalidate identity and state themselves.
/// `inspect` must independently revalidate identity and enumerate the entire owned
/// hierarchy. This crate intentionally supplies no live implementation of this trait.
pub trait Backend {
    fn inspect(&mut self, stage: Stage) -> Result<Observation, Fault>;
    fn activate(&mut self, expected: &BTreeSet<String>) -> Result<(), Fault>;
    /// Returns the identity of the specifically created child, never a name prefix.
    fn create_child(&mut self) -> Result<String, Fault>;
    fn write_low(
        &mut self,
        stage: Stage,
        identity: &str,
        expected: u64,
        value: u64,
    ) -> Result<(), Fault>;
    fn remove_child(&mut self, stage: Stage, identity: &str) -> Result<(), Fault>;
    fn restore_controllers(
        &mut self,
        stage: Stage,
        original: &BTreeSet<String>,
    ) -> Result<(), Fault>;
}
#[derive(Default)]
struct Progress {
    original: Option<Observation>,
    original_low: Option<u64>,
    child: Option<String>,
    activated: bool,
    low_applied: bool,
    low_verified_restored: bool,
    child_removed: bool,
    controllers_restored: bool,
    last: Option<Stage>,
    previous: Option<Stage>,
    final_observation: Option<Observation>,
}
fn conflict(message: &str) -> Fault {
    Fault::new(FaultKind::Conflict, message)
}
fn original(p: &Progress) -> &Observation {
    p.original.as_ref().expect("original captured")
}
fn expected_active(p: &Progress) -> BTreeSet<String> {
    let mut v = original(p).controllers.clone();
    v.insert("memory".into());
    v
}
fn validate_base(p: &Progress, o: &Observation, active: bool) -> Result<(), Fault> {
    let initial = original(p);
    if o.binding != initial.binding || !o.parent_empty {
        return Err(conflict("binding changed or parent populated"));
    }
    if o.available_controllers != initial.available_controllers {
        return Err(conflict("available controller set changed"));
    }
    let expected = if active {
        expected_active(p)
    } else {
        initial.controllers.clone()
    };
    if o.controllers != expected {
        return Err(conflict("parent controller state changed"));
    }
    Ok(())
}
fn validate_child<'a>(p: &Progress, o: &'a Observation) -> Result<&'a ChildObservation, Fault> {
    let c = o
        .child
        .as_ref()
        .ok_or_else(|| conflict("registered child missing"))?;
    if Some(&c.identity) != p.child.as_ref()
        || !c.empty
        || c.descendants != 0
        || !c.memory_inherited
    {
        return Err(conflict(
            "child identity, population or descendants changed",
        ));
    }
    Ok(c)
}
fn ensure_low(c: &ChildObservation, value: u64) -> Result<(), Fault> {
    if c.low != Some(value) {
        return Err(conflict("memory.low unavailable or changed"));
    }
    Ok(())
}
fn no_child(o: &Observation) -> Result<(), Fault> {
    if o.child.is_some() {
        Err(conflict("unexpected child"))
    } else {
        Ok(())
    }
}
// A telemetry refusal is known to precede mutation. Keep it distinct from a
// competing-state conflict so already verified resources can enter recovery.
fn forward_inspect<B: Backend>(b: &mut B, stage: Stage) -> Result<Observation, Fault> {
    let o = b.inspect(stage)?;
    if !o.safety_satisfied {
        return Err(Fault::new(
            FaultKind::Before,
            "fresh safety telemetry refused",
        ));
    }
    Ok(o)
}
fn forward<B: Backend>(
    b: &mut B,
    authorized: bool,
    requested: u64,
    cancelled: &mut impl FnMut(Stage) -> bool,
    p: &mut Progress,
) -> Result<(), (Stage, Fault)> {
    macro_rules! step {
        ($stage:expr, $body:expr) => {{
            let stage = $stage;
            if cancelled(stage) {
                return Err((stage, Fault::new(FaultKind::Before, "cancelled")));
            }
            let value = $body.map_err(|e| (stage, e))?;
            p.previous = p.last;
            p.last = Some(stage);
            value
        }};
    }
    step!(
        Stage::Authorization,
        if authorized && requested > 0 && requested <= crate::MAX_LOW_BYTES && requested % 4096 == 0
        {
            Ok(())
        } else {
            Err(Fault::new(
                FaultKind::Before,
                "authorization or bounded value invalid",
            ))
        }
    );
    let o = step!(Stage::Binding, forward_inspect(b, Stage::Binding));
    p.original = Some(o.clone());
    if o.binding.is_empty()
        || !o.parent_empty
        || !o.safety_satisfied
        || o.child.is_some()
        || o.controllers.contains("memory")
        || !o.available_controllers.contains("memory")
        || o.controllers.iter().any(|c| c != "cpu" && c != "pids")
    {
        return Err((
            Stage::Binding,
            conflict("initial binding or hierarchy is unsafe"),
        ));
    }
    let o = step!(
        Stage::CaptureControllers,
        forward_inspect(b, Stage::CaptureControllers)
    );
    validate_base(p, &o, false)
        .and_then(|_| no_child(&o))
        .map_err(|e| (Stage::CaptureControllers, e))?;
    if !o.safety_satisfied {
        return Err((
            Stage::CaptureControllers,
            conflict("memory safety gate failed"),
        ));
    }
    step!(Stage::Activate, b.activate(&original(p).controllers));
    p.activated = true;
    let o = step!(
        Stage::VerifyActivation,
        forward_inspect(b, Stage::VerifyActivation)
    );
    validate_base(p, &o, true)
        .and_then(|_| no_child(&o))
        .map_err(|e| (Stage::VerifyActivation, e))?;
    let child = step!(Stage::CreateChild, b.create_child());
    p.child = Some(child);
    let o = step!(Stage::VerifyChild, forward_inspect(b, Stage::VerifyChild));
    validate_base(p, &o, true).map_err(|e| (Stage::VerifyChild, e))?;
    if !validate_child(p, &o)
        .map_err(|e| (Stage::VerifyChild, e))?
        .memory_inherited
    {
        return Err((
            Stage::VerifyChild,
            conflict("child lacks memory controller"),
        ));
    }
    let o = step!(Stage::CaptureLow, forward_inspect(b, Stage::CaptureLow));
    validate_base(p, &o, true).map_err(|e| (Stage::CaptureLow, e))?;
    let c = validate_child(p, &o).map_err(|e| (Stage::CaptureLow, e))?;
    let low = c.low.ok_or_else(|| {
        (
            Stage::CaptureLow,
            Fault::new(FaultKind::Before, "memory.low unavailable"),
        )
    })?;
    p.original_low = Some(low);
    if low == requested {
        return Err((
            Stage::CaptureLow,
            Fault::new(FaultKind::Before, "test value equals original"),
        ));
    }
    let o = forward_inspect(b, Stage::ApplyLow).map_err(|e| (Stage::ApplyLow, e))?;
    validate_base(p, &o, true)
        .and_then(|_| validate_child(p, &o))
        .and_then(|c| ensure_low(c, low))
        .map_err(|e| (Stage::ApplyLow, e))?;
    if !o.safety_satisfied {
        return Err((Stage::ApplyLow, conflict("memory safety gate failed")));
    }
    step!(
        Stage::ApplyLow,
        b.write_low(Stage::ApplyLow, p.child.as_deref().unwrap(), low, requested)
    );
    p.low_applied = true;
    let o = step!(
        Stage::VerifyApplied,
        forward_inspect(b, Stage::VerifyApplied)
    );
    validate_base(p, &o, true)
        .and_then(|_| validate_child(p, &o))
        .and_then(|c| ensure_low(c, requested))
        .map_err(|e| (Stage::VerifyApplied, e))?;
    let o = forward_inspect(b, Stage::RestoreLow).map_err(|e| (Stage::RestoreLow, e))?;
    validate_base(p, &o, true)
        .and_then(|_| validate_child(p, &o))
        .and_then(|c| ensure_low(c, requested))
        .map_err(|e| (Stage::RestoreLow, e))?;
    step!(
        Stage::RestoreLow,
        b.write_low(
            Stage::RestoreLow,
            p.child.as_deref().unwrap(),
            requested,
            low
        )
    );
    p.low_applied = false;
    let o = step!(
        Stage::VerifyRestored,
        forward_inspect(b, Stage::VerifyRestored)
    );
    validate_base(p, &o, true)
        .and_then(|_| validate_child(p, &o))
        .and_then(|c| ensure_low(c, low))
        .map_err(|e| (Stage::VerifyRestored, e))?;
    p.low_verified_restored = true;
    let o = step!(Stage::VerifyEmpty, forward_inspect(b, Stage::VerifyEmpty));
    validate_base(p, &o, true)
        .and_then(|_| validate_child(p, &o))
        .and_then(|c| ensure_low(c, low))
        .map_err(|e| (Stage::VerifyEmpty, e))?;
    step!(
        Stage::RemoveChild,
        b.remove_child(Stage::RemoveChild, p.child.as_deref().unwrap())
    );
    p.child_removed = true;
    let o = forward_inspect(b, Stage::RemoveChild).map_err(|e| (Stage::RemoveChild, e))?;
    validate_base(p, &o, true)
        .and_then(|_| no_child(&o))
        .map_err(|e| (Stage::RemoveChild, e))?;
    step!(
        Stage::RestoreControllers,
        b.restore_controllers(Stage::RestoreControllers, &original(p).controllers)
    );
    p.controllers_restored = true;
    let o = step!(Stage::VerifyFinal, forward_inspect(b, Stage::VerifyFinal));
    validate_base(p, &o, false)
        .and_then(|_| no_child(&o))
        .map_err(|e| (Stage::VerifyFinal, e))?;
    p.final_observation = Some(o);
    Ok(())
}

/// Run the coordinator using a fixture backend. Cancellation is checked at every
/// forward stage; once recovery begins it runs to a bounded conclusion without
/// accepting further cancellation. There are no retries of ambiguous mutations.
pub fn run<B: Backend>(
    b: &mut B,
    authorized: bool,
    requested: u64,
    mut cancelled: impl FnMut(Stage) -> bool,
) -> Report {
    let mut p = Progress::default();
    let result = forward(b, authorized, requested, &mut cancelled, &mut p);
    let mut report = Report {
        outcome: Outcome::RecoveryUnavailable,
        last_completed: p.last,
        failed_transition: None,
        recovery_failed_transition: None,
        original: p.original.clone(),
        original_low: p.original_low,
        registered_child_identity: p.child.clone(),
        current: None,
        low_restoration_verified: if p.low_verified_restored {
            Certainty::Yes
        } else {
            Certainty::Unknown
        },
        child_remains: Certainty::Unknown,
        controller_restoration_verified: Certainty::Unknown,
        investigation: Vec::new(),
    };
    match result {
        Ok(()) => {
            report.outcome = Outcome::Complete;
            report.low_restoration_verified = Certainty::Yes;
            report.child_remains = Certainty::No;
            report.controller_restoration_verified = Certainty::Yes;
            report.current = p.final_observation;
            return report;
        }
        Err((stage, fault)) => {
            if p.last == Some(stage) {
                p.last = p.previous;
                report.last_completed = p.last;
            }
            report.failed_transition = Some(stage);
            report
                .investigation
                .push(format!("{stage:?}: {}", fault.message));
            if p.original.is_none() {
                report.outcome = match fault.kind {
                    FaultKind::Conflict => Outcome::Conflict,
                    FaultKind::Ambiguous => Outcome::Ambiguous,
                    FaultKind::Before | FaultKind::Unavailable => Outcome::RecoveryUnavailable,
                };
                return report;
            }
            if fault.kind != FaultKind::Before {
                report.outcome = match fault.kind {
                    FaultKind::Conflict => Outcome::Conflict,
                    FaultKind::Ambiguous => Outcome::Ambiguous,
                    FaultKind::Unavailable => Outcome::RecoveryUnavailable,
                    FaultKind::Before => unreachable!(),
                };
                diagnostic(b, &mut report);
                return report;
            }
        }
    }
    match recover(b, requested, &mut p, &mut report) {
        Ok(()) => {
            report.outcome = Outcome::RestoredAfterFailure;
            report
                .investigation
                .push("recovery completed; review the original failure".into());
        }
        Err((stage, fault)) => {
            report.recovery_failed_transition = Some(stage);
            report
                .investigation
                .push(format!("{stage:?}: {}", fault.message));
            report.outcome = match fault.kind {
                FaultKind::Conflict => Outcome::Conflict,
                FaultKind::Ambiguous => Outcome::Ambiguous,
                FaultKind::Unavailable => Outcome::RecoveryUnavailable,
                FaultKind::Before => Outcome::CleanupRequired,
            };
            diagnostic(b, &mut report);
        }
    }
    report.last_completed = p.last;
    report
}
fn diagnostic<B: Backend>(b: &mut B, report: &mut Report) {
    if let Ok(o) = recovery_inspect(b, Stage::RecoveryInspect) {
        if report
            .original
            .as_ref()
            .is_some_and(|original| original.binding == o.binding)
        {
            report.child_remains = match &o.child {
                Some(child)
                    if report.registered_child_identity.as_ref() == Some(&child.identity) =>
                {
                    Certainty::Yes
                }
                Some(_) => Certainty::Unknown,
                None => Certainty::No,
            };
        }
        report.current = Some(o);
    }
}
fn recovery_inspect<B: Backend>(b: &mut B, stage: Stage) -> Result<Observation, Fault> {
    b.inspect(stage).map_err(|mut fault| {
        if fault.kind == FaultKind::Before {
            fault.kind = FaultKind::Unavailable;
        }
        fault
    })
}
fn recover<B: Backend>(
    b: &mut B,
    requested: u64,
    p: &mut Progress,
    r: &mut Report,
) -> Result<(), (Stage, Fault)> {
    macro_rules! try_stage {
        ($stage:expr, $result:expr) => {{
            let stage = $stage;
            $result.map_err(|e| (stage, e))?
        }};
    }
    let o = try_stage!(
        Stage::RecoveryInspect,
        recovery_inspect(b, Stage::RecoveryInspect)
    );
    try_stage!(
        Stage::RecoveryInspect,
        validate_base(p, &o, p.activated && !p.controllers_restored)
    );
    if let Some(id) = &p.child {
        if p.child_removed {
            try_stage!(Stage::RecoveryInspect, no_child(&o));
            r.child_remains = Certainty::No;
        } else {
            let c = try_stage!(Stage::RecoveryInspect, validate_child(p, &o));
            r.child_remains = Certainty::Yes;
            if p.low_applied {
                try_stage!(Stage::RecoveryInspect, ensure_low(c, requested));
                let low = p.original_low.ok_or_else(|| {
                    (
                        Stage::RecoveryInspect,
                        Fault::new(FaultKind::Unavailable, "original memory.low unknown"),
                    )
                })?;
                try_stage!(
                    Stage::RecoveryRestoreLow,
                    b.write_low(Stage::RecoveryRestoreLow, id, requested, low)
                );
                p.low_applied = false;
                p.last = Some(Stage::RecoveryRestoreLow);
                let after = try_stage!(
                    Stage::RecoveryVerifyLow,
                    recovery_inspect(b, Stage::RecoveryVerifyLow)
                );
                try_stage!(Stage::RecoveryVerifyLow, validate_base(p, &after, true));
                try_stage!(
                    Stage::RecoveryVerifyLow,
                    validate_child(p, &after).and_then(|child| ensure_low(child, low))
                );
                p.low_verified_restored = true;
                p.last = Some(Stage::RecoveryVerifyLow);
            } else if let Some(low) = p.original_low {
                try_stage!(Stage::RecoveryInspect, ensure_low(c, low));
                p.low_verified_restored = true;
            }
            if p.original_low.is_some() {
                r.low_restoration_verified = Certainty::Yes;
            }
            try_stage!(
                Stage::RecoveryRemoveChild,
                b.remove_child(Stage::RecoveryRemoveChild, id)
            );
            p.child_removed = true;
            p.last = Some(Stage::RecoveryRemoveChild);
            let after = try_stage!(
                Stage::RecoveryVerifyRemoval,
                recovery_inspect(b, Stage::RecoveryVerifyRemoval)
            );
            try_stage!(
                Stage::RecoveryVerifyRemoval,
                validate_base(p, &after, true).and_then(|_| no_child(&after))
            );
            r.child_remains = Certainty::No;
            p.last = Some(Stage::RecoveryVerifyRemoval);
        }
    } else {
        try_stage!(Stage::RecoveryInspect, no_child(&o));
        r.child_remains = Certainty::No;
    }
    if p.activated && !p.controllers_restored {
        // Verify the child is absent immediately before restoring the controller.
        let before = try_stage!(
            Stage::RecoveryInspect,
            recovery_inspect(b, Stage::RecoveryInspect)
        );
        try_stage!(
            Stage::RecoveryInspect,
            validate_base(p, &before, true).and_then(|_| no_child(&before))
        );
        try_stage!(
            Stage::RecoveryRestoreControllers,
            b.restore_controllers(Stage::RecoveryRestoreControllers, &original(p).controllers)
        );
        p.controllers_restored = true;
        p.last = Some(Stage::RecoveryRestoreControllers);
    }
    let final_state = try_stage!(
        Stage::RecoveryVerifyControllers,
        recovery_inspect(b, Stage::RecoveryVerifyControllers)
    );
    try_stage!(
        Stage::RecoveryVerifyControllers,
        validate_base(p, &final_state, false).and_then(|_| no_child(&final_state))
    );
    r.current = Some(final_state);
    r.controller_restoration_verified = Certainty::Yes;
    r.child_remains = Certainty::No;
    p.last = Some(Stage::RecoveryVerifyControllers);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum When {
        Before,
        After,
    }
    struct Fixture {
        state: Observation,
        failures: Vec<(Stage, When)>,
        competing: Option<(Stage, &'static str)>,
        write_race: Option<Stage>,
        trace: Vec<String>,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                state: Observation {
                    binding: "unit:invocation:parent:manager".into(),
                    parent_empty: true,
                    safety_satisfied: true,
                    available_controllers: ["cpu".into(), "memory".into(), "pids".into()]
                        .into_iter()
                        .collect(),
                    controllers: ["cpu".into(), "pids".into()].into_iter().collect(),
                    child: None,
                },
                failures: Vec::new(),
                competing: None,
                write_race: None,
                trace: Vec::new(),
            }
        }
        fn fail(&mut self, stage: Stage, when: When, mutation: bool) -> Result<(), Fault> {
            if let Some(pos) = self.failures.iter().position(|x| *x == (stage, when)) {
                self.failures.remove(pos);
                return Err(Fault::new(
                    if mutation && when == When::After {
                        FaultKind::Ambiguous
                    } else {
                        FaultKind::Before
                    },
                    format!("injected {stage:?}"),
                ));
            }
            Ok(())
        }
        fn mutate(
            &mut self,
            stage: Stage,
            operation: impl FnOnce(&mut Observation),
        ) -> Result<(), Fault> {
            self.trace.push(format!("write:{stage:?}"));
            self.fail(stage, When::Before, true)?;
            operation(&mut self.state);
            self.fail(stage, When::After, true)
        }
    }
    impl Backend for Fixture {
        fn inspect(&mut self, stage: Stage) -> Result<Observation, Fault> {
            self.trace.push(format!("read:{stage:?}"));
            if !matches!(stage, Stage::ApplyLow | Stage::RestoreLow) {
                self.fail(stage, When::Before, false)?;
            }
            if self.competing.is_some_and(|(s, _)| s == stage) {
                match self.competing.take().unwrap().1 {
                    "controller" => {
                        self.state.controllers.insert("io".into());
                    }
                    "low" => self.state.child.as_mut().unwrap().low = Some(12288),
                    "identity" => {
                        self.state.child.as_mut().unwrap().identity = "replacement".into()
                    }
                    "populated" => self.state.child.as_mut().unwrap().empty = false,
                    "descendants" => self.state.child.as_mut().unwrap().descendants = 1,
                    "missing_controller" => {
                        self.state.child.as_mut().unwrap().memory_inherited = false
                    }
                    "binding" => self.state.binding = "stale".into(),
                    "missing_low" => self.state.child.as_mut().unwrap().low = None,
                    "extra" => return Err(conflict("unregistered extra cgroup")),
                    "removal_mismatch" => {
                        self.state.child = Some(ChildObservation {
                            identity: "other".into(),
                            empty: true,
                            descendants: 0,
                            memory_inherited: true,
                            low: Some(4096),
                        })
                    }
                    _ => unreachable!(),
                }
            }
            let observation = self.state.clone();
            if !matches!(stage, Stage::ApplyLow | Stage::RestoreLow) {
                self.fail(stage, When::After, false)?;
            }
            Ok(observation)
        }
        fn activate(&mut self, expected: &BTreeSet<String>) -> Result<(), Fault> {
            if &self.state.controllers != expected {
                return Err(conflict("controller changed before activation"));
            }
            self.mutate(Stage::Activate, |s| {
                s.controllers.insert("memory".into());
            })
        }
        fn create_child(&mut self) -> Result<String, Fault> {
            self.mutate(Stage::CreateChild, |s| {
                s.child = Some(ChildObservation {
                    identity: "child-inode".into(),
                    empty: true,
                    descendants: 0,
                    memory_inherited: true,
                    low: Some(4096),
                })
            })?;
            Ok("child-inode".into())
        }
        fn write_low(
            &mut self,
            stage: Stage,
            identity: &str,
            expected: u64,
            value: u64,
        ) -> Result<(), Fault> {
            if self.write_race == Some(stage) {
                self.write_race = None;
                self.state.child.as_mut().unwrap().low = Some(16384);
            }
            let c = self
                .state
                .child
                .as_ref()
                .ok_or_else(|| conflict("child absent"))?;
            if c.identity != identity || !c.empty || c.descendants != 0 || c.low != Some(expected) {
                return Err(conflict("child or memory.low changed before write"));
            }
            self.mutate(stage, |s| s.child.as_mut().unwrap().low = Some(value))
        }
        fn remove_child(&mut self, stage: Stage, identity: &str) -> Result<(), Fault> {
            let child = self
                .state
                .child
                .as_ref()
                .ok_or_else(|| conflict("child absent"))?;
            if child.identity != identity || !child.empty || child.descendants != 0 {
                return Err(conflict("refuse child removal"));
            }
            self.mutate(stage, |s| s.child = None)
        }
        fn restore_controllers(
            &mut self,
            stage: Stage,
            original: &BTreeSet<String>,
        ) -> Result<(), Fault> {
            if self.state.child.is_some() {
                return Err(conflict("child still present"));
            }
            let mut expected = original.clone();
            expected.insert("memory".into());
            if self.state.controllers != expected {
                return Err(conflict("controllers changed before restore"));
            }
            self.mutate(stage, |s| {
                s.controllers.remove("memory");
            })
        }
    }
    fn execute(f: &mut Fixture) -> Report {
        run(f, true, 8192, |_| false)
    }
    fn clean(f: &Fixture) -> bool {
        f.state.child.is_none() && !f.state.controllers.contains("memory")
    }
    const FORWARD: [Stage; 16] = [
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
    ];
    #[test]
    fn complete_and_explicit_authorization() {
        let mut f = Fixture::new();
        let r = execute(&mut f);
        assert_eq!(r.outcome, Outcome::Complete);
        assert_eq!(r.last_completed, Some(Stage::VerifyFinal));
        assert_eq!(r.original_low, Some(4096));
        assert_eq!(r.low_restoration_verified, Certainty::Yes);
        assert_eq!(r.child_remains, Certainty::No);
        assert_eq!(r.controller_restoration_verified, Certainty::Yes);
        assert!(clean(&f));
        let writes: Vec<_> = f
            .trace
            .iter()
            .filter(|s| s.starts_with("write:"))
            .map(String::as_str)
            .collect();
        assert_eq!(
            writes,
            [
                "write:Activate",
                "write:CreateChild",
                "write:ApplyLow",
                "write:RestoreLow",
                "write:RemoveChild",
                "write:RestoreControllers"
            ]
        );
        let mut f = Fixture::new();
        let r = run(&mut f, false, 8192, |_| false);
        assert_ne!(r.outcome, Outcome::Complete);
        assert!(f.trace.is_empty());
        for value in [0, 1, crate::MAX_LOW_BYTES + 4096] {
            let mut f = Fixture::new();
            assert!(run(&mut f, true, value, |_| false).outcome != Outcome::Complete);
            assert!(f.trace.is_empty());
        }
    }
    #[test]
    fn cancellation_at_each_stage_runs_ordered_bounded_recovery() {
        for stage in FORWARD {
            let mut f = Fixture::new();
            let r = run(&mut f, true, 8192, |s| s == stage);
            assert_eq!(r.failed_transition, Some(stage), "{stage:?}");
            assert_ne!(r.outcome, Outcome::Complete, "{stage:?}");
            assert!(clean(&f), "{stage:?}: {:?} {:?}", r.outcome, f.trace);
            if f.trace
                .iter()
                .any(|s| s == "write:RecoveryRestoreControllers")
            {
                let removal = f
                    .trace
                    .iter()
                    .position(|s| s == "write:RecoveryRemoveChild" || s == "write:RemoveChild");
                let restore = f
                    .trace
                    .iter()
                    .position(|s| s == "write:RecoveryRestoreControllers")
                    .unwrap();
                if let Some(removal) = removal {
                    assert!(removal < restore, "{stage:?}");
                }
            }
        }
    }
    #[test]
    fn each_failable_stage_before_and_read_only_after() {
        for stage in FORWARD.into_iter().skip(1) {
            let mut f = Fixture::new();
            f.failures.push((stage, When::Before));
            let r = execute(&mut f);
            assert_ne!(r.outcome, Outcome::Complete, "{stage:?}");
            assert_eq!(r.failed_transition, Some(stage), "{stage:?}");
            assert!(clean(&f), "{stage:?}: {:?} {:?}", r.outcome, f.trace);
        }
        for stage in [
            Stage::Binding,
            Stage::CaptureControllers,
            Stage::VerifyActivation,
            Stage::VerifyChild,
            Stage::CaptureLow,
            Stage::VerifyApplied,
            Stage::VerifyRestored,
            Stage::VerifyEmpty,
            Stage::VerifyFinal,
        ] {
            let mut f = Fixture::new();
            f.failures.push((stage, When::After));
            let r = execute(&mut f);
            assert_ne!(r.outcome, Outcome::Complete, "{stage:?}");
            assert_eq!(r.failed_transition, Some(stage), "{stage:?}");
            assert!(clean(&f), "{stage:?}: {:?} {:?}", r.outcome, f.trace);
        }
    }
    #[test]
    fn mutation_errors_after_effect_are_ambiguous_and_not_retried() {
        for stage in [
            Stage::Activate,
            Stage::CreateChild,
            Stage::ApplyLow,
            Stage::RestoreLow,
            Stage::RemoveChild,
            Stage::RestoreControllers,
        ] {
            let mut f = Fixture::new();
            f.failures.push((stage, When::After));
            let r = execute(&mut f);
            assert_eq!(r.outcome, Outcome::Ambiguous, "{stage:?}");
            assert_eq!(r.failed_transition, Some(stage));
            assert_eq!(
                f.trace
                    .iter()
                    .filter(|s| s.as_str() == format!("write:{stage:?}"))
                    .count(),
                1
            );
            assert!(
                !f.trace.iter().any(|s| s.starts_with("write:Recovery")),
                "{stage:?}"
            );
        }
    }
    #[test]
    fn competing_changes_and_identity_replacement_never_get_overwritten() {
        for (stage, change) in [
            (Stage::VerifyActivation, "controller"),
            (Stage::VerifyApplied, "low"),
            (Stage::VerifyChild, "identity"),
            (Stage::VerifyChild, "populated"),
            (Stage::VerifyChild, "descendants"),
            (Stage::VerifyChild, "missing_controller"),
            (Stage::VerifyChild, "extra"),
            (Stage::VerifyApplied, "binding"),
            (Stage::CaptureLow, "missing_low"),
            (Stage::RemoveChild, "removal_mismatch"),
        ] {
            let mut f = Fixture::new();
            f.competing = Some((stage, change));
            let r = execute(&mut f);
            assert_ne!(r.outcome, Outcome::Complete, "{change}");
            if change != "missing_low" {
                assert_eq!(r.outcome, Outcome::Conflict, "{change}");
                assert!(!f.trace.iter().any(|s| s.starts_with("write:Recovery")));
            }
            if change == "low" {
                assert_eq!(f.state.child.as_ref().unwrap().low, Some(12288));
            }
            if change == "removal_mismatch" {
                assert!(!f.trace.iter().any(|s| s == "write:RestoreControllers"));
            }
        }
    }
    #[test]
    fn recovery_failures_do_not_restore_parent_with_child() {
        for failed in [
            Stage::RecoveryRestoreLow,
            Stage::RecoveryVerifyLow,
            Stage::RecoveryRemoveChild,
            Stage::RecoveryVerifyRemoval,
            Stage::RecoveryRestoreControllers,
            Stage::RecoveryVerifyControllers,
        ] {
            let mut f = Fixture::new();
            f.failures.push((failed, When::Before));
            let r = run(&mut f, true, 8192, |stage| stage == Stage::RestoreLow);
            assert_ne!(r.outcome, Outcome::Complete, "{failed:?}");
            if f.state.child.is_some() {
                assert!(
                    !f.trace
                        .iter()
                        .any(|s| s == "write:RecoveryRestoreControllers"),
                    "{failed:?}"
                );
            }
        }
        let mut f = Fixture::new();
        f.failures = vec![
            (Stage::RecoveryRemoveChild, When::Before),
            (Stage::RecoveryRestoreControllers, When::Before),
        ];
        let r = run(&mut f, true, 8192, |stage| stage == Stage::RestoreLow);
        assert_eq!(r.outcome, Outcome::CleanupRequired);
        assert_eq!(r.failed_transition, Some(Stage::RestoreLow));
        assert_eq!(
            r.recovery_failed_transition,
            Some(Stage::RecoveryRemoveChild)
        );
        assert_eq!(r.registered_child_identity.as_deref(), Some("child-inode"));
        assert!(f.state.child.is_some());
        assert!(!f
            .trace
            .iter()
            .any(|s| s == "write:RecoveryRestoreControllers"));
        assert_eq!(f.failures.len(), 1);
    }
    #[test]
    fn reverse_order_and_conflicts_during_recovery() {
        let mut f = Fixture::new();
        let r = run(&mut f, true, 8192, |s| s == Stage::VerifyApplied);
        assert_eq!(r.outcome, Outcome::RestoredAfterFailure);
        let writes: Vec<_> = f
            .trace
            .iter()
            .filter(|s| s.starts_with("write:Recovery"))
            .map(String::as_str)
            .collect();
        assert_eq!(
            writes,
            [
                "write:RecoveryRestoreLow",
                "write:RecoveryRemoveChild",
                "write:RecoveryRestoreControllers"
            ]
        );
        assert_eq!(r.low_restoration_verified, Certainty::Yes);
        assert_eq!(r.controller_restoration_verified, Certainty::Yes);
        assert_eq!(r.child_remains, Certainty::No);
        for change in ["low", "identity", "populated", "binding", "controller"] {
            let mut f = Fixture::new();
            f.competing = Some((Stage::RecoveryInspect, change));
            let r = run(&mut f, true, 8192, |s| s == Stage::VerifyApplied);
            assert_eq!(r.outcome, Outcome::Conflict, "{change}");
            assert!(
                !f.trace.iter().any(|s| s.starts_with("write:Recovery")),
                "{change}"
            );
        }
        let mut f = Fixture::new();
        f.write_race = Some(Stage::RestoreLow);
        let r = execute(&mut f);
        assert_eq!(r.outcome, Outcome::Conflict);
        assert_eq!(f.state.child.as_ref().unwrap().low, Some(16384));
        assert!(!f
            .trace
            .iter()
            .any(|s| s == "write:RestoreLow" || s == "write:RestoreControllers"));
        for (stage, change) in [
            (Stage::RecoveryVerifyLow, "low"),
            (Stage::RecoveryVerifyControllers, "controller"),
        ] {
            let mut f = Fixture::new();
            f.competing = Some((stage, change));
            let r = run(&mut f, true, 8192, |s| s == Stage::VerifyApplied);
            assert_eq!(r.outcome, Outcome::Conflict);
            assert_eq!(r.recovery_failed_transition, Some(stage));
            if stage == Stage::RecoveryVerifyLow {
                assert!(f.state.child.is_some());
                assert!(!f
                    .trace
                    .iter()
                    .any(|s| s == "write:RecoveryRemoveChild"
                        || s == "write:RecoveryRestoreControllers"));
            }
        }
    }
    #[test]
    fn recovery_write_ambiguities_are_not_retried() {
        for failed in [
            Stage::RecoveryRestoreLow,
            Stage::RecoveryRemoveChild,
            Stage::RecoveryRestoreControllers,
        ] {
            let mut f = Fixture::new();
            f.failures.push((failed, When::After));
            let r = run(&mut f, true, 8192, |s| s == Stage::VerifyApplied);
            assert_eq!(r.outcome, Outcome::Ambiguous, "{failed:?}");
            assert_eq!(
                f.trace
                    .iter()
                    .filter(|s| s.as_str() == format!("write:{failed:?}"))
                    .count(),
                1
            );
            if f.state.child.is_some() {
                assert!(!f
                    .trace
                    .iter()
                    .any(|s| s == "write:RecoveryRestoreControllers"));
            }
        }
    }
}
