//! Universal CPU hardware, topology, frequency governor, idle, and instruction intelligence.
#![forbid(unsafe_code)]

use super::types::DiscoveryStatus;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, io::Read, path::Path};

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

/// Frequency governor and driver details for a CPU policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuPolicy {
    pub policy_id: u32,
    pub cpus: Vec<u32>,
    pub driver: Option<String>,
    pub active_governor: Option<String>,
    pub available_governors: Vec<String>,
    pub cur_freq_khz: Option<u64>,
    pub min_freq_khz: Option<u64>,
    pub max_freq_khz: Option<u64>,
    pub energy_perf_preference: Option<String>,
    pub available_epp: Vec<String>,
}

/// Discovered CPU hardware inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInventory {
    pub vendor_id: String,
    pub model_name: String,
    pub cpu_family: Option<u32>,
    pub cpu_model: Option<u32>,
    pub physical_packages: u32,
    pub physical_cores: u32,
    pub logical_cpus: u32,
    pub smt_active: bool,
    pub online_cpus: String,
    pub offline_cpus: Option<String>,
    pub policies: Vec<CpuPolicy>,
    pub scaling_driver: Option<String>,
    pub active_governors: Vec<String>,
    pub available_governors: Vec<String>,
    pub is_heterogeneous: bool,
    pub instruction_features: Vec<String>,
    pub core_throttle_events: u64,
    pub package_throttle_events: u64,
    pub cpuidle_driver: Option<String>,
    pub status: DiscoveryStatus,
}

impl Default for CpuInventory {
    fn default() -> Self {
        Self {
            vendor_id: "Unknown".into(),
            model_name: "Generic CPU".into(),
            cpu_family: None,
            cpu_model: None,
            physical_packages: 1,
            physical_cores: 1,
            logical_cpus: 1,
            smt_active: false,
            online_cpus: "0".into(),
            offline_cpus: None,
            policies: Vec::new(),
            scaling_driver: None,
            active_governors: Vec::new(),
            available_governors: Vec::new(),
            is_heterogeneous: false,
            instruction_features: Vec::new(),
            core_throttle_events: 0,
            package_throttle_events: 0,
            cpuidle_driver: None,
            status: DiscoveryStatus::Unknown,
        }
    }
}

