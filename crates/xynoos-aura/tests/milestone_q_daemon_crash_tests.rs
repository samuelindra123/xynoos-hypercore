//! Milestone Q: AURA Daemon Durable Crash Recovery and Lifecycle Tests.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc,
    },
};
use xynoos_aura::{
    AuraDaemon, DaemonConfig, DaemonMode, DurableInterventionLedger, DurableInterventionRecord,
    LeasePhase, PollingTier, RecoveryOutcome, TelemetrySample, TELEMETRY_SAMPLE_SCHEMA_VERSION,
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(100);

fn create_temp_harness(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let count = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!(
        "xynoos-q-daemon-{name}-{}-{}",
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
fn test_daemon_startup_recovery_reconciles_active_interventions_ledger() {
    let (base, state_dir, cgroup_root) = create_temp_harness("startup-recovery");
    let ledger_path = state_dir.join("active_interventions.json");

    let target_id = "user.slice/session-1.scope";
    let control = "memory.high";
    let original_value = 128 * 1024 * 1024;
    let applied_value = 64 * 1024 * 1024;

    let (cg_ino, ctrl_ino) = setup_mock_cgroup(&cgroup_root, target_id, control, applied_value);
    let cur_boot =
        xynoos_linux_control::journal::current_boot_id().unwrap_or_else(|| "boot-1".into());

    // Populate durable ledger as if daemon crashed while holding intervention
    let mut ledger = DurableInterventionLedger::new(cur_boot.clone());
    ledger.upsert_record(DurableInterventionRecord {
        lease_id: "lease-crash-test".into(),
        target_id: target_id.into(),
        control: control.into(),
        cgroup_inode: cg_ino,
        control_inode: ctrl_ino,
        original_value,
        applied_value,
        applied_at_epoch_ms: 1000,
        lease_duration_ms: 60_000,
        boot_id: cur_boot,
        owner_uid: 1000,
        process_pid: std::process::id(),
        status: LeasePhase::Applied,
    });
    ledger.persist(&ledger_path).unwrap();

    let config = DaemonConfig {
        mode: DaemonMode::ObserveOnly,
        interval_ms: 100,
        pid_file: None,
        journal_file: None,
        active_interventions_file: Some(ledger_path.clone()),
        auth_file: None,
        ..Default::default()
    };

    let mut daemon = AuraDaemon::new(config).unwrap();
    let rep = daemon
        .reconcile_startup_crash_recovery(Some(&cgroup_root))
        .unwrap();

    assert_eq!(rep.outcome, RecoveryOutcome::Restored);
    assert_eq!(rep.original_value, Some(original_value));

    // Verify control file in mock cgroup restored to 128 MiB
    let restored = fs::read_to_string(cgroup_root.join(target_id).join(control)).unwrap();
    assert_eq!(restored.trim(), original_value.to_string());

    assert_eq!(daemon.diagnostics().recovered_crashes_count, 1);
    assert!(!ledger_path.exists());

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_daemon_record_and_persist_intervention_creates_durable_ledger() {
    let (base, state_dir, cgroup_root) = create_temp_harness("record-persist");
    let ledger_path = state_dir.join("active_interventions.json");

    let target_id = "user.slice/session-2.scope";
    let control = "cpu.weight";
    let (cg_ino, ctrl_ino) = setup_mock_cgroup(&cgroup_root, target_id, control, 100);

    let config = DaemonConfig {
        mode: DaemonMode::ObserveOnly,
        interval_ms: 100,
        pid_file: None,
        journal_file: None,
        active_interventions_file: Some(ledger_path.clone()),
        auth_file: None,
        ..Default::default()
    };

    let mut daemon = AuraDaemon::new(config).unwrap();
    let sample = TelemetrySample {
        schema_version: TELEMETRY_SAMPLE_SCHEMA_VERSION,
        timestamp_epoch_ms: 1000,
        elapsed_interval_ms: 5000,
        mem_total_bytes: Some(4 * 1024 * 1024 * 1024),
        mem_available_bytes: Some(3 * 1024 * 1024 * 1024),
        mem_available_pct: Some(50.0),
        swap_total_bytes: None,
        swap_free_bytes: None,
        swap_in_delta: 0,
        swap_out_delta: 0,
        pgmajfault_delta: 0,
        psi_memory_some_avg10: Some(1.5),
        psi_memory_full_avg10: None,
        psi_cpu_some_avg10: Some(2.0),
        psi_io_some_avg10: Some(0.5),
        psi_io_full_avg10: None,
        cpu_utilization_pct: Some(25.0),
        cpu_throttle_delta: 0,
        load_one: Some(0.5),
        max_cpu_temp_c: Some(45.0),
        ac_online: Some(true),
        battery_capacity_pct: Some(100),
        gpu_active: false,
        gpu_freq_mhz: None,
        cgroup_memory_current_bytes: None,
        cgroup_memory_events_oom_delta: 0,
        cgroup_memory_events_high_delta: 0,
        evaluated_tier: PollingTier::SteadyState,
    };

    daemon.record_and_persist_intervention(
        target_id, control, cg_ino, ctrl_ino, 100, 200, 30_000, &sample, 1000,
    );

    // Verify in-memory coordinator has intervention
    assert_eq!(daemon.continuous_coordinator.active_interventions.len(), 1);

    // Verify durable ledger exists on disk
    assert!(ledger_path.exists());
    let loaded = DurableInterventionLedger::load(&ledger_path)
        .unwrap()
        .expect("ledger must exist on disk");
    assert_eq!(loaded.records.len(), 1);
    assert_eq!(loaded.records[0].target_id, target_id);
    assert_eq!(loaded.records[0].control, control);
    assert_eq!(loaded.records[0].applied_value, 200);

    // Revert all: must revert in-memory and clean up durable ledger on disk
    let count = daemon.revert_all_active_interventions();
    assert_eq!(count, 1);
    assert!(daemon
        .continuous_coordinator
        .active_interventions
        .is_empty());
    assert!(
        !ledger_path.exists(),
        "ledger must be cleaned up on clean revert"
    );

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_daemon_run_with_signal_reconciles_prior_crash_on_entry() {
    let (base, state_dir, cgroup_root) = create_temp_harness("signal-entry");
    let ledger_path = state_dir.join("active_interventions.json");

    let target_id = "user.slice/session-3.scope";
    let control = "memory.high";
    let original_value = 256 * 1024 * 1024;
    let applied_value = 128 * 1024 * 1024;

    let (cg_ino, ctrl_ino) = setup_mock_cgroup(&cgroup_root, target_id, control, applied_value);
    let cur_boot =
        xynoos_linux_control::journal::current_boot_id().unwrap_or_else(|| "boot-2".into());

    let mut ledger = DurableInterventionLedger::new(cur_boot.clone());
    ledger.upsert_record(DurableInterventionRecord {
        lease_id: "lease-entry-crash".into(),
        target_id: target_id.into(),
        control: control.into(),
        cgroup_inode: cg_ino,
        control_inode: ctrl_ino,
        original_value,
        applied_value,
        applied_at_epoch_ms: 1000,
        lease_duration_ms: 60_000,
        boot_id: cur_boot,
        owner_uid: 1000,
        process_pid: std::process::id(),
        status: LeasePhase::Applied,
    });
    ledger.persist(&ledger_path).unwrap();

    let config = DaemonConfig {
        mode: DaemonMode::ObserveOnly,
        interval_ms: 50,
        pid_file: None,
        journal_file: None,
        active_interventions_file: Some(ledger_path.clone()),
        auth_file: None,
        ..Default::default()
    };

    let mut daemon = AuraDaemon::new(config).unwrap();
    // Reconcile explicitly using mock cgroup root
    let _ = daemon.reconcile_startup_crash_recovery(Some(&cgroup_root));

    let stop_signal = Arc::new(AtomicBool::new(true));
    let diag = daemon.run_with_signal(stop_signal, Some(1)).unwrap();

    assert_eq!(diag.cycles_completed, 0); // stopped on entry
    assert!(!ledger_path.exists());

    let _ = fs::remove_dir_all(&base);
}
