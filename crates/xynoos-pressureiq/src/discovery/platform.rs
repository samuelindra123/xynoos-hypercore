//! Universal platform, distribution, init, container, and capability discovery.
#![forbid(unsafe_code)]

use super::types::DiscoveryStatus;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_READ_BYTES: u64 = 8192;

fn read_bounded(path: &Path) -> Result<String, DiscoveryStatus> {
    let mut file = fs::File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => DiscoveryStatus::Unsupported,
        std::io::ErrorKind::PermissionDenied => DiscoveryStatus::PermissionDenied,
        _ => DiscoveryStatus::Unavailable,
    })?;
    let mut buf = String::new();
    file.by_ref()
        .take(MAX_READ_BYTES)
        .read_to_string(&mut buf)
        .map_err(|_| DiscoveryStatus::Malformed)?;
    if buf.trim().is_empty() {
        return Err(DiscoveryStatus::Unavailable);
    }
    Ok(buf.trim().to_string())
}

/// Linux distribution family categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistributionFamily {
    Debian,
    RedHat,
    Arch,
    Suse,
    Alpine,
    Void,
    Gentoo,
    NixOS,
    Slackware,
    Unknown,
}

/// Discovered init system and service manager environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitSystem {
    Systemd,
    SysVinit,
    OpenRC,
    Runit,
    S6,
    BusyBoxInit,
    Unknown,
}

/// Standard C library implementation detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibcKind {
    Glibc,
    Musl,
    Unknown,
}

/// Compatibility tier classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityTier {
    SupportedAndVerified,
    Experimental,
    FixtureTestedOnly,
    PlanningOnly,
    Unsupported,
    Blocked,
}

/// Primary packaging backend categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageBackendKind {
    Debian,
    Rpm,
    Arch,
    VoidXbps,
    AlpineApk,
    Standalone,
    Unknown,
}

/// Container virtualization environment detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerEnvironment {
    Docker,
    Podman,
    Lxc,
    SystemdNsSpawn,
    GenericContainer,
    None,
}

/// Virtualization platform detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VirtualizationEnvironment {
    Physical,
    Kvm,
    Qemu,
    Vmware,
    VirtualBox,
    Xen,
    HyperV,
    Wsl1,
    Wsl2,
    UnknownVm,
}

/// Cgroup hierarchy classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CgroupHierarchy {
    V2Unified,
    V1Legacy,
    Hybrid,
    None,
}

/// Discovered platform details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInventory {
    pub distribution_id: String,
    pub distribution_family: DistributionFamily,
    pub id_like: Vec<String>,
    pub version_id: Option<String>,
    pub pretty_name: String,
    pub variant: Option<String>,
    pub kernel_version: String,
    pub architecture: String,
    pub init_system: InitSystem,
    pub service_manager: String,
    pub available_package_managers: Vec<String>,
    pub cgroup_hierarchy: CgroupHierarchy,
    pub available_cgroup_controllers: Vec<String>,
    pub cgroup_delegation_available: bool,
    pub psi_available: bool,
    pub namespaces_available: bool,
    pub container_environment: ContainerEnvironment,
    pub virtualization: VirtualizationEnvironment,
    pub is_wsl: bool,
    pub is_root: bool,
    pub libc_kind: LibcKind,
    pub status: DiscoveryStatus,
}

impl Default for PlatformInventory {
    fn default() -> Self {
        Self {
            distribution_id: "unknown".into(),
            distribution_family: DistributionFamily::Unknown,
            id_like: Vec::new(),
            version_id: None,
            pretty_name: "Linux (unidentified)".into(),
            variant: None,
            kernel_version: "unknown".into(),
            architecture: std::env::consts::ARCH.to_string(),
            init_system: InitSystem::Unknown,
            service_manager: "unknown".into(),
            available_package_managers: Vec::new(),
            cgroup_hierarchy: CgroupHierarchy::None,
            available_cgroup_controllers: Vec::new(),
            cgroup_delegation_available: false,
            psi_available: false,
            namespaces_available: false,
            container_environment: ContainerEnvironment::None,
            virtualization: VirtualizationEnvironment::Physical,
            is_wsl: false,
            is_root: false,
            libc_kind: LibcKind::Unknown,
            status: DiscoveryStatus::Unknown,
        }
    }
}

