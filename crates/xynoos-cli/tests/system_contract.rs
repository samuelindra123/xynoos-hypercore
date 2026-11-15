use std::process::Command;

#[test]
fn json_contract_and_explicit_missing_disk() {
    let output = Command::new(env!("CARGO_BIN_EXE_xynoos-cli"))
        .args([
            "system",
            "snapshot",
            "--json",
            "--disk",
            "xynoos-nonexistent",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["sequential"], true);
    assert_eq!(v["readings"]["disk"]["status"], "unavailable");
    assert!(v["readings"]["cpu_ticks"]["source"].is_string());
}

#[test]
fn sampling_has_readiness_and_expiration() {
    let output = Command::new(env!("CARGO_BIN_EXE_xynoos-cli"))
        .args([
            "system",
            "sample",
            "--json",
            "--count",
            "2",
            "--interval-ms",
            "100",
            "--freshness-ms",
            "0",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let rows: Vec<serde_json::Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0]["readings"]["cpu_utilization"]["status"],
        "not_yet_sampled"
    );
    assert_eq!(rows[1]["readings"]["cpu_utilization"]["status"], "stale");
}
