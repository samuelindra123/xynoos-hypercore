//! Common types, statuses, and ownership models for universal hardware and system discovery.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Structured discovery status for any inspected metric or capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryStatus {
    /// Metric or interface is present and verified.
    #[default]
    Available,
    /// Metric or feature is not supported by this kernel, hardware, or distribution.
    Unsupported,
    /// Metric path or interface is currently absent or offline.
    Unavailable,
    /// Access was denied due to filesystem permissions or lack of capability.
    PermissionDenied,
    /// Interface exposed malformed or unparseable data.
    Malformed,
    /// Conflicting subsystem state detected.
    Conflicting,
    /// State could not be deterministically determined.
    Unknown,
}

/// Provenance and ownership classification of existing system configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationOwnership {
    /// Configuration is at Linux kernel compiled-in or default state.
    KernelDefault,
    /// Configuration is managed by base distribution packages or vendor defaults.
    DistributionManaged,
    /// Configuration was explicitly declared by a human administrator in /etc.
    AdministratorManaged,
    /// Configuration is managed by third-party daemon (e.g. TLP, tuned, auto-cpufreq).
    ThirdPartyManaged,
    /// Configuration is owned and recorded in XYNOOS HyperCore state.
    HyperCoreManaged,
    /// Ownership could not be deterministically verified.
    #[default]
    UnknownOwnership,
}

/// Severity classification of configuration conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSeverity {
    /// Confirmed competing or mutually destructive management detected.
    ConfirmedConflict,
    /// Potential friction or suboptimal interaction detected.
    PotentialConflict,
    /// Multiple subsystems active but coexist compatibly.
    CompatibleCoexistence,
    /// Insufficient evidence to confirm or deny conflict.
    InsufficientEvidence,
}

/// Confidence rating for discovered capabilities and recommendations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

/// Privilege requirement level for an optimization or action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegeLevel {
    /// Can be inspected or executed without root privileges.
    Unprivileged,
    /// Requires delegated non-root cgroup write access.
    DelegatedCgroup,
    /// Requires root or sudo capability.
    RequiresRoot,
    /// Requires systemd unit installation or modification.
    SystemdAdmin,
}

/// Reversibility guarantee classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReversibilityGuarantee {
    /// Volatile runtime change, reverts automatically on reboot or process exit.
    VolatileRuntime,
    /// Fully reversible via transaction or journaled restoration.
    FullyReversible,
    /// Reversible by removing drop-in configuration file.
    ReversibleConfig,
    /// Requires reboot or manual administrator intervention to reverse.
    IrreversibleWithoutReboot,
}
