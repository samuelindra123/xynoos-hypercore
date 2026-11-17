use std::process::Command;
#[test]
fn live_evidence_replays_and_policy_is_deterministic() {
    let binary = env!("CARGO_BIN_EXE_xynoos-cli");
    let output = Command::new(binary)
        .args(["memory", "--interval-ms", "100"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(value["diagnosis"]["classes"].is_array());
    let file = std::env::temp_dir().join(format!(
        "xynoos-memory-evidence-{}.json",
        std::process::id()
    ));
    std::fs::write(&file, serde_json::to_vec(&value["evidence"]).unwrap()).unwrap();
    let replay = || {
        Command::new(binary)
            .arg("memory")
            .arg("--replay")
            .arg(&file)
            .output()
            .unwrap()
    };
    let a = replay();
    let b = replay();
    std::fs::remove_file(file).unwrap();
    assert!(a.status.success() && b.status.success());
    assert_eq!(a.stdout, b.stdout);
    let replayed: serde_json::Value = serde_json::from_slice(&a.stdout).unwrap();
    assert_eq!(value["diagnosis"], replayed["diagnosis"]);
    assert_eq!(value["aura"], replayed["aura"]);
}
