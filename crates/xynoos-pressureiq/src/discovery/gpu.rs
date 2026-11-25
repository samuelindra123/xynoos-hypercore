//! Universal GPU hardware, driver, DRM render node, and power/frequency intelligence.
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

/// GPU vendor classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Intel,
    Amd,
    Nvidia,
    ArmMali,
    QualcommAdreno,
    Unknown,
}

/// GPU form factor classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuType {
    Integrated,
    Discrete,
    Virtual,
    Unknown,
}

/// Detailed information on a single GPU device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDevice {
    pub card_name: String,
    pub pci_slot: Option<String>,
    pub vendor: GpuVendor,
    pub vendor_hex: Option<String>,
    pub device_hex: Option<String>,
    pub gpu_type: GpuType,
    pub driver: Option<String>,
    pub render_node: Option<String>,
    pub cur_freq_mhz: Option<u64>,
    pub min_freq_mhz: Option<u64>,
    pub max_freq_mhz: Option<u64>,
    pub vram_total_bytes: Option<u64>,
    pub power_control: Option<String>,
    pub video_accel_candidate: bool,
}

/// Overall GPU subsystem inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInventory {
    pub devices: Vec<GpuDevice>,
    pub primary_gpu: Option<GpuDevice>,
    pub has_hardware_acceleration: bool,
    pub status: DiscoveryStatus,
}

impl Default for GpuInventory {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            primary_gpu: None,
            has_hardware_acceleration: false,
            status: DiscoveryStatus::Unknown,
        }
    }
}

pub fn discover_gpu(sys_root: &Path) -> GpuInventory {
    let mut inv = GpuInventory::default();
    let drm_dir = sys_root.join("class/drm");

    if let Ok(entries) = fs::read_dir(&drm_dir) {
        let mut devices = Vec::new();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Match card0, card1, etc.
            if name.starts_with("card") && name.chars().skip(4).all(|c| c.is_ascii_digit()) {
                let card_path = entry.path();
                let device_sym = card_path.join("device");

                let vendor_str = read_bounded(&device_sym.join("vendor")).ok();
                let device_str = read_bounded(&device_sym.join("device")).ok();

                let vendor = match vendor_str.as_deref() {
                    Some("0x8086") | Some("8086") => GpuVendor::Intel,
                    Some("0x1002") | Some("1002") => GpuVendor::Amd,
                    Some("0x10de") | Some("10de") => GpuVendor::Nvidia,
                    Some("0x13b5") | Some("13b5") => GpuVendor::ArmMali,
                    _ => GpuVendor::Unknown,
                };

                let driver = fs::read_link(device_sym.join("driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

                // PCI slot name: resolve symlink of device
                let pci_slot = fs::read_link(&device_sym)
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

                // Classify GPU type
                let gpu_type = match vendor {
                    GpuVendor::Intel => {
                        // Most Intel GPUs on mobile/laptops are integrated, unless Arc discrete (0x56a0 etc)
                        if driver.as_deref() == Some("i915") || driver.as_deref() == Some("xe") {
                            GpuType::Integrated
                        } else {
                            GpuType::Unknown
                        }
                    }
                    GpuVendor::Amd => {
                        // Check if device path has APU indicator or discrete
                        GpuType::Integrated
                    }
                    GpuVendor::Nvidia => GpuType::Discrete,
                    _ => GpuType::Unknown,
                };

                // Render node (e.g. renderD128)
                let mut render_node = None;
                if let Ok(render_entries) = fs::read_dir(&drm_dir) {
                    for re in render_entries.flatten() {
                        let rname = re.file_name().to_string_lossy().to_string();
                        if rname.starts_with("renderD") {
                            render_node = Some(rname);
                            break;
                        }
                    }
                }

                // Intel GPU frequencies
                let cur_freq = read_u64(&card_path.join("gt_cur_freq_mhz"))
                    .or_else(|| read_u64(&card_path.join("gt/gt0/rps_cur_freq_mhz")));
                let min_freq = read_u64(&card_path.join("gt_min_freq_mhz"))
                    .or_else(|| read_u64(&card_path.join("gt/gt0/rps_min_freq_mhz")));
                let max_freq = read_u64(&card_path.join("gt_max_freq_mhz"))
                    .or_else(|| read_u64(&card_path.join("gt/gt0/rps_max_freq_mhz")));

                // Power control
                let pwr_control = read_bounded(&device_sym.join("power/control")).ok();

                let has_render = render_node.is_some();

                devices.push(GpuDevice {
                    card_name: name,
                    pci_slot,
                    vendor,
                    vendor_hex: vendor_str,
                    device_hex: device_str,
                    gpu_type,
                    driver,
                    render_node,
                    cur_freq_mhz: cur_freq,
                    min_freq_mhz: min_freq,
                    max_freq_mhz: max_freq,
                    vram_total_bytes: None,
                    power_control: pwr_control,
                    video_accel_candidate: has_render,
                });
            }
        }

        devices.sort_by(|a, b| a.card_name.cmp(&b.card_name));
        inv.primary_gpu = devices.first().cloned();
        inv.has_hardware_acceleration = devices.iter().any(|d| d.video_accel_candidate);
        inv.devices = devices;
    }

    inv.status = if inv.devices.is_empty() {
        DiscoveryStatus::Unsupported
    } else {
        DiscoveryStatus::Available
    };

    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_defaults() {
        let gpu = GpuInventory::default();
        assert!(gpu.devices.is_empty());
        assert!(!gpu.has_hardware_acceleration);
    }
}
