//! Versioned, deterministic configuration action schema and typed allowlisted mutations.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use xynoos_pressureiq::discovery::types::{
    ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee,
};

pub const CONFIG_ACTION_SCHEMA_VERSION: u32 = 1;

/// High-level target subsystem for configuration actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSubsystem {
    Memory,
    Compression,
    Swap,
    Sysctl,
    TransparentHugePages,
    Cgroup,
    Cpu,
    Storage,
    Power,
}

/// Explicit semantic classification of a proposed action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClassification {
    /// Beneficial existing configuration detected; preserve without mutation.
    Preserve,
    /// Beneficial optimization missing; propose configuring safely.
    Configure,
    /// Suboptimal configuration detected; propose improving setting.
    Improve,
    /// Action deferred due to current workload conditions or power profile.
    Defer,
    /// Feature unsupported on current kernel, architecture, or hardware.
    Unsupported,
    /// Competing daemon or policy conflict detected; avoid uncoordinated changes.
    Conflict,
    /// Intrusive action requiring explicit operator authorization.
    RequiresAuthorization,
    /// Insufficient evidence to justify a safe recommendation.
    InsufficientEvidence,
}

/// Explicit scope required for authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationScope {
    /// Scope for memory, zram, zswap, swap, and memory sysctl actions.
    MemoryOptimization,
    /// System-wide setup scope covering all supported subsystems.
    FullSetup,
}

/// Verification procedure to assert that an action has taken effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "detail")]
pub enum VerificationProcedure {
    /// Read back a sysctl parameter value from `/proc/sys/...`
    SysctlReadback { key: String, expected_value: u64 },
    /// Verify that a drop-in configuration file exists and matches expected SHA-256
    DropInFileExists {
        path: String,
        expected_sha256: String,
    },
    /// Verify sysfs parameter content matches expected string
    SysfsReadback {
        sysfs_path: String,
        expected_value: String,
    },
    /// No-op verification for preserved settings
    NoOpPreserved,
}

/// Reversal procedure to restore state upon rollback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "detail")]
pub enum ReversalProcedure {
    /// Restore sysctl runtime parameter to original value
    RestoreSysctl { key: String, original_value: u64 },
    /// Delete a HyperCore-managed drop-in file if its SHA-256 matches the recorded hash
    DeleteDropIn {
        target_path: String,
        expected_sha256: String,
    },
    /// Restore a sysfs setting to its original string value
    RestoreSysfs {
        sysfs_path: String,
        original_value: String,
    },
    /// Reset or unmount a zram block device
    ResetZramDevice { device_name: String },
    /// Preserved configuration requires no rollback mutation
    NoOpPreserved,
}

/// Persistence requirement for applied settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceRequirement {
    RuntimeOnly,
    DropInFile,
    RuntimeAndDropIn,
    VolatileSysfs,
}

/// Strongly typed, allowlisted configuration mutation.
///
/// Refuses arbitrary shell commands, arbitrary scripts, or unbounded file writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mutation_type", content = "payload")]
pub enum TypedMutation {
    /// Modify an allowlisted sysctl runtime setting
    ApplySysctl {
        key: String,
        target_value: u64,
        original_value: Option<u64>,
    },
    /// Write a managed sysctl drop-in file (e.g. `/etc/sysctl.d/99-xynoos-memory.conf`)
    WriteSysctlDropIn {
        dropin_filename: String,
        content: String,
        key: String,
        target_value: u64,
    },
    /// Configure ZRAM device parameters
    ConfigureZramDevice {
        device_name: String,
        disksize_bytes: u64,
        algorithm: String,
        priority: i32,
    },
    /// Configure ZSWAP runtime sysfs parameters
    ConfigureZswap {
        parameter: String,
        target_value: String,
        original_value: Option<String>,
    },
    /// Configure Transparent Huge Pages mode (runtime)
    ConfigureTransparentHugePages {
        mode: String,
        original_mode: Option<String>,
    },
}

/// Fully auditable, versioned configuration action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationAction {
    pub schema_version: u32,
    pub action_id: String,
    pub target_subsystem: ConfigSubsystem,
    pub target_setting: String,
    pub current_observed_value: String,
    pub desired_value: String,
    pub current_ownership: ConfigurationOwnership,
    pub required_privileges: PrivilegeLevel,
    pub required_capability: String,
    pub expected_benefit: String,
    pub potential_side_effects: String,
    pub confidence: ConfidenceLevel,
    pub reversibility: ReversibilityGuarantee,
    pub classification: ActionClassification,
    pub authorization_scope: AuthorizationScope,
    pub preconditions: Vec<String>,
    pub dependencies: Vec<String>,
    pub conflict_status: Option<String>,
    pub verification_procedure: VerificationProcedure,
    pub reversal_procedure: ReversalProcedure,
    pub persistence_requirements: PersistenceRequirement,
    pub mutation: Option<TypedMutation>,
}

impl ConfigurationAction {
    /// Whether this action requires privileged system execution to apply.
    pub fn is_privileged(&self) -> bool {
        self.required_privileges == PrivilegeLevel::RequiresRoot
    }

    /// Whether this action represents a mutation (Configure or Improve) rather than Preserve.
    pub fn is_mutation(&self) -> bool {
        matches!(
            self.classification,
            ActionClassification::Configure | ActionClassification::Improve
        ) && self.mutation.is_some()
    }

    /// Validate that this action conforms to schema and safety rules.
    pub fn validate(&self) -> Result<(), String> {
        if self.action_id.trim().is_empty() {
            return Err("Action ID cannot be empty".into());
        }
        if self.target_setting.trim().is_empty() {
            return Err("Target setting cannot be empty".into());
        }
        if self.is_mutation() && self.mutation.is_none() {
            return Err(format!(
                "Action {} is marked as mutation but has no TypedMutation payload",
                self.action_id
            ));
        }
        Ok(())
    }
}
