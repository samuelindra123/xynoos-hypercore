#![forbid(unsafe_code)]
//! Milestone J.6: Pressure-Aware Policy Correctness & 512 MiB Ceiling Hardening Tests.

use std::collections::BTreeMap;
use std::time::Duration;
use xynoos_aura::{
    daemon::AuraDaemon,
    interplay::{AuraKernelCoordinator, KernelTuningRecommendation},
    tiering::{AuraTieringCoordinator, PressureBridge, PressureStabilizer},
    AdaptiveTieringPolicy, DaemonConfig, DaemonMode,
};
use xynoos_core::MemoryPressure;
use xynoos_linux_control::MAX_HIGH_BYTES;
use xynoos_pressureiq::intelligence::{diagnose, Diagnosis, Observation};

fn make_diag(classes: Vec<&str>, avail_pct: Option<f64>, reliable: bool) -> Diagnosis {
    Diagnosis {
        classes: classes.into_iter().map(String::from).collect(),
        evidence: vec!["milestone-j6-test".into()],
        limitations: vec![],
        available_pct: avail_pct,
        rates_per_second: BTreeMap::new(),
        disk_bytes_per_second: BTreeMap::new(),
        zram_ratios: BTreeMap::new(),
        interval_seconds: Some(1.0),
        reliable,
    }
}

