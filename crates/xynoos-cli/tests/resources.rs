use std::process::Command;
#[test]
fn resource_contract_is_read_only_and_reports_capabilities() {
    let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .arg("resources")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["aura"]["mode"], "read_only");
    assert_eq!(v["aura"]["execution_available"], false);
    for group in ["cpu", "gpu", "stability"] {
        assert!(v["hardware"][group].is_array());
        for device in v["hardware"][group].as_array().unwrap() {
            for r in device["readings"].as_array().unwrap() {
                assert!(r["source"].is_string() && r["unit"].is_string());
                if r["status"] != "available" {
                    assert!(r["value"].is_null());
                }
            }
        }
    }
    assert!(!Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .args(["resources", "--apply"])
        .status()
        .unwrap()
        .success());
}
