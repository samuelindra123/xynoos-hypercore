//! Explicit authorization models separating unprivileged planning from privileged execution.
#![forbid(unsafe_code)]

use super::{
    action::AuthorizationScope,
    fs_safety::{atomic_write_file, safe_read_bounded, validate_cgroup_path},
};
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

pub const RUNTIME_AUTH_SCHEMA_VERSION: u32 = 1;
pub const MAX_SETUP_AUTH_DURATION_MS: u64 = 3_600_000; // Max 1 hour for setup authorization tokens
pub const MAX_RUNTIME_AUTH_DURATION_MS: u64 = 7 * 24 * 3600 * 1000; // Max 7 days for daemon contracts

/// Cryptographically bound or explicit operator authorization token for setup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupAuthorization {
    pub authorized_by: String,
    pub scope: AuthorizationScope,
    pub plan_fingerprint: String,
    pub allowed_action_ids: Option<Vec<String>>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

impl SetupAuthorization {
    /// Create a new one-time authorization token with a bounded validity window.
    pub fn new(
        authorized_by: impl Into<String>,
        scope: AuthorizationScope,
        plan_fingerprint: impl Into<String>,
        now_ms: u64,
        validity_duration_ms: u64,
    ) -> Self {
        let bounded_duration = if validity_duration_ms == 0 {
            MAX_SETUP_AUTH_DURATION_MS // Default to 1 hour max window
        } else {
            validity_duration_ms.min(MAX_SETUP_AUTH_DURATION_MS)
        };
        Self {
            authorized_by: authorized_by.into(),
            scope,
            plan_fingerprint: plan_fingerprint.into(),
            allowed_action_ids: None,
            created_at_unix_ms: now_ms,
            expires_at_unix_ms: now_ms.saturating_add(bounded_duration),
        }
    }

    /// Attach an explicit set of authorized action IDs.
    pub fn with_allowed_actions(mut self, action_ids: Vec<String>) -> Self {
        self.allowed_action_ids = Some(action_ids);
        self
    }

    /// Check whether the authorization is active and covers the required scope and plan.
    pub fn is_valid(
        &self,
        required_scope: AuthorizationScope,
        current_plan_fingerprint: &str,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        if self.plan_fingerprint.is_empty() || current_plan_fingerprint.is_empty() {
            return Err("Plan fingerprint cannot be empty");
        }
        if now_ms < self.created_at_unix_ms {
            return Err("Authorization token creation timestamp is in the future");
        }
        if now_ms > self.expires_at_unix_ms {
            return Err("Authorization token has expired");
        }
        if self.plan_fingerprint != current_plan_fingerprint {
            return Err("Plan fingerprint mismatch: authorization does not match current plan");
        }
        match (self.scope, required_scope) {
            (AuthorizationScope::FullSetup, _) => Ok(()),
            (AuthorizationScope::MemoryOptimization, AuthorizationScope::MemoryOptimization) => {
                Ok(())
            }
            (AuthorizationScope::CpuOptimization, AuthorizationScope::CpuOptimization) => Ok(()),
            (AuthorizationScope::StorageOptimization, AuthorizationScope::StorageOptimization) => {
                Ok(())
            }
            (AuthorizationScope::PowerOptimization, AuthorizationScope::PowerOptimization) => {
                Ok(())
            }
            _ => Err("Granted authorization scope does not cover requested action scope"),
        }
    }

    /// Check whether an action ID is permitted by this authorization token.
    pub fn allows_action_id(&self, action_id: &str) -> bool {
        match &self.allowed_action_ids {
            Some(allowed) => allowed.iter().any(|id| id == action_id),
            None => true,
        }
    }
}

/// Strongly typed, bounded runtime authorization contract governing continuous daemon actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeAuthorizationContract {
    pub schema_version: u32,
    pub authorized_by: String,
    pub allowed_scopes: Vec<AuthorizationScope>,
    pub allowed_target_types: Vec<String>,
    pub allowed_target_identities: Vec<String>,
    pub allowed_controls: Vec<String>,
    pub max_cpu_weight: u32,
    pub min_cpu_weight: u32,
    pub max_memory_headroom_bytes: u64,
    pub max_intervention_duration_secs: u64,
    pub max_interventions_per_minute: u32,
    pub is_revoked: bool,
    pub revocation_reason: Option<String>,
    pub plan_fingerprint: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

impl Default for RuntimeAuthorizationContract {
    fn default() -> Self {
        Self {
            schema_version: RUNTIME_AUTH_SCHEMA_VERSION,
            authorized_by: "system".into(),
            allowed_scopes: vec![
                AuthorizationScope::MemoryOptimization,
                AuthorizationScope::CpuOptimization,
            ],
            allowed_target_types: vec!["cgroup_v2".into()],
            allowed_target_identities: vec!["*".into()],
            allowed_controls: vec![
                "cpu.weight".into(),
                "memory.low".into(),
                "memory.high".into(),
            ],
            max_cpu_weight: 10000,
            min_cpu_weight: 10,
            max_memory_headroom_bytes: 512 * 1024 * 1024,
            max_intervention_duration_secs: 120,
            max_interventions_per_minute: 6,
            is_revoked: false,
            revocation_reason: None,
            plan_fingerprint: String::new(),
            created_at_unix_ms: 0,
            expires_at_unix_ms: u64::MAX,
        }
    }
}

