//! Explicit authorization models separating unprivileged planning from privileged execution.
#![forbid(unsafe_code)]

use super::action::AuthorizationScope;
use serde::{Deserialize, Serialize};

/// Cryptographically bound or explicit operator authorization token.
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
