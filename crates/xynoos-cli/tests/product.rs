use std::process::Command;

#[test]
fn product_json_and_read_only_contract() {
    for command in [
        "--version",
        "help",
        "status",
        "doctor",
        "capabilities",
        "benchmark",
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
            .args([command, "--json"])
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "{command}: {:?}", out.stderr);
        assert!(!out.stdout.contains(&27));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["product"], "Xynoos HyperCore");
        assert_eq!(v["physical_kernel_mutation"], false);
    }
}

#[test]
fn plain_overview_and_bounded_diagnostics() {
    let out = Command::new(env!("CARGO_BIN_EXE_xynoos"))
        .env("TERM", "dumb")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!out.stdout.contains(&27));
    assert!(String::from_utf8_lossy(&out.stdout).contains("XYNOOS HYPERCORE"));
    for args in [
        ["diagnose", "--count", "0"],
        ["diagnose", "--count", "1000"],
        ["status", "--unknown", "1"],
    ] {
        assert!(!Command::new(env!("CARGO_BIN_EXE_xynoos"))
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
}
