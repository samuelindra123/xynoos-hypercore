//! Comprehensive CLI test suite for Milestone L:
//! Reversible configuration engine, authorized automatic setup foundation, ownership ledger, and doctor.
#![forbid(unsafe_code)]

use std::{fs, process::Command};

#[test]
fn test_milestone_l_cli_doctor() {
    let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["doctor", "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["checks"].is_array());
    assert!(v["ledger_entries_count"].is_number());
    assert!(v["active_conflicts_count"].is_number());
}

#[test]
fn test_milestone_l_cli_setup_plan_json() {
    let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["setup", "--plan", "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert!(v["actions"].is_array());
    assert!(v["configuration_actions"].is_array());
}

#[test]
fn test_milestone_l_cli_apply_requires_authorization() {
    let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["setup", "--apply"])
        .output()
        .unwrap();

    assert!(!out.status.success());
    let err_msg = String::from_utf8_lossy(&out.stderr);
    assert!(err_msg.contains("AUTHORIZATION REQUIRED"));
}

#[test]
fn test_milestone_l_cli_authorized_apply_status_and_rollback_lifecycle() {
    let root = std::env::temp_dir().join("xynoos_cli_milestone_l_test");
    let _ = fs::remove_dir_all(&root);

    let sys_root = root.join("sys");
    let proc_root = root.join("proc");
    let etc_root = root.join("etc");
    let state_dir = root.join("var/lib/xynoos");

    fs::create_dir_all(&sys_root).unwrap();
    fs::create_dir_all(proc_root.join("sys/vm")).unwrap();
    fs::create_dir_all(etc_root.join("sysctl.d")).unwrap();
    fs::create_dir_all(&state_dir).unwrap();

    // Set baseline sysctl
    let swappiness_path = proc_root.join("sys/vm/swappiness");
    fs::write(&swappiness_path, b"60\n").unwrap();

    // 1. Run setup --apply --authorized on fixture roots
    let out_apply = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args([
            "setup",
            "--apply",
            "--authorized",
            "--json",
            "--sys-root",
            sys_root.to_str().unwrap(),
            "--proc-root",
            proc_root.to_str().unwrap(),
            "--etc-root",
            etc_root.to_str().unwrap(),
            "--state-dir",
            state_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        out_apply.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out_apply.stderr)
    );
    let v_apply: serde_json::Value = serde_json::from_slice(&out_apply.stdout).unwrap();
    assert_eq!(v_apply["operation"], "apply");
    assert!(v_apply["applied_actions"].as_u64().unwrap() >= 1);

    // Verify runtime mutated
    let current_swappiness = fs::read_to_string(&swappiness_path).unwrap();
    assert_eq!(current_swappiness.trim(), "100");

    // 2. Run setup --status
    let out_status = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args([
            "setup",
            "--status",
            "--json",
            "--etc-root",
            etc_root.to_str().unwrap(),
            "--state-dir",
            state_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(out_status.status.success());
    let v_status: serde_json::Value = serde_json::from_slice(&out_status.stdout).unwrap();
    assert!(v_status["managed_entries"].is_object());

    // 3. Run setup --rollback
    let out_rollback = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args([
            "setup",
            "--rollback",
            "--json",
            "--proc-root",
            proc_root.to_str().unwrap(),
            "--etc-root",
            etc_root.to_str().unwrap(),
            "--state-dir",
            state_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(out_rollback.status.success());
    let v_rollback: serde_json::Value = serde_json::from_slice(&out_rollback.stdout).unwrap();
    assert_eq!(v_rollback["operation"], "rollback");

    // Verify restored back to 60
    let restored_swappiness = fs::read_to_string(&swappiness_path).unwrap();
    assert_eq!(restored_swappiness.trim(), "60");

    // Clean up
    let _ = fs::remove_dir_all(&root);
}
