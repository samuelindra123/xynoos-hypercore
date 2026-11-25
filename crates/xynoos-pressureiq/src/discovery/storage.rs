//! Universal storage, block device, media type, I/O scheduler, and filesystem intelligence.
#![forbid(unsafe_code)]

use super::types::DiscoveryStatus;
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};

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

/// Physical storage media classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageMediaType {
    NvmeSsd,
    SataSsd,
    EmmcFlash,
    RotationalHdd,
    VirtualDisk,
    DeviceMapper,
    ZramDisk,
    Unknown,
}

/// Discovered block device characteristics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDeviceInfo {
    pub name: String,
    pub media_type: StorageMediaType,
    pub rotational: bool,
    pub size_bytes: u64,
    pub active_scheduler: Option<String>,
    pub available_schedulers: Vec<String>,
    pub read_ahead_kb: Option<u64>,
    pub nr_requests: Option<u64>,
    pub logical_block_size: Option<u64>,
    pub discard_supported: bool,
    pub discard_granularity_bytes: Option<u64>,
    pub is_removable: bool,
}

/// Root filesystem mount details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootFilesystemInfo {
    pub device_source: String,
    pub resolved_physical_device: Option<String>,
    pub fstype: String,
    pub mount_options: Vec<String>,
    pub has_noatime: bool,
    pub has_discard: bool,
    pub has_compression: bool,
}

/// Discovered storage subsystem inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInventory {
    pub root_filesystem: Option<RootFilesystemInfo>,
    pub block_devices: Vec<BlockDeviceInfo>,
    pub primary_storage_type: StorageMediaType,
    pub has_rotational_storage: bool,
    pub has_nvme: bool,
    pub has_emmc: bool,
    pub has_discard_support: bool,
    pub status: DiscoveryStatus,
}

impl Default for StorageInventory {
    fn default() -> Self {
        Self {
            root_filesystem: None,
            block_devices: Vec::new(),
            primary_storage_type: StorageMediaType::Unknown,
            has_rotational_storage: false,
            has_nvme: false,
            has_emmc: false,
            has_discard_support: false,
            status: DiscoveryStatus::Unknown,
        }
    }
}