fn sample_obs() -> Observation {
    Observation {
        timestamp_ms: Some(1000),
        collection_ms: 10,
        mem_total: Some(3800 * 1024 * 1024), // 3.8 GiB total (like N5000)
        mem_available: Some(372 * 1024 * 1024), // 9.8% available RAM
        swap_total: Some(4000 * 1024 * 1024), // 4.0 GiB swap (zram + disk)
        swap_free: Some(2500 * 1024 * 1024), // plenty of swap free
        memory_some_10: Some(0.0),           // 0% memory PSI avg10
        memory_some_60: Some(0.0),
        memory_full_10: Some(0.0),
        io_some_10: Some(0.0),
        vm: Some(
            [
                ("pgscan_direct", 0),
                ("pgsteal_direct", 0),
                ("pgscan_kswapd", 0),
                ("pgsteal_kswapd", 0),
                ("pswpin", 0),
                ("pswpout", 0),
                ("pgmajfault", 0),
                ("allocstall_normal", 0),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect(),
        ),
        disks: Some(BTreeMap::new()),
        zram: Some(vec![]),
    }
}

#[test]
fn test_low_available_ram_without_stalls_evaluates_to_moderate_not_high() {
    // Exact scenario from physical N5000 J.5 report:
    // 9.8% available RAM, 0% PSI, active zram and swap, zero allocation stalls
    let a = sample_obs();
    let b = a.clone();
    let diag = diagnose(&a, &b, Duration::from_secs(1), Duration::ZERO);

    assert!(diag.reliable);
    assert_eq!(diag.classes, ["normal"]); // PressureIQ correctly identifies no starvation
    let avail = diag.available_pct.unwrap();
    assert!((9.7..9.9).contains(&avail));

    // Hardened PressureBridge must NOT evaluate 9.8% RAM with 0 PSI as High
    let pressure = PressureBridge::evaluate_pressure(&diag);
    assert_eq!(
        pressure,
        MemoryPressure::Moderate,
        "Low available RAM without sustained stalls must evaluate to Moderate, never High!"
    );

    // AuraKernelCoordinator under Moderate must recommend ProtectEssential, NEVER ThrottleBackground
    let coordinator =
        AuraKernelCoordinator::new(AuraTieringCoordinator::new(AdaptiveTieringPolicy::default()));
    let rec = coordinator.recommend(pressure);
    match rec {
        KernelTuningRecommendation::ProtectEssential { bytes } => {
            assert_eq!(bytes, coordinator.default_protection_bytes);
        }
        other => panic!("Expected ProtectEssential under Moderate, got: {:?}", other),
    }
}

#[test]
fn test_low_available_ram_with_rising_psi_and_reclaim_escalates_to_high() {
    let a = sample_obs();
    let mut b = a.clone();
    // Memory PSI rises to 3.5%, direct page scan occurs
    b.memory_some_10 = Some(3.5);
    b.memory_some_60 = Some(2.0);
    b.vm.as_mut().unwrap().insert("pgscan_direct".into(), 500);

    let diag = diagnose(&a, &b, Duration::from_secs(1), Duration::ZERO);
    assert!(diag.classes.contains(&"active_direct_reclaim".into()));

    let pressure = PressureBridge::evaluate_pressure(&diag);
    assert_eq!(
        pressure,
        MemoryPressure::High,
        "Low RAM with active direct reclaim must escalate to High!"
    );

    let coordinator =
        AuraKernelCoordinator::new(AuraTieringCoordinator::new(AdaptiveTieringPolicy::default()));
    let rec = coordinator.recommend(pressure);
    match rec {
        KernelTuningRecommendation::ThrottleBackground { bytes } => {
            assert_eq!(bytes, coordinator.default_throttle_bytes);
        }
        other => panic!("Expected ThrottleBackground under High, got: {:?}", other),
    }
}

#[test]
fn test_high_swap_utilization_without_pressure() {
    let a = sample_obs();
    let mut b = a.clone();
    // 85% swap utilized (15% free), but 0% PSI stalls and no swap thrashing churn
    b.swap_total = Some(4000 * 1024 * 1024);
    b.swap_free = Some(600 * 1024 * 1024); // 15% free
    let diag = diagnose(&a, &b, Duration::from_secs(1), Duration::ZERO);

    assert!(diag.classes.contains(&"high_swap_utilization".into()));
    let pressure = PressureBridge::evaluate_pressure(&diag);
    assert_eq!(
        pressure,
        MemoryPressure::Moderate,
        "High swap utilization without active stalls must evaluate to Moderate!"
    );
}

#[test]
fn test_swap_exhaustion_risk_triggers_critical() {
    let a = sample_obs();
    let mut b = a.clone();
    // Swap nearly exhausted (< 5% free) with low available RAM (< 10%)
    b.swap_total = Some(4000 * 1024 * 1024);
    b.swap_free = Some(100 * 1024 * 1024); // 2.5% free
    b.mem_available = Some(200 * 1024 * 1024); // ~5% RAM
    let diag = diagnose(&a, &b, Duration::from_secs(1), Duration::ZERO);

    assert!(diag.classes.contains(&"swap_exhaustion_risk".into()));
    let pressure = PressureBridge::evaluate_pressure(&diag);
    assert_eq!(
        pressure,
        MemoryPressure::Critical,
        "Swap exhaustion risk must trigger Critical to prevent kernel OOM killer!"
    );

    let coordinator =
        AuraKernelCoordinator::new(AuraTieringCoordinator::new(AdaptiveTieringPolicy::default()));
    assert!(matches!(
        coordinator.recommend(pressure),
        KernelTuningRecommendation::EmergencyLimit { .. }
    ));
}

#[test]
fn test_invalid_or_missing_telemetry_fails_closed_to_moderate() {
    // Missing available RAM
    let mut d_missing = make_diag(vec!["normal"], None, true);
    d_missing.interval_seconds = Some(1.0);
    let (p1, v1) = PressureBridge::evaluate_pressure_with_status(&d_missing);
    assert_eq!(p1, MemoryPressure::Moderate);
    assert_eq!(v1, xynoos_aura::TelemetryValidation::MissingMetrics);

    // Negative interval
    let mut d_neg = make_diag(vec!["normal"], Some(50.0), true);
    d_neg.interval_seconds = Some(-1.0);
    let (p2, v2) = PressureBridge::evaluate_pressure_with_status(&d_neg);
    assert_eq!(p2, MemoryPressure::Moderate);
    assert_eq!(v2, xynoos_aura::TelemetryValidation::InvalidRange);

    // Contradictory signals: normal combined with swap_thrashing
    let d_contra = make_diag(vec!["normal", "swap_thrashing"], Some(20.0), true);
    let (p3, v3) = PressureBridge::evaluate_pressure_with_status(&d_contra);
    assert_eq!(p3, MemoryPressure::Moderate);
    assert_eq!(v3, xynoos_aura::TelemetryValidation::ContradictorySignals);
}

#[test]
fn test_stale_telemetry_fails_closed() {
    let d_stale = make_diag(vec!["normal"], Some(90.0), false);
    let (pressure, validation) = PressureBridge::evaluate_pressure_with_status(&d_stale);
    assert_eq!(pressure, MemoryPressure::Moderate);
    assert_eq!(validation, xynoos_aura::TelemetryValidation::Unreliable);
}

#[test]
fn test_transient_spikes_and_recovery_hysteresis() {
    let mut stabilizer = PressureStabilizer::new(3, 1);

    // Initial low pressure
    assert_eq!(stabilizer.step(MemoryPressure::Low), MemoryPressure::Low);

    // Transient spike to Moderate: candidate ticks = 1, current remains Low
    assert_eq!(
        stabilizer.step(MemoryPressure::Moderate),
        MemoryPressure::Low
    );

    // Emergency spike to Critical: must escalate IMMEDIATELY (0 ticks) for OOM prevention
    assert_eq!(
        stabilizer.step(MemoryPressure::Critical),
        MemoryPressure::Critical
    );

    // Pressure subsides to Low: de-escalation must hold for 3 consecutive ticks
    assert_eq!(
        stabilizer.step(MemoryPressure::Low),
        MemoryPressure::Critical,
        "Tick 1 of de-escalation hold"
    );
    assert_eq!(
        stabilizer.step(MemoryPressure::Low),
        MemoryPressure::Critical,
        "Tick 2 of de-escalation hold"
    );
    assert_eq!(
        stabilizer.step(MemoryPressure::Low),
        MemoryPressure::Low,
        "Tick 3: de-escalation hold satisfied, transitions to Low"
    );
}

#[test]
fn test_512mib_ceiling_and_admin_limit_weakening_protection() {
    let daemon = AuraDaemon::new(DaemonConfig {
        mode: DaemonMode::Transactional,
        ..Default::default()
    })
    .unwrap();

    let rec_throttle = KernelTuningRecommendation::ThrottleBackground {
        bytes: 64 * 1024 * 1024,
    };

    // 1. Current usage >= MAX_HIGH_BYTES (512 MiB) is rejected
    assert!(daemon
        .compute_workload_aware_action(&rec_throttle, MAX_HIGH_BYTES)
        .is_none());
    assert!(daemon
        .compute_workload_aware_action(&rec_throttle, 600 * 1024 * 1024)
        .is_none());

    // 2. Aligned target <= current usage is rejected
    // 100 MiB usage -> target 125 MiB (> usage) -> Ok
    let action_100 = daemon
        .compute_workload_aware_action(&rec_throttle, 100 * 1024 * 1024)
        .unwrap();
    assert_eq!(action_100.bytes(), 125 * 1024 * 1024);

    // 3. Weakening pre-existing administrator limits:
    // Admin set memory.high to 100 MiB; proposing 125 MiB would loosen admin limit -> REJECTED (None)
    assert!(daemon
        .compute_workload_aware_action_with_admin_guard(
            &rec_throttle,
            100 * 1024 * 1024,
            Some(100 * 1024 * 1024)
        )
        .is_none());

    // Admin left memory.high unconstrained (u64::MAX); proposing 125 MiB is allowed
    assert!(daemon
        .compute_workload_aware_action_with_admin_guard(
            &rec_throttle,
            100 * 1024 * 1024,
            Some(u64::MAX)
        )
        .is_some());

    // Admin set memory.low protection to 64 MiB; proposing 11 MiB (10 MiB * 1.10) would weaken protection -> REJECTED (None)
    let rec_protect = KernelTuningRecommendation::ProtectEssential {
        bytes: 16 * 1024 * 1024,
    };
    assert!(daemon
        .compute_workload_aware_action_with_admin_guard(
            &rec_protect,
            10 * 1024 * 1024,
            Some(64 * 1024 * 1024)
        )
        .is_none());

    // Admin left memory.low at 0; proposing 11 MiB increases protection -> allowed
    assert!(daemon
        .compute_workload_aware_action_with_admin_guard(&rec_protect, 10 * 1024 * 1024, Some(0))
        .is_some());
}
