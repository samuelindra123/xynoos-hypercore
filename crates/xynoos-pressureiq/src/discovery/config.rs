//! Universal configuration intelligence, persistent configuration parsing, and ownership classification.
#![forbid(unsafe_code)]

use super::types::{ConfigurationOwnership, DiscoveryStatus};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Read, path::Path};

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

/// A single discovered configuration declaration with provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigItem {
    pub key: String,
    pub runtime_value: Option<String>,
    pub persistent_value: Option<String>,
    pub source_file: Option<String>,
    pub ownership: ConfigurationOwnership,
    pub has_discrepancy: bool,
}

/// Discovered zram service configuration file details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZramConfigFile {
    pub path: String,
    pub managed_by: String,
    pub declared_algorithm: Option<String>,
    pub declared_fraction: Option<String>,
}

/// Discovered fstab swap entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FstabSwapEntry {
    pub spec: String,
    pub file_type: String,
    pub options: String,
    pub priority: Option<i32>,
}

/// Comprehensive configuration inventory across sysctl, fstab, and tuning files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationInventory {
    pub kernel_cmdline: Option<String>,
    pub sysctl_items: BTreeMap<String, ConfigItem>,
    pub zram_configs: Vec<ZramConfigFile>,
    pub fstab_swaps: Vec<FstabSwapEntry>,
    pub cpufreq_config: Option<ConfigItem>,
    pub hypercore_managed_items: Vec<String>,
    pub status: DiscoveryStatus,
}

impl Default for ConfigurationInventory {
    fn default() -> Self {
        Self {
            kernel_cmdline: None,
            sysctl_items: BTreeMap::new(),
            zram_configs: Vec::new(),
            fstab_swaps: Vec::new(),
            cpufreq_config: None,
            hypercore_managed_items: Vec::new(),
            status: DiscoveryStatus::Unknown,
        }
    }
}

