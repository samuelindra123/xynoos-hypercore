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
/// An activation held in memory until one explicit restoration attempt.
/// Dropping this value never writes to the kernel.
pub struct ControllerSession {
    backend: LinuxController,
    activation: Option<Activation>,
}
impl ControllerSession {
    pub fn activate(
        binding: crate::systemd_identity::Binding,
        authorized: bool,
    ) -> Result<Self, String> {
        let mut backend = LinuxController::new(binding, authorized)?;
        let activation = activate_memory(&mut backend, authorized)?;
        Ok(Self {
            backend,
            activation: Some(activation),
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
        let activation = self
            .activation
            .take()
            .ok_or("restoration already attempted")?;
        restore_memory(&mut self.backend, &activation, true)
    }
}
impl ControllerBackend for LinuxController {
    fn verify_unit(&mut self) -> Result<ParentIdentity, String> {
        self.binding.revalidate()?;
        let stat = crate::transaction::read_at(self.binding.parent(), c"cgroup.stat")?;
        if stat
            .lines()
            .filter(|line| line.trim() == "nr_descendants 1")
            .count()
            != 1
        {
            return Err("unexpected delegated hierarchy descendants".into());
        }
        let (device, inode) = self.binding.parent_identity();
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
}
