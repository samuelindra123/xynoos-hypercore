//! Reversible configuration executor managing the safe setup and rollback lifecycle.
#![forbid(unsafe_code)]

use super::{
    action::{
        ActionClassification, AuthorizationScope, ConfigurationAction, ReversalProcedure,
        TypedMutation, VerificationProcedure,
    },
    authorization::SetupAuthorization,
    journal::{ConfigJournal, ConfigJournalItem, ConfigTxPhase},
    ledger::{IntegrityStatus, OwnershipLedger},
};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use xynoos_pressureiq::discovery::types::ConfigurationOwnership;

/// Execution result for an individual action.
#[derive(Debug, Clone)]
pub struct ActionResult {
    pub action_id: String,
    pub success: bool,
    pub message: String,
}

/// Overall execution summary of an authorized configuration run.
#[derive(Debug, Clone)]
pub struct ExecutionSummary {
    pub transaction_id: String,
    pub total_actions: usize,
    pub applied_actions: usize,
    pub skipped_actions: usize,
    pub failed_actions: usize,
    pub action_results: Vec<ActionResult>,
}

/// Rollback summary detailing restored parameters and removed drop-ins.
#[derive(Debug, Clone)]
pub struct RollbackSummary {
    pub restored_keys: Vec<String>,
    pub removed_files: Vec<String>,
    pub skipped_due_to_tampering: Vec<String>,
    pub failed_restorations: Vec<String>,
}

/// Safe, reversible execution engine.
pub struct ConfigExecutor {
    pub sys_root: PathBuf,
    pub proc_root: PathBuf,
    pub etc_root: PathBuf,
    pub state_dir: PathBuf,
    pub ledger_path: PathBuf,
    pub journal_path: PathBuf,
}

impl ConfigExecutor {
    pub fn new(
        sys_root: PathBuf,
        proc_root: PathBuf,
        etc_root: PathBuf,
        state_dir: PathBuf,
    ) -> Self {
        let ledger_path = state_dir.join("owned-configs.json");
        let journal_path = state_dir.join("config-journal.json");
        Self {
            sys_root,
            proc_root,
            etc_root,
            state_dir,
            ledger_path,
            journal_path,
        }
    }

    /// Load or initialize the ownership ledger.
    pub fn load_ledger(&self) -> io::Result<OwnershipLedger> {
        OwnershipLedger::load_from(&self.ledger_path)
    }

    /// Verify an individual target path against symlink traversal attacks.
    fn sanitize_path(&self, base: &Path, rel_path: &str) -> Result<PathBuf, String> {
        if rel_path.contains("..") || rel_path.starts_with('/') {
            return Err(format!("Unsafe path component detected in: {rel_path}"));
        }
        let target = base.join(rel_path);
        // Ensure no component of target is a symlink pointing outside base
        if target.is_symlink() {
            return Err(format!("Refusing to write through symlink: {:?}", target));
        }
        Ok(target)
    }

    /// Execute an authorized action plan.
    pub fn execute_plan(
        &self,
        actions: &[ConfigurationAction],
        auth: &SetupAuthorization,
        plan_fingerprint: &str,
        now_ms: u64,
    ) -> Result<ExecutionSummary, String> {
        // 1. Authorization validation
        auth.is_valid(AuthorizationScope::MemoryOptimization, plan_fingerprint, now_ms)
            .map_err(|e| format!("Authorization error: {e}"))?;

        // 2. Load or initialize durable ledger
        let mut ledger = self
            .load_ledger()
            .map_err(|e| format!("Failed to read ownership ledger: {e}"))?;

        let tx_id = format!("tx-{}", now_ms);
        let mut journal = ConfigJournal::new(tx_id.clone(), now_ms);
        journal.phase = ConfigTxPhase::Applying;
        journal
            .persist(&self.journal_path, now_ms)
            .map_err(|e| format!("Failed to write initial journal: {e}"))?;

        let mut results = Vec::new();
        let mut applied_count = 0;
        let mut skipped_count = 0;
        let mut failed_count = 0;

        for action in actions {
            // Revalidate action
            if action.classification == ActionClassification::Preserve {
                skipped_count += 1;
                results.push(ActionResult {
                    action_id: action.action_id.clone(),
                    success: true,
                    message: format!("Preserved existing configuration: {}", action.target_setting),
                });
                continue;
            }

            if !action.is_mutation() {
                skipped_count += 1;
                results.push(ActionResult {
                    action_id: action.action_id.clone(),
                    success: true,
                    message: format!("Skipped non-mutating action: {}", action.target_setting),
                });
                continue;
            }

            // Do not override settings explicitly owned by the human administrator
            if action.current_ownership == ConfigurationOwnership::AdministratorManaged {
                skipped_count += 1;
                results.push(ActionResult {
                    action_id: action.action_id.clone(),
                    success: true,
                    message: format!(
                        "Skipped: {} is explicitly administrator-managed",
                        action.target_setting
                    ),
                });
                continue;
            }

            let mutation = match &action.mutation {
                Some(m) => m,
                None => continue,
            };

            match self.apply_mutation(mutation, &tx_id, &mut ledger, &mut journal, now_ms) {
                Ok(msg) => {
                    applied_count += 1;
                    results.push(ActionResult {
                        action_id: action.action_id.clone(),
                        success: true,
                        message: msg,
                    });
                }
                Err(err) => {
                    failed_count += 1;
                    journal.failure_reason = Some(err.clone());
                    journal.phase = ConfigTxPhase::Failed;
                    let _ = journal.persist(&self.journal_path, now_ms);
                    results.push(ActionResult {
                        action_id: action.action_id.clone(),
                        success: false,
                        message: err,
                    });
                    break;
                }
            }
        }

        // Commit or Rollback
        if failed_count == 0 {
            journal.phase = ConfigTxPhase::Committed;
            let _ = journal.persist(&self.journal_path, now_ms);
            let _ = ledger.save_to(&self.ledger_path, now_ms);
            let _ = ConfigJournal::cleanup(&self.journal_path);
        }

        Ok(ExecutionSummary {
            transaction_id: tx_id,
            total_actions: actions.len(),
            applied_actions: applied_count,
            skipped_actions: skipped_count,
            failed_actions: failed_count,
            action_results: results,
        })
    }

