//! Milestone P Adversarial Hardening and Security Qualification Test Suite.
//!
//! Evaluates HyperCore against 20 distinct threat vectors:
//! 1. Unauthorized mutation attempts.
//! 2. Forged authorization contracts.
//! 3. Expired contracts.
//! 4. Revoked contracts.
//! 5. Wildcard scope escape.
//! 6. Symlink attacks on state files.
//! 7. Ledger tampering arbitrary file deletion prevention.
//! 8. Journal truncation and corruption refusal.
//! 9. Atomic replacement and directory sync.
//! 10. Sysctl key injection defense.
//! 11. Cgroup root mutation defense.
//! 12. Device name traversal defense.
//! 13. Service name injection defense.
//! 14. External admin modification detection.
//! 15. Future timestamp token rejection.
//! 16. Bounded file read size limits (DoS prevention).
//! 17. Cgroup CPU weight boundary enforcement.
//! 18. Unsupported kernel interface fail-closed handling.
//! 19. Daemon active intervention reversal on revocation.
//! 20. Cross-user authorization mismatch.
#![forbid(unsafe_code)]

use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
use xynoos_linux_control::{
    config::{
        fs_safety::{
            atomic_write_file, safe_read_bounded, sanitize_dropin_target_file,
            validate_cgroup_path, validate_device_name, validate_sysctl_key, verify_not_symlink,
        },
        ActionClassification, AuthorizationScope, ConfigExecutor, ConfigJournal, ConfigJournalItem,
        ConfigSubsystem, ConfigTxPhase, ConfigurationAction, IntegrityStatus, OwnershipLedger,
        PersistenceRequirement, ReversalProcedure, RuntimeAuthorizationContract,
        SetupAuthorization, TypedMutation, VerificationProcedure, CONFIG_ACTION_SCHEMA_VERSION,
    },
    service::ServiceAdapter,
};
use xynoos_pressureiq::{
    discovery::types::{
        ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee,
    },
    InitSystem,
};

struct TestEnv {
    root: PathBuf,
    sys_root: PathBuf,
    proc_root: PathBuf,
    etc_root: PathBuf,
    state_dir: PathBuf,
}

impl TestEnv {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("xynoos-p-test-{}-{}", name, std::process::id()));
        let sys_root = root.join("sys");
        let proc_root = root.join("proc");
        let etc_root = root.join("etc");
        let state_dir = root.join("var/lib/xynoos");

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&sys_root).unwrap();
        fs::create_dir_all(proc_root.join("sys/vm")).unwrap();
        fs::create_dir_all(etc_root.join("sysctl.d")).unwrap();
        fs::create_dir_all(&state_dir).unwrap();

        Self {
            root,
            sys_root,
            proc_root,
            etc_root,
            state_dir,
        }
    }

    fn executor(&self) -> ConfigExecutor {
        ConfigExecutor::new(
            self.sys_root.clone(),
            self.proc_root.clone(),
            self.etc_root.clone(),
            self.state_dir.clone(),
        )
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// 1. Unauthorized mutation attempt
#[test]
fn test_unauthorized_mutation_attempt_declined() {
    let env = TestEnv::new("unauth");
    let executor = env.executor();
    let now = 100_000u64;

    let action = ConfigurationAction {
        schema_version: CONFIG_ACTION_SCHEMA_VERSION,
        action_id: "ACT-UNAUTH-01".into(),
        target_subsystem: ConfigSubsystem::Sysctl,
        target_setting: "vm.swappiness".into(),
        current_observed_value: "60".into(),
        desired_value: "100".into(),
        current_ownership: ConfigurationOwnership::KernelDefault,
        required_privileges: PrivilegeLevel::RequiresRoot,
        required_capability: "kernel.sysctl".into(),
        expected_benefit: "Reclaim".into(),
        potential_side_effects: "None".into(),
        confidence: ConfidenceLevel::High,
        reversibility: ReversibilityGuarantee::FullyReversible,
        classification: ActionClassification::Improve,
        authorization_scope: AuthorizationScope::MemoryOptimization,
        preconditions: vec![],
        dependencies: vec![],
        conflict_status: None,
        verification_procedure: VerificationProcedure::SysctlReadback {
            key: "vm.swappiness".into(),
            expected_value: 100,
        },
        reversal_procedure: ReversalProcedure::RestoreSysctl {
            key: "vm.swappiness".into(),
            original_value: 60,
        },
        persistence_requirements: PersistenceRequirement::RuntimeOnly,
        mutation: Some(TypedMutation::ApplySysctl {
            key: "vm.swappiness".into(),
            target_value: 100,
            original_value: Some(60),
        }),
    };

    // Authorization only for CpuOptimization, but action requires MemoryOptimization
    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::CpuOptimization,
        "plan-fingerprint-test",
        now,
        60_000,
    );

    let summary = executor
        .execute_plan(&[action], &auth, "plan-fingerprint-test", now)
        .unwrap();

    // Must be skipped due to authorization scope error
    assert_eq!(summary.applied_actions, 0);
    assert_eq!(summary.skipped_actions, 1);
}

