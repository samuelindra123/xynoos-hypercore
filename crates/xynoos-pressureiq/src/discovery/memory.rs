//! Universal memory, swap, zram, zswap, and compression intelligence.
#![forbid(unsafe_code)]

use super::types::DiscoveryStatus;
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

fn read_u64(path: &Path) -> Option<u64> {
    read_bounded(path).ok()?.parse::<u64>().ok()
}

/// Details of an active swap device or file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapDevice {
    pub path: String,
    pub swap_type: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub priority: i32,
    pub is_zram: bool,
}

/// Statistics and algorithms for a discovered zram block device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZramDevice {
    pub device_name: String,
    pub disksize_bytes: u64,
    pub mem_used_total_bytes: u64,
    pub orig_data_bytes: u64,
    pub compr_data_bytes: u64,
    pub active_algorithm: Option<String>,
    pub supported_algorithms: Vec<String>,
    pub compression_ratio: f64,
}

/// Discovered zswap compressed cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZswapConfiguration {
    pub available: bool,
    pub enabled: bool,
    pub compressor: Option<String>,
    pub zpool: Option<String>,
    pub max_pool_percent: Option<u32>,
}

/// Discovered memory sysctl tuning values.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemorySysctlValues {
    pub swappiness: Option<u32>,
    pub vfs_cache_pressure: Option<u32>,
    pub dirty_ratio: Option<u32>,
    pub dirty_background_ratio: Option<u32>,
    pub page_cluster: Option<u32>,
    pub watermark_scale_factor: Option<u32>,
    pub watermark_boost_factor: Option<u32>,
    pub min_free_kbytes: Option<u64>,
}

/// Active or configured OOM management daemons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OomManagement {
    pub earlyoom_present: bool,
    pub earlyoom_active: bool,
    pub systemd_oomd_present: bool,
    pub systemd_oomd_active: bool,
    pub nohang_present: bool,
    pub nohang_active: bool,
    pub oomd_present: bool,
    pub oomd_active: bool,
}

/// Discovered memory inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInventory {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub free_bytes: u64,
    pub buffers_bytes: u64,
    pub cached_bytes: u64,
    pub page_size_bytes: u64,
    pub numa_nodes: u32,
    pub thp_enabled: Option<String>,
    pub thp_defrag: Option<String>,
    pub swap_devices: Vec<SwapDevice>,
    pub total_swap_bytes: u64,
    pub free_swap_bytes: u64,
    pub zram_devices: Vec<ZramDevice>,
    pub zram_total_disksize_bytes: u64,
    pub zram_total_used_memory_bytes: u64,
    pub zswap: ZswapConfiguration,
    pub memory_sysctl: MemorySysctlValues,
    pub oom_managers: OomManagement,
    pub zram_tuning_services: Vec<String>,
    pub status: DiscoveryStatus,
}

impl Default for MemoryInventory {
    fn default() -> Self {
        Self {
            total_bytes: 0,
            available_bytes: 0,
            free_bytes: 0,
            buffers_bytes: 0,
            cached_bytes: 0,
            page_size_bytes: 4096,
            numa_nodes: 1,
            thp_enabled: None,
            thp_defrag: None,
            swap_devices: Vec::new(),
            total_swap_bytes: 0,
            free_swap_bytes: 0,
            zram_devices: Vec::new(),
            zram_total_disksize_bytes: 0,
            zram_total_used_memory_bytes: 0,
            zswap: ZswapConfiguration {
                available: false,
                enabled: false,
                compressor: None,
                zpool: None,
                max_pool_percent: None,
            },
            memory_sysctl: MemorySysctlValues::default(),
            oom_managers: OomManagement {
                earlyoom_present: false,
                earlyoom_active: false,
                systemd_oomd_present: false,
                systemd_oomd_active: false,
                nohang_present: false,
                nohang_active: false,
                oomd_present: false,
                oomd_active: false,
            },
            zram_tuning_services: Vec::new(),
            status: DiscoveryStatus::Unknown,
        }
    }
}

