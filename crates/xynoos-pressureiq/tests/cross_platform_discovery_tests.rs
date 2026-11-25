//! Cross-platform discovery and optimization planning fixture tests.
#![forbid(unsafe_code)]

use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::Duration,
};
use xynoos_pressureiq::{
    discovery::inventory::CachedSystemInventory, CgroupHierarchy, ConfigurationOwnership,
    ConflictSeverity, DistributionFamily, InitSystem, OptimizationPlan, ProposedActionType,
    StorageMediaType, Subsystem, SystemInventory, INVENTORY_SCHEMA_VERSION, PLAN_SCHEMA_VERSION,
};

struct TestFixture {
    root: PathBuf,
}

impl TestFixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("xynoos-k-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sys")).unwrap();
        fs::create_dir_all(root.join("proc")).unwrap();
        fs::create_dir_all(root.join("etc")).unwrap();
        Self { root }
    }

    fn write_file(&self, rel: &str, content: &str) {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    fn sys_path(&self) -> PathBuf {
        self.root.join("sys")
    }

    fn proc_path(&self) -> PathBuf {
        self.root.join("proc")
    }

    fn etc_path(&self) -> PathBuf {
        self.root.join("etc")
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn test_debian_n5000_physical_fixture() {
    let fix = TestFixture::new("debian-n5000");

    // /etc/os-release
    fix.write_file(
        "etc/os-release",
        "PRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\nNAME=\"Debian GNU/Linux\"\nVERSION_ID=\"13\"\nVERSION=\"13 (trixie)\"\nID=debian\n",
    );
    fix.write_file("proc/sys/kernel/osrelease", "6.12.63+deb13-amd64\n");
    fix.write_file("proc/version", "Linux version 6.12.63+deb13-amd64\n");
    fix.write_file("proc/1/comm", "systemd\n");

    // /proc/cpuinfo (Intel N5000, 4 cores, no SMT)
    let cpuinfo = "processor\t: 0\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 122\nmodel name\t: Intel(R) Pentium(R) Silver N5000 CPU @ 1.10GHz\nphysical id\t: 0\ncore id\t\t: 0\nflags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 sse sse2 ss ht tm pbe syscall nx lm constant_tsc arch_perfmon rep_good nopl xtopology nonstop_tsc cpuid aperfmperf tsc_known_freq pni pclmulqdq dtes64 monitor ds_cpl vmx est tm2 ssse3 sdbg cx16 xtpr pdcm sse4_1 sse4_2 x2apic movbe popcnt tsc_deadline_timer aes xsave rdrand lahf_lm 3dnowprefetch epb intel_pt ssbd ibrs ibpb stibp tpr_shadow flexpriority ept vpid ept_ad fsgsbase tsc_adjust smep erms mpx rdt_a rdseed smap clflushopt clwb sha_ni arch_capabilities\n\nprocessor\t: 1\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 122\nmodel name\t: Intel(R) Pentium(R) Silver N5000 CPU @ 1.10GHz\nphysical id\t: 0\ncore id\t\t: 1\nflags\t\t: sse4_1 sse4_2 aes sha1 sha2\n\nprocessor\t: 2\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 122\nmodel name\t: Intel(R) Pentium(R) Silver N5000 CPU @ 1.10GHz\nphysical id\t: 0\ncore id\t\t: 2\nflags\t\t: sse4_1 sse4_2 aes sha1 sha2\n\nprocessor\t: 3\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 122\nmodel name\t: Intel(R) Pentium(R) Silver N5000 CPU @ 1.10GHz\nphysical id\t: 0\ncore id\t\t: 3\nflags\t\t: sse4_1 sse4_2 aes sha1 sha2\n";
    fix.write_file("proc/cpuinfo", cpuinfo);
    fix.write_file("sys/devices/system/cpu/online", "0-3\n");
    fix.write_file(
        "sys/devices/system/cpu/cpufreq/policy0/scaling_driver",
        "intel_cpufreq\n",
    );
    fix.write_file(
        "sys/devices/system/cpu/cpufreq/policy0/scaling_governor",
        "schedutil\n",
    );
    fix.write_file(
        "sys/devices/system/cpu/cpufreq/policy0/scaling_available_governors",
        "performance powersave schedutil\n",
    );
    fix.write_file(
        "sys/devices/system/cpu/cpufreq/policy0/cpuinfo_min_freq",
        "800000\n",
    );
    fix.write_file(
        "sys/devices/system/cpu/cpufreq/policy0/cpuinfo_max_freq",
        "2700000\n",
    );
    fix.write_file(
        "sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq",
        "800000\n",
    );

    // Memory (4GB RAM)
    fix.write_file("proc/meminfo", "MemTotal:        3861240 kB\nMemFree:         1524300 kB\nMemAvailable:    2221450 kB\nBuffers:          124500 kB\nCached:           950000 kB\nSwapTotal:       5119992 kB\nSwapFree:        5119992 kB\n");
    fix.write_file("proc/swaps", "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n/dev/zram0                              partition\t4071420\t\t0\t\t100\n/dev/mmcblk0p2                          partition\t1048572\t\t0\t\t-2\n");
    fix.write_file("sys/block/zram0/disksize", "4169134080\n");
    fix.write_file("sys/block/zram0/comp_algorithm", "[lz4] lzo zstd\n");
    fix.write_file("sys/block/zram0/mm_stat", "0 0 0 0 0 0 0\n");
    fix.write_file(
        "sys/kernel/mm/transparent_hugepage/enabled",
        "always [madvise] never\n",
    );

    // Cgroup v2
    fix.write_file(
        "sys/fs/cgroup/cgroup.controllers",
        "cpuset cpu io memory pids\n",
    );
    fix.write_file(
        "sys/fs/cgroup/user.slice/cgroup.controllers",
        "cpuset cpu io memory pids\n",
    );

    // PSI
    fix.write_file(
        "proc/pressure/cpu",
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
    );
    fix.write_file(
        "proc/pressure/memory",
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
    );
    fix.write_file(
        "proc/pressure/io",
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
    );

    // GPU (Intel UHD 605, i915)
    fix.write_file("sys/class/drm/card0/device/vendor", "0x8086\n");
    fix.write_file("sys/class/drm/card0/device/device", "0x3185\n");
    fix.write_file(
        "sys/class/drm/renderD128/uevent",
        "MAJOR=226\nMINOR=128\nDEVNAME=dri/renderD128\n",
    );

    // Storage (eMMC mmcblk0)
    fix.write_file(
        "proc/mounts",
        "/dev/mmcblk0p3 / ext4 rw,relatime,errors=remount-ro 0 0\n",
    );
    fix.write_file("sys/block/mmcblk0/queue/rotational", "0\n");
    fix.write_file(
        "sys/block/mmcblk0/queue/scheduler",
        "[mq-deadline] kyber bfq none\n",
    );
    fix.write_file("sys/block/mmcblk0/size", "121536512\n"); // ~58 GiB
    fix.write_file("sys/block/mmcblk0/queue/discard_granularity", "512\n");
    fix.write_file("sys/block/mmcblk0/queue/discard_max_bytes", "2147483648\n");

    // Sysctl & Tuning
    fix.write_file("proc/sys/vm/swappiness", "100\n");
    fix.write_file(
        "etc/sysctl.d/99-zram.conf",
        "vm.swappiness = 100\nvm.page-cluster = 0\n",
    );
    fix.write_file(
        "etc/systemd/zram-generator.conf",
        "[zram0]\nzram-size = min(ram, 4096)\ncompression-algorithm = lz4\n",
    );

    // Collect inventory
    let inv = SystemInventory::collect_from(&fix.sys_path(), &fix.proc_path(), &fix.etc_path());

    assert_eq!(inv.schema_version, INVENTORY_SCHEMA_VERSION);
    assert_eq!(inv.platform.distribution_id, "debian");
    assert_eq!(inv.platform.distribution_family, DistributionFamily::Debian);
    assert_eq!(inv.platform.init_system, InitSystem::Systemd);
    assert_eq!(inv.platform.cgroup_hierarchy, CgroupHierarchy::V2Unified);
    assert!(inv.platform.psi_available);

    // CPU checks
    assert_eq!(inv.cpu.vendor_id, "GenuineIntel");
    assert_eq!(inv.cpu.logical_cpus, 4);
    assert_eq!(inv.cpu.physical_cores, 4);
    assert!(!inv.cpu.smt_active);
    assert!(!inv.cpu.is_heterogeneous);
    assert_eq!(inv.cpu.scaling_driver.as_deref(), Some("intel_cpufreq"));

    // Memory & ZRAM
    assert_eq!(inv.memory.swap_devices.len(), 2);
    assert_eq!(inv.memory.zram_devices.len(), 1);
    assert_eq!(
        inv.memory.zram_devices[0].active_algorithm.as_deref(),
        Some("lz4")
    );

    // Storage
    assert_eq!(
        inv.storage.primary_storage_type,
        StorageMediaType::EmmcFlash
    );
    assert!(!inv.storage.has_rotational_storage);

    // Configuration & Conflicts
    assert_eq!(inv.tuning.zram_configs.len(), 1);
    assert_eq!(
        inv.tuning
            .sysctl_items
            .get("vm.swappiness")
            .unwrap()
            .ownership,
        ConfigurationOwnership::AdministratorManaged
    );

    // Plan Generation
    let plan = OptimizationPlan::generate(&inv);
    assert_eq!(plan.schema_version, PLAN_SCHEMA_VERSION);
    assert!(plan.summary_preserve_count > 0);

    // Existing zram and swappiness must be PRESERVED
    let zram_action = plan
        .actions
        .iter()
        .find(|a| a.subsystem == Subsystem::Compression)
        .unwrap();
    assert_eq!(zram_action.action_type, ProposedActionType::Preserve);

    let swappiness_action = plan
        .actions
        .iter()
        .find(|a| a.target_parameter == "vm.swappiness")
        .unwrap();
    assert_eq!(swappiness_action.action_type, ProposedActionType::Preserve);
}

#[test]
fn test_competing_daemons_and_conflicts_fixture() {
    let fix = TestFixture::new("conflicts");

    fix.write_file(
        "etc/os-release",
        "ID=ubuntu\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n",
    );
    fix.write_file("proc/sys/kernel/osrelease", "6.8.0-generic\n");
    fix.write_file("proc/version", "Linux version 6.8.0-generic\n");

    // Competing power daemons running in /proc
    fix.write_file("proc/101/comm", "tlp\n");
    fix.write_file("proc/102/comm", "power-profiles-daemon\n");

    // Competing OOM daemons
    fix.write_file("proc/201/comm", "earlyoom\n");
    fix.write_file("proc/202/comm", "systemd-oomd\n");

    // Multiple zram managers
    fix.write_file("etc/systemd/zram-generator.conf", "[zram0]\n");
    fix.write_file("etc/default/zramswap", "ALGO=lzo\nPERCENT=50\n");

    // ZSWAP + ZRAM both enabled
    fix.write_file("sys/module/zswap/parameters/enabled", "Y\n");
    fix.write_file("sys/module/zswap/parameters/compressor", "zstd\n");
    fix.write_file("sys/block/zram0/disksize", "2147483648\n");
    fix.write_file("sys/block/zram0/comp_algorithm", "[lz4] lzo\n");

    let inv = SystemInventory::collect_from(&fix.sys_path(), &fix.proc_path(), &fix.etc_path());

    // Check conflicts detected
    let conf_daemons = inv.conflicts.iter().find(|c| c.rule_id == "CONF-002");
    assert!(conf_daemons.is_some());
    assert_eq!(
        conf_daemons.unwrap().severity,
        ConflictSeverity::ConfirmedConflict
    );

    let conf_oom = inv.conflicts.iter().find(|c| c.rule_id == "CONF-003");
    assert!(conf_oom.is_some());
    assert_eq!(
        conf_oom.unwrap().severity,
        ConflictSeverity::ConfirmedConflict
    );

    let conf_zram = inv.conflicts.iter().find(|c| c.rule_id == "CONF-001");
    assert!(conf_zram.is_some());
    assert_eq!(
        conf_zram.unwrap().severity,
        ConflictSeverity::ConfirmedConflict
    );

    let conf_zswap = inv.conflicts.iter().find(|c| c.rule_id == "CONF-004");
    assert!(conf_zswap.is_some());
    assert_eq!(
        conf_zswap.unwrap().severity,
        ConflictSeverity::PotentialConflict
    );

    // Plan must reflect conflicts without mutating host
    let plan = OptimizationPlan::generate(&inv);
    let cpu_action = plan
        .actions
        .iter()
        .find(|a| a.subsystem == Subsystem::Cpu)
        .unwrap();
    assert_eq!(cpu_action.action_type, ProposedActionType::Conflict);
}

#[test]
fn test_fedora_nvme_btrfs_fixture() {
    let fix = TestFixture::new("fedora-nvme");

    fix.write_file(
        "etc/os-release",
        "ID=fedora\nVERSION_ID=\"40\"\nPRETTY_NAME=\"Fedora Linux 40\"\n",
    );
    fix.write_file("proc/sys/kernel/osrelease", "6.8.5-301.fc40.x86_64\n");
    fix.write_file("proc/version", "Linux version 6.8.5-301.fc40.x86_64\n");
    fix.write_file("proc/1/comm", "systemd\n");

    // High RAM (32GB)
    fix.write_file(
        "proc/meminfo",
        "MemTotal:       32768000 kB\nMemFree:        20000000 kB\nMemAvailable:   28000000 kB\n",
    );

    // NVMe storage with suboptimal scheduler
    fix.write_file("proc/mounts", "/dev/nvme0n1p3 / btrfs rw,seclabel,relatime,compress=zstd:1,ssd,discard=async,subvolroot 0 0\n");
    fix.write_file("sys/block/nvme0n1/queue/rotational", "0\n");
    fix.write_file("sys/block/nvme0n1/queue/scheduler", "[mq-deadline] none\n");
    fix.write_file("sys/block/nvme0n1/size", "1000204886\n");
    fix.write_file("sys/block/nvme0n1/queue/discard_granularity", "512\n");
    fix.write_file("sys/block/nvme0n1/queue/discard_max_bytes", "2147483648\n");

    let inv = SystemInventory::collect_from(&fix.sys_path(), &fix.proc_path(), &fix.etc_path());

    assert_eq!(inv.platform.distribution_family, DistributionFamily::RedHat);
    assert_eq!(inv.storage.primary_storage_type, StorageMediaType::NvmeSsd);
    assert!(inv.storage.has_nvme);

    let plan = OptimizationPlan::generate(&inv);
    // NVMe with mq-deadline should trigger an IMPROVE recommendation to set 'none'
    let sched_action = plan
        .actions
        .iter()
        .find(|a| a.target_parameter.ends_with(".scheduler"))
        .unwrap();
    assert_eq!(sched_action.action_type, ProposedActionType::Improve);
    assert_eq!(sched_action.proposed_setting.as_deref(), Some("none"));
}

#[test]
fn test_cached_system_inventory_behavior() {
    let fix = TestFixture::new("cached-inv");
    fix.write_file("etc/os-release", "ID=arch\nPRETTY_NAME=\"Arch Linux\"\n");
    fix.write_file("proc/sys/kernel/osrelease", "6.9.1-arch1-1\n");
    fix.write_file("proc/version", "Linux version 6.9.1-arch1-1\n");

    let initial = SystemInventory::collect_from(&fix.sys_path(), &fix.proc_path(), &fix.etc_path());
    let mut cached = CachedSystemInventory::with_initial(initial, Duration::from_millis(50));

    assert_eq!(cached.current().platform.distribution_id, "arch");

    // Within TTL, returns existing cached inventory
    let _ = cached.get_or_refresh();
    assert_eq!(cached.current().platform.distribution_id, "arch");

    // Invalidation resets expiration
    cached.invalidate();
}

#[test]
fn test_dormant_vs_active_oom_packages_regression() {
    let fix = TestFixture::new("dormant-oom");
    fix.write_file("etc/os-release", "ID=debian\nPRETTY_NAME=\"Debian\"\n");
    fix.write_file("proc/sys/kernel/osrelease", "6.12.0\n");
    fix.write_file("proc/version", "Linux version 6.12.0\n");

    // Both earlyoom and systemd-oomd service definitions installed on disk
    fix.write_file("etc/systemd/system/earlyoom.service", "[Service]\n");
    fix.write_file("etc/systemd/system/systemd-oomd.service", "[Service]\n");

    // But NO process in /proc for either!
    let inv = SystemInventory::collect_from(&fix.sys_path(), &fix.proc_path(), &fix.etc_path());

    // Must NOT flag ConfirmedConflict
    let hard_conf = inv.conflicts.iter().find(|c| c.rule_id == "CONF-003");
    assert!(hard_conf.is_none(), "Dormant installed packages must not be flagged as ConfirmedConflict");

    // Instead flags advisory CompatibleCoexistence
    let adv_conf = inv.conflicts.iter().find(|c| c.rule_id == "CONF-003-ADV");
    assert!(adv_conf.is_some());
    assert_eq!(adv_conf.unwrap().severity, ConflictSeverity::CompatibleCoexistence);
}

#[test]
fn test_zram_and_zswap_tiering_with_disk_swap_regression() {
    let fix = TestFixture::new("tiered-swap");
    fix.write_file("etc/os-release", "ID=debian\nPRETTY_NAME=\"Debian\"\n");
    fix.write_file("proc/sys/kernel/osrelease", "6.12.0\n");
    fix.write_file("proc/version", "Linux version 6.12.0\n");

    // Active ZRAM swap device AND active disk swap partition
    fix.write_file(
        "proc/swaps",
        "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n/dev/zram0                              partition\t2097152\t\t0\t\t100\n/dev/sda2                               partition\t4194304\t\t0\t\t-2\n",
    );
    fix.write_file("sys/block/zram0/disksize", "2147483648\n");
    fix.write_file("sys/block/zram0/comp_algorithm", "[lz4] lzo\n");

    // ZSWAP enabled
    fix.write_file("sys/module/zswap/parameters/enabled", "Y\n");
    fix.write_file("sys/module/zswap/parameters/compressor", "zstd\n");

    let inv = SystemInventory::collect_from(&fix.sys_path(), &fix.proc_path(), &fix.etc_path());

    // When disk swap is present, coexistence is tiered and compatible
    let conf_zswap = inv.conflicts.iter().find(|c| c.rule_id == "CONF-004");
    assert!(conf_zswap.is_some());
    assert_eq!(
        conf_zswap.unwrap().severity,
        ConflictSeverity::CompatibleCoexistence,
        "ZRAM + ZSWAP with disk swap must be recognized as compatible tiering"
    );
}