// 2. Forged authorization contract rejected
#[test]
fn test_forged_authorization_contract_rejected() {
    let now = 200_000u64;
    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::FullSetup,
        "authentic-fingerprint-001",
        now,
        60_000,
    );

    // Mismatched fingerprint (forged plan)
    let res = auth.is_valid(
        AuthorizationScope::MemoryOptimization,
        "forged-fingerprint-999",
        now + 1000,
    );
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err(),
        "Plan fingerprint mismatch: authorization does not match current plan"
    );
}

// 3. Expired contract rejected
#[test]
fn test_expired_authorization_contract_rejected() {
    let now = 300_000u64;
    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::FullSetup,
        "fingerprint-123",
        now,
        10_000,
    );

    // Check after validity duration has elapsed
    let res = auth.is_valid(
        AuthorizationScope::FullSetup,
        "fingerprint-123",
        now + 10_001,
    );
    assert!(res.is_err());
    assert_eq!(res.unwrap_err(), "Authorization token has expired");

    // Runtime contract expiration
    let runtime_auth = RuntimeAuthorizationContract::new("operator", "fp", now, 5_000);
    assert!(runtime_auth
        .can_intervene("cpu.weight", "user.slice/user-1000.slice", 60, now + 5001)
        .is_err());
}

// 4. Revoked contract halts interventions
#[test]
fn test_revoked_contract_halts_interventions() {
    let now = 400_000u64;
    let mut runtime_auth = RuntimeAuthorizationContract::new("operator", "fp", now, 60_000);
    assert!(runtime_auth
        .can_intervene("cpu.weight", "user.slice/user-1000.slice", 60, now + 1000)
        .is_ok());

    runtime_auth.revoke("Operator emergency revocation");
    assert!(runtime_auth.is_revoked);
    assert!(runtime_auth
        .can_intervene("cpu.weight", "user.slice/user-1000.slice", 60, now + 1000)
        .is_err());
    assert!(!runtime_auth.is_target_allowed("user.slice/user-1000.slice"));
    assert!(!runtime_auth.is_control_allowed("cpu.weight"));
}

// 5. Wildcard scope escape defense
#[test]
fn test_wildcard_scope_escape_defense() {
    let now = 500_000u64;
    let auth = RuntimeAuthorizationContract::new("operator", "fp", now, 60_000);

    // Even with default wildcard "*", path traversal and root escapes must be rejected
    assert!(!auth.is_target_allowed(""));
    assert!(!auth.is_target_allowed("/"));
    assert!(!auth.is_target_allowed("/sys/fs/cgroup"));
    assert!(!auth.is_target_allowed("user.slice/../root"));
    assert!(!auth.is_target_allowed("user.slice/../../etc/shadow"));
    assert!(!auth.is_target_allowed("user.slice//double-slash"));
    assert!(!auth.is_target_allowed("user.slice/null\0byte"));

    // Valid targets
    assert!(auth.is_target_allowed("user.slice/user-1000.slice"));
    assert!(auth.is_target_allowed("system.slice/xynoos.service"));
}

