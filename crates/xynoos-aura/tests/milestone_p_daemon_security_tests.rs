//! Milestone P Daemon Security and Lifecycle Hardening Tests.
#![forbid(unsafe_code)]

use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
};
use xynoos_aura::daemon::{AuraDaemon, DaemonConfig, DaemonMode, PidLock};

#[test]
fn test_daemon_pid_lock_prevents_duplicate_instance() {
    let tmp = std::env::temp_dir().join(format!("xynoos-pid-test-{}", std::process::id()));
    let _ = fs::create_dir_all(&tmp);
    let lock_path = tmp.join("daemon.pid");
    let _ = fs::remove_file(&lock_path);

    let lock1 = PidLock::acquire(lock_path.clone());
    assert!(lock1.is_ok());

    // Second acquisition of the same live lock must fail
    let lock2 = PidLock::acquire(lock_path.clone());
    assert!(lock2.is_err());
    assert!(lock2.unwrap_err().contains("Daemon sudah berjalan"));

    drop(lock1);
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_daemon_pid_lock_symlink_attack_rejected() {
    let tmp = std::env::temp_dir().join(format!("xynoos-pid-symlink-{}", std::process::id()));
    let _ = fs::create_dir_all(&tmp);
    let target_file = tmp.join("innocent_target.txt");
    fs::write(&target_file, b"original content").unwrap();

    let symlink_path = tmp.join("attack.pid");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target_file, &symlink_path).unwrap();

    // PidLock must refuse to acquire or overwrite through existing symlink
    let res = PidLock::acquire(symlink_path.clone());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .contains("Refusing to acquire PID lock through existing symlink"));

    // Target file content preserved
    assert_eq!(fs::read(&target_file).unwrap(), b"original content");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_daemon_reverts_active_interventions_on_shutdown() {
    let config = DaemonConfig {
        mode: DaemonMode::ObserveOnly,
        interval_ms: 100,
        pid_file: None,
        journal_file: None,
        auth_file: None,
        ..Default::default()
    };

    let mut daemon = AuraDaemon::new(config).unwrap();

    // Artificially inject an active intervention into coordinator
    daemon.continuous_coordinator.active_interventions.push(
        xynoos_aura::continuous_coordinator::InterventionRecord {
            target_id: "user.slice/test-session.scope".into(),
            control: "cpu.weight".into(),
            original_value: 100,
            applied_value: 200,
            applied_at_epoch_ms: 1000,
            lease_duration_ms: 60_000,
            pre_psi_memory: None,
            pre_psi_cpu: None,
            pre_psi_io: None,
            post_psi_memory: None,
            post_psi_cpu: None,
            post_psi_io: None,
            evaluated_cycles: 0,
            status: xynoos_aura::continuous_coordinator::InterventionStatus::Applied,
            message: "Test intervention".into(),
        },
    );

    assert_eq!(daemon.continuous_coordinator.active_interventions.len(), 1);

    // Trigger stop signal
    let stop_signal = Arc::new(AtomicBool::new(true));
    let diag = daemon.run_with_signal(stop_signal, Some(1)).unwrap();

    // Verify all active interventions were drained and reverted on shutdown
    assert!(daemon
        .continuous_coordinator
        .active_interventions
        .is_empty());
    assert_eq!(
        daemon.continuous_coordinator.state,
        xynoos_aura::continuous_coordinator::CoordinatorState::ShuttingDown
    );
    assert_eq!(diag.active_interventions.len(), 0);
}