pub fn discover_configuration(proc_root: &Path, etc_root: &Path) -> ConfigurationInventory {
    let mut inv = ConfigurationInventory::default();

    // 1. Kernel command-line (/proc/cmdline)
    let cmdline_path = proc_root.join("cmdline");
    if let Ok(cmd) = read_bounded(&cmdline_path) {
        inv.kernel_cmdline = Some(cmd);
    }

    // 2. Sysctl inspection: runtime vs persistent configuration
    // Targeted performance and memory sysctl parameters
    let target_sysctls = [
        "vm.swappiness",
        "vm.vfs_cache_pressure",
        "vm.dirty_ratio",
        "vm.dirty_background_ratio",
        "vm.page-cluster",
        "vm.watermark_scale_factor",
        "vm.watermark_boost_factor",
        "kernel.sched_autogroup_enabled",
    ];

    let mut persistent_declarations: BTreeMap<String, (String, String, ConfigurationOwnership)> =
        BTreeMap::new();

    // Scan sysctl configuration locations in reverse priority order:
    // /usr/lib/sysctl.d -> /run/sysctl.d -> /etc/sysctl.d -> /etc/sysctl.conf
    let conf_sources = [
        (
            etc_root.join("../usr/lib/sysctl.d"),
            ConfigurationOwnership::DistributionManaged,
        ),
        (
            etc_root.join("../run/sysctl.d"),
            ConfigurationOwnership::ThirdPartyManaged,
        ),
        (
            etc_root.join("sysctl.d"),
            ConfigurationOwnership::AdministratorManaged,
        ),
    ];

    for (dir, default_owner) in &conf_sources {
        if let Ok(entries) = fs::read_dir(dir) {
            let mut conf_files: Vec<_> = entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "conf"))
                .map(|e| e.path())
                .collect();
            conf_files.sort();

            for conf_path in conf_files {
                let fname = conf_path.file_name().unwrap_or_default().to_string_lossy();
                let owner = if fname.contains("hypercore") || fname.contains("xynoos") {
                    ConfigurationOwnership::HyperCoreManaged
                } else if fname.contains("tlp") || fname.contains("tuned") {
                    ConfigurationOwnership::ThirdPartyManaged
                } else {
                    *default_owner
                };

                if let Ok(content) = read_bounded(&conf_path) {
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with('#')
                            || trimmed.starts_with(';')
                            || trimmed.is_empty()
                        {
                            continue;
                        }
                        if let Some((k, v)) = trimmed.split_once('=') {
                            let key = k.trim().to_string();
                            let val = v.trim().to_string();
                            persistent_declarations
                                .insert(key, (val, conf_path.display().to_string(), owner));
                        }
                    }
                }
            }
        }
    }

    // Direct /etc/sysctl.conf has highest admin priority
    let main_sysctl = etc_root.join("sysctl.conf");
    if let Ok(content) = read_bounded(&main_sysctl) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with(';') || trimmed.is_empty() {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let key = k.trim().to_string();
                let val = v.trim().to_string();
                persistent_declarations.insert(
                    key,
                    (
                        val,
                        main_sysctl.display().to_string(),
                        ConfigurationOwnership::AdministratorManaged,
                    ),
                );
            }
        }
    }

    // Now reconcile with actual runtime sysctl values in /proc/sys
    for &key in &target_sysctls {
        let rel_path = key.replace('.', "/");
        let runtime_path = proc_root.join("sys").join(rel_path);
        let runtime_val = read_bounded(&runtime_path).ok();

        let (pers_val, src_file, owner) = if let Some((v, s, o)) = persistent_declarations.get(key)
        {
            (Some(v.clone()), Some(s.clone()), *o)
        } else {
            (None, None, ConfigurationOwnership::KernelDefault)
        };

        let discrepancy = match (&runtime_val, &pers_val) {
            (Some(r), Some(p)) => r != p,
            (Some(_), None) => false, // Runtime exists at kernel default
            _ => false,
        };

        if owner == ConfigurationOwnership::HyperCoreManaged {
            inv.hypercore_managed_items.push(key.to_string());
        }

        inv.sysctl_items.insert(
            key.to_string(),
            ConfigItem {
                key: key.to_string(),
                runtime_value: runtime_val,
                persistent_value: pers_val,
                source_file: src_file,
                ownership: owner,
                has_discrepancy: discrepancy,
            },
        );
    }

    // 3. /etc/fstab swaps
    let fstab_path = etc_root.join("fstab");
    if let Ok(content) = read_bounded(&fstab_path) {
        let mut swaps = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            let cols: Vec<&str> = trimmed.split_whitespace().collect();
            if cols.len() >= 3 && cols[2] == "swap" {
                let spec = cols[0].to_string();
                let opts = if cols.len() >= 4 { cols[3] } else { "defaults" };
                let mut prio = None;
                for opt in opts.split(',') {
                    if let Some(p) = opt.strip_prefix("pri=") {
                        prio = p.parse::<i32>().ok();
                    }
                }
                swaps.push(FstabSwapEntry {
                    spec,
                    file_type: "swap".into(),
                    options: opts.to_string(),
                    priority: prio,
                });
            }
        }
        inv.fstab_swaps = swaps;
    }

    // 4. Zram configuration files
    let zram_generator_conf = etc_root.join("systemd/zram-generator.conf");
    if let Ok(content) = read_bounded(&zram_generator_conf) {
        let mut algo = None;
        let mut frac = None;
        for line in content.lines() {
            let t = line.trim();
            if let Some((k, v)) = t.split_once('=') {
                match k.trim() {
                    "compression-algorithm" => algo = Some(v.trim().to_string()),
                    "zram-size" | "fraction" => frac = Some(v.trim().to_string()),
                    _ => {}
                }
            }
        }
        inv.zram_configs.push(ZramConfigFile {
            path: zram_generator_conf.display().to_string(),
            managed_by: "systemd-zram-generator".into(),
            declared_algorithm: algo,
            declared_fraction: frac,
        });
    }

    let zramswap_default = etc_root.join("default/zramswap");
    if let Ok(content) = read_bounded(&zramswap_default) {
        let mut algo = None;
        let mut frac = None;
        for line in content.lines() {
            let t = line.trim();
            if let Some((k, v)) = t.split_once('=') {
                match k.trim() {
                    "ALGO" => algo = Some(v.trim().trim_matches('"').to_string()),
                    "PERCENT" => frac = Some(v.trim().trim_matches('"').to_string()),
                    _ => {}
                }
            }
        }
        inv.zram_configs.push(ZramConfigFile {
            path: zramswap_default.display().to_string(),
            managed_by: "zram-tools".into(),
            declared_algorithm: algo,
            declared_fraction: frac,
        });
    }

    // 5. CPU governor configuration
    let cpufreq_default = etc_root.join("default/cpufrequtils");
    if let Ok(content) = read_bounded(&cpufreq_default) {
        for line in content.lines() {
            if let Some((k, v)) = line.trim().split_once('=') {
                if k.trim() == "GOVERNOR" {
                    inv.cpufreq_config = Some(ConfigItem {
                        key: "cpufreq.governor".into(),
                        runtime_value: None,
                        persistent_value: Some(v.trim().trim_matches('"').to_string()),
                        source_file: Some(cpufreq_default.display().to_string()),
                        ownership: ConfigurationOwnership::AdministratorManaged,
                        has_discrepancy: false,
                    });
                }
            }
        }
    }

    inv.status = DiscoveryStatus::Available;
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let cfg = ConfigurationInventory::default();
        assert!(cfg.zram_configs.is_empty());
        assert!(cfg.fstab_swaps.is_empty());
    }
}
