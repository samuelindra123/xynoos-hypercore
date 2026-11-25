//! High-precision, capability-aware adaptive telemetry scheduler with multi-rate sampling.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const TELEMETRY_SAMPLE_SCHEMA_VERSION: u32 = 1;
pub const MAX_TELEMETRY_RING_BUFFER: usize = 64;

/// Adaptive polling interval category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PollingTier {
    /// Active resource pressure or rapid workload changes (500–1000 ms).
    ActivePressure,
    /// Normal steady state operation (2000–5000 ms).
    SteadyState,
    /// Low-activity quiescent idle state (5000–15000 ms).
    LowActivityIdle,
}

impl PollingTier {
    pub fn target_interval_ms(&self, base_ms: u64) -> u64 {
        match self {
            Self::ActivePressure => (base_ms / 4).clamp(500, 1000),
            Self::SteadyState => base_ms.clamp(2000, 5000),
            Self::LowActivityIdle => (base_ms * 3).clamp(5000, 15000),
        }
    }
}

/// Instantaneous telemetry sample capturing multi-subsystem indicators.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetrySample {
    pub schema_version: u32,
    pub timestamp_epoch_ms: u64,
    pub elapsed_interval_ms: u64,

    // Memory
    pub mem_total_bytes: Option<u64>,
    pub mem_available_bytes: Option<u64>,
    pub mem_available_pct: Option<f64>,
    pub swap_total_bytes: Option<u64>,
    pub swap_free_bytes: Option<u64>,
    pub swap_in_delta: u64,
    pub swap_out_delta: u64,
    pub pgmajfault_delta: u64,

    // Pressure Stall Information (PSI)
    pub psi_memory_some_avg10: Option<f64>,
    pub psi_memory_full_avg10: Option<f64>,
    pub psi_cpu_some_avg10: Option<f64>,
    pub psi_io_some_avg10: Option<f64>,
    pub psi_io_full_avg10: Option<f64>,

    // CPU & Workload
    pub cpu_utilization_pct: Option<f64>,
    pub cpu_throttle_delta: u64,
    pub load_one: Option<f64>,

    // Thermal & Power
    pub max_cpu_temp_c: Option<f64>,
    pub ac_online: Option<bool>,
    pub battery_capacity_pct: Option<u32>,

    // GPU & Graphics
    pub gpu_active: bool,
    pub gpu_freq_mhz: Option<u64>,

    // Cgroup & Workload Context
    pub cgroup_memory_current_bytes: Option<u64>,
    pub cgroup_memory_events_oom_delta: u64,
    pub cgroup_memory_events_high_delta: u64,

    // Scheduler Tier
    pub evaluated_tier: PollingTier,
}

#[derive(Default)]
struct CumulativeCounters {
    cpu_busy_ticks: u64,
    cpu_total_ticks: u64,
    pswpin: u64,
    pswpout: u64,
    pgmajfault: u64,
    core_throttle_count: u64,
    package_throttle_count: u64,
    cgroup_oom_count: u64,
    cgroup_high_count: u64,
}

/// Capability-aware adaptive telemetry scheduler.
pub struct TelemetryScheduler {
    pub proc_root: PathBuf,
    pub sys_root: PathBuf,
    pub base_interval_ms: u64,
    last_sample_instant: Option<Instant>,
    last_counters: Option<CumulativeCounters>,
    history: VecDeque<TelemetrySample>,
    current_tier: PollingTier,
}

impl TelemetryScheduler {
    pub fn new(proc_root: PathBuf, sys_root: PathBuf, base_interval_ms: u64) -> Self {
        Self {
            proc_root,
            sys_root,
            base_interval_ms,
            last_sample_instant: None,
            last_counters: None,
            history: VecDeque::with_capacity(MAX_TELEMETRY_RING_BUFFER),
            current_tier: PollingTier::SteadyState,
        }
    }

    pub fn current_tier(&self) -> PollingTier {
        self.current_tier
    }

    pub fn next_polling_interval(&self) -> Duration {
        Duration::from_millis(self.current_tier.target_interval_ms(self.base_interval_ms))
    }

    pub fn recent_history(&self) -> &VecDeque<TelemetrySample> {
        &self.history
    }

    pub fn last_sample(&self) -> Option<&TelemetrySample> {
        self.history.back()
    }