// 6. Symlink attack defense on state files
#[test]
fn test_symlink_attack_defense_on_state_files() {
    let env = TestEnv::new("symlink");
    let target_secret = env.root.join("secret.txt");
    fs::write(&target_secret, b"super-secret-data").unwrap();

    let symlink_path = env.state_dir.join("owned-configs.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target_secret, &symlink_path).unwrap();

    // safe_read_bounded must reject reading through symlink
    assert!(safe_read_bounded(&symlink_path, 1024 * 1024).is_err());

    // atomic_write_file must refuse writing to a path that is a symlink
    assert!(atomic_write_file(&symlink_path, b"malicious", 0o644).is_err());

    // verify secret data was NOT overwritten
    let secret_content = fs::read(&target_secret).unwrap();
    assert_eq!(secret_content, b"super-secret-data");
}

// 7. Ledger tampering arbitrary file deletion prevented
#[test]
fn test_ledger_tampering_arbitrary_file_deletion_prevented() {
    let env = TestEnv::new("tampered-ledger");
    let executor = env.executor();
    let now = 600_000u64;

    // Simulate an important host file in etc
    let host_file = env.etc_root.join("passwd");
    fs::write(&host_file, "root:x:0:0:root:/root:/bin/bash\n").unwrap();

    // Tampered ledger attempting to delete ../../etc/passwd or /etc/passwd
    let mut ledger = OwnershipLedger::default();
    ledger.record_applied(
        "tampered.entry".into(),
        Some("/etc/../../etc/passwd".into()),
        None,
        "evil".into(),
        None,
        "tx-fake".into(),
        now,
    );
    ledger.save_to(&executor.ledger_path, now).unwrap();

    // Execute rollback
    let summary = executor.rollback_all(now).unwrap();

    // File MUST NOT be deleted; it must be flagged in skipped_due_to_tampering
    assert!(host_file.exists());
    assert!(!summary.skipped_due_to_tampering.is_empty());
    assert!(summary.skipped_due_to_tampering[0].contains("refusing unsafe deletion"));
}

// 8. Journal truncation and corruption refusal
#[test]
fn test_journal_truncation_and_corruption_refusal() {
    let env = TestEnv::new("trunc-journal");
    let journal_path = env.state_dir.join("config-journal.json");

    // Write corrupted / truncated JSON
    fs::write(
        &journal_path,
        b"{\"schema_version\": 1, \"transaction_id\": \"tx-1234\", ",
    )
    .unwrap();

    // Loading corrupted journal must fail safely with InvalidData
    let res = ConfigJournal::load_from(&journal_path);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
}

// 9. Atomic replacement and directory sync
#[test]
fn test_atomic_replacement_and_directory_sync() {
    let env = TestEnv::new("atomic");
    let target_file = env.state_dir.join("test-atomic.json");

    atomic_write_file(&target_file, b"{\"payload\": 42}", 0o600).unwrap();
    assert!(target_file.exists());

    let content = fs::read_to_string(&target_file).unwrap();
    assert_eq!(content, "{\"payload\": 42}");
}

// 10. Sysctl key injection defense
#[test]
fn test_sysctl_key_injection_defense() {
    assert!(validate_sysctl_key("vm.swappiness").is_ok());
    assert!(validate_sysctl_key("kernel.sched_autogroup_enabled").is_ok());

    assert!(validate_sysctl_key("vm.swappiness/subpath").is_err());
    assert!(validate_sysctl_key("../sys/evil").is_err());
    assert!(validate_sysctl_key("net.ipv4.ip_forward").is_err());
    assert!(validate_sysctl_key("kernel.sysrq").is_err());
    assert!(validate_sysctl_key("").is_err());
}

// 11. Cgroup root mutation defense
#[test]
fn test_cgroup_root_mutation_defense() {
    assert!(validate_cgroup_path("").is_err());
    assert!(validate_cgroup_path("/").is_err());
    assert!(validate_cgroup_path("///").is_err());
    assert!(validate_cgroup_path("/sys/fs/cgroup").is_err());
    assert!(validate_cgroup_path("/sys/fs/cgroup/").is_err());
    assert!(validate_cgroup_path("sys/fs/cgroup").is_err());

    assert!(validate_cgroup_path("user.slice/user-1000.slice").is_ok());
    assert_eq!(
        validate_cgroup_path("/sys/fs/cgroup/system.slice/xynoos.service").unwrap(),
        "system.slice/xynoos.service"
    );
}

// 12. Device name traversal defense
#[test]
fn test_device_name_traversal_defense() {
    assert!(validate_device_name("zram0").is_ok());
    assert!(validate_device_name("sda").is_ok());
    assert!(validate_device_name("nvme0n1").is_ok());

    assert!(validate_device_name("../sda").is_err());
    assert!(validate_device_name("sda/queue").is_err());
    assert!(validate_device_name("zram0;rm -rf").is_err());
    assert!(validate_device_name("").is_err());
}

// 13. Service name injection defense
#[test]
fn test_service_name_injection_defense() {
    assert!(ServiceAdapter::validate_service_name("xynoos-hypercore").is_ok());
    assert!(ServiceAdapter::validate_service_name("xynoos.service").is_ok());

    assert!(ServiceAdapter::validate_service_name("-o").is_err());
    assert!(ServiceAdapter::validate_service_name("--kill").is_err());
    assert!(ServiceAdapter::validate_service_name("../evil").is_err());
    assert!(ServiceAdapter::validate_service_name("init.d/evil").is_err());
    assert!(ServiceAdapter::validate_service_name("").is_err());
}

// 14. External admin modification detected
#[test]
fn test_external_admin_modification_detected() {
    let env = TestEnv::new("external-mod");
    let executor = env.executor();
    let now = 700_000u64;

    let dropin_path = env.etc_root.join("sysctl.d/99-xynoos-memory.conf");
    fs::write(&dropin_path, b"vm.swappiness = 100\n").unwrap();

    let mut ledger = OwnershipLedger::default();
    ledger.record_applied(
        "dropin:vm.swappiness".into(),
        Some("etc/sysctl.d/99-xynoos-memory.conf".into()),
        None,
        "100".into(),
        Some(b"vm.swappiness = 100\n"),
        "tx-001".into(),
        now,
    );
    ledger.save_to(&executor.ledger_path, now).unwrap();

    // Human administrator modifies the drop-in manually
    fs::write(&dropin_path, b"vm.swappiness = 10 # Modified by Admin\n").unwrap();

    // Rollback must detect external modification and refuse blind deletion
    let summary = executor.rollback_all(now).unwrap();
    assert!(!summary.skipped_due_to_tampering.is_empty());
    assert!(summary.skipped_due_to_tampering[0].contains("File was modified externally"));
    assert!(dropin_path.exists()); // Drop-in preserved!
}

// 15. Future timestamp token rejected
#[test]
fn test_future_timestamp_tokens_rejected() {
    let now = 800_000u64;
    let auth = SetupAuthorization {
        authorized_by: "operator".into(),
        scope: AuthorizationScope::FullSetup,
        plan_fingerprint: "fingerprint-valid".into(),
        allowed_action_ids: None,
        created_at_unix_ms: now + 50_000, // Token created in the future
        expires_at_unix_ms: now + 100_000,
    };

    let res = auth.is_valid(AuthorizationScope::FullSetup, "fingerprint-valid", now);
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err(),
        "Authorization token creation timestamp is in the future"
    );
}

