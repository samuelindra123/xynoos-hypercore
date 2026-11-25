//! Unit and integration tests for the reversible configuration engine, ownership ledger,
//! durable journal, authorization model, and universal memory optimization planner.
#![forbid(unsafe_code)]

use std::fs;
use xynoos_linux_control::config::{
    ActionClassification, AuthorizationScope, ConfigExecutor, ConfigJournal, ConfigJournalItem,
    ConfigSubsystem, ConfigTxPhase, ConfigurationAction, IntegrityStatus, MemoryPlanner,
    MemoryProfile, OwnershipLedger, PersistenceRequirement, ReversalProcedure, SetupAuthorization,
    TypedMutation, VerificationProcedure, CONFIG_ACTION_SCHEMA_VERSION,
};
use xynoos_pressureiq::discovery::{
    inventory::SystemInventory,
    types::{ConfidenceLevel, ConfigurationOwnership, PrivilegeLevel, ReversibilityGuarantee},
};

#[test]
fn test_configuration_action_validation() {
    let action = ConfigurationAction {
        schema_version: CONFIG_ACTION_SCHEMA_VERSION,
        action_id: "ACT-TEST-001".into(),
        target_subsystem: ConfigSubsystem::Sysctl,
        target_setting: "vm.swappiness".into(),
        current_observed_value: "60".into(),
        desired_value: "100".into(),
        current_ownership: ConfigurationOwnership::KernelDefault,
        required_privileges: PrivilegeLevel::RequiresRoot,
        required_capability: "kernel.sysctl".into(),
        expected_benefit: "Optimize reclaim".into(),
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

    assert!(action.validate().is_ok());
    assert!(action.is_privileged());
    assert!(action.is_mutation());
}

#[test]
fn test_authorization_scope_and_expiration() {
    let now = 1_000_000u64;
    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::MemoryOptimization,
        "plan-fingerprint-abc",
        now,
        60_000,
    );

    // Valid within window and matching fingerprint
    assert!(auth
        .is_valid(
            AuthorizationScope::MemoryOptimization,
            "plan-fingerprint-abc",
            now + 10_000
        )
        .is_ok());

    // Fails on fingerprint mismatch (tampering defense)
    assert!(auth
        .is_valid(
            AuthorizationScope::MemoryOptimization,
            "plan-fingerprint-tampered",
            now + 10_000
        )
        .is_err());

    // Fails on expiration
    assert!(auth
        .is_valid(
            AuthorizationScope::MemoryOptimization,
            "plan-fingerprint-abc",
            now + 70_000
        )
        .is_err());
}

