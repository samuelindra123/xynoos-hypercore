//! Allowlisted service-control design. No implementation may write through this API yet.
use serde::Serialize;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Operation {
    PowerSaver,
    Balanced,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    PowerProfilesDaemon,
    Tuned,
    Unknown,
    Conflict,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejection {
    pub operation: Operation,
    pub reasons: Vec<&'static str>,
    pub required_lifecycle: [&'static str; 6],
}
/// Explicit opt-in is necessary but insufficient. Fail closed until an audited
/// backend implements identity-bound snapshots, validation, timeout and rollback.
pub fn prepare(
    operation: Operation,
    opt_in: bool,
    permission: bool,
    manager: Manager,
) -> Rejection {
    let mut reasons = vec!["No writable service backend is implemented"];
    if !opt_in {
        reasons.push("Explicit opt-in absent");
    }
    if !permission {
        reasons.push("Permission not established");
    }
    if matches!(manager, Manager::Unknown | Manager::Conflict) {
        reasons.push("Exclusive power-manager ownership not established");
    }
    Rejection {
        operation,
        reasons,
        required_lifecycle: [
            "Snapshot manager identity and active profile",
            "Validate supported profile and authorization",
            "Execute allowlisted D-Bus request with timeout",
            "Read back and validate resulting state",
            "Restore original state on failure; retain unresolved recovery",
            "Record before/after, outcome and rollback audit",
        ],
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_combinations_fail_closed() {
        for opt in [false, true] {
            for permission in [false, true] {
                for manager in [
                    Manager::PowerProfilesDaemon,
                    Manager::Tuned,
                    Manager::Unknown,
                    Manager::Conflict,
                ] {
                    for operation in [Operation::PowerSaver, Operation::Balanced] {
                        assert!(prepare(operation, opt, permission, manager)
                            .reasons
                            .contains(&"No writable service backend is implemented"));
                    }
                }
            }
        }
    }
}
