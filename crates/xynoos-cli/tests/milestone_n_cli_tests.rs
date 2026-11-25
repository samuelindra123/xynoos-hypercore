//! Milestone N CLI integration tests covering runtime status, explain, disable, and daemon observability.
#![forbid(unsafe_code)]

use std::fs;
use std::process::Command;

fn target_bin() -> String {
    let p = env!("CARGO_BIN_EXE_xynoos");
    p.to_string()
}

#[test]
fn test_cli_runtime_status_json() {
    let temp_dir = std::env::temp_dir().join(format!("xynoos-cli-status-{}", std::process::id()));
    let state_dir = temp_dir.join("state");
    let proc_dir = temp_dir.join("proc");
    let sys_dir = temp_dir.join("sys");
    let etc_dir = temp_dir.join("etc");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&proc_dir).unwrap();
    fs::create_dir_all(&sys_dir).unwrap();
    fs::create_dir_all(&etc_dir).unwrap();

    let output = Command::new(target_bin())
        .args([
            "runtime",
            "status",
            "--json",
            "--state-dir",
            state_dir.to_str().unwrap(),
            "--proc-root",
            proc_dir.to_str().unwrap(),
            "--sys-root",
            sys_dir.to_str().unwrap(),
            "--etc-root",
            etc_dir.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute xynoos runtime status");

    assert!(output.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Valid JSON output");

    assert_eq!(parsed["schema_version"], 1);
    assert!(parsed.get("daemon_state").is_some());
    assert!(parsed.get("workload_classification").is_some());
    assert!(parsed.get("polling_tier").is_some());
    assert!(parsed.get("authorization").is_some());
    assert!(parsed.get("telemetry").is_some());

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_cli_runtime_explain_json() {
    let temp_dir = std::env::temp_dir().join(format!("xynoos-cli-explain-{}", std::process::id()));
    let state_dir = temp_dir.join("state");
    let proc_dir = temp_dir.join("proc");
    let sys_dir = temp_dir.join("sys");
    let etc_dir = temp_dir.join("etc");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&proc_dir).unwrap();
    fs::create_dir_all(&sys_dir).unwrap();
    fs::create_dir_all(&etc_dir).unwrap();

    let output = Command::new(target_bin())
        .args([
            "runtime",
            "explain",
            "--json",
            "--state-dir",
            state_dir.to_str().unwrap(),
            "--proc-root",
            proc_dir.to_str().unwrap(),
            "--sys-root",
            sys_dir.to_str().unwrap(),
            "--etc-root",
            etc_dir.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute xynoos runtime explain");

    assert!(output.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Valid JSON output");

    assert_eq!(parsed["schema_version"], 1);
    assert!(parsed.get("workload").is_some());
    assert!(parsed.get("explanation").is_some());
    assert!(parsed.get("recommendations").is_some());
    assert!(parsed.get("telemetry_rationale").is_some());

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_cli_runtime_disable_json() {
    let temp_dir = std::env::temp_dir().join(format!("xynoos-cli-disable-{}", std::process::id()));
    let state_dir = temp_dir.join("state");
    let proc_dir = temp_dir.join("proc");
    let sys_dir = temp_dir.join("sys");
    let etc_dir = temp_dir.join("etc");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&proc_dir).unwrap();
    fs::create_dir_all(&sys_dir).unwrap();
    fs::create_dir_all(&etc_dir).unwrap();

    let output = Command::new(target_bin())
        .args([
            "runtime",
            "disable",
            "--json",
            "--state-dir",
            state_dir.to_str().unwrap(),
            "--proc-root",
            proc_dir.to_str().unwrap(),
            "--sys-root",
            sys_dir.to_str().unwrap(),
            "--etc-root",
            etc_dir.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute xynoos runtime disable");

    assert!(output.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Valid JSON output");

    assert_eq!(parsed["operation"], "runtime_disable");
    assert_eq!(parsed["status"], "revoked");

    // Check that authorization.json now marks is_revoked = true
    let auth_content = fs::read_to_string(state_dir.join("authorization.json")).unwrap();
    let auth_json: serde_json::Value = serde_json::from_str(&auth_content).unwrap();
    assert_eq!(auth_json["is_revoked"], true);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_cli_daemon_status_json() {
    let output = Command::new(target_bin())
        .args(["daemon", "--status"])
        .output()
        .expect("Failed to execute daemon --status");

    assert!(output.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Valid JSON output");

    assert_eq!(parsed["status"], "configured");
    assert!(parsed.get("supported_modes").is_some());
    assert!(parsed.get("cgroup_v2_available").is_some());
}