#[test]
fn test_ownership_ledger_and_tampering_detection() {
    let temp_dir = std::env::temp_dir().join("xynoos_ledger_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let file_path = temp_dir.join("test_dropin.conf");
    let original_bytes = b"vm.swappiness = 100\n";
    fs::write(&file_path, original_bytes).unwrap();

    let mut ledger = OwnershipLedger::default();
    ledger.record_applied(
        "dropin:vm.swappiness".into(),
        Some(file_path.to_string_lossy().to_string()),
        None,
        "100".into(),
        Some(original_bytes),
        "tx-100".into(),
        1000,
    );

    let entry = ledger.entries.get("dropin:vm.swappiness").unwrap();
    let status_clean = ledger.check_entry_integrity(entry, std::path::Path::new("/"));
    assert_eq!(status_clean, IntegrityStatus::Clean);

    // Simulate external tampering by human administrator
    fs::write(&file_path, b"vm.swappiness = 10 # altered by admin\n").unwrap();
    let status_tampered = ledger.check_entry_integrity(entry, std::path::Path::new("/"));
    assert_eq!(status_tampered, IntegrityStatus::ModifiedExternally);

    // Clean up
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_executor_apply_and_rollback_lifecycle() {
    let root = std::env::temp_dir().join("xynoos_executor_test");
    let _ = fs::remove_dir_all(&root);

    let sys_root = root.join("sys");
    let proc_root = root.join("proc");
    let etc_root = root.join("etc");
    let state_dir = root.join("var/lib/xynoos");

    fs::create_dir_all(&sys_root).unwrap();
    fs::create_dir_all(proc_root.join("sys/vm")).unwrap();
    fs::create_dir_all(etc_root.join("sysctl.d")).unwrap();
    fs::create_dir_all(&state_dir).unwrap();

    // Baseline sysctl runtime value
    let swappiness_file = proc_root.join("sys/vm/swappiness");
    fs::write(&swappiness_file, b"60\n").unwrap();

    let executor = ConfigExecutor::new(
        sys_root.clone(),
        proc_root.clone(),
        etc_root.clone(),
        state_dir.clone(),
    );

    let actions = vec![
        ConfigurationAction {
            schema_version: CONFIG_ACTION_SCHEMA_VERSION,
            action_id: "ACT-APPLY-SWAPPINESS".into(),
            target_subsystem: ConfigSubsystem::Sysctl,
            target_setting: "vm.swappiness".into(),
            current_observed_value: "60".into(),
            desired_value: "100".into(),
            current_ownership: ConfigurationOwnership::KernelDefault,
            required_privileges: PrivilegeLevel::RequiresRoot,
            required_capability: "kernel.sysctl".into(),
            expected_benefit: "Zram caching".into(),
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
            persistence_requirements: PersistenceRequirement::RuntimeAndDropIn,
            mutation: Some(TypedMutation::ApplySysctl {
                key: "vm.swappiness".into(),
                target_value: 100,
                original_value: Some(60),
            }),
        },
        ConfigurationAction {
            schema_version: CONFIG_ACTION_SCHEMA_VERSION,
            action_id: "ACT-DROPIN-SWAPPINESS".into(),
            target_subsystem: ConfigSubsystem::Sysctl,
            target_setting: "etc/sysctl.d/99-xynoos-memory.conf".into(),
            current_observed_value: "None".into(),
            desired_value: "Drop-in file".into(),
            current_ownership: ConfigurationOwnership::KernelDefault,
            required_privileges: PrivilegeLevel::RequiresRoot,
            required_capability: "fs.sysctl_d".into(),
            expected_benefit: "Persistence".into(),
            potential_side_effects: "None".into(),
            confidence: ConfidenceLevel::High,
            reversibility: ReversibilityGuarantee::FullyReversible,
            classification: ActionClassification::Configure,
            authorization_scope: AuthorizationScope::MemoryOptimization,
            preconditions: vec![],
            dependencies: vec![],
            conflict_status: None,
            verification_procedure: VerificationProcedure::DropInFileExists {
                path: "etc/sysctl.d/99-xynoos-memory.conf".into(),
                expected_sha256: "".into(),
            },
            reversal_procedure: ReversalProcedure::DeleteDropIn {
                target_path: "etc/sysctl.d/99-xynoos-memory.conf".into(),
                expected_sha256: "".into(),
            },
            persistence_requirements: PersistenceRequirement::DropInFile,
            mutation: Some(TypedMutation::WriteSysctlDropIn {
                dropin_filename: "99-xynoos-memory.conf".into(),
                content: "vm.swappiness = 100\n".into(),
                key: "vm.swappiness".into(),
                target_value: 100,
            }),
        },
    ];

    let now = 2_000_000u64;
    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::MemoryOptimization,
        "plan-hash-123",
        now,
        60_000,
    );

    // Apply plan
    let summary = executor
        .execute_plan(&actions, &auth, "plan-hash-123", now)
        .expect("Execution must succeed");
    assert_eq!(summary.applied_actions, 2);
    assert_eq!(summary.failed_actions, 0);

    // Verify runtime value mutated to 100
    let current_val = fs::read_to_string(&swappiness_file).unwrap();
    assert_eq!(current_val.trim(), "100");

    // Verify drop-in file created
    let dropin_file = etc_root.join("sysctl.d/99-xynoos-memory.conf");
    assert!(dropin_file.exists());

    // Verify ownership ledger has recorded both
    let ledger = executor.load_ledger().unwrap();
    assert!(ledger.is_owned("vm.swappiness"));
    assert!(ledger.is_owned("dropin:vm.swappiness"));

    // Now test Rollback
    let rollback_summary = executor
        .rollback_all(now + 1000)
        .expect("Rollback must succeed");
    assert_eq!(rollback_summary.failed_restorations.len(), 0);

    // Verify runtime value restored back to 60
    let restored_val = fs::read_to_string(&swappiness_file).unwrap();
    assert_eq!(restored_val.trim(), "60");

    // Verify drop-in file removed
    assert!(!dropin_file.exists());

    // Verify ownership ledger empty
    let ledger_after = executor.load_ledger().unwrap();
    assert!(!ledger_after.is_owned("vm.swappiness"));

    // Clean up
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_rollback_refuses_externally_tampered_dropin() {
    let root = std::env::temp_dir().join(format!("xynoos-tamper-test-{}", std::process::id()));
    let sys_root = root.join("sys");
    let proc_root = root.join("proc");
    let etc_root = root.join("etc");
    let state_dir = root.join("var/lib/xynoos");

    let swappiness_dir = proc_root.join("sys/vm");
    fs::create_dir_all(&swappiness_dir).unwrap();
    let swappiness_file = swappiness_dir.join("swappiness");
    fs::write(&swappiness_file, "60\n").unwrap();

    let executor = ConfigExecutor::new(
        sys_root.clone(),
        proc_root.clone(),
        etc_root.clone(),
        state_dir.clone(),
    );

    let actions = vec![ConfigurationAction {
        schema_version: CONFIG_ACTION_SCHEMA_VERSION,
        action_id: "ACT-DROPIN-SWAPPINESS".into(),
        target_subsystem: ConfigSubsystem::Sysctl,
        target_setting: "etc/sysctl.d/99-xynoos-memory.conf".into(),
        current_observed_value: "None".into(),
        desired_value: "Drop-in file".into(),
        current_ownership: ConfigurationOwnership::KernelDefault,
        required_privileges: PrivilegeLevel::RequiresRoot,
        required_capability: "fs.sysctl_d".into(),
        expected_benefit: "Persistence".into(),
        potential_side_effects: "None".into(),
        confidence: ConfidenceLevel::High,
        reversibility: ReversibilityGuarantee::FullyReversible,
        classification: ActionClassification::Configure,
        authorization_scope: AuthorizationScope::MemoryOptimization,
        preconditions: vec![],
        dependencies: vec![],
        conflict_status: None,
        verification_procedure: VerificationProcedure::DropInFileExists {
            path: "etc/sysctl.d/99-xynoos-memory.conf".into(),
            expected_sha256: "".into(),
        },
        reversal_procedure: ReversalProcedure::DeleteDropIn {
            target_path: "etc/sysctl.d/99-xynoos-memory.conf".into(),
            expected_sha256: "".into(),
        },
        persistence_requirements: PersistenceRequirement::DropInFile,
        mutation: Some(TypedMutation::WriteSysctlDropIn {
            dropin_filename: "99-xynoos-memory.conf".into(),
            content: "vm.swappiness = 100\n".into(),
            key: "vm.swappiness".into(),
            target_value: 100,
        }),
    }];

    let now = 3_000_000u64;
    let auth = SetupAuthorization::new(
        "operator",
        AuthorizationScope::MemoryOptimization,
        "plan-hash-tamper",
        now,
        60_000,
    );

    executor
        .execute_plan(&actions, &auth, "plan-hash-tamper", now)
        .expect("Execution must succeed");

    let dropin_file = etc_root.join("sysctl.d/99-xynoos-memory.conf");
    assert!(dropin_file.exists());

    // Tamper with the file externally (administrator append)
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&dropin_file)
        .unwrap();
    writeln!(file, "# Tampered by sysadmin").unwrap();

    // Rollback must refuse to remove the tampered file
    let rollback_summary = executor.rollback_all(now + 1000).unwrap();
    assert_eq!(rollback_summary.skipped_due_to_tampering.len(), 1);
    assert!(
        dropin_file.exists(),
        "Tampered file must NOT be deleted by rollback"
    );

    // Clean up
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn test_memory_planner_profile_and_actions() {
    let mut inv = SystemInventory::default();
    inv.memory.total_bytes = 4 * 1024 * 1024 * 1024; // 4 GiB N5000 style

    let profile = MemoryPlanner::determine_profile(&inv);
    assert_eq!(profile, MemoryProfile::LowMemoryLaptop);

    let actions = MemoryPlanner::plan(&inv);
    assert!(!actions.is_empty());

    // Should recommend ZRAM configuration with LZ4 algorithm
    let zram_action = actions.iter().find(|a| a.action_id == "ACT-ZRAM-CONFIGURE");
    assert!(zram_action.is_some());
    let zaction = zram_action.unwrap();
    assert_eq!(zaction.classification, ActionClassification::Configure);
    assert!(zaction.desired_value.contains("lz4"));
}

#[test]
fn test_config_journal_lifecycle() {
    let root = std::env::temp_dir().join(format!("xynoos-journal-test-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let journal_path = root.join("journal.json");

    let mut journal = ConfigJournal::new("tx-12345".into());
    assert_eq!(journal.phase, ConfigTxPhase::Prepared);

    journal.record_item(ConfigJournalItem {
        action_id: "test-act".into(),
        target_key: "vm.swappiness".into(),
        target_file: None,
        original_value: Some("60".into()),
        applied_value: "100".into(),
        is_applied: true,
    });
    journal.phase = ConfigTxPhase::Applying;
    journal.persist(&journal_path, 1_000_000).unwrap();

    let loaded = ConfigJournal::load_from(&journal_path).unwrap();
    assert!(loaded.is_some());
    let j = loaded.unwrap();
    assert_eq!(j.transaction_id, "tx-12345");
    assert_eq!(j.phase, ConfigTxPhase::Applying);
    assert_eq!(j.items.len(), 1);

    ConfigJournal::cleanup(&journal_path).unwrap();
    assert!(!journal_path.exists());

    let _ = fs::remove_dir_all(&root);
}
