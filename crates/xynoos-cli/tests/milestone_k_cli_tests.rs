use std::process::Command;

#[test]
fn test_milestone_k_cli_inspect_and_json() {
    let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["inspect", "--json"])
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
    assert!(v["platform"]["distribution_id"].is_string());
    assert!(v["cpu"]["logical_cpus"].is_number());
    assert!(v["memory"]["page_size_bytes"].is_number());
    assert!(v["storage"]["block_devices"].is_array());
    assert!(v["conflicts"].is_array());
}

#[test]
fn test_milestone_k_cli_tuning_audit() {
    let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["tuning", "audit", "--json"])
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
    assert!(v["tuning"]["sysctl_items"].is_object());
    assert!(v["conflicts"].is_array());
}

#[test]
fn test_milestone_k_cli_setup_plan_and_rejection() {
    // 1. Setup --plan should succeed
    let out_plan = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["setup", "--plan", "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(
        out_plan.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out_plan.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out_plan.stdout).unwrap();

    assert_eq!(v["schema_version"], 1);
    assert!(v["actions"].is_array());
    assert!(v["summary_preserve_count"].is_number());

    // 2. Setup --apply MUST be refused during Milestone K
    let out_apply = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["setup", "--apply"])
        .output()
        .unwrap();

    assert!(!out_apply.status.success());
    let err_msg = String::from_utf8_lossy(&out_apply.stderr);
    assert!(err_msg.contains("setup --apply' is not permitted during Milestone K"));

    // 3. Setup without --plan must show usage
    let out_nopick = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["setup"])
        .output()
        .unwrap();
    assert!(!out_nopick.status.success());
}
