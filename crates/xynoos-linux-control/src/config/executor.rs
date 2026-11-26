//! Reversible configuration executor managing the safe setup and rollback lifecycle.
#![forbid(unsafe_code)]

use super::{
    action::{ActionClassification, ConfigurationAction, TypedMutation},
    authorization::SetupAuthorization,
    fs_safety::{
        atomic_write_file, sanitize_dropin_target_file, validate_cgroup_path, validate_device_name,
        validate_sysctl_key, verify_not_symlink,
    },
    journal::{ConfigJournal, ConfigJournalItem, ConfigTxPhase},
    ledger::{IntegrityStatus, OwnershipLedger},
};
use std::{fs, io, path::PathBuf};
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

    /// Execute an authorized action plan.
    pub fn execute_plan(
        &self,
        actions: &[ConfigurationAction],
        auth: &SetupAuthorization,
        plan_fingerprint: &str,
        now_ms: u64,
    ) -> Result<ExecutionSummary, String> {
        // 1. Authorization validation (fingerprint and expiration)
        if now_ms > auth.expires_at_unix_ms {
            return Err("Authorization token has expired".into());
        }
        if auth.plan_fingerprint != plan_fingerprint {
            return Err(
                "Plan fingerprint mismatch: authorization does not match current plan".into(),
            );
        }

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
            // Check action authorization
            if !auth.allows_action_id(&action.action_id) {
                skipped_count += 1;
                results.push(ActionResult {
                    action_id: action.action_id.clone(),
                    success: false,
                    message: format!(
                        "Skipped: action {} is not in authorized action list",
                        action.action_id
                    ),
                });
                continue;
            }

            if action.is_mutation() {
                if let Err(e) = auth.is_valid(action.authorization_scope, plan_fingerprint, now_ms)
                {
                    skipped_count += 1;
                    results.push(ActionResult {
                        action_id: action.action_id.clone(),
                        success: false,
                        message: format!("Skipped: authorization scope error: {e}"),
                    });
                    continue;
                }
            }

            // Revalidate action
            if action.classification == ActionClassification::Preserve {
                skipped_count += 1;
                results.push(ActionResult {
                    action_id: action.action_id.clone(),
                    success: true,
                    message: format!(
                        "Preserved existing configuration: {}",
                        action.target_setting
                    ),
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
            ledger
                .save_to(&self.ledger_path, now_ms)
                .map_err(|e| format!("Failed to commit ownership ledger: {e}"))?;
            let _ = ConfigJournal::cleanup(&self.journal_path);
        } else {
            // Transaction failed: roll back all partial items recorded in this journal
            for item in journal.items.iter().rev() {
                if item.is_applied {
                    if let Some(orig) = &item.original_value {
                        if item.target_key.starts_with("sysctl.") || !item.target_key.contains('.')
                        {
                            let k = item
                                .target_key
                                .strip_prefix("sysctl.")
                                .unwrap_or(&item.target_key);
                            let sysctl_rel = format!("sys/{}", k.replace('.', "/"));
                            let sysctl_path = self.proc_root.join(sysctl_rel);
                            if sysctl_path.exists() {
                                let _ = fs::write(&sysctl_path, orig.as_bytes());
                            }
                        }
                    }
                    if let Some(rel_f) = &item.target_file {
                        let full_f = self
                            .etc_root
                            .join(rel_f.trim_start_matches("etc/").trim_start_matches('/'));
                        if full_f.exists() {
                            let _ = fs::remove_file(&full_f);
                        }
                    }
                    ledger.remove_entry(&item.target_key);
                }
            }
            journal.phase = ConfigTxPhase::Restored;
            let _ = journal.persist(&self.journal_path, now_ms);
            let _ = ledger.save_to(&self.ledger_path, now_ms);
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
                validate_sysctl_key(key).map_err(|e| format!("Invalid sysctl key: {e}"))?;
                let sysctl_rel = format!("sys/{}", key.replace('.', "/"));
                let sysctl_path = self.proc_root.join(&sysctl_rel);

                if let Some(parent) = sysctl_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                verify_not_symlink(&sysctl_path)
                    .map_err(|e| format!("Security violation for sysctl {key}: {e}"))?;

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
                let dropin_path = sanitize_dropin_target_file(
                    &format!("sysctl.d/{dropin_filename}"),
                    &self.etc_root,
                )?;
                let bytes = content.as_bytes();

                atomic_write_file(&dropin_path, bytes, 0o644)
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
                validate_device_name(device_name)
                    .map_err(|e| format!("Invalid zram device name: {e}"))?;
                let swaps_path = self.proc_root.join("swaps");
                if swaps_path.exists() {
                    if let Ok(swaps_content) = fs::read_to_string(&swaps_path) {
                        if swaps_content.lines().any(|l| l.contains(device_name)) {
                            return Err(format!(
                                "ZRAM device {device_name} is actively populated swap. Refusing unsafe live reconfiguration."
                            ));
                        }
                    }
                }

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
                    let readback = fs::read_to_string(&p_path)
                        .map_err(|e| format!("Failed reading back zswap {parameter}: {e}"))?;
                    if readback.trim() != target_value.trim() {
                        return Err(format!(
                            "Zswap verification failed for {parameter}: expected {target_value}, read {readback}"
                        ));
                    }
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
                let thp_path = self.sys_root.join("kernel/mm/transparent_hugepage/enabled");
                if thp_path.exists() {
                    fs::write(&thp_path, mode.as_bytes())
                        .map_err(|e| format!("Failed writing THP mode: {e}"))?;
                    let readback = fs::read_to_string(&thp_path)
                        .map_err(|e| format!("Failed reading back THP mode: {e}"))?;
                    let expected_bracket = format!("[{mode}]");
                    if readback.trim() != mode && !readback.contains(&expected_bracket) {
                        return Err(format!(
                            "THP verification failed: expected {expected_bracket} or {mode}, read {readback}"
                        ));
                    }
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

            TypedMutation::ApplyCpuGovernor {
                cpu_range,
                governor,
                original_governor,
            } => {
                let allowed = [
                    "schedutil",
                    "powersave",
                    "performance",
                    "ondemand",
                    "conservative",
                ];
                if !allowed.contains(&governor.as_str()) {
                    return Err(format!(
                        "Disallowed or unrecognized CPU governor: {governor}"
                    ));
                }
                let cpu_root = self.sys_root.join("devices/system/cpu");
                if cpu_root.exists() {
                    let mut entries: Vec<_> = fs::read_dir(&cpu_root)
                        .map_err(|e| format!("Failed reading sysfs cpu root: {e}"))?
                        .flatten()
                        .filter(|e| e.file_name().to_string_lossy().starts_with("cpu"))
                        .filter(|e| {
                            e.file_name()
                                .to_string_lossy()
                                .strip_prefix("cpu")
                                .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
                        })
                        .collect();
                    entries.sort_by_key(|e| e.file_name());

                    for entry in entries {
                        let gov_path = entry.path().join("cpufreq/scaling_governor");
                        if gov_path.exists() {
                            fs::write(&gov_path, governor.as_bytes())
                                .map_err(|e| format!("Failed writing CPU governor: {e}"))?;
                            let readback = fs::read_to_string(&gov_path)
                                .map_err(|e| format!("Failed reading back CPU governor: {e}"))?;
                            if readback.trim() != governor.trim() {
                                return Err(format!(
                                    "CPU governor verification failed: expected {governor}, read {readback}"
                                ));
                            }
                        }
                    }
                }

                journal.record_item(ConfigJournalItem {
                    action_id: format!("cpu-gov-{cpu_range}"),
                    target_key: format!("cpu.governor.{cpu_range}"),
                    target_file: None,
                    original_value: original_governor.clone(),
                    applied_value: governor.clone(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("cpu.governor.{cpu_range}"),
                    None,
                    original_governor.clone(),
                    governor.clone(),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!(
                    "Applied CPU governor {governor} to {cpu_range} CPUs"
                ))
            }

            TypedMutation::ApplyCpuEpp {
                cpu_range,
                epp,
                original_epp,
            } => {
                let allowed = [
                    "performance",
                    "balance_performance",
                    "balance_power",
                    "power",
                    "default",
                ];
                if !allowed.contains(&epp.as_str()) {
                    return Err(format!("Disallowed or unrecognized CPU EPP: {epp}"));
                }
                let cpu_root = self.sys_root.join("devices/system/cpu");
                if cpu_root.exists() {
                    let mut entries: Vec<_> = fs::read_dir(&cpu_root)
                        .map_err(|e| format!("Failed reading sysfs cpu root: {e}"))?
                        .flatten()
                        .filter(|e| e.file_name().to_string_lossy().starts_with("cpu"))
                        .filter(|e| {
                            e.file_name()
                                .to_string_lossy()
                                .strip_prefix("cpu")
                                .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
                        })
                        .collect();
                    entries.sort_by_key(|e| e.file_name());

                    for entry in entries {
                        let epp_path = entry.path().join("cpufreq/energy_performance_preference");
                        if epp_path.exists() {
                            fs::write(&epp_path, epp.as_bytes())
                                .map_err(|e| format!("Failed writing CPU EPP: {e}"))?;
                            let readback = fs::read_to_string(&epp_path)
                                .map_err(|e| format!("Failed reading back CPU EPP: {e}"))?;
                            if readback.trim() != epp.trim() {
                                return Err(format!(
                                    "CPU EPP verification failed: expected {epp}, read {readback}"
                                ));
                            }
                        }
                    }
                }

                journal.record_item(ConfigJournalItem {
                    action_id: format!("cpu-epp-{cpu_range}"),
                    target_key: format!("cpu.epp.{cpu_range}"),
                    target_file: None,
                    original_value: original_epp.clone(),
                    applied_value: epp.clone(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("cpu.epp.{cpu_range}"),
                    None,
                    original_epp.clone(),
                    epp.clone(),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!("Applied CPU EPP {epp} to {cpu_range} CPUs"))
            }

            TypedMutation::ApplyCpuFrequencyBounds {
                cpu_range,
                min_freq_khz,
                max_freq_khz,
                original_min,
                original_max,
            } => {
                let cpu_root = self.sys_root.join("devices/system/cpu");
                if cpu_root.exists() {
                    for entry in fs::read_dir(&cpu_root).into_iter().flatten().flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("cpu")
                            && name
                                .strip_prefix("cpu")
                                .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
                        {
                            if let Some(min_f) = min_freq_khz {
                                let min_p = entry.path().join("cpufreq/scaling_min_freq");
                                if min_p.exists() {
                                    let _ = fs::write(&min_p, min_f.to_string().as_bytes());
                                }
                            }
                            if let Some(max_f) = max_freq_khz {
                                let max_p = entry.path().join("cpufreq/scaling_max_freq");
                                if max_p.exists() {
                                    let _ = fs::write(&max_p, max_f.to_string().as_bytes());
                                }
                            }
                        }
                    }
                }

                let applied_str = format!("{:?}:{:?}", min_freq_khz, max_freq_khz);
                let orig_str = format!("{:?}:{:?}", original_min, original_max);
                journal.record_item(ConfigJournalItem {
                    action_id: format!("cpu-bounds-{cpu_range}"),
                    target_key: format!("cpu.freq_bounds.{cpu_range}"),
                    target_file: None,
                    original_value: Some(orig_str.clone()),
                    applied_value: applied_str.clone(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("cpu.freq_bounds.{cpu_range}"),
                    None,
                    Some(orig_str),
                    applied_str,
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!("Applied CPU frequency bounds to {cpu_range} CPUs"))
            }

            TypedMutation::ApplyBlockIoScheduler {
                device_name,
                scheduler,
                original_scheduler,
            } => {
                validate_device_name(device_name)
                    .map_err(|e| format!("Unsafe block device name: {e}"))?;
                let allowed = ["none", "mq-deadline", "bfq", "kyber"];
                if !allowed.contains(&scheduler.as_str()) {
                    return Err(format!(
                        "Disallowed or unrecognized I/O scheduler: {scheduler}"
                    ));
                }
                let sched_path = self
                    .sys_root
                    .join(format!("block/{device_name}/queue/scheduler"));
                if sched_path.exists() {
                    fs::write(&sched_path, scheduler.as_bytes()).map_err(|e| {
                        format!("Failed writing I/O scheduler for {device_name}: {e}")
                    })?;
                    let readback = fs::read_to_string(&sched_path).map_err(|e| {
                        format!("Failed reading back I/O scheduler for {device_name}: {e}")
                    })?;
                    let expected_bracket = format!("[{scheduler}]");
                    if readback.trim() != scheduler && !readback.contains(&expected_bracket) {
                        return Err(format!(
                            "I/O scheduler verification failed for {device_name}: expected {expected_bracket} or {scheduler}, read {readback}"
                        ));
                    }
                }

                journal.record_item(ConfigJournalItem {
                    action_id: format!("storage-sched-{device_name}"),
                    target_key: format!("block.scheduler.{device_name}"),
                    target_file: None,
                    original_value: original_scheduler.clone(),
                    applied_value: scheduler.clone(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("block.scheduler.{device_name}"),
                    None,
                    original_scheduler.clone(),
                    scheduler.clone(),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!(
                    "Applied I/O scheduler {scheduler} to {device_name}"
                ))
            }

            TypedMutation::ApplyBlockReadAhead {
                device_name,
                read_ahead_kb,
                original_kb,
            } => {
                validate_device_name(device_name)
                    .map_err(|e| format!("Unsafe block device name: {e}"))?;
                if !(16..=2048).contains(read_ahead_kb) {
                    return Err(format!(
                        "Read-ahead value {read_ahead_kb} out of permitted bounds (16..=2048)"
                    ));
                }
                let ra_path = self
                    .sys_root
                    .join(format!("block/{device_name}/queue/read_ahead_kb"));
                if ra_path.exists() {
                    fs::write(&ra_path, read_ahead_kb.to_string().as_bytes())
                        .map_err(|e| format!("Failed writing read-ahead for {device_name}: {e}"))?;
                    let readback = fs::read_to_string(&ra_path).map_err(|e| {
                        format!("Failed reading back read-ahead for {device_name}: {e}")
                    })?;
                    if readback.trim() != read_ahead_kb.to_string() {
                        return Err(format!(
                            "Read-ahead verification failed for {device_name}: expected {read_ahead_kb}, read {readback}"
                        ));
                    }
                }

                journal.record_item(ConfigJournalItem {
                    action_id: format!("storage-ra-{device_name}"),
                    target_key: format!("block.read_ahead.{device_name}"),
                    target_file: None,
                    original_value: original_kb.map(|k| k.to_string()),
                    applied_value: read_ahead_kb.to_string(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("block.read_ahead.{device_name}"),
                    None,
                    original_kb.map(|k| k.to_string()),
                    read_ahead_kb.to_string(),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!(
                    "Applied read-ahead {read_ahead_kb} KiB to {device_name}"
                ))
            }

            TypedMutation::ApplyPowerProfile {
                profile,
                original_profile,
            } => {
                let allowed = ["balanced", "power-saver", "performance"];
                if !allowed.contains(&profile.as_str()) {
                    return Err(format!(
                        "Disallowed or unrecognized power profile: {profile}"
                    ));
                }
                let p_path = self.sys_root.join("firmware/acpi/platform_profile");
                if p_path.exists() {
                    fs::write(&p_path, profile.as_bytes())
                        .map_err(|e| format!("Failed writing platform profile: {e}"))?;
                    let readback = fs::read_to_string(&p_path)
                        .map_err(|e| format!("Failed reading back platform profile: {e}"))?;
                    if readback.trim() != profile.trim() {
                        return Err(format!(
                            "Power profile verification failed: expected {profile}, read {readback}"
                        ));
                    }
                }

                journal.record_item(ConfigJournalItem {
                    action_id: "power-profile".into(),
                    target_key: "power.profile".into(),
                    target_file: None,
                    original_value: original_profile.clone(),
                    applied_value: profile.clone(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    "power.profile".into(),
                    None,
                    original_profile.clone(),
                    profile.clone(),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!("Applied platform power profile: {profile}"))
            }

            TypedMutation::ApplyCgroupCpuWeight {
                cgroup_path,
                weight,
                original_weight,
            } => {
                if !(1..=10000).contains(weight) {
                    return Err(format!(
                        "Cgroup CPU weight {weight} out of bounds (1..=10000)"
                    ));
                }
                let cg_clean = validate_cgroup_path(cgroup_path)
                    .map_err(|e| format!("Invalid cgroup path: {e}"))?;
                let w_path = self
                    .sys_root
                    .join("fs/cgroup")
                    .join(&cg_clean)
                    .join("cpu.weight");
                verify_not_symlink(&w_path).map_err(|e| format!("Security violation: {e}"))?;
                if w_path.exists() {
                    fs::write(&w_path, weight.to_string().as_bytes())
                        .map_err(|e| format!("Failed writing cpu.weight: {e}"))?;
                    let readback = fs::read_to_string(&w_path)
                        .map_err(|e| format!("Failed reading back cpu.weight: {e}"))?;
                    if readback.trim() != weight.to_string() {
                        return Err(format!(
                            "Cpu weight verification failed: expected {weight}, read {readback}"
                        ));
                    }
                }

                journal.record_item(ConfigJournalItem {
                    action_id: format!("cgroup-weight-{cg_clean}"),
                    target_key: format!("cgroup.cpu_weight.{cg_clean}"),
                    target_file: None,
                    original_value: original_weight.map(|w| w.to_string()),
                    applied_value: weight.to_string(),
                    is_applied: true,
                });
                let _ = journal.persist(&self.journal_path, now_ms);

                ledger.record_applied(
                    format!("cgroup.cpu_weight.{cg_clean}"),
                    None,
                    original_weight.map(|w| w.to_string()),
                    weight.to_string(),
                    None,
                    tx_id.to_string(),
                    now_ms,
                );

                Ok(format!("Applied cgroup CPU weight {weight} to {cg_clean}"))
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

            // Subsystem-specific rollback routing
            if key.starts_with("cpu.governor.") {
                if let Some(orig) = &entry.original_value {
                    let cpu_root = self.sys_root.join("devices/system/cpu");
                    if cpu_root.exists() {
                        for entry in fs::read_dir(&cpu_root).into_iter().flatten().flatten() {
                            let gov_p = entry.path().join("cpufreq/scaling_governor");
                            if gov_p.exists() {
                                let _ = fs::write(&gov_p, orig.as_bytes());
                            }
                        }
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if key.starts_with("cpu.epp.") {
                if let Some(orig) = &entry.original_value {
                    let cpu_root = self.sys_root.join("devices/system/cpu");
                    if cpu_root.exists() {
                        for entry in fs::read_dir(&cpu_root).into_iter().flatten().flatten() {
                            let epp_p = entry.path().join("cpufreq/energy_performance_preference");
                            if epp_p.exists() {
                                let _ = fs::write(&epp_p, orig.as_bytes());
                            }
                        }
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if key.starts_with("cpu.freq_bounds.") {
                if let Some(orig) = &entry.original_value {
                    let parts: Vec<&str> = orig.split(':').collect();
                    if parts.len() == 2 {
                        let parse_freq = |p: &str| -> Option<u64> {
                            p.strip_prefix("Some(")?.strip_suffix(')')?.parse().ok()
                        };
                        let min_freq = parse_freq(parts[0]);
                        let max_freq = parse_freq(parts[1]);
                        let cpu_root = self.sys_root.join("devices/system/cpu");
                        if cpu_root.exists() {
                            for entry in fs::read_dir(&cpu_root).into_iter().flatten().flatten() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                if name.starts_with("cpu")
                                    && name
                                        .strip_prefix("cpu")
                                        .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
                                {
                                    if let Some(min_f) = min_freq {
                                        let min_p = entry.path().join("cpufreq/scaling_min_freq");
                                        if min_p.exists() {
                                            let _ = fs::write(&min_p, min_f.to_string().as_bytes());
                                        }
                                    }
                                    if let Some(max_f) = max_freq {
                                        let max_p = entry.path().join("cpufreq/scaling_max_freq");
                                        if max_p.exists() {
                                            let _ = fs::write(&max_p, max_f.to_string().as_bytes());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if let Some(dev_name) = key.strip_prefix("block.scheduler.") {
                if let Some(orig) = &entry.original_value {
                    let sched_p = self
                        .sys_root
                        .join(format!("block/{dev_name}/queue/scheduler"));
                    if sched_p.exists() {
                        let _ = fs::write(&sched_p, orig.as_bytes());
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if let Some(dev_name) = key.strip_prefix("block.read_ahead.") {
                if let Some(orig) = &entry.original_value {
                    let ra_p = self
                        .sys_root
                        .join(format!("block/{dev_name}/queue/read_ahead_kb"));
                    if ra_p.exists() {
                        let _ = fs::write(&ra_p, orig.as_bytes());
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if key == "power.profile" {
                if let Some(orig) = &entry.original_value {
                    let p_path = self.sys_root.join("firmware/acpi/platform_profile");
                    if p_path.exists() {
                        let _ = fs::write(&p_path, orig.as_bytes());
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if let Some(cg_path) = key.strip_prefix("cgroup.cpu_weight.") {
                if let Some(orig) = &entry.original_value {
                    let w_path = self
                        .sys_root
                        .join("fs/cgroup")
                        .join(cg_path)
                        .join("cpu.weight");
                    if w_path.exists() {
                        let _ = fs::write(&w_path, orig.as_bytes());
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if key == "transparent_hugepage.enabled" {
                if let Some(orig) = &entry.original_value {
                    let thp_path = self.sys_root.join("kernel/mm/transparent_hugepage/enabled");
                    if thp_path.exists() {
                        let _ = fs::write(&thp_path, orig.as_bytes());
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if let Some(param) = key.strip_prefix("zswap.") {
                if let Some(orig) = &entry.original_value {
                    let p_path = self
                        .sys_root
                        .join(format!("module/zswap/parameters/{param}"));
                    if p_path.exists() {
                        let _ = fs::write(&p_path, orig.as_bytes());
                    }
                    restored_keys.push(format!("{key} -> {orig}"));
                }
            } else if !key.starts_with("dropin:") && !key.starts_with("zram.") {
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

            // Remove managed drop-in file if present with strict path sanitization
            if let Some(target_file) = &entry.target_file {
                match sanitize_dropin_target_file(target_file, &self.etc_root) {
                    Ok(full_path) => {
                        if full_path.exists() {
                            if let Err(e) = fs::remove_file(&full_path) {
                                failed.push(format!("{}: {e}", full_path.display()));
                                continue;
                            }
                            removed_files.push(full_path.display().to_string());
                        }
                    }
                    Err(err) => {
                        skipped_tampered.push(format!("{key}: {err}; refusing unsafe deletion"));
                        continue;
                    }
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
