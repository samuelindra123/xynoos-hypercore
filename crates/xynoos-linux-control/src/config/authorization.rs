//! Explicit authorization models separating unprivileged planning from privileged execution.
#![forbid(unsafe_code)]

use super::action::AuthorizationScope;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

pub const RUNTIME_AUTH_SCHEMA_VERSION: u32 = 1;

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
        Self {
            authorized_by: authorized_by.into(),
            scope,
            plan_fingerprint: plan_fingerprint.into(),
            allowed_action_ids: None,
            created_at_unix_ms: now_ms,
            expires_at_unix_ms: now_ms.saturating_add(validity_duration_ms),
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
            expires_at_unix_ms: if validity_duration_ms == 0 {
                u64::MAX
            } else {
                now_ms.saturating_add(validity_duration_ms)
            },
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
        // Reject path traversal and root cgroup escape attempts
        if target_identity.is_empty()
            || target_identity.contains("..")
            || target_identity == "/"
            || target_identity == "/sys/fs/cgroup"
        {
            return false;
        }
        self.allowed_target_identities.iter().any(|pattern| {
            if pattern == "*" {
                // Permitted for any non-root target within cgroups
                !target_identity.is_empty() && target_identity != "/"
            } else if pattern.ends_with('*') {
                target_identity.starts_with(pattern.trim_end_matches('*'))
            } else {
                pattern == target_identity
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
        if now_ms > self.expires_at_unix_ms {
            return Err("Runtime authorization contract has expired");
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

    /// Load the contract from disk if present.
    pub fn load_from(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(path) {
                // Reject world-writable contract files to prevent untrusted manipulation
                if meta.permissions().mode() & 0o002 != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Insecure authorization contract file: world-writable",
                    ));
                }
            }
        }
        let mut f = File::open(path)?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        serde_json::from_str(&s)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    /// Save the contract to disk atomically.
    pub fn save_to(&self, path: &Path, _now_ms: u64) -> io::Result<()> {
        if path.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Refusing to save authorization through a symlink",
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_path = path.with_extension("tmp");
        let payload = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        {
            let mut file = File::create(&temp_path)?;
            file.write_all(payload.as_bytes())?;
            file.sync_all()?;
        }

        fs::rename(&temp_path, path)?;
        Ok(())
    }
}
