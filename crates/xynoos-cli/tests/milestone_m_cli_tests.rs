//! Comprehensive CLI test suite for Milestone M:
//! Whole-system adaptive optimization, CPU/GPU/Storage/Power intelligence, guided setup, and continuous runtime.
#![forbid(unsafe_code)]

use std::{fs, process::Command};

#[test]
fn test_milestone_m_cli_guided_auto_setup_preview() {
    let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["setup", "--auto", "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["mode"], "guided_setup_preview");
    assert!(v["plan_fingerprint"].is_string());
    assert_eq!(v["authorization_required"], true);
    assert!(v["summary"]["total_actions"].as_u64().unwrap() > 0);
    assert!(v["actions"].is_array());
}

#[test]
fn test_milestone_m_cli_whole_system_plan() {
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
    assert!(v["configuration_actions"].is_array());
    assert!(v["plan_summary"].is_object());
    assert!(v["plan_fingerprint"].is_string());

    // Verify presence of multi-subsystem actions
    let actions = v["configuration_actions"].as_array().unwrap();
    let subsystems: Vec<&str> = actions
        .iter()
        .filter_map(|a| a["target_subsystem"].as_str())
        .collect();
    assert!(subsystems.contains(&"sysctl") || subsystems.contains(&"memory"));
    assert!(subsystems.contains(&"cpu"));
}

#[test]
fn test_milestone_m_cli_guided_auto_authorized_execution_and_rollback() {
    let root = std::env::temp_dir().join(format!("xynoos_cli_m_test_{}", std::process::id()));
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

    // Setup CPU governor fixture in sysfs
    let cpu0_gov = sys_root.join("devices/system/cpu/cpu0/cpufreq");
    fs::create_dir_all(&cpu0_gov).unwrap();
    fs::write(cpu0_gov.join("scaling_governor"), b"powersave\n").unwrap();

    // 1. Run setup --auto --authorized on fixture roots
    let out_auto = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args([
            "setup",
            "--auto",
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
        out_auto.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out_auto.stderr)
    );
    let v_apply: serde_json::Value = serde_json::from_slice(&out_auto.stdout).unwrap();
    assert_eq!(v_apply["operation"], "apply");
    assert!(v_apply["plan_fingerprint"].is_string());
    assert!(v_apply["applied_actions"].as_u64().unwrap() >= 1);

    // Verify runtime mutated
    let current_swappiness = fs::read_to_string(&swappiness_path).unwrap();
    assert_eq!(current_swappiness.trim(), "100");

    // 2. Verify status
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

    // 3. Rollback
    let out_rollback = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args([
            "setup",
            "--rollback",
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

    assert!(out_rollback.status.success());
    let v_rollback: serde_json::Value = serde_json::from_slice(&out_rollback.stdout).unwrap();
    assert_eq!(v_rollback["operation"], "rollback");

    // Verify swappiness restored
    let restored_swappiness = fs::read_to_string(&swappiness_path).unwrap();
    assert_eq!(restored_swappiness.trim(), "60");

    let _ = fs::remove_dir_all(&root);
}