    /// Collect a fresh telemetry sample from the filesystem roots.
    pub fn collect_sample(&mut self, cgroup_path: Option<&Path>) -> TelemetrySample {
        let now = Instant::now();
        let epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let elapsed_ms = self
            .last_sample_instant
            .map(|prev| now.duration_since(prev).as_millis() as u64)
            .unwrap_or(self.base_interval_ms);
        self.last_sample_instant = Some(now);

        // 1. Meminfo
        let (mem_total, mem_available, swap_total, swap_free) = self.read_meminfo();
        let mem_available_pct = match (mem_available, mem_total) {
            (Some(a), Some(t)) if t > 0 => Some((a as f64 / t as f64) * 100.0),
            _ => None,
        };

        // 2. Vmstat deltas
        let (cur_pswpin, cur_pswpout, cur_pgmajfault) = self.read_vmstat();
        let (swap_in_delta, swap_out_delta, pgmajfault_delta) = match &self.last_counters {
            Some(prev) => (
                cur_pswpin.saturating_sub(prev.pswpin),
                cur_pswpout.saturating_sub(prev.pswpout),
                cur_pgmajfault.saturating_sub(prev.pgmajfault),
            ),
            None => (0, 0, 0),
        };

        // 3. PSI
        let (psi_mem_some, psi_mem_full) = self.read_psi("memory");
        let (psi_cpu_some, _) = self.read_psi("cpu");
        let (psi_io_some, psi_io_full) = self.read_psi("io");

        // 4. CPU stat and utilization
        let (cur_busy, cur_total) = self.read_cpu_ticks();
        let cpu_utilization_pct = match &self.last_counters {
            Some(prev) if cur_total > prev.cpu_total_ticks => {
                let busy_d = cur_busy.saturating_sub(prev.cpu_busy_ticks) as f64;
                let total_d = cur_total.saturating_sub(prev.cpu_total_ticks) as f64;
                Some((busy_d / total_d) * 100.0)
            }
            _ => None,
        };

        // 5. Throttle deltas
        let (cur_core_throttle, cur_pkg_throttle) = self.read_throttle_counts();
        let cpu_throttle_delta = match &self.last_counters {
            Some(prev) => {
                let d1 = cur_core_throttle.saturating_sub(prev.core_throttle_count);
                let d2 = cur_pkg_throttle.saturating_sub(prev.package_throttle_count);
                d1.saturating_add(d2)
            }
            None => 0,
        };

        // 6. Load average
        let load_one = self.read_loadavg();

        // 7. Thermal & Power
        let max_cpu_temp_c = self.read_max_thermal();
        let ac_online = self.read_ac_online();
        let battery_capacity_pct = self.read_battery_capacity();

        // 8. GPU telemetry
        let (gpu_active, gpu_freq_mhz) = self.read_gpu_telemetry();

        // 9. Cgroup telemetry
        let (cg_mem_curr, cur_oom, cur_high) = self.read_cgroup(cgroup_path);
        let (cg_oom_delta, cg_high_delta) = match &self.last_counters {
            Some(prev) => (
                cur_oom.saturating_sub(prev.cgroup_oom_count),
                cur_high.saturating_sub(prev.cgroup_high_count),
            ),
            None => (0, 0),
        };

        // Update cumulative counters
        self.last_counters = Some(CumulativeCounters {
            cpu_busy_ticks: cur_busy,
            cpu_total_ticks: cur_total,
            pswpin: cur_pswpin,
            pswpout: cur_pswpout,
            pgmajfault: cur_pgmajfault,
            core_throttle_count: cur_core_throttle,
            package_throttle_count: cur_pkg_throttle,
            cgroup_oom_count: cur_oom,
            cgroup_high_count: cur_high,
        });

        // 10. Classify Polling Tier
        let tier = Self::evaluate_tier(
            mem_available_pct,
            psi_mem_some,
            psi_cpu_some,
            psi_io_some,
            cpu_throttle_delta,
            cpu_utilization_pct,
            swap_in_delta + swap_out_delta,
        );
        self.current_tier = tier;

        let sample = TelemetrySample {
            schema_version: TELEMETRY_SAMPLE_SCHEMA_VERSION,
            timestamp_epoch_ms: epoch_ms,
            elapsed_interval_ms: elapsed_ms,
            mem_total_bytes: mem_total,
            mem_available_bytes: mem_available,
            mem_available_pct,
            swap_total_bytes: swap_total,
            swap_free_bytes: swap_free,
            swap_in_delta,
            swap_out_delta,
            pgmajfault_delta,
            psi_memory_some_avg10: psi_mem_some,
            psi_memory_full_avg10: psi_mem_full,
            psi_cpu_some_avg10: psi_cpu_some,
            psi_io_some_avg10: psi_io_some,
            psi_io_full_avg10: psi_io_full,
            cpu_utilization_pct,
            cpu_throttle_delta,
            load_one,
            max_cpu_temp_c,
            ac_online,
            battery_capacity_pct,
            gpu_active,
            gpu_freq_mhz,
            cgroup_memory_current_bytes: cg_mem_curr,
            cgroup_memory_events_oom_delta: cg_oom_delta,
            cgroup_memory_events_high_delta: cg_high_delta,
            evaluated_tier: tier,
        };

        if self.history.len() >= MAX_TELEMETRY_RING_BUFFER {
            self.history.pop_front();
        }
        self.history.push_back(sample.clone());

        sample
    }