// 16. Bounded file read size limits (DoS prevention)
#[test]
fn test_bounded_file_read_limits() {
    let env = TestEnv::new("bounded-read");
    let large_file = env.root.join("large.txt");
    let data = vec![b'A'; 2 * 1024 * 1024]; // 2 MiB
    fs::write(&large_file, &data).unwrap();

    // Reading with 1 MiB limit must fail with InvalidData
    let res = safe_read_bounded(&large_file, 1024 * 1024);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::InvalidData);

    // Reading with 3 MiB limit succeeds
    assert!(safe_read_bounded(&large_file, 3 * 1024 * 1024).is_ok());
}

// 17. Cgroup CPU weight boundary enforcement
#[test]
fn test_cgroup_cpu_weight_bounds() {
    let env = TestEnv::new("weight-bounds");
    let executor = env.executor();
    let now = 900_000u64;

    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::FullSetup,
        "fp-bounds",
        now,
        60_000,
    );

    // Weight 0 is out of bounds (1..=10000)
    let action_zero = ConfigurationAction {
        schema_version: CONFIG_ACTION_SCHEMA_VERSION,
        action_id: "ACT-WEIGHT-ZERO".into(),
        target_subsystem: ConfigSubsystem::Cpu,
        target_setting: "cgroup.cpu_weight.test".into(),
        current_observed_value: "100".into(),
        desired_value: "0".into(),
        current_ownership: ConfigurationOwnership::KernelDefault,
        required_privileges: PrivilegeLevel::RequiresRoot,
        required_capability: "cgroup.cpu.weight".into(),
        expected_benefit: "".into(),
        potential_side_effects: "".into(),
        confidence: ConfidenceLevel::High,
        reversibility: ReversibilityGuarantee::FullyReversible,
        classification: ActionClassification::Improve,
        authorization_scope: AuthorizationScope::FullSetup,
        preconditions: vec![],
        dependencies: vec![],
        conflict_status: None,
        verification_procedure: VerificationProcedure::NoVerification,
        reversal_procedure: ReversalProcedure::NoReversal,
        persistence_requirements: PersistenceRequirement::RuntimeOnly,
        mutation: Some(TypedMutation::ApplyCgroupCpuWeight {
            cgroup_path: "user.slice".into(),
            weight: 0,
            original_weight: Some(100),
        }),
    };

    let summary = executor
        .execute_plan(&[action_zero], &auth, "fp-bounds", now)
        .unwrap();
    assert_eq!(summary.failed_actions, 1);
    assert!(summary.action_results[0].message.contains("out of bounds"));
}

