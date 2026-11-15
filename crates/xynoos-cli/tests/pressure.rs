use std::process::Command;
#[test]
fn invalid_bounds_and_missing_options() {
    for args in [
        &["pressure", "sample", "--count", "0"][..],
        &["pressure", "sample", "--interval-ms", "99"][..],
        &["pressure", "sample", "--output"][..],
    ] {
        assert!(!Command::new(env!("CARGO_BIN_EXE_xynoos-cli"))
            .args(args)
            .status()
            .unwrap()
            .success());
    }
}
#[test]
fn snapshot_has_csv_columns() {
    let r = Command::new(env!("CARGO_BIN_EXE_xynoos-cli"))
        .args(["pressure", "snapshot"])
        .output()
        .unwrap();
    assert!(r.status.success());
    let s = String::from_utf8(r.stdout).unwrap();
    let mut lines = s.lines();
    let a = lines.next().unwrap();
    let b = lines.next().unwrap();
    assert_eq!(a.split(',').count(), b.split(',').count());
}

#[test]
fn human_snapshot() {
    let r = Command::new(env!("CARGO_BIN_EXE_xynoos-cli"))
        .args(["pressure", "snapshot", "--human"])
        .output()
        .unwrap();
    assert!(r.status.success());
    assert!(String::from_utf8(r.stdout).unwrap().contains("PressureIQ"));
}
#[test]
fn bounded_file_output_and_no_overwrite() {
    let name = format!(
        "xynoos-pressureiq-{}-{}.csv",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(name);
    let executable = env!("CARGO_BIN_EXE_xynoos-cli");
    let run = || {
        Command::new(executable)
            .args([
                "pressure",
                "sample",
                "--count",
                "2",
                "--interval-ms",
                "100",
                "--output",
            ])
            .arg(&path)
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents.lines().count(), 3);
    assert!(!run().status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn unified_sample_csv() {
    let r = Command::new(env!("CARGO_BIN_EXE_xynoos-cli"))
        .args(["system", "sample", "--count", "2", "--interval-ms", "100"])
        .output()
        .unwrap();
    assert!(r.status.success());
    let s = String::from_utf8(r.stdout).unwrap();
    let rows: Vec<_> = s.lines().collect();
    assert_eq!(rows.len(), 3);
    let n = rows[0].split(',').count();
    assert_eq!(rows[1].split(',').count(), n);
    assert_eq!(rows[2].split(',').count(), n);
    assert_eq!(rows[1].split(',').nth(2), Some(""));
}