pub fn discover_storage(sys_root: &Path, proc_root: &Path) -> StorageInventory {
    let mut inv = StorageInventory::default();

    // 1. Parse /proc/mounts to find the root filesystem "/"
    let mounts_path = proc_root.join("mounts");
    if let Ok(content) = read_bounded(&mounts_path) {
        for line in content.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 4 && cols[1] == "/" {
                let dev_src = cols[0].to_string();
                let fstype = cols[2].to_string();
                let opts: Vec<String> = cols[3].split(',').map(|s| s.to_string()).collect();

                let has_noatime = opts.iter().any(|o| o == "noatime");
                let has_discard = opts.iter().any(|o| o == "discard");
                let has_compression = opts.iter().any(|o| o.starts_with("compress"));

                // Resolve physical device (e.g. /dev/mmcblk0p3 -> mmcblk0)
                let resolved = if let Some(base) = dev_src.strip_prefix("/dev/") {
                    if let Some(p) = base.strip_prefix("mmcblk") {
                        if let Some((num, _)) = p.split_once('p') {
                            Some(format!("mmcblk{num}"))
                        } else {
                            Some(base.to_string())
                        }
                    } else if let Some(p) = base.strip_prefix("nvme") {
                        if let Some((ctrl, _)) = p.split_once('p') {
                            Some(format!("nvme{ctrl}"))
                        } else {
                            Some(base.to_string())
                        }
                    } else if base.starts_with("sd") {
                        Some(base.chars().take(3).collect())
                    } else {
                        Some(base.to_string())
                    }
                } else {
                    None
                };

                inv.root_filesystem = Some(RootFilesystemInfo {
                    device_source: dev_src,
                    resolved_physical_device: resolved,
                    fstype,
                    mount_options: opts,
                    has_noatime,
                    has_discard,
                    has_compression,
                });
                break;
            }
        }
    }

    // 2. Discover block devices in /sys/block
    let sys_block = sys_root.join("block");
    if let Ok(entries) = fs::read_dir(&sys_block) {
        let mut devices = Vec::new();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip loop devices and ram devices to keep inventory bounded and noise-free
            if name.starts_with("loop") || name.starts_with("ram") {
                continue;
            }

            let path = entry.path();
            let queue = path.join("queue");

            let rotational = read_u64(&queue.join("rotational"))
                .map(|v| v == 1)
                .unwrap_or(false);

            // Determine media type
            let media_type = if name.starts_with("nvme") {
                StorageMediaType::NvmeSsd
            } else if name.starts_with("mmcblk") {
                StorageMediaType::EmmcFlash
            } else if name.starts_with("zram") {
                StorageMediaType::ZramDisk
            } else if name.starts_with("dm-") {
                StorageMediaType::DeviceMapper
            } else if name.starts_with("vd") || name.starts_with("xvd") {
                StorageMediaType::VirtualDisk
            } else if name.starts_with("sd") {
                if rotational {
                    StorageMediaType::RotationalHdd
                } else {
                    StorageMediaType::SataSsd
                }
            } else if rotational {
                StorageMediaType::RotationalHdd
            } else {
                StorageMediaType::Unknown
            };

            // Size in 512-byte sectors
            let size_sectors = read_u64(&path.join("size")).unwrap_or(0);
            let size_bytes = size_sectors.saturating_mul(512);

            // Scheduler
            let mut active_sched = None;
            let mut avail_scheds = Vec::new();
            if let Ok(sched_str) = read_bounded(&queue.join("scheduler")) {
                for token in sched_str.split_whitespace() {
                    if token.starts_with('[') && token.ends_with(']') {
                        let active = token.trim_matches('[').trim_matches(']');
                        active_sched = Some(active.to_string());
                        avail_scheds.push(active.to_string());
                    } else {
                        avail_scheds.push(token.to_string());
                    }
                }
            }

            let read_ahead = read_u64(&queue.join("read_ahead_kb"));
            let nr_req = read_u64(&queue.join("nr_requests"));
            let logical_block = read_u64(&queue.join("logical_block_size"));
            let discard_gran = read_u64(&queue.join("discard_granularity"));
            let discard_max = read_u64(&queue.join("discard_max_bytes"));
            let discard_supported =
                discard_gran.is_some_and(|g| g > 0) && discard_max.is_some_and(|m| m > 0);
            let removable = read_u64(&path.join("removable"))
                .map(|v| v == 1)
                .unwrap_or(false);

            devices.push(BlockDeviceInfo {
                name,
                media_type,
                rotational,
                size_bytes,
                active_scheduler: active_sched,
                available_schedulers: avail_scheds,
                read_ahead_kb: read_ahead,
                nr_requests: nr_req,
                logical_block_size: logical_block,
                discard_supported,
                discard_granularity_bytes: discard_gran,
                is_removable: removable,
            });
        }

        devices.sort_by(|a, b| a.name.cmp(&b.name));

        inv.has_rotational_storage = devices.iter().any(|d| d.rotational);
        inv.has_nvme = devices
            .iter()
            .any(|d| d.media_type == StorageMediaType::NvmeSsd);
        inv.has_emmc = devices
            .iter()
            .any(|d| d.media_type == StorageMediaType::EmmcFlash);
        inv.has_discard_support = devices.iter().any(|d| d.discard_supported);

        // Classify primary storage type based on root device or dominant disk
        if let Some(rf) = &inv.root_filesystem {
            if let Some(target) = &rf.resolved_physical_device {
                if let Some(d) = devices.iter().find(|d| d.name == *target) {
                    inv.primary_storage_type = d.media_type;
                }
            }
        }
        if inv.primary_storage_type == StorageMediaType::Unknown {
            if inv.has_nvme {
                inv.primary_storage_type = StorageMediaType::NvmeSsd;
            } else if inv.has_emmc {
                inv.primary_storage_type = StorageMediaType::EmmcFlash;
            } else if let Some(first_sd) = devices.iter().find(|d| d.name.starts_with("sd")) {
                inv.primary_storage_type = first_sd.media_type;
            }
        }

        inv.block_devices = devices;
    }

    inv.status = DiscoveryStatus::Available;
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_defaults() {
        let st = StorageInventory::default();
        assert_eq!(st.primary_storage_type, StorageMediaType::Unknown);
        assert!(!st.has_rotational_storage);
    }
}