pub fn discover_cpu(sys_root: &Path, proc_root: &Path) -> CpuInventory {
    let mut inv = CpuInventory::default();
    let cpu_sys = sys_root.join("devices/system/cpu");

    // 1. Parse /proc/cpuinfo for vendor, model, family, flags, and core count
    let cpuinfo_path = proc_root.join("cpuinfo");
    if let Ok(content) = read_bounded(&cpuinfo_path) {
        let mut physical_package_ids = BTreeSet::new();
        let mut core_ids = BTreeSet::new();
        let mut processor_count = 0u32;
        let mut cur_package_id = 0u32;

        for line in content.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim();
                let val = v.trim();
                match key {
                    "processor" => processor_count += 1,
                    "vendor_id" if inv.vendor_id == "Unknown" => inv.vendor_id = val.to_string(),
                    "model name" if inv.model_name == "Generic CPU" => {
                        inv.model_name = val.to_string()
                    }
                    "cpu family" if inv.cpu_family.is_none() => inv.cpu_family = val.parse().ok(),
                    "model" if inv.cpu_model.is_none() => inv.cpu_model = val.parse().ok(),
                    "physical id" => {
                        if let Ok(id) = val.parse::<u32>() {
                            cur_package_id = id;
                            physical_package_ids.insert(id);
                        }
                    }
                    "core id" => {
                        if let Ok(id) = val.parse::<u32>() {
                            core_ids.insert((cur_package_id, id));
                        }
                    }
                    "flags" | "Features" if inv.instruction_features.is_empty() => {
                        let filter_flags = [
                            "avx", "avx2", "avx512f", "sse4_1", "sse4_2", "aes", "neon", "asimd",
                            "sha1", "sha2",
                        ];
                        for f in val.split_whitespace() {
                            if filter_flags.iter().any(|&ff| ff.eq_ignore_ascii_case(f)) {
                                inv.instruction_features.push(f.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if processor_count > 0 {
            inv.logical_cpus = processor_count;
        }
        if !physical_package_ids.is_empty() {
            inv.physical_packages = physical_package_ids.len() as u32;
        }
        if !core_ids.is_empty() {
            inv.physical_cores = core_ids.len() as u32;
        } else {
            inv.physical_cores = inv.logical_cpus;
        }
    }

    inv.smt_active = inv.logical_cpus > inv.physical_cores;

    // 2. Online & Offline CPU states
    if let Ok(on) = read_bounded(&cpu_sys.join("online")) {
        inv.online_cpus = on;
    }
    if let Ok(off) = read_bounded(&cpu_sys.join("offline")) {
        inv.offline_cpus = Some(off);
    }

    // 3. CPUFreq policies
    let cpufreq_dir = cpu_sys.join("cpufreq");
    let mut all_governors: BTreeSet<String> = BTreeSet::new();
    let mut all_active_governors: BTreeSet<String> = BTreeSet::new();
    let mut max_freqs: BTreeSet<u64> = BTreeSet::new();

    if let Ok(entries) = fs::read_dir(&cpufreq_dir) {
        let mut policies = Vec::new();
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if let Some(num_str) = fname.strip_prefix("policy") {
                if let Ok(pol_id) = num_str.parse::<u32>() {
                    let path = entry.path();
                    let driver = read_bounded(&path.join("scaling_driver")).ok();
                    let active_gov = read_bounded(&path.join("scaling_governor")).ok();
                    let avail_govs: Vec<String> =
                        read_bounded(&path.join("scaling_available_governors"))
                            .map(|s| s.split_whitespace().map(|w| w.to_string()).collect())
                            .unwrap_or_default();
                    let cpus_str = read_bounded(&path.join("affected_cpus"))
                        .or_else(|_| read_bounded(&path.join("related_cpus")))
                        .unwrap_or_default();
                    let cpus: Vec<u32> = cpus_str
                        .split_whitespace()
                        .filter_map(|w| w.parse().ok())
                        .collect();

                    let cur_freq = read_u64(&path.join("scaling_cur_freq"))
                        .or_else(|| read_u64(&path.join("cpuinfo_cur_freq")));
                    let min_freq = read_u64(&path.join("cpuinfo_min_freq"))
                        .or_else(|| read_u64(&path.join("scaling_min_freq")));
                    let max_freq = read_u64(&path.join("cpuinfo_max_freq"))
                        .or_else(|| read_u64(&path.join("scaling_max_freq")));

                    if let Some(mf) = max_freq {
                        max_freqs.insert(mf);
                    }

                    let epp = read_bounded(&path.join("energy_performance_preference")).ok();
                    let avail_epp =
                        read_bounded(&path.join("energy_performance_available_preferences"))
                            .map(|s| s.split_whitespace().map(|w| w.to_string()).collect())
                            .unwrap_or_default();

                    if let Some(g) = &active_gov {
                        all_active_governors.insert(g.clone());
                    }
                    for g in &avail_govs {
                        all_governors.insert(g.clone());
                    }
                    if inv.scaling_driver.is_none() && driver.is_some() {
                        inv.scaling_driver = driver.clone();
                    }

                    policies.push(CpuPolicy {
                        policy_id: pol_id,
                        cpus,
                        driver,
                        active_governor: active_gov,
                        available_governors: avail_govs,
                        cur_freq_khz: cur_freq,
                        min_freq_khz: min_freq,
                        max_freq_khz: max_freq,
                        energy_perf_preference: epp,
                        available_epp: avail_epp,
                    });
                }
            }
        }
        policies.sort_by_key(|p| p.policy_id);
        inv.policies = policies;
    }

    // Heterogeneous topology check (e.g. multiple distinct max frequencies across policies)
    inv.is_heterogeneous = max_freqs.len() > 1;

    inv.active_governors = all_active_governors.into_iter().collect();
    inv.available_governors = all_governors.into_iter().collect();

    // 4. Throttle event counters (read from cpu0)
    let throttle_dir = cpu_sys.join("cpu0/thermal_throttle");
    inv.core_throttle_events = read_u64(&throttle_dir.join("core_throttle_count")).unwrap_or(0);
    inv.package_throttle_events =
        read_u64(&throttle_dir.join("package_throttle_count")).unwrap_or(0);

    // 5. CPU idle driver
    let cpuidle_dir = cpu_sys.join("cpuidle");
    inv.cpuidle_driver = read_bounded(&cpuidle_dir.join("current_driver")).ok();

    inv.status = DiscoveryStatus::Available;
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_defaults() {
        let cpu = CpuInventory::default();
        assert_eq!(cpu.physical_packages, 1);
        assert!(!cpu.is_heterogeneous);
    }
}
