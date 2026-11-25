//! Milestone N Continuous Runtime & Workload Controller Integration Tests.
#![forbid(unsafe_code)]

use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use xynoos_aura::{
    ContinuousCoordinator, CoordinatorState, DaemonConfig, DaemonMode, InterventionStatus,
    PollingTier, TelemetrySample, TelemetryScheduler, WorkloadClass, WorkloadController,
    TELEMETRY_SAMPLE_SCHEMA_VERSION,
};
use xynoos_linux_control::config::RuntimeAuthorizationContract;

#[test]
fn test_telemetry_scheduler_multi_rate_intervals() {
    let temp_dir =
        std::env::temp_dir().join(format!("xynoos-test-telemetry-{}", std::process::id()));
    let proc_dir = temp_dir.join("proc");
    let sys_dir = temp_dir.join("sys");
    fs::create_dir_all(&proc_dir).unwrap();
    fs::create_dir_all(&sys_dir).unwrap();

    let scheduler = TelemetryScheduler::new(proc_dir, sys_dir, 5000);
    assert_eq!(scheduler.current_tier(), PollingTier::SteadyState);

    // Initial interval for steady state base=5000 should be 5000ms
    assert_eq!(
        scheduler.next_polling_interval(),
        Duration::from_millis(5000)
    );

    // Test tier clamping
    assert_eq!(PollingTier::ActivePressure.target_interval_ms(5000), 1000);
    assert_eq!(PollingTier::ActivePressure.target_interval_ms(2000), 500);
    assert_eq!(PollingTier::SteadyState.target_interval_ms(5000), 5000);
    assert_eq!(PollingTier::LowActivityIdle.target_interval_ms(5000), 15000);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_workload_classification_heuristics() {
    // 1. Quiescent Idle
    let mut sample = TelemetrySample {
        schema_version: TELEMETRY_SAMPLE_SCHEMA_VERSION,
        timestamp_epoch_ms: 1000,
        elapsed_interval_ms: 5000,
        mem_total_bytes: Some(4 * 1024 * 1024 * 1024),
        mem_available_bytes: Some(3 * 1024 * 1024 * 1024),
        mem_available_pct: Some(75.0),
        swap_total_bytes: Some(2 * 1024 * 1024 * 1024),
        swap_free_bytes: Some(2 * 1024 * 1024 * 1024),
        swap_in_delta: 0,
        swap_out_delta: 0,
        pgmajfault_delta: 0,
        psi_memory_some_avg10: Some(0.0),
        psi_memory_full_avg10: Some(0.0),
        psi_cpu_some_avg10: Some(0.0),
        psi_io_some_avg10: Some(0.0),
        psi_io_full_avg10: Some(0.0),
        cpu_utilization_pct: Some(1.2),
        cpu_throttle_delta: 0,
        load_one: Some(0.05),
        max_cpu_temp_c: Some(42.0),
        ac_online: Some(true),
        battery_capacity_pct: Some(100),
        gpu_active: false,
        gpu_freq_mhz: None,
        cgroup_memory_current_bytes: None,
        cgroup_memory_events_oom_delta: 0,
        cgroup_memory_events_high_delta: 0,
        evaluated_tier: PollingTier::LowActivityIdle,
    };

    let (class, conf) = WorkloadController::classify(&sample);
    assert_eq!(class, WorkloadClass::Idle);
    assert!(conf >= 0.90);

    // 2. Video Playback
    sample.gpu_active = true;
    sample.cpu_utilization_pct = Some(22.0);
    let (class, _) = WorkloadController::classify(&sample);
    assert_eq!(class, WorkloadClass::VideoPlayback);

    // 3. Background Software Compilation (high CPU + I/O pressure + faults)
    sample.gpu_active = false;
    sample.cpu_utilization_pct = Some(92.0);
    sample.psi_io_some_avg10 = Some(4.5);
    sample.pgmajfault_delta = 120;
    let (class, _) = WorkloadController::classify(&sample);
    assert_eq!(class, WorkloadClass::BackgroundCompilation);

    // Protection check for background compilation
    let rec = WorkloadController::evaluate(&sample, Some(100), Some(256 * 1024 * 1024));
    assert!(rec.protect_background_progress);
    assert!(rec.recommended_cpu_weight.unwrap() >= 60);

    // 4. Memory-intensive workload
    sample.cpu_utilization_pct = Some(35.0);
    sample.psi_memory_some_avg10 = Some(15.2);
    sample.swap_in_delta = 45;
    let (class, _) = WorkloadController::classify(&sample);
    assert_eq!(class, WorkloadClass::MemoryIntensive);

    // 5. I/O-intensive workload
    sample.psi_memory_some_avg10 = Some(0.2);
    sample.swap_in_delta = 0;
    sample.psi_io_some_avg10 = Some(28.0);
    sample.cpu_utilization_pct = Some(25.0);
    let (class, _) = WorkloadController::classify(&sample);
    assert_eq!(class, WorkloadClass::IoIntensive);
}

#[test]
fn test_continuous_coordinator_multi_resource_arbitration() {
    let mut coordinator = ContinuousCoordinator::new();
    let auth = RuntimeAuthorizationContract::default();

    let mut sample = TelemetrySample {
        schema_version: TELEMETRY_SAMPLE_SCHEMA_VERSION,
        timestamp_epoch_ms: 1000,
        elapsed_interval_ms: 5000,
        mem_total_bytes: Some(4 * 1024 * 1024 * 1024),
        mem_available_bytes: Some(3 * 1024 * 1024 * 1024),
        mem_available_pct: Some(75.0),
        swap_total_bytes: None,
        swap_free_bytes: None,
        swap_in_delta: 0,
        swap_out_delta: 0,
        pgmajfault_delta: 0,
        psi_memory_some_avg10: Some(1.0),
        psi_memory_full_avg10: Some(0.0),
        psi_cpu_some_avg10: Some(2.0),
        psi_io_some_avg10: Some(1.0),
        psi_io_full_avg10: Some(0.0),
        cpu_utilization_pct: Some(50.0),
        cpu_throttle_delta: 0,
        load_one: Some(1.0),
        max_cpu_temp_c: Some(50.0),
        ac_online: Some(true),
        battery_capacity_pct: Some(80),
        gpu_active: false,
        gpu_freq_mhz: None,
        cgroup_memory_current_bytes: Some(100 * 1024 * 1024),
        cgroup_memory_events_oom_delta: 0,
        cgroup_memory_events_high_delta: 0,
        evaluated_tier: PollingTier::SteadyState,
    };

    // Valid intervention
    let res = coordinator.can_intervene("workload", "cpu.weight", &sample, &auth, 100_000);
    assert!(res.is_ok());

    // Thermal throttling constraint
    sample.max_cpu_temp_c = Some(88.0);
    let res = coordinator.can_intervene("workload", "cpu.weight", &sample, &auth, 100_000);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("thermal"));
    sample.max_cpu_temp_c = Some(50.0);

    // Severe I/O saturation constraint
    sample.psi_io_some_avg10 = Some(30.0);
    let res = coordinator.can_intervene("workload", "cpu.weight", &sample, &auth, 100_000);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("I/O saturation"));
    sample.psi_io_some_avg10 = Some(1.0);

    // Low battery constraint
    sample.ac_online = Some(false);
    sample.battery_capacity_pct = Some(12);
    let res = coordinator.can_intervene("workload", "cpu.weight", &sample, &auth, 100_000);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("low battery"));
}

