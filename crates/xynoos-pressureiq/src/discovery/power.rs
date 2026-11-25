//! Universal power supply, battery, thermal zone, platform profile, and daemon intelligence.
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

/// Service operational status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Running,
    InstalledOnly,
    Absent,
    Unknown,
}

/// Discovered power management daemons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerDaemons {
    pub tlp: ServiceState,
    pub power_profiles_daemon: ServiceState,
    pub tuned: ServiceState,
    pub auto_cpufreq: ServiceState,
    pub thermald: ServiceState,
    pub laptop_mode_tools: ServiceState,
}

/// Discovered thermal zone reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalZoneInfo {
    pub name: String,
    pub zone_type: String,
    pub temp_millidegrees_c: Option<u64>,
    pub temp_degrees_c: Option<f64>,
}

/// Battery information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub name: String,
    pub present: bool,
    pub capacity_pct: Option<u32>,
    pub status: Option<String>,
    pub technology: Option<String>,
}

/// Discovered power and thermal inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerInventory {
    pub ac_online: Option<bool>,
    pub has_battery: bool,
    pub batteries: Vec<BatteryInfo>,
    pub platform_power_profile: Option<String>,
    pub available_power_profiles: Vec<String>,
    pub thermal_zones: Vec<ThermalZoneInfo>,
    pub max_cpu_temp_degrees_c: Option<f64>,
    pub power_daemons: PowerDaemons,
    pub status: DiscoveryStatus,
}

impl Default for PowerInventory {
    fn default() -> Self {
        Self {
            ac_online: None,
            has_battery: false,
            batteries: Vec::new(),
            platform_power_profile: None,
            available_power_profiles: Vec::new(),
            thermal_zones: Vec::new(),
            max_cpu_temp_degrees_c: None,
            power_daemons: PowerDaemons {
                tlp: ServiceState::Absent,
                power_profiles_daemon: ServiceState::Absent,
                tuned: ServiceState::Absent,
                auto_cpufreq: ServiceState::Absent,
                thermald: ServiceState::Absent,
                laptop_mode_tools: ServiceState::Absent,
            },
            status: DiscoveryStatus::Unknown,
        }
    }
}

pub fn discover_power(sys_root: &Path, proc_root: &Path, etc_root: &Path) -> PowerInventory {
    let mut inv = PowerInventory::default();

    // 1. Power supplies in /sys/class/power_supply
    let ps_dir = sys_root.join("class/power_supply");
    if let Ok(entries) = fs::read_dir(&ps_dir) {
        let mut bats = Vec::new();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let p = entry.path();
            let ptype = read_bounded(&p.join("type")).unwrap_or_default();

            if ptype.eq_ignore_ascii_case("Mains")
                || name.starts_with("AC")
                || name.starts_with("ADP")
            {
                if let Some(on) = read_u64(&p.join("online")) {
                    inv.ac_online = Some(on == 1);
                }
            } else if ptype.eq_ignore_ascii_case("Battery") || name.starts_with("BAT") {
                let present = read_u64(&p.join("present")).map(|v| v == 1).unwrap_or(true);
                let cap = read_u64(&p.join("capacity")).map(|v| v as u32);
                let status = read_bounded(&p.join("status")).ok();
                let tech = read_bounded(&p.join("technology")).ok();

                bats.push(BatteryInfo {
                    name,
                    present,
                    capacity_pct: cap,
                    status,
                    technology: tech,
                });
            }
        }
        inv.has_battery = !bats.is_empty();
        inv.batteries = bats;
    }

    // 2. ACPI platform profile
    let acpi_profile = sys_root.join("firmware/acpi/platform_profile");
    if let Ok(prof) = read_bounded(&acpi_profile) {
        inv.platform_power_profile = Some(prof);
    }
    let acpi_avail = sys_root.join("firmware/acpi/platform_profile_choices");
    if let Ok(choices) = read_bounded(&acpi_avail) {
        inv.available_power_profiles = choices.split_whitespace().map(|s| s.to_string()).collect();
    }

    // 3. Thermal zones in /sys/class/thermal
    let thermal_dir = sys_root.join("class/thermal");
    if let Ok(entries) = fs::read_dir(&thermal_dir) {
        let mut zones = Vec::new();
        let mut max_temp = 0.0f64;

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("thermal_zone") {
                let p = entry.path();
                let ztype = read_bounded(&p.join("type")).unwrap_or_else(|_| "unknown".into());
                let temp_m = read_u64(&p.join("temp"));
                let temp_c = temp_m.map(|m| (m as f64) / 1000.0);

                if let Some(tc) = temp_c {
                    if (ztype.contains("pkg") || ztype.contains("cpu") || ztype.contains("x86"))
                        && tc > max_temp
                    {
                        max_temp = tc;
                    }
                }

                zones.push(ThermalZoneInfo {
                    name,
                    zone_type: ztype,
                    temp_millidegrees_c: temp_m,
                    temp_degrees_c: temp_c,
                });
            }
        }
        zones.sort_by(|a, b| a.name.cmp(&b.name));
        if max_temp > 0.0 {
            inv.max_cpu_temp_degrees_c = Some(max_temp);
        }
        inv.thermal_zones = zones;
    }

    // 4. Power management daemons
    let check_daemon = |service_name: &str, binary_name: &str| -> ServiceState {
        // First check running process in proc_root
        let mut is_running = false;
        if let Ok(entries) = fs::read_dir(proc_root) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(comm) = read_bounded(&entry.path().join("comm")) {
                        if comm.eq_ignore_ascii_case(binary_name) {
                            is_running = true;
                            break;
                        }
                    }
                }
            }
        }
        if is_running {
            return ServiceState::Running;
        }

        // Second check installed unit or etc config
        let installed = etc_root
            .join("systemd/system")
            .join(format!("{service_name}.service"))
            .exists()
            || etc_root
                .join("../run/systemd/system")
                .join(format!("{service_name}.service"))
                .exists()
            || etc_root.join("init.d").join(service_name).exists()
            || etc_root.join(format!("default/{service_name}")).exists();

        if installed {
            ServiceState::InstalledOnly
        } else {
            ServiceState::Absent
        }
    };

    inv.power_daemons.tlp = check_daemon("tlp", "tlp");
    inv.power_daemons.power_profiles_daemon =
        check_daemon("power-profiles-daemon", "power-profiles-daemon");
    inv.power_daemons.tuned = check_daemon("tuned", "tuned");
    inv.power_daemons.auto_cpufreq = check_daemon("auto-cpufreq", "auto-cpufreq");
    inv.power_daemons.thermald = check_daemon("thermald", "thermald");
    inv.power_daemons.laptop_mode_tools = check_daemon("laptop-mode-tools", "laptop_mode");

    inv.status = DiscoveryStatus::Available;
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_defaults() {
        let p = PowerInventory::default();
        assert!(!p.has_battery);
        assert_eq!(p.power_daemons.tlp, ServiceState::Absent);
    }
}
