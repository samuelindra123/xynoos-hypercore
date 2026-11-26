//! Crash-resilient durable journal for persistent system configuration operations.
#![forbid(unsafe_code)]

use super::fs_safety::{atomic_write_file, safe_read_bounded, verify_not_symlink};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

pub const CONFIG_JOURNAL_SCHEMA_VERSION: u32 = 1;

/// Operational phase of a configuration transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigTxPhase {
    /// Action plan validated and recorded prior to mutation.
    Prepared,
    /// Mutations actively being applied to kernel/sysfs/drop-ins.
    Applying,
    /// All mutations successfully applied and verified.
    Committed,
    /// Reversal is actively in progress.
    Restoring,
    /// Configuration successfully returned to prior baseline.
    Restored,
    /// Execution failed or encountered unresolvable conflict.
    Failed,
    /// Execution aborted due to pre-flight check or authorization failure.
    Aborted,
}

/// A single recorded step within a configuration transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigJournalItem {
    pub action_id: String,
    pub target_key: String,
    pub target_file: Option<String>,
    pub original_value: Option<String>,
    pub applied_value: String,
    pub is_applied: bool,
}

/// Durable on-disk representation of an in-flight or completed configuration transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigJournal {
    pub schema_version: u32,
    pub transaction_id: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub phase: ConfigTxPhase,
    pub items: Vec<ConfigJournalItem>,
    pub failure_reason: Option<String>,
}

impl ConfigJournal {
    /// Initialize a new prepared configuration journal.
    pub fn new(transaction_id: String, now_ms: u64) -> Self {
        Self {
            schema_version: CONFIG_JOURNAL_SCHEMA_VERSION,
            transaction_id,
            created_at_unix_ms: now_ms,
            updated_at_unix_ms: now_ms,
            phase: ConfigTxPhase::Prepared,
            items: Vec::new(),
            failure_reason: None,
        }
    }

    /// Load existing journal from disk with bounded size and permission validation.
    pub fn load_from(path: &Path) -> io::Result<Self> {
        let content = safe_read_bounded(path, 5 * 1024 * 1024)?;
        serde_json::from_str(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    /// Atomically persist the journal with unique temporary file and fsync.
    pub fn persist(&mut self, path: &Path, now_ms: u64) -> io::Result<()> {
        self.updated_at_unix_ms = now_ms;
        let payload = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        atomic_write_file(path, payload.as_bytes(), 0o600)
    }

    /// Add a step to the journal.
    pub fn record_item(&mut self, item: ConfigJournalItem) {
        self.items.push(item);
    }

    /// Clean up the journal file upon successful completion or clean rollback, refusing symlinks.
    pub fn cleanup(path: &Path) -> io::Result<()> {
        if path.exists() {
            verify_not_symlink(path)?;
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