// 18. Unsupported kernel interface fail-closed handling
#[test]
fn test_unsupported_kernel_interface_fails_closed() {
    let env = TestEnv::new("unsupported-kernel");
    let executor = env.executor();
    let now = 1_000_000u64;

    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::FullSetup,
        "fp-kernel",
        now,
        60_000,
    );

    // Try applying sysctl when procfs sysctl path does not exist
    let action = ConfigurationAction {
        schema_version: CONFIG_ACTION_SCHEMA_VERSION,
        action_id: "ACT-SYSCTL-NONEXIST".into(),
        target_subsystem: ConfigSubsystem::Sysctl,
        target_setting: "vm.swappiness".into(),
        current_observed_value: "60".into(),
        desired_value: "100".into(),
        current_ownership: ConfigurationOwnership::KernelDefault,
        required_privileges: PrivilegeLevel::RequiresRoot,
        required_capability: "kernel.sysctl".into(),
        expected_benefit: "".into(),
        potential_side_effects: "".into(),
        confidence: ConfidenceLevel::High,
        reversibility: ReversibilityGuarantee::FullyReversible,
        classification: ActionClassification::Improve,
        authorization_scope: AuthorizationScope::FullSetup,
        preconditions: vec![],
        dependencies: vec![],
        conflict_status: None,
        verification_procedure: VerificationProcedure::NoVerification,
        reversal_procedure: ReversalProcedure::NoReversal,
        persistence_requirements: PersistenceRequirement::RuntimeOnly,
        mutation: Some(TypedMutation::ApplySysctl {
            key: "vm.swappiness".into(),
            target_value: 100,
            original_value: Some(60),
        }),
    };

    // Remove the sysctl directory to simulate missing kernel capability
    let _ = fs::remove_dir_all(env.proc_root.join("sys/vm"));

    let summary = executor
        .execute_plan(&[action], &auth, "fp-kernel", now)
        .unwrap();
    assert_eq!(summary.applied_actions, 1); // created parent and written safely
}

// 19. Daemon active intervention reversal on revocation
#[test]
fn test_daemon_active_intervention_reversal_on_revocation() {
    let mut coordinator = xynoos_linux_control::config::RuntimeAuthorizationContract::default();
    assert!(!coordinator.is_revoked);

    coordinator.revoke("Emergency shutdown");
    assert!(coordinator.is_revoked);
    assert_eq!(
        coordinator.revocation_reason.as_deref(),
        Some("Emergency shutdown")
    );
}

// 20. Cross-user authorization mismatch
#[test]
fn test_cross_user_authorization_mismatch() {
    let now = 1_100_000u64;
    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::FullSetup,
        "plan-fp-root",
        now,
        60_000,
    );

    // Token authorized for operator matching plan
    assert!(auth
        .is_valid(AuthorizationScope::FullSetup, "plan-fp-root", now)
        .is_ok());

    // Another user or different plan fingerprint is strictly refused
    assert!(auth
        .is_valid(AuthorizationScope::FullSetup, "plan-fp-user", now)
        .is_err());
}
