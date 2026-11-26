#![forbid(unsafe_code)]

use std::fs;
use xynoos_linux_control::{
    config::RuntimeAuthorizationContract,
    service::{ServiceAdapter, ServiceState},
};
use xynoos_pressureiq::{
    assess_compatibility, CgroupHierarchy, CompatibilityTier, DistributionFamily, InitSystem,
    LibcKind, PackageBackendKind, PlatformInventory,
};

fn make_temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xynoos-test-o-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_platform_compatibility_assessment_debian_systemd() {
    let inv = PlatformInventory {
        distribution_id: "debian".into(),
        distribution_family: DistributionFamily::Debian,
        init_system: InitSystem::Systemd,
        cgroup_hierarchy: CgroupHierarchy::V2Unified,
        psi_available: true,
        is_root: true,
        libc_kind: LibcKind::Glibc,
        ..Default::default()
    };

    let assessment = assess_compatibility(&inv);
    assert_eq!(assessment.tier, CompatibilityTier::SupportedAndVerified);
    assert_eq!(
        assessment.primary_package_backend,
        PackageBackendKind::Debian
    );
    assert!(assessment.cgroup_v2_ready);
    assert!(assessment.psi_ready);
    assert!(!assessment.requires_musl_build);
    assert!(!assessment.is_read_only_mode);
}

#[test]
fn test_platform_compatibility_assessment_alpine_musl() {
    let inv = PlatformInventory {
        distribution_id: "alpine".into(),
        distribution_family: DistributionFamily::Alpine,
        init_system: InitSystem::OpenRC,
        cgroup_hierarchy: CgroupHierarchy::V2Unified,
        psi_available: true,
        is_root: false,
        libc_kind: LibcKind::Musl,
        ..Default::default()
    };

    let assessment = assess_compatibility(&inv);
    assert_eq!(assessment.tier, CompatibilityTier::Experimental);
    assert_eq!(
        assessment.primary_package_backend,
        PackageBackendKind::AlpineApk
    );
    assert!(assessment.requires_musl_build);
    assert!(assessment.is_read_only_mode);
    assert!(assessment
        .notes
        .iter()
        .any(|n| n.contains("Musl libc detected")));
    assert!(assessment
        .notes
        .iter()
        .any(|n| n.contains("Non-root environment")));
}

#[test]
fn test_service_adapter_systemd_lifecycle_in_fixture() {
    let dir = make_temp_dir("systemd");
    let etc = dir.join("etc");
    let proc = dir.join("proc");
    let run = dir.join("run");
    fs::create_dir_all(etc.join("systemd/system")).unwrap();
    fs::create_dir_all(&proc).unwrap();
    fs::create_dir_all(run.join("xynoos")).unwrap();

    let adapter = ServiceAdapter::new(InitSystem::Systemd, etc.clone(), proc, run);

    // Before unit installation
    assert!(!adapter.is_installed("xynoos-hypercore"));
    assert_eq!(
        adapter.status("xynoos-hypercore").unwrap(),
        ServiceState::NotInstalled
    );

    // Install unit file
    let unit_file = etc.join("systemd/system/xynoos-hypercore.service");
    fs::write(&unit_file, "[Unit]\nDescription=Test\n").unwrap();
    assert!(adapter.is_installed("xynoos-hypercore"));

    // Enable service (creates wants symlink)
    adapter.enable("xynoos-hypercore").unwrap();
    let wants_link = etc.join("systemd/system/multi-user.target.wants/xynoos-hypercore.service");
    assert!(wants_link.exists() || wants_link.is_symlink());

    // Disable service
    adapter.disable("xynoos-hypercore").unwrap();
    assert!(!wants_link.exists());
}

#[test]
fn test_service_adapter_openrc_lifecycle_in_fixture() {
    let dir = make_temp_dir("openrc");
    let etc = dir.join("etc");
    let proc = dir.join("proc");
    let run = dir.join("run");
    fs::create_dir_all(etc.join("init.d")).unwrap();
    fs::create_dir_all(&proc).unwrap();
    fs::create_dir_all(&run).unwrap();

    let adapter = ServiceAdapter::new(InitSystem::OpenRC, etc.clone(), proc, run);

    let script = etc.join("init.d/xynoos-hypercore");
    fs::write(&script, "#!/sbin/openrc-run\n").unwrap();
    assert!(adapter.is_installed("xynoos-hypercore"));

    adapter.enable("xynoos-hypercore").unwrap();
    let runlevel_link = etc.join("runlevels/default/xynoos-hypercore");
    assert!(runlevel_link.exists() || runlevel_link.is_symlink());

    adapter.disable("xynoos-hypercore").unwrap();
    assert!(!runlevel_link.exists());
}

#[test]
fn test_wildcard_scope_enforcement_and_traversal_rejection() {
    let mut auth = RuntimeAuthorizationContract {
        allowed_target_identities: vec!["xynoos/*".into(), "user.slice/*".into()],
        ..Default::default()
    };

    // Permitted valid targets
    assert!(auth.is_target_allowed("xynoos/session-1"));
    assert!(auth.is_target_allowed("user.slice/user-1000.slice"));

    // Path traversal attacks must be rejected
    assert!(!auth.is_target_allowed("xynoos/../system.slice"));
    assert!(!auth.is_target_allowed("user.slice/../../etc/shadow"));

    // Root escapes must be rejected
    assert!(!auth.is_target_allowed("/"));
    assert!(!auth.is_target_allowed("/sys/fs/cgroup"));
    assert!(!auth.is_target_allowed(""));

    // Revocation must reject everything
    auth.revoke("test emergency");
    assert!(!auth.is_target_allowed("xynoos/session-1"));
}

#[test]
fn test_world_writable_contract_rejection() {
    let dir = make_temp_dir("perms");
    let contract_path = dir.join("authorization.json");

    let auth = RuntimeAuthorizationContract::default();
    auth.save_to(&contract_path, 1000).unwrap();
    assert!(contract_path.exists());

    // Under normal permissions, load succeeds
    let loaded = RuntimeAuthorizationContract::load_from(&contract_path);
    assert!(loaded.is_ok());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Set world-writable permission 0666
        fs::set_permissions(&contract_path, fs::Permissions::from_mode(0o666)).unwrap();
        let loaded_insecure = RuntimeAuthorizationContract::load_from(&contract_path);
        assert!(loaded_insecure.is_err());
    }
}
