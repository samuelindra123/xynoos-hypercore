use std::process::Command;
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xynoos-cli"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn full_observe_command_is_structured_and_read_only() {
    let o = cli(&["mvp", "observe", "--count", "1"]);
    assert!(o.status.success(), "{:?}", o);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["kernel_mutations"], 0);
    assert_eq!(v["owned_buffer_mutations"], 0);
    assert_eq!(v["evidence"], "live_read_only");
    assert_eq!(v["plan"]["mode"], "observe");
    assert!(v["telemetry"]["readings"]["memory_psi"]["source"].is_string());
    assert_eq!(v["plan"]["kernel_write_available"], false);
}
#[test]
fn full_simulation_command_exercises_real_policy_and_store() {
    let o = cli(&["mvp", "simulate"]);
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["kernel_mutations"], 0);
    assert_eq!(v["telemetry"], "deterministic_replay");
    assert_eq!(v["safety_refusals"], 2);
    assert_eq!(v["rows"][2]["plan"]["action"], "compress_owned_pages");
    assert!(
        v["rows"][2]["workload"]["compressed_pages"]
            .as_u64()
            .unwrap()
            > 0
    );
}
#[test]
fn physical_admission_cannot_be_enabled_by_authorize_or_environment() {
    for args in [
        &["mvp", "controlled"][..],
        &["mvp", "controlled", "--authorize"][..],
    ] {
        let o = Command::new(env!("CARGO_BIN_EXE_xynoos-cli"))
            .args(args)
            .env("XYNOOS_ALLOW_KERNEL_MUTATION", "1")
            .output()
            .unwrap();
        assert!(!o.status.success());
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(v["kernel_mutations"], 0);
        assert_eq!(v["plan"]["outcome"], "refused");
    }
}
#[test]
fn recovery_has_no_replay_option_and_cli_rejects_invalid_bounds() {
    let o = cli(&["mvp", "recovery"]);
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["automatic_recovery_available"], false);
    assert_eq!(v["physical_mutation_available"], false);
    for args in [
        vec!["mvp", "observe", "--count", "61"],
        vec!["mvp", "observe", "--interval-ms", "0"],
        vec!["mvp", "simulate", "--authorize"],
        vec!["mvp", "recovery", "--replay"],
        vec!["mvp", "observe", "--count", "1", "--count", "2"],
        vec!["mvp", "simulate", "--trace", "/dev/null"],
    ] {
        assert!(!cli(&args).status.success());
    }
    assert!(cli(&["mvp", "observe", "--human"]).status.success());
    assert!(cli(&["mvp", "simulate", "--human"]).status.success());
}

#[test]
fn distribution_excludes_controlled_mode_and_journal_cli_preserves_evidence() {
    let binary = env!("CARGO_BIN_EXE_xynoos-mvp");
    assert!(!Command::new(binary)
        .args(["controlled", "--authorize"])
        .output()
        .unwrap()
        .status
        .success());
    let dir = std::env::temp_dir().join(format!("xynoos-mvp-cli-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("journal.jsonl");
    let o = Command::new(binary)
        .args(["simulate", "--journal"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(o.status.success(), "{:?}", o);
    let before = std::fs::read(&path).unwrap();
    let o = Command::new(binary)
        .args(["recovery", "--journal"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["journal"]["status"], "simulation_restored");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!Command::new(binary)
        .args(["simulate", "--journal"])
        .arg(&path)
        .output()
        .unwrap()
        .status
        .success());
    std::fs::remove_dir_all(dir).unwrap();
}