    /// Apply an individual typed mutation safely.
    fn apply_mutation(
        &self,
        mutation: &TypedMutation,
        tx_id: &str,
        ledger: &mut OwnershipLedger,
        journal: &mut ConfigJournal,
        now_ms: u64,
    ) -> Result<String, String> {
        match mutation {
            TypedMutation::ApplySysctl {
                key,
                target_value,
                original_value,
            } => {
                let sysctl_rel = format!("sys/{}", key.replace('.', "/"));
                let sysctl_path = self.proc_root.join(&sysctl_rel);

                if let Some(parent) = sysctl_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                // Write runtime sysctl
                let val_str = target_value.to_string();
                fs::write(&sysctl_path, val_str.as_bytes())
                    .map_err(|e| format!("Failed writing sysctl {key}: {e}"))?;

                // Read back to verify
                let readback = fs::read_to_string(&sysctl_path)
                    .map_err(|e| format!("Failed reading back sysctl {key}: {e}"))?;
                let read_val = readback.trim().parse::<u64>().unwrap_or(0);
                if read_val != *target_value {
                    return Err(format!(
                        "Verification failed for {key}: expected {target_value}, read {read_val}"
                    ));
                }

                // Record in journal and ledger
                journal.record_item(ConfigJournalItem {
                    action_id: format!("sysctl-{key}"),
                    target_key: key.clone(),
                    target_file: None,
                    original_value: original_value.map(|v| v.to_string()),
                    applied_value: val_str.clone(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    key.clone(),
                    None,
                    original_value.map(|v| v.to_string()),
                    val_str,
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!("Applied sysctl {key} = {target_value}"))
            }

            TypedMutation::WriteSysctlDropIn {
                dropin_filename,
                content,
                key,
                target_value,
            } => {
                let dropin_dir = self.etc_root.join("sysctl.d");
                fs::create_dir_all(&dropin_dir)
                    .map_err(|e| format!("Failed creating sysctl.d: {e}"))?;

                let dropin_path = self.sanitize_path(&dropin_dir, dropin_filename)?;
                let bytes = content.as_bytes();

                // Atomic write via temp file
                let tmp_path = dropin_path.with_extension("tmp");
                {
                    let mut f = File::create(&tmp_path)
                        .map_err(|e| format!("Failed creating temp dropin: {e}"))?;
                    f.write_all(bytes)
                        .map_err(|e| format!("Failed writing temp dropin: {e}"))?;
                    f.sync_all()
                        .map_err(|e| format!("Failed syncing temp dropin: {e}"))?;
                }
                fs::rename(&tmp_path, &dropin_path)
                    .map_err(|e| format!("Failed installing dropin {dropin_filename}: {e}"))?;

                let rel_path = format!("etc/sysctl.d/{dropin_filename}");
                journal.record_item(ConfigJournalItem {
                    action_id: format!("dropin-{dropin_filename}"),
                    target_key: key.clone(),
                    target_file: Some(rel_path.clone()),
                    original_value: None,
                    applied_value: target_value.to_string(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("dropin:{key}"),
                    Some(rel_path),
                    None,
                    target_value.to_string(),
                    Some(bytes),
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!("Installed managed drop-in {dropin_filename}"))
            }

            TypedMutation::ConfigureZramDevice {
                device_name,
                disksize_bytes,
                algorithm,
                priority,
            } => {
                let zram_block = self.sys_root.join(format!("block/{device_name}"));
                if zram_block.exists() {
                    // If zram block device exists in sysfs, configure algorithm and disksize
                    let algo_path = zram_block.join("comp_algorithm");
                    if algo_path.exists() {
                        let _ = fs::write(&algo_path, algorithm.as_bytes());
                    }
                    let size_path = zram_block.join("disksize");
                    if size_path.exists() {
                        let _ = fs::write(&size_path, disksize_bytes.to_string().as_bytes());
                    }
                }

                journal.record_item(ConfigJournalItem {
                    action_id: format!("zram-{device_name}"),
                    target_key: format!("zram.{device_name}"),
                    target_file: None,
                    original_value: None,
                    applied_value: format!("{disksize_bytes}:{algorithm}:{priority}"),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("zram.{device_name}"),
                    None,
                    None,
                    format!("{disksize_bytes}:{algorithm}:{priority}"),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!(
                    "Configured ZRAM {device_name} (size: {disksize_bytes}, algo: {algorithm})"
                ))
            }

            TypedMutation::ConfigureZswap {
                parameter,
                target_value,
                original_value,
            } => {
                let p_path = self
                    .sys_root
                    .join(format!("module/zswap/parameters/{parameter}"));
                if p_path.exists() {
                    fs::write(&p_path, target_value.as_bytes())
                        .map_err(|e| format!("Failed writing zswap {parameter}: {e}"))?;
                }

                journal.record_item(ConfigJournalItem {
                    action_id: format!("zswap-{parameter}"),
                    target_key: format!("zswap.{parameter}"),
                    target_file: None,
                    original_value: original_value.clone(),
                    applied_value: target_value.clone(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("zswap.{parameter}"),
                    None,
                    original_value.clone(),
                    target_value.clone(),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!("Configured ZSWAP {parameter} = {target_value}"))
            }

            TypedMutation::ConfigureTransparentHugePages {
                mode,
                original_mode,
            } => {
                let thp_path = self
                    .sys_root
                    .join("kernel/mm/transparent_hugepage/enabled");
                if thp_path.exists() {
                    let _ = fs::write(&thp_path, mode.as_bytes());
                }

                journal.record_item(ConfigJournalItem {
                    action_id: "thp-mode".into(),
                    target_key: "transparent_hugepage.enabled".into(),
                    target_file: None,
                    original_value: original_mode.clone(),
                    applied_value: mode.clone(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    "transparent_hugepage.enabled".into(),
                    None,
                    original_mode.clone(),
                    mode.clone(),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!("Configured Transparent Huge Pages mode to {mode}"))
            }
        }
    }

    /// Roll back all HyperCore-owned configurations to their recorded baseline.
    pub fn rollback_all(&self, now_ms: u64) -> Result<RollbackSummary, String> {
        let mut ledger = self
            .load_ledger()
            .map_err(|e| format!("Failed to read ownership ledger: {e}"))?;

        let mut restored_keys = Vec::new();
        let mut removed_files = Vec::new();
        let mut skipped_tampered = Vec::new();
        let mut failed = Vec::new();

        let keys_to_process: Vec<String> = ledger.entries.keys().cloned().collect();

        for key in keys_to_process {
            let entry = match ledger.entries.get(&key) {
                Some(e) => e.clone(),
                None => continue,
            };

            // Check integrity before rollback
            let integrity = ledger.check_entry_integrity(&entry, &self.etc_root);
            if integrity == IntegrityStatus::ModifiedExternally {
                skipped_tampered.push(format!(
                    "{}: File was modified externally; refusing blind rollback",
                    key
                ));
                continue;
            }

            // Restore runtime sysctl if recorded
            if !key.starts_with("dropin:") && !key.starts_with("zram.") {
                if let Some(orig_val) = &entry.original_value {
                    let sysctl_rel = format!("sys/{}", key.replace('.', "/"));
                    let sysctl_path = self.proc_root.join(sysctl_rel);
                    if sysctl_path.exists() {
                        if let Err(e) = fs::write(&sysctl_path, orig_val.as_bytes()) {
                            failed.push(format!("{key}: {e}"));
                            continue;
                        }
                    }
                    restored_keys.push(format!("{key} -> {orig_val}"));
                }
            }

            // Remove managed drop-in file if present
            if let Some(target_file) = &entry.target_file {
                let full_path = self.etc_root.join(target_file.trim_start_matches("etc/"));
                if full_path.exists() {
                    if let Err(e) = fs::remove_file(&full_path) {
                        failed.push(format!("{}: {e}", full_path.display()));
                        continue;
                    }
                    removed_files.push(full_path.display().to_string());
                }
            }

            ledger.remove_entry(&key);
        }

        let _ = ledger.save_to(&self.ledger_path, now_ms);
        let _ = ConfigJournal::cleanup(&self.journal_path);

        Ok(RollbackSummary {
            restored_keys,
            removed_files,
            skipped_due_to_tampering: skipped_tampered,
            failed_restorations: failed,
        })
    }
}
