//! Controller-activation state machine. A backend must prove unit ownership and
//! confine operations to one held directory descriptor. No production backend
//! is registered yet; this is exercised only by deterministic fixtures.
//! Checks and controller writes are separate kernel operations: another actor can
//! change state between them. A successful result retains the initial controller
//! set in memory only. On error, crash, or process termination it is not persisted
//! and no automatic recovery or deactivation is performed.
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
    if let Err(e) = backend.enable_memory() {
        return Err(format!("activation write failed; state ambiguous: {e}"));
    }
    if backend
        .verify_unit()
        .map_err(|e| format!("unit verification after activation failed; state ambiguous: {e}"))?
        != identity
    {
        return Err("unit changed after activation; state ambiguous".into());
    }
    let after = backend
        .enabled()
        .map_err(|e| format!("activation read-back failed; state ambiguous: {e}"))?;
    let after_set = controllers(&after)
        .map_err(|e| format!("invalid activation read-back; state ambiguous: {e}"))?;
    let original_controllers = before_set.iter().map(|s| (*s).to_owned()).collect();
    let mut expected = before_set;
    expected.insert("memory");
    if after_set != expected {
        return Err("activation read-back mismatch; state ambiguous".into());
    }
    Ok(Activation::Enabled {
        original_controllers,
    })
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
}
