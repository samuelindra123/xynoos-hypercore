//! Milestone N Scoped Authorization Contract & Safe Recovery Tests.
#![forbid(unsafe_code)]

use std::fs;
use xynoos_linux_control::config::{
    ActionClassification, AuthorizationScope, ConfigExecutor, ConfigurationAction,
    PersistenceRequirement, ReversalProcedure, RuntimeAuthorizationContract, SetupAuthorization,
    TypedMutation, VerificationProcedure, CONFIG_ACTION_SCHEMA_VERSION,
};
use xynoos_pressureiq::discovery::types::{
    ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee,
};

#[test]
fn test_runtime_authorization_contract_lifecycle() {
    let temp_dir = std::env::temp_dir().join(format!("xynoos-auth-test-{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();
    let auth_path = temp_dir.join("authorization.json");

    let now_ms = 1_000_000u64;
    let mut auth =
        RuntimeAuthorizationContract::new("operator", "test-plan-fingerprint", now_ms, 300_000);

    // Target identity matching
    assert!(auth.is_target_allowed("/sys/fs/cgroup/xynoos.slice"));
    assert!(auth.is_control_allowed("cpu.weight"));
    assert!(auth.is_control_allowed("memory.high"));
    assert!(!auth.is_control_allowed("arbitrary.root.setting"));

    // Check intervention permission
    let check = auth.can_intervene(
        "cpu.weight",
        "/sys/fs/cgroup/xynoos.slice",
        60,
        now_ms + 1000,
    );
    assert!(check.is_ok());

    // Expiration check
    let expired = auth.can_intervene(
        "cpu.weight",
        "/sys/fs/cgroup/xynoos.slice",
        60,
        now_ms + 400_000,
    );
    assert!(expired.is_err());
    assert!(expired.unwrap_err().contains("expired"));

    // Emergency revocation
    auth.revoke("Operator requested emergency revocation");
    assert!(auth.is_revoked);
    assert!(!auth.is_control_allowed("cpu.weight"));

    let revoked_check = auth.can_intervene(
        "cpu.weight",
        "/sys/fs/cgroup/xynoos.slice",
        60,
        now_ms + 1000,
    );
    assert!(revoked_check.is_err());
    assert!(revoked_check.unwrap_err().contains("revoked"));

    // Save and load
    auth.save_to(&auth_path, now_ms).unwrap();
    let loaded = RuntimeAuthorizationContract::load_from(&auth_path).unwrap();
    assert!(loaded.is_revoked);
    assert_eq!(
        loaded.revocation_reason.as_deref(),
        Some("Operator requested emergency revocation")
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_executor_cpu_frequency_bounds_rollback() {
    let temp_dir = std::env::temp_dir().join(format!("xynoos-exec-bounds-{}", std::process::id()));
    let sys_root = temp_dir.join("sys");
    let proc_root = temp_dir.join("proc");
    let etc_root = temp_dir.join("etc");
    let state_dir = temp_dir.join("var/lib/xynoos");

    let cpu0_cpufreq = sys_root.join("devices/system/cpu/cpu0/cpufreq");
    fs::create_dir_all(&cpu0_cpufreq).unwrap();
    fs::create_dir_all(&proc_root).unwrap();
    fs::create_dir_all(&etc_root).unwrap();
    fs::create_dir_all(&state_dir).unwrap();

    fs::write(cpu0_cpufreq.join("scaling_min_freq"), b"800000").unwrap();
    fs::write(cpu0_cpufreq.join("scaling_max_freq"), b"2400000").unwrap();

    let executor = ConfigExecutor::new(sys_root.clone(), proc_root, etc_root, state_dir);

    let action = ConfigurationAction {
        schema_version: CONFIG_ACTION_SCHEMA_VERSION,
        action_id: "CPU-BOUNDS-001".into(),
        target_subsystem: xynoos_linux_control::config::ConfigSubsystem::Cpu,
        target_setting: "cpufreq.frequency_bounds".into(),
        current_observed_value: "800000:2400000".into(),
        desired_value: "1000000:2000000".into(),
        current_ownership: ConfigurationOwnership::KernelDefault,
        required_privileges: PrivilegeLevel::RequiresRoot,
        required_capability: "cpu.frequency_bounds".into(),
        expected_benefit: "Bound frequency range".into(),
        potential_side_effects: "Reduces peak boost clock".into(),
        confidence: ConfidenceLevel::High,
        reversibility: ReversibilityGuarantee::FullyReversible,
        classification: ActionClassification::Improve,
        authorization_scope: AuthorizationScope::CpuOptimization,
        preconditions: vec![],
        dependencies: vec![],
        conflict_status: None,
        verification_procedure: VerificationProcedure::NoOpPreserved,
        reversal_procedure: ReversalProcedure::NoOpPreserved,
        persistence_requirements: PersistenceRequirement::RuntimeOnly,
        mutation: Some(TypedMutation::ApplyCpuFrequencyBounds {
            cpu_range: "all".into(),
            min_freq_khz: Some(1000000),
            max_freq_khz: Some(2000000),
            original_min: Some(800000),
            original_max: Some(2400000),
        }),
    };

    let now_ms = 1_000_000u64;
    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::FullSetup,
        "plan-fp",
        now_ms,
        60_000,
    );

    let summary = executor
        .execute_plan(&[action], &auth, "plan-fp", now_ms)
        .unwrap();
    assert_eq!(summary.applied_actions, 1);
    assert_eq!(summary.failed_actions, 0);

    // Verify applied values in mock sysfs
    assert_eq!(
        fs::read_to_string(cpu0_cpufreq.join("scaling_min_freq")).unwrap(),
        "1000000"
    );
    assert_eq!(
        fs::read_to_string(cpu0_cpufreq.join("scaling_max_freq")).unwrap(),
        "2000000"
    );

    // Perform rollback
    let rb_summary = executor.rollback_all(now_ms + 1000).unwrap();
    assert_eq!(rb_summary.restored_keys.len(), 1);

    // Verify original values restored
    assert_eq!(
        fs::read_to_string(cpu0_cpufreq.join("scaling_min_freq")).unwrap(),
        "800000"
    );
    assert_eq!(
        fs::read_to_string(cpu0_cpufreq.join("scaling_max_freq")).unwrap(),
        "2400000"
    );

    let _ = fs::remove_dir_all(temp_dir);
}