pub fn discover_platform(sys_root: &Path, proc_root: &Path, etc_root: &Path) -> PlatformInventory {
    let mut inv = PlatformInventory::default();

    // 1. Distribution discovery via /etc/os-release or /usr/lib/os-release
    let os_release_path = etc_root.join("os-release");
    let os_release_fallback = etc_root.join("../usr/lib/os-release");
    let os_release_content = read_bounded(&os_release_path)
        .or_else(|_| read_bounded(&os_release_fallback))
        .unwrap_or_default();

    let mut raw_id = String::new();
    let mut raw_id_like = String::new();
    let mut raw_version_id = None;
    let mut raw_pretty_name = None;
    let mut raw_variant = None;

    for line in os_release_content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
            match k.trim() {
                "ID" => raw_id = val.to_lowercase(),
                "ID_LIKE" => raw_id_like = val.to_lowercase(),
                "VERSION_ID" => raw_version_id = Some(val),
                "PRETTY_NAME" => raw_pretty_name = Some(val),
                "VARIANT_ID" | "VARIANT" => raw_variant = Some(val),
                _ => {}
            }
        }
    }

    inv.distribution_id = if raw_id.is_empty() {
        "unknown".into()
    } else {
        raw_id
    };
    inv.id_like = raw_id_like
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    inv.version_id = raw_version_id;
    inv.variant = raw_variant;
    inv.pretty_name = raw_pretty_name.unwrap_or_else(|| {
        if inv.distribution_id != "unknown" {
            inv.distribution_id.clone()
        } else {
            "Linux (unidentified)".into()
        }
    });

    // Classify distribution family conservatively
    let matches_id = |id: &str| inv.distribution_id == id || inv.id_like.iter().any(|l| l == id);
    inv.distribution_family = if matches_id("debian")
        || matches_id("ubuntu")
        || matches_id("linuxmint")
        || matches_id("mx")
    {
        DistributionFamily::Debian
    } else if matches_id("fedora")
        || matches_id("rhel")
        || matches_id("centos")
        || matches_id("rocky")
        || matches_id("alma")
    {
        DistributionFamily::RedHat
    } else if matches_id("arch") || matches_id("manjaro") || matches_id("endeavouros") {
        DistributionFamily::Arch
    } else if matches_id("suse")
        || matches_id("opensuse")
        || matches_id("opensuse-tumbleweed")
        || matches_id("opensuse-leap")
    {
        DistributionFamily::Suse
    } else if matches_id("alpine") {
        DistributionFamily::Alpine
    } else if matches_id("void") {
        DistributionFamily::Void
    } else if matches_id("gentoo") {
        DistributionFamily::Gentoo
    } else if matches_id("nixos") {
        DistributionFamily::NixOS
    } else if matches_id("slackware") {
        DistributionFamily::Slackware
    } else {
        DistributionFamily::Unknown
    };

    // 2. Kernel version
    let osrelease_path = proc_root.join("sys/kernel/osrelease");
    if let Ok(v) = read_bounded(&osrelease_path) {
        inv.kernel_version = v;
    } else if let Ok(v) = read_bounded(&proc_root.join("version")) {
        inv.kernel_version = v.split_whitespace().nth(2).unwrap_or("unknown").to_string();
    }

    // 3. Init system & Service manager
    // Check /run/systemd/system or /proc/1/comm or symlinks
    let run_systemd = etc_root.join("../run/systemd/system");
    let proc_1_comm = proc_root.join("1/comm");
    let comm_1 = read_bounded(&proc_1_comm).unwrap_or_default();

    if run_systemd.exists() || comm_1 == "systemd" {
        inv.init_system = InitSystem::Systemd;
        inv.service_manager = "systemctl".into();
    } else if comm_1 == "init" {
        // Could be SysVinit, OpenRC, or Busybox
        if etc_root.join("init.d").is_dir() && etc_root.join("inittab").exists() {
            inv.init_system = InitSystem::SysVinit;
            inv.service_manager = "service".into();
        } else if etc_root.join("openrc").exists() || etc_root.join("runlevels").exists() {
            inv.init_system = InitSystem::OpenRC;
            inv.service_manager = "rc-service".into();
        } else {
            inv.init_system = InitSystem::SysVinit;
            inv.service_manager = "service".into();
        }
    } else if comm_1 == "runit" || etc_root.join("runit").exists() {
        inv.init_system = InitSystem::Runit;
        inv.service_manager = "sv".into();
    } else if comm_1 == "openrc-init" {
        inv.init_system = InitSystem::OpenRC;
        inv.service_manager = "rc-service".into();
    } else if comm_1 == "s6-svscan" {
        inv.init_system = InitSystem::S6;
        inv.service_manager = "s6-rc".into();
    } else {
        inv.init_system = InitSystem::Unknown;
        inv.service_manager = "unknown".into();
    }

    // 4. Package managers (check presence in standard paths if searchable or distribution-based)
    let mut pkg_mgrs = Vec::new();
    let bin_dirs = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];
    for (cmd, mgr) in [
        ("apt-get", "apt"),
        ("dpkg", "dpkg"),
        ("pacman", "pacman"),
        ("dnf", "dnf"),
        ("yum", "yum"),
        ("zypper", "zypper"),
        ("apk", "apk"),
        ("xbps-install", "xbps"),
        ("emerge", "emerge"),
        ("nix-env", "nix"),
    ] {
        for b in &bin_dirs {
            let candidate = PathBuf::from(b).join(cmd);
            if candidate.exists() {
                if !pkg_mgrs.contains(&mgr.to_string()) {
                    pkg_mgrs.push(mgr.to_string());
                }
                break;
            }
        }
    }
    inv.available_package_managers = pkg_mgrs;

    // 5. Cgroups discovery
    let cgroup_controllers = sys_root.join("fs/cgroup/cgroup.controllers");
    let cgroup_proc = proc_root.join("cgroups");
    if cgroup_controllers.exists() {
        inv.cgroup_hierarchy = CgroupHierarchy::V2Unified;
        if let Ok(ctrls) = read_bounded(&cgroup_controllers) {
            inv.available_cgroup_controllers =
                ctrls.split_whitespace().map(|s| s.to_string()).collect();
        }
    } else if cgroup_proc.exists() {
        inv.cgroup_hierarchy = CgroupHierarchy::V1Legacy;
    } else {
        inv.cgroup_hierarchy = CgroupHierarchy::None;
    }

    // Check delegation (presence of user.slice or writable cgroup.subtree_control)
    let user_slice = sys_root.join("fs/cgroup/user.slice");
    inv.cgroup_delegation_available = user_slice.exists()
        || inv
            .available_cgroup_controllers
            .iter()
            .any(|c| c == "memory" || c == "cpu");

    // 6. PSI availability
    let psi_cpu = proc_root.join("pressure/cpu");
    let psi_memory = proc_root.join("pressure/memory");
    let psi_io = proc_root.join("pressure/io");
    inv.psi_available = psi_cpu.exists() && psi_memory.exists() && psi_io.exists();

    // 7. Namespaces
    let ns_dir = proc_root.join("self/ns");
    inv.namespaces_available = ns_dir.exists();

    // 8. Container detection
    if etc_root.join("../.dockerenv").exists() {
        inv.container_environment = ContainerEnvironment::Docker;
    } else if etc_root.join("../run/.containerenv").exists() {
        inv.container_environment = ContainerEnvironment::Podman;
    } else if let Ok(sched) = read_bounded(&proc_root.join("1/sched")) {
        if sched.contains("docker") {
            inv.container_environment = ContainerEnvironment::Docker;
        } else if sched.contains("lxc") {
            inv.container_environment = ContainerEnvironment::Lxc;
        }
    }

    // 9. Virtualization and WSL detection
    let proc_version = read_bounded(&proc_root.join("version")).unwrap_or_default();
    if proc_version.to_lowercase().contains("microsoft") || proc_version.contains("WSL") {
        inv.is_wsl = true;
        if proc_version.contains("WSL2") || inv.cgroup_hierarchy == CgroupHierarchy::V2Unified {
            inv.virtualization = VirtualizationEnvironment::Wsl2;
        } else {
            inv.virtualization = VirtualizationEnvironment::Wsl1;
        }
    } else {
        let dmi_prod = read_bounded(&sys_root.join("class/dmi/id/product_name"))
            .unwrap_or_default()
            .to_lowercase();
        let dmi_vendor = read_bounded(&sys_root.join("class/dmi/id/sys_vendor"))
            .unwrap_or_default()
            .to_lowercase();

        if dmi_prod.contains("kvm") || dmi_vendor.contains("red hat") || dmi_vendor.contains("qemu")
        {
            inv.virtualization = VirtualizationEnvironment::Kvm;
        } else if dmi_prod.contains("qemu") {
            inv.virtualization = VirtualizationEnvironment::Qemu;
        } else if dmi_prod.contains("vmware") || dmi_vendor.contains("vmware") {
            inv.virtualization = VirtualizationEnvironment::Vmware;
        } else if dmi_prod.contains("virtualbox") || dmi_vendor.contains("innotek") {
            inv.virtualization = VirtualizationEnvironment::VirtualBox;
        } else if sys_root.join("hypervisor/type").exists() {
            inv.virtualization = VirtualizationEnvironment::UnknownVm;
        } else {
            inv.virtualization = VirtualizationEnvironment::Physical;
        }
    }

    // 10. Root detection via /proc/self/status (safe Rust, no libc or unsafe needed)
    inv.is_root = false;
    if let Ok(status_str) = read_bounded(&proc_root.join("self/status")) {
        for line in status_str.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                // parts[1] is effective UID
                if let Some(euid_str) = parts.get(1) {
                    if let Ok(euid) = euid_str.parse::<u32>() {
                        inv.is_root = euid == 0;
                    }
                }
                break;
            }
        }
    }

    // 11. Detect libc kind
    let check_lib_path = |sub: &str| {
        etc_root.join(format!("../lib/{sub}")).exists()
            || etc_root.join(format!("../usr/lib/{sub}")).exists()
            || etc_root.join(format!("../lib64/{sub}")).exists()
            || Path::new(&format!("/lib/{sub}")).exists()
            || Path::new(&format!("/usr/lib/{sub}")).exists()
            || Path::new(&format!("/lib64/{sub}")).exists()
    };
    if check_lib_path("ld-musl-x86_64.so.1")
        || check_lib_path("ld-musl-aarch64.so.1")
        || check_lib_path("libc.musl-x86_64.so.1")
        || inv.distribution_family == DistributionFamily::Alpine
    {
        inv.libc_kind = LibcKind::Musl;
    } else if check_lib_path("ld-linux-x86-64.so.2")
        || check_lib_path("ld-linux-aarch64.so.1")
        || check_lib_path("libc.so.6")
        || inv.distribution_family == DistributionFamily::Debian
        || inv.distribution_family == DistributionFamily::RedHat
        || inv.distribution_family == DistributionFamily::Arch
    {
        inv.libc_kind = LibcKind::Glibc;
    } else {
        inv.libc_kind = LibcKind::Unknown;
    }

    inv.status = DiscoveryStatus::Available;
    inv
}

