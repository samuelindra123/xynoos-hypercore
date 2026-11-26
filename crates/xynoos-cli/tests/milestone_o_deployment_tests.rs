#![forbid(unsafe_code)]

use std::fs;
use std::process::Command;

fn make_temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xynoos-cli-o-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn target_bin() -> String {
    let p = env!("CARGO_BIN_EXE_xynoos");
    p.to_string()
}

fn setup_mock_fs(dir: &std::path::Path) {
    let sys = dir.join("sys");
    let proc = dir.join("proc");
    let etc = dir.join("etc");
    let state = dir.join("var/lib/xynoos");

    fs::create_dir_all(&sys).unwrap();
    fs::create_dir_all(&proc).unwrap();
    fs::create_dir_all(&etc).unwrap();
    fs::create_dir_all(&state).unwrap();

    // Mock os-release
    fs::write(
        etc.join("os-release"),
        "ID=debian\nVERSION_ID=\"13\"\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\n",
    )
    .unwrap();

    // Mock init systemd
    fs::create_dir_all(proc.join("1")).unwrap();
    fs::write(proc.join("1/comm"), "systemd\n").unwrap();

    // Mock kernel version
    fs::create_dir_all(proc.join("sys/kernel")).unwrap();
    fs::write(proc.join("sys/kernel/osrelease"), "6.12.0-xynoos-test\n").unwrap();

    // Mock memory
    fs::write(
        proc.join("meminfo"),
        "MemTotal:        4000000 kB\nMemFree:         2000000 kB\nMemAvailable:    2500000 kB\nSwapTotal:       4000000 kB\nSwapFree:        4000000 kB\n",
    ).unwrap();

    // Mock CPU
    fs::write(
        proc.join("cpuinfo"),
        "processor\t: 0\nmodel name\t: Intel(R) Pentium(R) Silver N5000 CPU @ 1.10GHz\n\nprocessor\t: 1\nmodel name\t: Intel(R) Pentium(R) Silver N5000 CPU @ 1.10GHz\n\n",
    ).unwrap();

    // Mock cgroups v2
    fs::create_dir_all(sys.join("fs/cgroup")).unwrap();
    fs::write(
        sys.join("fs/cgroup/cgroup.controllers"),
        "cpuset cpu io memory pids\n",
    )
    .unwrap();

    // Mock PSI
    fs::create_dir_all(proc.join("pressure")).unwrap();
    fs::write(
        proc.join("pressure/memory"),
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
    ).unwrap();
    fs::write(
        proc.join("pressure/cpu"),
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
    )
    .unwrap();
    fs::write(
        proc.join("pressure/io"),
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
    ).unwrap();
}

#[test]
fn test_doctor_command_json_in_fixture() {
    let dir = make_temp_dir("doctor");
    setup_mock_fs(&dir);

    let output = Command::new(target_bin())
        .args([
            "doctor",
            "--json",
            "--sys-root",
            dir.join("sys").to_str().unwrap(),
            "--proc-root",
            dir.join("proc").to_str().unwrap(),
            "--etc-root",
            dir.join("etc").to_str().unwrap(),
            "--state-dir",
            dir.join("var/lib/xynoos").to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute xynoos doctor");

    assert!(output.status.success());
    let s = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["platform"]["distribution_id"], "debian");
    assert_eq!(json["compatibility"]["tier"], "supported_and_verified");
}

#[test]
fn test_setup_auto_preview_and_apply_lifecycle() {
    let dir = make_temp_dir("lifecycle");
    setup_mock_fs(&dir);

    let state_dir = dir.join("var/lib/xynoos");
    let sys_str = dir.join("sys").to_str().unwrap().to_string();
    let proc_str = dir.join("proc").to_str().unwrap().to_string();
    let etc_str = dir.join("etc").to_str().unwrap().to_string();
    let state_str = state_dir.to_str().unwrap().to_string();

    let base_args = [
        "--sys-root",
        sys_str.as_str(),
        "--proc-root",
        proc_str.as_str(),
        "--etc-root",
        etc_str.as_str(),
        "--state-dir",
        state_str.as_str(),
        "--json",
    ];

    // 1. Preview mode (xynoos setup --auto without --authorized)
    let mut preview_args = vec!["setup", "--auto"];
    preview_args.extend_from_slice(&base_args);
    let output_preview = Command::new(target_bin())
        .args(&preview_args)
        .output()
        .expect("Failed to execute setup --auto");
    assert!(output_preview.status.success());
    assert!(!state_dir.join("authorization.json").exists());

    // 2. Apply without --authorized must fail
    let mut unauth_args = vec!["setup", "--apply"];
    unauth_args.extend_from_slice(&base_args);
    let output_unauth = Command::new(target_bin())
        .args(&unauth_args)
        .output()
        .expect("Failed to execute setup --apply unauth");
    assert!(!output_unauth.status.success());

    // 3. Authorized auto setup execution
    let mut auto_auth_args = vec!["setup", "--auto", "--authorized"];
    auto_auth_args.extend_from_slice(&base_args);
    let output_auth = Command::new(target_bin())
        .args(&auto_auth_args)
        .output()
        .expect("Failed to execute setup --auto --authorized");
    assert!(output_auth.status.success());
    assert!(state_dir.join("authorization.json").exists());
    assert!(state_dir.join("owned-configs.json").exists());

    // 4. Query status
    let mut status_args = vec!["setup", "--status"];
    status_args.extend_from_slice(&base_args);
    let output_status = Command::new(target_bin())
        .args(&status_args)
        .output()
        .expect("Failed to execute setup --status");
    assert!(output_status.status.success());

    // 5. Emergency runtime disable
    let mut disable_args = vec!["runtime", "disable"];
    disable_args.extend_from_slice(&base_args);
    let output_disable = Command::new(target_bin())
        .args(&disable_args)
        .output()
        .expect("Failed to execute runtime disable");
    assert!(output_disable.status.success());

    // 6. Rollback all configuration
    let mut rollback_args = vec!["setup", "--rollback"];
    rollback_args.extend_from_slice(&base_args);
    let output_rollback = Command::new(target_bin())
        .args(&rollback_args)
        .output()
        .expect("Failed to execute setup --rollback");
    assert!(output_rollback.status.success());
}