pub fn discover_memory(sys_root: &Path, proc_root: &Path, etc_root: &Path) -> MemoryInventory {
    let mut inv = MemoryInventory::default();

    // 1. /proc/meminfo
    let meminfo_path = proc_root.join("meminfo");
    if let Ok(content) = read_bounded(&meminfo_path) {
        let mut map = BTreeMap::new();
        for line in content.lines() {
            if let Some((k, rest)) = line.split_once(':') {
                let num_str = rest.split_whitespace().next().unwrap_or("0");
                if let Ok(v) = num_str.parse::<u64>() {
                    // /proc/meminfo reports values in KiB
                    map.insert(k.trim(), v * 1024);
                }
            }
        }
        inv.total_bytes = map.get("MemTotal").copied().unwrap_or(0);
        inv.free_bytes = map.get("MemFree").copied().unwrap_or(0);
        inv.available_bytes = map.get("MemAvailable").copied().unwrap_or(inv.free_bytes);
        inv.buffers_bytes = map.get("Buffers").copied().unwrap_or(0);
        inv.cached_bytes = map.get("Cached").copied().unwrap_or(0);
        inv.total_swap_bytes = map.get("SwapTotal").copied().unwrap_or(0);
        inv.free_swap_bytes = map.get("SwapFree").copied().unwrap_or(0);
    }

    // 2. NUMA nodes
    let node_dir = sys_root.join("devices/system/node");
    if let Ok(entries) = fs::read_dir(&node_dir) {
        let node_count = entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .strip_prefix("node")
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
            })
            .count();
        if node_count > 0 {
            inv.numa_nodes = node_count as u32;
        }
    }

    // 3. Transparent Huge Pages
    let thp_dir = sys_root.join("kernel/mm/transparent_hugepage");
    if let Ok(s) = read_bounded(&thp_dir.join("enabled")) {
        inv.thp_enabled = Some(s);
    }
    if let Ok(s) = read_bounded(&thp_dir.join("defrag")) {
        inv.thp_defrag = Some(s);
    }

    // 4. /proc/swaps
    let swaps_path = proc_root.join("swaps");
    if let Ok(content) = read_bounded(&swaps_path) {
        let mut swaps = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            if idx == 0 {
                continue; // Skip header
            }
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 5 {
                let dev_path = cols[0].to_string();
                let dev_type = cols[1].to_string();
                let total_kb: u64 = cols[2].parse().unwrap_or(0);
                let used_kb: u64 = cols[3].parse().unwrap_or(0);
                let prio: i32 = cols[4].parse().unwrap_or(-1);
                let is_zram = dev_path.contains("zram");

                swaps.push(SwapDevice {
                    path: dev_path,
                    swap_type: dev_type,
                    total_bytes: total_kb * 1024,
                    used_bytes: used_kb * 1024,
                    priority: prio,
                    is_zram,
                });
            }
        }
        inv.swap_devices = swaps;
    }

    // 5. ZRAM devices in /sys/block/zram*
    let sys_block = sys_root.join("block");
    if let Ok(entries) = fs::read_dir(&sys_block) {
        let mut zram_devs = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("zram") {
                let p = entry.path();
                let disksize = read_u64(&p.join("disksize")).unwrap_or(0);
                let mut mem_used = 0u64;
                let mut orig_size = 0u64;
                let mut compr_size = 0u64;

                // /sys/block/zramX/mm_stat provides space-separated counters:
                // orig_data_size compr_data_size mem_used_total mem_limit mem_used_max same_pages pages_compacted ...
                if let Ok(mm_stat) = read_bounded(&p.join("mm_stat")) {
                    let parts: Vec<&str> = mm_stat.split_whitespace().collect();
                    if parts.len() >= 3 {
                        orig_size = parts[0].parse().unwrap_or(0);
                        compr_size = parts[1].parse().unwrap_or(0);
                        mem_used = parts[2].parse().unwrap_or(0);
                    }
                } else {
                    mem_used = read_u64(&p.join("mem_used_total")).unwrap_or(0);
                    orig_size = read_u64(&p.join("orig_data_size")).unwrap_or(0);
                    compr_size = read_u64(&p.join("compr_data_size")).unwrap_or(0);
                }

                let mut active_algo = None;
                let mut supported_algos = Vec::new();
                if let Ok(algo_str) = read_bounded(&p.join("comp_algorithm")) {
                    for token in algo_str.split_whitespace() {
                        if token.starts_with('[') && token.ends_with(']') {
                            let clean = token.trim_matches('[').trim_matches(']');
                            active_algo = Some(clean.to_string());
                            supported_algos.push(clean.to_string());
                        } else {
                            supported_algos.push(token.to_string());
                        }
                    }
                }

                let ratio = if compr_size > 0 {
                    orig_size as f64 / compr_size as f64
                } else {
                    1.0
                };

                zram_devs.push(ZramDevice {
                    device_name: name,
                    disksize_bytes: disksize,
                    mem_used_total_bytes: mem_used,
                    orig_data_bytes: orig_size,
                    compr_data_bytes: compr_size,
                    active_algorithm: active_algo,
                    supported_algorithms: supported_algos,
                    compression_ratio: ratio,
                });
            }
        }
        zram_devs.sort_by(|a, b| a.device_name.cmp(&b.device_name));
        inv.zram_total_disksize_bytes = zram_devs.iter().map(|z| z.disksize_bytes).sum();
        inv.zram_total_used_memory_bytes = zram_devs.iter().map(|z| z.mem_used_total_bytes).sum();
        inv.zram_devices = zram_devs;
    }

    // 6. ZSWAP parameters
    let zswap_dir = sys_root.join("module/zswap/parameters");
    if zswap_dir.exists() {
        inv.zswap.available = true;
        inv.zswap.enabled = read_bounded(&zswap_dir.join("enabled"))
            .map(|s| s == "Y" || s == "1")
            .unwrap_or(false);
        inv.zswap.compressor = read_bounded(&zswap_dir.join("compressor")).ok();
        inv.zswap.zpool = read_bounded(&zswap_dir.join("zpool")).ok();
        inv.zswap.max_pool_percent =
            read_u64(&zswap_dir.join("max_pool_percent")).map(|v| v as u32);
    }

    // 7. Memory sysctl values
    let sysctl_vm = proc_root.join("sys/vm");
    inv.memory_sysctl.swappiness = read_u64(&sysctl_vm.join("swappiness")).map(|v| v as u32);
    inv.memory_sysctl.vfs_cache_pressure =
        read_u64(&sysctl_vm.join("vfs_cache_pressure")).map(|v| v as u32);
    inv.memory_sysctl.dirty_ratio = read_u64(&sysctl_vm.join("dirty_ratio")).map(|v| v as u32);
    inv.memory_sysctl.dirty_background_ratio =
        read_u64(&sysctl_vm.join("dirty_background_ratio")).map(|v| v as u32);
    inv.memory_sysctl.page_cluster = read_u64(&sysctl_vm.join("page-cluster")).map(|v| v as u32);
    inv.memory_sysctl.watermark_scale_factor =
        read_u64(&sysctl_vm.join("watermark_scale_factor")).map(|v| v as u32);
    inv.memory_sysctl.watermark_boost_factor =
        read_u64(&sysctl_vm.join("watermark_boost_factor")).map(|v| v as u32);
    inv.memory_sysctl.min_free_kbytes = read_u64(&sysctl_vm.join("min_free_kbytes"));

    // 8. OOM Managers check
    // Check running processes or systemd units / init configurations
    let check_service = |name: &str| -> bool {
        etc_root
            .join("systemd/system")
            .join(format!("{name}.service"))
            .exists()
            || etc_root
                .join("../run/systemd/system")
                .join(format!("{name}.service"))
                .exists()
            || etc_root.join("init.d").join(name).exists()
            || etc_root.join(format!("default/{name}")).exists()
    };
    inv.oom_managers.earlyoom_present = check_service("earlyoom");
    inv.oom_managers.systemd_oomd_present = check_service("systemd-oomd");
    inv.oom_managers.nohang_present = check_service("nohang");
    inv.oom_managers.oomd_present = check_service("oomd");

    // Also check running processes in /proc if accessible
    if let Ok(entries) = fs::read_dir(proc_root) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(comm) = read_bounded(&entry.path().join("comm")) {
                    match comm.as_str() {
                        "earlyoom" => {
                            inv.oom_managers.earlyoom_present = true;
                            inv.oom_managers.earlyoom_active = true;
                        }
                        "systemd-oomd" => {
                            inv.oom_managers.systemd_oomd_present = true;
                            inv.oom_managers.systemd_oomd_active = true;
                        }
                        "nohang" => {
                            inv.oom_managers.nohang_present = true;
                            inv.oom_managers.nohang_active = true;
                        }
                        "oomd" => {
                            inv.oom_managers.oomd_present = true;
                            inv.oom_managers.oomd_active = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // 9. Zram tuning services
    let mut zram_services = Vec::new();
    if check_service("zram-generator") || etc_root.join("systemd/zram-generator.conf").exists() {
        zram_services.push("zram-generator".into());
    }
    if check_service("zram-tools") || etc_root.join("default/zramswap").exists() {
        zram_services.push("zram-tools".into());
    }
    if check_service("zram-config") {
        zram_services.push("zram-config".into());
    }
    inv.zram_tuning_services = zram_services;

    inv.status = DiscoveryStatus::Available;
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_defaults() {
        let mem = MemoryInventory::default();
        assert_eq!(mem.page_size_bytes, 4096);
        assert_eq!(mem.numa_nodes, 1);
        assert!(!mem.zswap.available);
    }
}
