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
pub trait ControllerBackend {
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
pub fn activate_memory<B: ControllerBackend>(
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
pub fn restore_memory<B: ControllerBackend>(
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

/// Linux implementation bound to the live transient service that contains this process.
/// The caller must explicitly authorize each activation and restoration call as well.
struct LinuxController {
    binding: crate::systemd_identity::Binding,
    authorized: bool,
    child: Option<crate::bound_child::BoundChild>,
    child_unresolved: bool,
}
impl LinuxController {
    fn new(binding: crate::systemd_identity::Binding, authorized: bool) -> Result<Self, String> {
        if !authorized {
            return Err("explicit authorization required".into());
        }
        binding.revalidate()?;
        Ok(Self {
            binding,
            authorized,
            child: None,
            child_unresolved: false,
        })
    }
    fn write(&mut self, command: &[u8]) -> Result<(), String> {
        use std::os::{fd::AsRawFd, unix::fs::FileExt};
        if !self.authorized {
            return Err("explicit authorization required".into());
        }
        self.binding.revalidate()?;
        let f = crate::transaction::openat(
            self.binding.parent().as_raw_fd(),
            c"cgroup.subtree_control",
            libc::O_WRONLY,
        )?;
        let n = f.write_at(command, 0).map_err(|e| e.to_string())?;
        if n != command.len() {
            return Err("short controller write".into());
        }
        Ok(())
    }
    fn read(&self, name: &std::ffi::CStr) -> Result<String, String> {
        self.binding.revalidate()?;
        crate::transaction::read_at(self.binding.parent(), name)
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
                binding,
                authorized: false,
                child: None,
                child_unresolved: false,
            },
        })
    }
    pub fn inspect(&mut self) -> Result<ControllerReadiness, String> {
        inspect_readiness(&mut self.backend)
    }
    pub fn binding_diagnostic(&self) -> serde_json::Value {
        self.backend.binding.diagnostic()
    }
}
/// An activation held in memory until one explicit restoration attempt.
/// Dropping this value never writes to the kernel.
pub struct ControllerSession {
    backend: LinuxController,
    activation: Option<Activation>,
    original_before_attempt: Option<Vec<String>>,
    activation_unresolved: bool,
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
impl ControllerSession {
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
        })
    }
    pub fn activate(
        binding: crate::systemd_identity::Binding,
        authorized: bool,
    ) -> Result<Self, String> {
        let mut session = Self::prepare(binding, authorized)?;
        session.activate_prepared()?;
        Ok(session)
    }
    /// Capture initial state before activation. A failed attempt remains unresolved;
    /// no automatic retry or rollback is performed on an ambiguous write.
    pub fn activate_prepared(&mut self) -> Result<(), String> {
        if self.activation.is_some() {
            return Err("activation already attempted".into());
        }
        self.activation = Some(activate_with_tracking(
            &mut self.backend,
            &mut self.original_before_attempt,
            &mut self.activation_unresolved,
        )?);
        Ok(())
    }
    /// The observed pre-attempt state does not by itself authorize recovery.
    pub fn activation_diagnostic(&self) -> serde_json::Value {
        serde_json::json!({
            "binding": self.backend.binding.diagnostic(),
            "original_controllers": self.original_before_attempt,
            "activation_unresolved": self.activation_unresolved,
            "child_unresolved": self.backend.child_unresolved,
            "child_registered": self.backend.child.is_some(),
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
    /// A failed authorized attempt is never retried automatically.
    pub fn restore(&mut self, authorized: bool) -> Result<(), String> {
        if !authorized {
            return Err("explicit authorization required".into());
        }
        if self.activation_unresolved
            || self.backend.child.is_some()
            || self.backend.child_unresolved
        {
            return Err("child cleanup not verified; controller restoration refused".into());
        }
        let activation = self
            .activation
            .take()
            .ok_or("restoration already attempted")?;
        self.activation_unresolved = true;
        restore_memory(&mut self.backend, &activation, true)?;
        self.activation_unresolved = false;
        Ok(())
    }
    /// Create one empty child relative to the held, validated parent descriptor.
    /// A failed creation may leave a child; the session is then blocked from restoration.
    pub fn create_empty_child(&mut self) -> Result<(), String> {
        if self.activation.is_none() || self.activation_unresolved {
            return Err("verified controller activation required".into());
        }
        if self.backend.child.is_some() || self.backend.child_unresolved {
            return Err("child already registered or unresolved".into());
        }
        let id = self.backend.verify_unit()?;
        if !self.backend.processes()?.trim().is_empty() {
            return Err("parent populated".into());
        }
        if self.backend.verify_unit()? != id {
            return Err("parent identity changed before child creation".into());
        }
        self.backend.child_unresolved = true;
        let expected = crate::bound_child::FileId {
            dev: id.device,
            ino: id.inode,
        };
        let child =
            crate::bound_child::BoundChild::create(self.backend.binding.parent(), expected)?;
        self.backend.child = Some(child);
        self.backend.child_unresolved = false;
        if let Err(e) = self.backend.verify_unit() {
            self.backend.child_unresolved = true;
            return Err(format!(
                "created child preserved; hierarchy validation failed: {e}"
            ));
        }
        Ok(())
    }
    /// Inspect the registered child's inherited controller list without control writes.
    pub fn child_controllers(&mut self) -> Result<String, String> {
        let id = self.backend.verify_unit()?;
        let child = self.backend.child.as_ref().ok_or("no registered child")?;
        child.controllers(
            self.backend.binding.parent(),
            crate::bound_child::FileId {
                dev: id.device,
                ino: id.inode,
            },
        )
    }
    /// Remove only the registered, still-empty child; failed cleanup blocks restoration.
    pub fn remove_empty_child(&mut self) -> Result<(), String> {
        let id = self.backend.verify_unit()?;
        let child = self.backend.child.as_ref().ok_or("no registered child")?;
        self.backend.child_unresolved = true;
        child.remove(
            self.backend.binding.parent(),
            crate::bound_child::FileId {
                dev: id.device,
                ino: id.inode,
            },
        )?;
        self.backend.child = None;
        self.backend.child_unresolved = false;
        if let Err(e) = self.backend.verify_unit() {
            self.backend.child_unresolved = true;
            return Err(format!("hierarchy verification after removal failed: {e}"));
        }
        Ok(())
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
        self.binding.revalidate()?;
        if !crate::transaction::read_at(self.binding.parent(), c"cgroup.procs")?
            .trim()
            .is_empty()
        {
            return Err("parent populated".into());
        }
        let stat = crate::transaction::read_at(self.binding.parent(), c"cgroup.stat")?;
        let manager_stat = crate::transaction::read_at(self.binding.manager(), c"cgroup.stat")?;
        let manager_procs = crate::transaction::read_at(self.binding.manager(), c"cgroup.procs")?;
        let (device, inode) = self.binding.parent_identity();
        if let Some(child) = &self.child {
            child.verify(
                self.binding.parent(),
                crate::bound_child::FileId {
                    dev: device,
                    ino: inode,
                },
            )?;
        }
        validate_hierarchy(
            &stat,
            &manager_stat,
            &manager_procs,
            std::process::id(),
            self.child.is_some(),
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