impl RuntimeAuthorizationContract {
    /// Create a new scoped runtime authorization contract.
    pub fn new(
        authorized_by: impl Into<String>,
        plan_fingerprint: impl Into<String>,
        now_ms: u64,
        validity_duration_ms: u64,
    ) -> Self {
        let expires_at = if validity_duration_ms == 0 {
            now_ms.saturating_add(MAX_RUNTIME_AUTH_DURATION_MS) // Default to bounded 7-day validity
        } else {
            now_ms.saturating_add(validity_duration_ms.min(MAX_RUNTIME_AUTH_DURATION_MS))
        };
        Self {
            schema_version: RUNTIME_AUTH_SCHEMA_VERSION,
            authorized_by: authorized_by.into(),
            allowed_scopes: vec![
                AuthorizationScope::MemoryOptimization,
                AuthorizationScope::CpuOptimization,
            ],
            allowed_target_types: vec!["cgroup_v2".into()],
            allowed_target_identities: vec!["*".into()],
            allowed_controls: vec![
                "cpu.weight".into(),
                "memory.low".into(),
                "memory.high".into(),
            ],
            max_cpu_weight: 10000,
            min_cpu_weight: 10,
            max_memory_headroom_bytes: 512 * 1024 * 1024,
            max_intervention_duration_secs: 120,
            max_interventions_per_minute: 6,
            is_revoked: false,
            revocation_reason: None,
            plan_fingerprint: plan_fingerprint.into(),
            created_at_unix_ms: now_ms,
            expires_at_unix_ms: expires_at,
        }
    }

    /// Check if a specific control is allowed by this authorization.
    pub fn is_control_allowed(&self, control: &str) -> bool {
        if self.is_revoked {
            return false;
        }
        self.allowed_controls.iter().any(|c| c == control)
    }

    /// Check if target identity is covered by authorized identities.
    pub fn is_target_allowed(&self, target_identity: &str) -> bool {
        if self.is_revoked {
            return false;
        }
        let validated_target = match validate_cgroup_path(target_identity) {
            Ok(v) => v,
            Err(_) => return false,
        };

        self.allowed_target_identities.iter().any(|pattern| {
            if pattern == "*" || pattern == "/sys/fs/cgroup/*" {
                // Permitted for any non-root valid cgroup target
                true
            } else if let Some(prefix) = pattern.strip_suffix("/*") {
                let clean_prefix = prefix
                    .strip_prefix("/sys/fs/cgroup/")
                    .unwrap_or(prefix)
                    .trim_matches('/');
                validated_target.starts_with(clean_prefix)
                    && validated_target.len() > clean_prefix.len()
            } else if let Some(prefix) = pattern.strip_suffix('*') {
                let clean_prefix = prefix
                    .strip_prefix("/sys/fs/cgroup/")
                    .unwrap_or(prefix)
                    .trim_matches('/');
                validated_target.starts_with(clean_prefix)
            } else {
                let clean_pattern = pattern
                    .strip_prefix("/sys/fs/cgroup/")
                    .unwrap_or(pattern)
                    .trim_matches('/');
                clean_pattern == validated_target
            }
        })
    }

    /// Check whether a proposed runtime intervention is permitted.
    pub fn can_intervene(
        &self,
        control: &str,
        target_identity: &str,
        requested_duration_secs: u64,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        if self.is_revoked {
            return Err("Runtime authorization has been revoked");
        }
        if now_ms < self.created_at_unix_ms {
            return Err("Runtime authorization contract creation timestamp is in the future");
        }
        if now_ms > self.expires_at_unix_ms {
            return Err("Runtime authorization contract has expired");
        }
        if !self.allowed_target_types.iter().any(|t| t == "cgroup_v2") {
            return Err("Cgroup v2 target type is not permitted in authorized scopes");
        }
        if !self.is_control_allowed(control) {
            return Err("Proposed control is not permitted in authorized scope");
        }
        if !self.is_target_allowed(target_identity) {
            return Err("Target identity is outside authorized cgroup scope");
        }
        if requested_duration_secs > self.max_intervention_duration_secs {
            return Err("Intervention duration exceeds maximum permitted authorization lease");
        }
        Ok(())
    }

    /// Immediately revoke runtime authorization (emergency disablement).
    pub fn revoke(&mut self, reason: &str) {
        self.is_revoked = true;
        self.revocation_reason = Some(reason.to_string());
    }

    /// Load the contract from disk if present with security and permission validation.
    pub fn load_from(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = safe_read_bounded(path, 1024 * 1024)?;
        serde_json::from_str(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    /// Save the contract to disk atomically with fsync and safe permissions.
    pub fn save_to(&self, path: &Path, _now_ms: u64) -> io::Result<()> {
        let payload = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        atomic_write_file(path, payload.as_bytes(), 0o644)
    }
}