/// Formalized platform compatibility assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCompatibilityAssessment {
    pub tier: CompatibilityTier,
    pub primary_package_backend: PackageBackendKind,
    pub supported_init_adapters: Vec<InitSystem>,
    pub cgroup_v2_ready: bool,
    pub psi_ready: bool,
    pub memory_optimization_supported: bool,
    pub cpu_optimization_supported: bool,
    pub storage_optimization_supported: bool,
    pub power_optimization_supported: bool,
    pub requires_musl_build: bool,
    pub is_read_only_mode: bool,
    pub notes: Vec<String>,
}

/// Assess whole-system compatibility and operational tier for the discovered platform.
pub fn assess_compatibility(inv: &PlatformInventory) -> PlatformCompatibilityAssessment {
    let mut notes = Vec::new();

    // 1. Package backend
    let primary_package_backend = match inv.distribution_family {
        DistributionFamily::Debian => PackageBackendKind::Debian,
        DistributionFamily::RedHat | DistributionFamily::Suse => PackageBackendKind::Rpm,
        DistributionFamily::Arch => PackageBackendKind::Arch,
        DistributionFamily::Void => PackageBackendKind::VoidXbps,
        DistributionFamily::Alpine => PackageBackendKind::AlpineApk,
        _ => PackageBackendKind::Standalone,
    };

    // 2. Supported init adapters
    let mut supported_init_adapters = Vec::new();
    match inv.init_system {
        InitSystem::Systemd => supported_init_adapters.push(InitSystem::Systemd),
        InitSystem::SysVinit => supported_init_adapters.push(InitSystem::SysVinit),
        InitSystem::OpenRC => supported_init_adapters.push(InitSystem::OpenRC),
        InitSystem::Runit => supported_init_adapters.push(InitSystem::Runit),
        _ => {}
    }

    // 3. Capabilities
    let cgroup_v2_ready = inv.cgroup_hierarchy == CgroupHierarchy::V2Unified;
    let psi_ready = inv.psi_available;
    let memory_optimization_supported = true;
    let cpu_optimization_supported = true;
    let storage_optimization_supported = true;
    let power_optimization_supported = true;
    let requires_musl_build = inv.libc_kind == LibcKind::Musl;
    let is_read_only_mode = !inv.is_root;

    if is_read_only_mode {
        notes.push("Non-root environment: mutating setup requires root privileges; operating in read-only observe mode.".into());
    }

    if !cgroup_v2_ready {
        notes.push("Cgroup v2 unified hierarchy unavailable: fine-grained memory/cpu cgroup throttling disabled, falling back to sysctl tuning.".into());
    }

    if !psi_ready {
        notes.push("Kernel PSI metrics unavailable: memory pressure detection operates via meminfo/vmstat metrics.".into());
    }

    if requires_musl_build {
        notes.push("Musl libc detected: requires musl-compiled binary artifact.".into());
    }

    if inv.container_environment != ContainerEnvironment::None {
        notes.push(format!("Container environment detected ({:?}): host-wide kernel mutations (swap, sysctl) may be restricted.", inv.container_environment));
    }

    // 4. Determine overall compatibility tier
    let tier = match inv.distribution_family {
        DistributionFamily::Debian => {
            if inv.init_system == InitSystem::Systemd || inv.init_system == InitSystem::SysVinit {
                CompatibilityTier::SupportedAndVerified
            } else {
                CompatibilityTier::Experimental
            }
        }
        DistributionFamily::RedHat | DistributionFamily::Arch => CompatibilityTier::Experimental,
        DistributionFamily::Void | DistributionFamily::Alpine => CompatibilityTier::Experimental,
        DistributionFamily::Unknown => CompatibilityTier::PlanningOnly,
        _ => CompatibilityTier::Experimental,
    };

    PlatformCompatibilityAssessment {
        tier,
        primary_package_backend,
        supported_init_adapters,
        cgroup_v2_ready,
        psi_ready,
        memory_optimization_supported,
        cpu_optimization_supported,
        storage_optimization_supported,
        power_optimization_supported,
        requires_musl_build,
        is_read_only_mode,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_defaults() {
        let p = PlatformInventory::default();
        assert_eq!(p.distribution_id, "unknown");
        assert_eq!(p.distribution_family, DistributionFamily::Unknown);
        assert!(!p.is_root);
    }
}
