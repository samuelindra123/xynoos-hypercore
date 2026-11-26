//! Milestone Q: Production Release Candidate Durable Crash & SIGKILL Recovery Qualification Tests.
//!
//! Mandatory verification for Phase 1:
//! - Durable intervention metadata persistence and CRC32 integrity.
//! - SIGKILL simulated termination recovery without normal cleanup handlers.
//! - Previous-boot journal invalidation (boot ID mismatch fail-closed).
//! - Target cgroup deletion/recreation and inode mismatch detection (fail-closed).
//! - External administrator conflict detection (refusal of blind rollback).
//! - Multi-target durable intervention reconciliation.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use xynoos_linux_control::journal::{
    current_boot_id, reconcile_durable_interventions_sysfs, DurableInterventionLedger,
    DurableInterventionRecord, LeasePhase, RecoveryOutcome,
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn create_temp_cgroup_harness(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!(
        "xynoos-q-test-{name}-{}-{}",
        std::process::id(),
        count
    ));
    let state_dir = base.join("var_lib_xynoos");
    let cgroup_root = base.join("sys_fs_cgroup");

    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&cgroup_root).unwrap();

    (base, state_dir, cgroup_root)
}

fn setup_mock_cgroup(
    cgroup_root: &Path,
    target_id: &str,
    control: &str,
    initial_value: u64,
) -> (u64, u64) {
    let target_dir = cgroup_root.join(target_id.trim_start_matches('/'));
    fs::create_dir_all(&target_dir).unwrap();
    let control_file = target_dir.join(control);
    let val_str = if initial_value == u64::MAX {
        "max".to_string()
    } else {
        initial_value.to_string()
    };
    fs::write(&control_file, val_str.as_bytes()).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let dir_ino = fs::symlink_metadata(&target_dir).unwrap().ino();
        let file_ino = fs::symlink_metadata(&control_file).unwrap().ino();
        (dir_ino, file_ino)
    }
    #[cfg(not(unix))]
    {
        (1001, 2001)
    }
}