#[test]
fn test_oscillation_circuit_breaker() {
    let mut coordinator = ContinuousCoordinator::new();
    let auth = RuntimeAuthorizationContract::default();

    let sample = TelemetrySample {
        schema_version: TELEMETRY_SAMPLE_SCHEMA_VERSION,
        timestamp_epoch_ms: 1000,
        elapsed_interval_ms: 5000,
        mem_total_bytes: Some(4 * 1024 * 1024 * 1024),
        mem_available_bytes: Some(3 * 1024 * 1024 * 1024),
        mem_available_pct: Some(75.0),
        swap_total_bytes: None,
        swap_free_bytes: None,
        swap_in_delta: 0,
        swap_out_delta: 0,
        pgmajfault_delta: 0,
        psi_memory_some_avg10: Some(1.0),
        psi_memory_full_avg10: None,
        psi_cpu_some_avg10: Some(1.0),
        psi_io_some_avg10: Some(1.0),
        psi_io_full_avg10: None,
        cpu_utilization_pct: Some(20.0),
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

    // Simulate rapid policy oscillations
    coordinator.record_policy_flip("target_cgroup", 10_000);
    coordinator.record_policy_flip("target_cgroup", 15_000);
    assert!(!coordinator.is_circuit_breaker_tripped(20_000));

    // Third flip trips circuit breaker
    coordinator.record_policy_flip("target_cgroup", 20_000);
    assert!(coordinator.is_circuit_breaker_tripped(25_000));
    assert_eq!(coordinator.state, CoordinatorState::CoolingDown);

    // Can intervene must decline while circuit breaker is tripped
    let res = coordinator.can_intervene("target_cgroup", "cpu.weight", &sample, &auth, 25_000);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Circuit breaker"));
}

#[test]
fn test_closed_loop_regression_rollback() {
    let mut coordinator = ContinuousCoordinator::new();

    let pre_sample = TelemetrySample {
        schema_version: TELEMETRY_SAMPLE_SCHEMA_VERSION,
        timestamp_epoch_ms: 1000,
        elapsed_interval_ms: 5000,
        mem_total_bytes: Some(4 * 1024 * 1024 * 1024),
        mem_available_bytes: Some(3 * 1024 * 1024 * 1024),
        mem_available_pct: Some(75.0),
        swap_total_bytes: None,
        swap_free_bytes: None,
        swap_in_delta: 0,
        swap_out_delta: 0,
        pgmajfault_delta: 0,
        psi_memory_some_avg10: Some(2.0),
        psi_memory_full_avg10: None,
        psi_cpu_some_avg10: Some(5.0),
        psi_io_some_avg10: Some(2.0),
        psi_io_full_avg10: None,
        cpu_utilization_pct: Some(30.0),
        cpu_throttle_delta: 0,
        load_one: Some(0.8),
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

    coordinator.record_intervention_applied(
        "workload_cgroup",
        "cpu.weight",
        100,
        200,
        60_000,
        &pre_sample,
        10_000,
    );
    assert_eq!(coordinator.active_interventions.len(), 1);

    // Cycle 1: Evaluation in progress
    let actions = coordinator.evaluate_closed_loop(&pre_sample, 15_000);
    assert!(actions.is_empty());

    // Cycle 2: Severe regression occurs (Memory PSI spikes from 2.0 to 25.0)
    let mut post_sample = pre_sample.clone();
    post_sample.psi_memory_some_avg10 = Some(25.0);

    let actions = coordinator.evaluate_closed_loop(&post_sample, 20_000);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].status, InterventionStatus::Regressed);
    assert!(actions[0].message.contains("regression detected"));
    assert!(coordinator.active_interventions.is_empty());
}

#[test]
fn test_aura_daemon_observe_only_milestone_n() {
    let cfg = DaemonConfig {
        mode: DaemonMode::ObserveOnly,
        interval_ms: 100,
        pid_file: None,
        journal_file: None,
        adaptive_polling: true,
        ..Default::default()
    };

    let mut daemon = xynoos_aura::AuraDaemon::new(cfg).unwrap();
    let stop_sig = Arc::new(AtomicBool::new(false));

    // Run 3 bounded cycles
    let diag = daemon.run_with_signal(stop_sig, Some(3)).unwrap();
    assert_eq!(diag.mode, DaemonMode::ObserveOnly);
    assert_eq!(diag.cycles_completed, 3);
    assert!(diag.effective_interval_ms >= 100);
    assert!(diag.polling_tier.is_some());
    assert!(!diag.circuit_breaker_active);
}