    fn evaluate_tier(
        mem_avail_pct: Option<f64>,
        psi_mem: Option<f64>,
        psi_cpu: Option<f64>,
        psi_io: Option<f64>,
        throttle_delta: u64,
        cpu_util: Option<f64>,
        swap_activity_delta: u64,
    ) -> PollingTier {
        let is_pressure = psi_mem.is_some_and(|v| v >= 3.0)
            || psi_cpu.is_some_and(|v| v >= 10.0)
            || psi_io.is_some_and(|v| v >= 8.0)
            || throttle_delta > 0
            || swap_activity_delta >= 10;

        if is_pressure {
            return PollingTier::ActivePressure;
        }

        let is_active = cpu_util.is_some_and(|u| u >= 8.0)
            || mem_avail_pct.is_some_and(|p| p <= 25.0)
            || swap_activity_delta > 0;

        if is_active {
            PollingTier::SteadyState
        } else {
            PollingTier::LowActivityIdle
        }
    }

    fn read_meminfo(&self) -> (Option<u64>, Option<u64>, Option<u64>, Option<u64>) {
        let path = self.proc_root.join("meminfo");
        let Ok(text) = fs::read_to_string(path) else {
            return (None, None, None, None);
        };
        let mut total = None;
        let mut avail = None;
        let mut swap_total = None;
        let mut swap_free = None;

        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let Some(key) = parts.next() else { continue };
            let Some(val_str) = parts.next() else {
                continue;
            };
            let Ok(val_kb) = val_str.parse::<u64>() else {
                continue;
            };
            let val_bytes = val_kb.saturating_mul(1024);

            match key {
                "MemTotal:" => total = Some(val_bytes),
                "MemAvailable:" => avail = Some(val_bytes),
                "SwapTotal:" => swap_total = Some(val_bytes),
                "SwapFree:" => swap_free = Some(val_bytes),
                _ => {}
            }
        }
        (total, avail, swap_total, swap_free)
    }

    fn read_vmstat(&self) -> (u64, u64, u64) {
        let path = self.proc_root.join("vmstat");
        let Ok(text) = fs::read_to_string(path) else {
            return (0, 0, 0);
        };
        let mut pswpin = 0;
        let mut pswpout = 0;
        let mut pgmajfault = 0;

        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let Some(k) = parts.next() else { continue };
            let Some(v) = parts.next().and_then(|s| s.parse::<u64>().ok()) else {
                continue;
            };
            match k {
                "pswpin" => pswpin = v,
                "pswpout" => pswpout = v,
                "pgmajfault" => pgmajfault = v,
                _ => {}
            }
        }
        (pswpin, pswpout, pgmajfault)
    }

    fn read_psi(&self, kind: &str) -> (Option<f64>, Option<f64>) {
        let path = self.proc_root.join(format!("pressure/{kind}"));
        let Ok(text) = fs::read_to_string(path) else {
            return (None, None);
        };
        let mut some_avg10 = None;
        let mut full_avg10 = None;

        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("some ") {
                for token in rest.split_whitespace() {
                    if let Some(v_str) = token.strip_prefix("avg10=") {
                        some_avg10 = v_str.parse::<f64>().ok();
                    }
                }
            } else if let Some(rest) = line.strip_prefix("full ") {
                for token in rest.split_whitespace() {
                    if let Some(v_str) = token.strip_prefix("avg10=") {
                        full_avg10 = v_str.parse::<f64>().ok();
                    }
                }
            }
        }
        (some_avg10, full_avg10)
    }

    fn read_cpu_ticks(&self) -> (u64, u64) {
        let path = self.proc_root.join("stat");
        let Ok(text) = fs::read_to_string(path) else {
            return (0, 0);
        };
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("cpu ") {
                let ticks: Vec<u64> = rest
                    .split_whitespace()
                    .filter_map(|s| s.parse::<u64>().ok())
                    .collect();
                if ticks.len() >= 4 {
                    let user = ticks[0];
                    let nice = ticks[1];
                    let system = ticks[2];
                    let idle = ticks[3];
                    let iowait = ticks.get(4).copied().unwrap_or(0);
                    let irq = ticks.get(5).copied().unwrap_or(0);
                    let softirq = ticks.get(6).copied().unwrap_or(0);
                    let steal = ticks.get(7).copied().unwrap_or(0);

                    let busy = user + nice + system + irq + softirq + steal;
                    let total = busy + idle + iowait;
                    return (busy, total);
                }
            }
        }
        (0, 0)
    }

    fn read_throttle_counts(&self) -> (u64, u64) {
        let cpu0_throttle = self
            .sys_root
            .join("devices/system/cpu/cpu0/thermal_throttle");
        let core = fs::read_to_string(cpu0_throttle.join("core_throttle_count"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let pkg = fs::read_to_string(cpu0_throttle.join("package_throttle_count"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        (core, pkg)
    }

    fn read_loadavg(&self) -> Option<f64> {
        let path = self.proc_root.join("loadavg");
        let text = fs::read_to_string(path).ok()?;
        text.split_whitespace().next()?.parse::<f64>().ok()
    }

    fn read_max_thermal(&self) -> Option<f64> {
        let thermal_dir = self.sys_root.join("class/thermal");
        let Ok(entries) = fs::read_dir(thermal_dir) else {
            return None;
        };
        let mut max_c: Option<f64> = None;

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("thermal_zone") {
                if let Ok(raw) = fs::read_to_string(entry.path().join("temp")) {
                    if let Ok(milli) = raw.trim().parse::<f64>() {
                        let c = milli / 1000.0;
                        if c > 0.0 && c < 150.0 {
                            max_c = Some(max_c.map_or(c, |prev| prev.max(c)));
                        }
                    }
                }
            }
        }
        max_c
    }

    fn read_ac_online(&self) -> Option<bool> {
        let ps_dir = self.sys_root.join("class/power_supply");
        let Ok(entries) = fs::read_dir(ps_dir) else {
            return None;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(typ) = fs::read_to_string(path.join("type")) {
                if typ.trim() == "Mains" {
                    if let Ok(online) = fs::read_to_string(path.join("online")) {
                        return Some(online.trim() == "1");
                    }
                }
            }
        }
        None
    }

    fn read_battery_capacity(&self) -> Option<u32> {
        let ps_dir = self.sys_root.join("class/power_supply");
        let Ok(entries) = fs::read_dir(ps_dir) else {
            return None;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(typ) = fs::read_to_string(path.join("type")) {
                if typ.trim() == "Battery" {
                    if let Ok(cap) = fs::read_to_string(path.join("capacity")) {
                        if let Ok(num) = cap.trim().parse::<u32>() {
                            return Some(num);
                        }
                    }
                }
            }
        }
        None
    }

    fn read_gpu_telemetry(&self) -> (bool, Option<u64>) {
        let drm_dir = self.sys_root.join("class/drm");
        let Ok(entries) = fs::read_dir(drm_dir) else {
            return (false, None);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("card") && !name.contains('-') {
                let freq = fs::read_to_string(path.join("gt_cur_freq_mhz"))
                    .or_else(|_| fs::read_to_string(path.join("device/drm/card0/gt_cur_freq_mhz")))
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok());

                let rc6_res = fs::read_to_string(path.join("power/rc6_residency_ms")).ok();
                let is_active = rc6_res.is_some() || freq.is_some();
                return (is_active, freq);
            }
        }
        (false, None)
    }

    fn read_cgroup(&self, cgroup_path: Option<&Path>) -> (Option<u64>, u64, u64) {
        let Some(cg) = cgroup_path else {
            return (None, 0, 0);
        };
        let mem_curr = fs::read_to_string(cg.join("memory.current"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());

        let mut oom = 0;
        let mut high = 0;
        if let Ok(events) = fs::read_to_string(cg.join("memory.events")) {
            for line in events.lines() {
                let mut parts = line.split_whitespace();
                let Some(k) = parts.next() else { continue };
                let Some(v) = parts.next().and_then(|s| s.parse::<u64>().ok()) else {
                    continue;
                };
                match k {
                    "oom" => oom = v,
                    "high" => high = v,
                    _ => {}
                }
            }
        }
        (mem_curr, oom, high)
    }
}