#[test]
fn test_milestone_q_sigkill_crash_recovery_restores_baseline() {
    let (base, state_dir, cgroup_root) = create_temp_cgroup_harness("sigkill-recovery");
    let ledger_path = state_dir.join("active_interventions.json");

    // 1. Setup mock cgroup: baseline memory.high = 64 MiB
    let target_id = "user.slice/user-1000.slice";
    let control = "memory.high";
    let original_value = 64 * 1024 * 1024;
    let applied_value = 32 * 1024 * 1024;
    let (cg_ino, ctrl_ino) = setup_mock_cgroup(&cgroup_root, target_id, control, original_value);

    // 2. Continuous coordinator applies intervention: changes control to 32 MiB and persists to disk
    let control_file = cgroup_root.join(target_id).join(control);
    fs::write(&control_file, applied_value.to_string().as_bytes()).unwrap();

    let cur_boot = current_boot_id().unwrap_or_else(|| "mock-boot-id-1".into());
    let mut ledger = DurableInterventionLedger::new(cur_boot.clone());
    let record = DurableInterventionRecord {
        lease_id: "lease-sigkill-01".into(),
        target_id: target_id.into(),
        control: control.into(),
        cgroup_inode: cg_ino,
        control_inode: ctrl_ino,
        original_value,
        applied_value,
        applied_at_epoch_ms: 1000,
        lease_duration_ms: 30_000,
        boot_id: cur_boot,
        owner_uid: 1000,
        process_pid: std::process::id(),
        status: LeasePhase::Applied,
    };
    ledger.upsert_record(record);
    ledger.persist(&ledger_path).unwrap();

    // Verify cgroup is currently modified and ledger exists on disk
    let current_val = fs::read_to_string(&control_file).unwrap();
    assert_eq!(current_val.trim(), applied_value.to_string());
    assert!(ledger_path.exists());

    // 3. SIMULATE SIGKILL: daemon process abruptly terminates without executing cleanup handlers
    // The durable ledger remains on disk at `ledger_path`.

    // 4. Daemon restarts: crash reconciliation runs
    let report = reconcile_durable_interventions_sysfs(&ledger_path, Some(&cgroup_root))
        .expect("recovery must execute successfully");

    assert_eq!(report.outcome, RecoveryOutcome::Restored);
    assert_eq!(report.original_value, Some(original_value));
    assert_eq!(report.current_value, Some(applied_value));

    // Verify baseline original value (64 MiB) was successfully restored
    let restored_val = fs::read_to_string(&control_file).unwrap();
    assert_eq!(restored_val.trim(), original_value.to_string());

    // Verify ledger was cleaned up after successful complete restoration
    assert!(!ledger_path.exists());

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_milestone_q_boot_id_mismatch_fails_closed_without_kernel_mutation() {
    let (base, state_dir, cgroup_root) = create_temp_cgroup_harness("boot-mismatch");
    let ledger_path = state_dir.join("active_interventions.json");

    let target_id = "user.slice/user-1000.slice";
    let control = "memory.high";
    let current_kernel_value = 128 * 1024 * 1024;
    let (cg_ino, ctrl_ino) =
        setup_mock_cgroup(&cgroup_root, target_id, control, current_kernel_value);

    // Create a journal with a DIFFERENT prior boot ID
    let prior_boot = "00000000-0000-0000-0000-000000000000".to_string();
    let mut ledger = DurableInterventionLedger::new(prior_boot.clone());
    let record = DurableInterventionRecord {
        lease_id: "lease-stale-boot-01".into(),
        target_id: target_id.into(),
        control: control.into(),
        cgroup_inode: cg_ino,
        control_inode: ctrl_ino,
        original_value: 16 * 1024 * 1024,
        applied_value: 32 * 1024 * 1024,
        applied_at_epoch_ms: 1000,
        lease_duration_ms: 30_000,
        boot_id: prior_boot,
        owner_uid: 1000,
        process_pid: 9999,
        status: LeasePhase::Applied,
    };
    ledger.upsert_record(record);
    ledger.persist(&ledger_path).unwrap();

    let control_file = cgroup_root.join(target_id).join(control);

    // Recovery runs on a newly booted system
    let report = reconcile_durable_interventions_sysfs(&ledger_path, Some(&cgroup_root))
        .expect("reconciliation must handle boot mismatch gracefully");

    assert_eq!(report.outcome, RecoveryOutcome::Invalidated);

    // CRITICAL INVARIANT: Current kernel value MUST NOT be clobbered by prior-boot values!
    let current_val = fs::read_to_string(&control_file).unwrap();
    assert_eq!(current_val.trim(), current_kernel_value.to_string());

    // Ledger was quarantined with invalidated extension
    assert!(!ledger_path.exists());
    let entries: Vec<_> = fs::read_dir(&state_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.iter().any(|n| n.contains("invalidated.boot")));

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_milestone_q_cgroup_recreation_inode_mismatch_fails_closed() {
    let (base, state_dir, cgroup_root) = create_temp_cgroup_harness("inode-mismatch");
    let ledger_path = state_dir.join("active_interventions.json");

    let target_id = "user.slice/user-1000.slice";
    let control = "memory.high";
    let original_value = 64 * 1024 * 1024;
    let applied_value = 32 * 1024 * 1024;

    let (_old_cg_ino, _old_ctrl_ino) =
        setup_mock_cgroup(&cgroup_root, target_id, control, applied_value);

    let cur_boot = current_boot_id().unwrap_or_else(|| "mock-boot-id-2".into());
    let mut ledger = DurableInterventionLedger::new(cur_boot.clone());
    // Simulate recorded inode does not match current cgroup (target was deleted & recreated)
    let stale_recorded_inode = 999_888_777;
    let record = DurableInterventionRecord {
        lease_id: "lease-recreated-01".into(),
        target_id: target_id.into(),
        control: control.into(),
        cgroup_inode: stale_recorded_inode,
        control_inode: stale_recorded_inode + 1,
        original_value,
        applied_value,
        applied_at_epoch_ms: 1000,
        lease_duration_ms: 30_000,
        boot_id: cur_boot,
        owner_uid: 1000,
        process_pid: std::process::id(),
        status: LeasePhase::Applied,
    };
    ledger.upsert_record(record);
    ledger.persist(&ledger_path).unwrap();

    // Recovery runs
    let report = reconcile_durable_interventions_sysfs(&ledger_path, Some(&cgroup_root))
        .expect("reconciliation must handle inode mismatch gracefully");

    // Inode mismatch must be marked Invalidated fail-closed
    assert_eq!(report.outcome, RecoveryOutcome::Invalidated);

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_milestone_q_external_administrator_conflict_refuses_blind_rollback() {
    let (base, state_dir, cgroup_root) = create_temp_cgroup_harness("admin-conflict");
    let ledger_path = state_dir.join("active_interventions.json");

    let target_id = "user.slice/user-1000.slice";
    let control = "memory.high";
    let original_value = 64 * 1024 * 1024;
    let applied_value = 32 * 1024 * 1024;
    let admin_configured_value = 48 * 1024 * 1024;

    let (cg_ino, ctrl_ino) = setup_mock_cgroup(&cgroup_root, target_id, control, applied_value);

    let cur_boot = current_boot_id().unwrap_or_else(|| "mock-boot-id-3".into());
    let mut ledger = DurableInterventionLedger::new(cur_boot.clone());
    let record = DurableInterventionRecord {
        lease_id: "lease-conflict-01".into(),
        target_id: target_id.into(),
        control: control.into(),
        cgroup_inode: cg_ino,
        control_inode: ctrl_ino,
        original_value,
        applied_value,
        applied_at_epoch_ms: 1000,
        lease_duration_ms: 30_000,
        boot_id: cur_boot,
        owner_uid: 1000,
        process_pid: std::process::id(),
        status: LeasePhase::Applied,
    };
    ledger.upsert_record(record);
    ledger.persist(&ledger_path).unwrap();

    let control_file = cgroup_root.join(target_id).join(control);

    // External administrator altered memory.high to 48 MiB while daemon was down
    fs::write(&control_file, admin_configured_value.to_string().as_bytes()).unwrap();

    // Recovery executes
    let report = reconcile_durable_interventions_sysfs(&ledger_path, Some(&cgroup_root))
        .expect("reconciliation must execute without panicking");

    // Conflict detected: external change must NOT be blindly reverted to 64 MiB
    assert_eq!(report.outcome, RecoveryOutcome::Conflict);

    // Invariant: administrator value (48 MiB) remains intact!
    let current_val = fs::read_to_string(&control_file).unwrap();
    assert_eq!(current_val.trim(), admin_configured_value.to_string());

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_milestone_q_corrupt_ledger_crc_quarantined() {
    let (base, state_dir, cgroup_root) = create_temp_cgroup_harness("crc-corrupt");
    let ledger_path = state_dir.join("active_interventions.json");

    let cur_boot = current_boot_id().unwrap_or_else(|| "mock-boot-id-4".into());
    let mut ledger = DurableInterventionLedger::new(cur_boot.clone());
    let record = DurableInterventionRecord {
        lease_id: "lease-crc-01".into(),
        target_id: "system.slice/test.service".into(),
        control: "cpu.weight".into(),
        cgroup_inode: 5555,
        control_inode: 6666,
        original_value: 100,
        applied_value: 200,
        applied_at_epoch_ms: 1000,
        lease_duration_ms: 10_000,
        boot_id: cur_boot,
        owner_uid: 0,
        process_pid: 1,
        status: LeasePhase::Applied,
    };
    ledger.upsert_record(record);
    ledger.persist(&ledger_path).unwrap();

    // Tamper with data without recalculating CRC
    let text = fs::read_to_string(&ledger_path).unwrap();
    let tampered = text.replace("200", "250");
    fs::write(&ledger_path, tampered.as_bytes()).unwrap();

    // Reconciliation must fail CRC check and quarantine
    let res = reconcile_durable_interventions_sysfs(&ledger_path, Some(&cgroup_root));
    assert!(res.is_err(), "must return error on CRC mismatch");

    let entries: Vec<_> = fs::read_dir(&state_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.iter().any(|n| n.contains("corrupt.crc")));

    let _ = fs::remove_dir_all(&base);
}
