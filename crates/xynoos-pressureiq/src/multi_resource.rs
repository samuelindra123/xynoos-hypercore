//! Read-only multi-resource PressureIQ telemetry (CPU, memory, I/O PSI, meminfo, and authorized cgroup stats).
//! Unavailable, permission-denied, reset, or malformed sources remain explicit errors and are never fabricated.
use crate::{
    intelligence::{diagnose, Diagnosis, Observation},
    parse_meminfo, parse_psi, parse_vmstat,
    system::{cpu_utilization, parse_cpu, Cpu},
    Error, Psi,
};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn read_bounded(path: &Path) -> Result<String, Error> {
    let mut f = File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::Unsupported,
        std::io::ErrorKind::PermissionDenied => Error::Permission,
        _ => Error::Io(e.kind().to_string()),
    })?;
    let mut text = String::new();
    f.by_ref()
        .take(1_048_577)
        .read_to_string(&mut text)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => Error::Permission,
            _ => Error::Io(e.kind().to_string()),
        })?;
    if text.len() > 1_048_576 {
        return Err(Error::Bounds);
    }
    Ok(text)
}

fn read_u64_cgroup_value(path: &Path) -> Result<u64, Error> {
    let text = read_bounded(path)?;
    let trimmed = text.trim();
    if trimmed == "max" {
        return Ok(u64::MAX);
    }
    trimmed.parse::<u64>().map_err(|e| {
        if matches!(e.kind(), std::num::IntErrorKind::PosOverflow) {
            Error::Overflow("cgroup".into())
        } else {
            Error::Malformed("cgroup".into())
        }
    })
}

/// Multi-resource point-in-time snapshot from read-only procfs/cgroupfs interfaces.
#[derive(Debug, Clone)]
pub struct MultiResourceSnapshot {
    pub monotonic: Instant,
    pub collection_ms: u64,
    pub memory_obs: Observation,
    pub cpu_psi: Result<Psi, Error>,
    pub memory_psi: Result<Psi, Error>,
    pub io_psi: Result<Psi, Error>,
    pub cpu: Result<Cpu, Error>,
    pub cgroup_path: Option<PathBuf>,
    pub cgroup_memory_current: Result<u64, Error>,
    pub cgroup_memory_low: Result<u64, Error>,
    pub cgroup_memory_high: Result<u64, Error>,
}

/// Collect multi-resource telemetry from a configurable proc root (e.g. `/proc` or test fixture)
/// and an optional explicit cgroup directory.
pub fn collect_multi_resource_from(
    proc_root: &Path,
    cgroup_path: Option<&Path>,
) -> MultiResourceSnapshot {
    let start = Instant::now();
    let memory_snap = crate::collect_from(proc_root);

    let cpu_psi = read_bounded(&proc_root.join("pressure/cpu")).and_then(|s| parse_psi(&s));
    let memory_psi = match &memory_snap.psi.result {
        Ok(p) => Ok(p.clone()),
        Err(e) => Err(e.clone()),
    };
    let io_psi = read_bounded(&proc_root.join("pressure/io")).and_then(|s| parse_psi(&s));
    let cpu = read_bounded(&proc_root.join("stat")).and_then(|s| parse_cpu(&s));

    let mem = memory_snap.mem.result.as_ref().ok();
    let get_mem = |k: &str| mem.and_then(|m| m.bytes.get(k).copied());

    let memory_obs = Observation {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_millis()).ok()),
        collection_ms: start.elapsed().as_millis() as u64,
        mem_total: get_mem("MemTotal"),
        mem_available: get_mem("MemAvailable"),
        swap_total: get_mem("SwapTotal"),
        swap_free: get_mem("SwapFree"),
        memory_some_10: memory_psi.as_ref().ok().map(|p| p.some.avg10),
        memory_some_60: memory_psi.as_ref().ok().map(|p| p.some.avg60),
        memory_full_10: memory_psi
            .as_ref()
            .ok()
            .and_then(|p| p.full.as_ref().map(|f| f.avg10)),
        io_some_10: io_psi.as_ref().ok().map(|p| p.some.avg10),
        vm: memory_snap.vm.result.ok().map(|v| v.counters),
        disks: None,
        zram: None,
    };

    let (cgroup_memory_current, cgroup_memory_low, cgroup_memory_high) = match cgroup_path {
        Some(cg) => (
            read_u64_cgroup_value(&cg.join("memory.current")),
            read_u64_cgroup_value(&cg.join("memory.low")),
            read_u64_cgroup_value(&cg.join("memory.high")),
        ),
        None => (
            Err(Error::Unsupported),
            Err(Error::Unsupported),
            Err(Error::Unsupported),
        ),
    };

    // Touch parse_meminfo / parse_vmstat imports only via collect_from; keep module lean.
    let _ = (parse_meminfo, parse_vmstat);

    MultiResourceSnapshot {
        monotonic: start,
        collection_ms: start.elapsed().as_millis() as u64,
        memory_obs,
        cpu_psi,
        memory_psi,
        io_psi,
        cpu,
        cgroup_path: cgroup_path.map(Path::to_path_buf),
        cgroup_memory_current,
        cgroup_memory_low,
        cgroup_memory_high,
    }
}

/// Collect multi-resource telemetry from `/proc` and optionally the current process's cgroup.
pub fn collect_multi_resource(cgroup_path: Option<&Path>) -> MultiResourceSnapshot {
    collect_multi_resource_from(Path::new("/proc"), cgroup_path)
}

/// Multi-resource diagnosis preserving explicit source reliability without fabricated fallbacks.
#[derive(Debug, Clone)]
pub struct MultiResourceDiagnosis {
    pub memory_diagnosis: Diagnosis,
    pub cpu_psi_some_10: Option<f64>,
    pub cpu_psi_full_10: Option<f64>,
    pub cpu_utilization_pct: Option<f64>,
    pub io_psi_some_10: Option<f64>,
    pub io_psi_full_10: Option<f64>,
    pub mem_available_bytes: Option<u64>,
    pub mem_total_bytes: Option<u64>,
    pub cgroup_memory_current_bytes: Option<u64>,
    pub cpu_pressure_reliable: bool,
    pub io_pressure_reliable: bool,
    pub memory_pressure_reliable: bool,
    pub telemetry_age_ms: u64,
    pub limitations: Vec<String>,
}

fn validate_psi_val(v: f64) -> Option<f64> {
    (v.is_finite() && (0.0..=100.0).contains(&v)).then_some(v)
}

/// Diagnose multi-resource contention across two sequential snapshots.
pub fn diagnose_multi_resource(
    prev: &MultiResourceSnapshot,
    curr: &MultiResourceSnapshot,
    now: Instant,
    max_age: Duration,
) -> MultiResourceDiagnosis {
    let interval = curr
        .monotonic
        .checked_duration_since(prev.monotonic)
        .unwrap_or(Duration::ZERO);
    let age = now
        .checked_duration_since(curr.monotonic)
        .unwrap_or(Duration::MAX);

    let memory_diagnosis = diagnose(&prev.memory_obs, &curr.memory_obs, interval, age);
    let mut limitations = memory_diagnosis.limitations.clone();

    let stale_or_slow = age > max_age || curr.collection_ms > 5000 || interval.is_zero();
    if age > max_age {
        limitations.push("multi-resource observation exceeded freshness limit".into());
    }

    let cpu_psi_some_10 = curr
        .cpu_psi
        .as_ref()
        .ok()
        .and_then(|p| validate_psi_val(p.some.avg10));
    let cpu_psi_full_10 = curr
        .cpu_psi
        .as_ref()
        .ok()
        .and_then(|p| p.full.as_ref().and_then(|f| validate_psi_val(f.avg10)));
    if let Err(e) = &curr.cpu_psi {
        limitations.push(format!("cpu_psi unavailable: {e}"));
    }

    let io_psi_some_10 = curr
        .io_psi
        .as_ref()
        .ok()
        .and_then(|p| validate_psi_val(p.some.avg10));
    let io_psi_full_10 = curr
        .io_psi
        .as_ref()
        .ok()
        .and_then(|p| p.full.as_ref().and_then(|f| validate_psi_val(f.avg10)));
    if let Err(e) = &curr.io_psi {
        limitations.push(format!("io_psi unavailable: {e}"));
    }

    let cpu_utilization_pct = match (&prev.cpu, &curr.cpu) {
        (Ok(a), Ok(b)) if !interval.is_zero() => match cpu_utilization(a, b, interval) {
            Ok(pct) if pct.is_finite() && (0.0..=100.0).contains(&pct) => Some(pct),
            Ok(_) => {
                limitations.push("cpu utilization out of bounds".into());
                None
            }
            Err(e) => {
                limitations.push(format!("cpu utilization unavailable: {e}"));
                None
            }
        },
        (Err(e), _) | (_, Err(e)) => {
            limitations.push(format!("cpu stat unavailable: {e}"));
            None
        }
        _ => None,
    };

    let mem_total_bytes = curr.memory_obs.mem_total;
    let mem_available_bytes = match (curr.memory_obs.mem_total, curr.memory_obs.mem_available) {
        (Some(total), Some(avail)) if total > 0 && avail <= total => Some(avail),
        (Some(_), Some(_)) => {
            limitations.push("contradictory meminfo: MemAvailable > MemTotal".into());
            None
        }
        _ => None,
    };

    let cpu_pressure_reliable =
        !stale_or_slow && (cpu_psi_some_10.is_some() || cpu_utilization_pct.is_some());
    let io_pressure_reliable = !stale_or_slow && io_psi_some_10.is_some();
    let memory_pressure_reliable =
        !stale_or_slow && memory_diagnosis.reliable && mem_available_bytes.is_some();

    MultiResourceDiagnosis {
        memory_diagnosis,
        cpu_psi_some_10,
        cpu_psi_full_10,
        cpu_utilization_pct,
        io_psi_some_10,
        io_psi_full_10,
        mem_available_bytes,
        mem_total_bytes,
        cgroup_memory_current_bytes: curr.cgroup_memory_current.as_ref().ok().copied(),
        cpu_pressure_reliable,
        io_pressure_reliable,
        memory_pressure_reliable,
        telemetry_age_ms: age.as_millis().min(u64::MAX as u128) as u64,
        limitations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_psi_and_counter_reset_never_fabricates_metrics() {
        let dir = std::env::temp_dir().join(format!("xynoos_piq_multi_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("pressure")).unwrap();

        // Only meminfo and vmstat exist; /proc/pressure/cpu and /proc/pressure/io are missing
        fs::write(
            dir.join("meminfo"),
            "MemTotal: 4096000 kB\nMemAvailable: 2048000 kB\n",
        )
        .unwrap();
        fs::write(
            dir.join("vmstat"),
            "pgfault 100\npgmajfault 1\npswpin 0\npswpout 0\npgscan_direct 0\n",
        )
        .unwrap();
        fs::write(
            dir.join("stat"),
            "cpu 200 0 50 1000 0 0 0 0 0 0\ncpu0 100 0 25 500 0 0 0 0 0 0\n",
        )
        .unwrap();

        let snap1 = collect_multi_resource_from(&dir, None);
        assert!(matches!(snap1.cpu_psi, Err(Error::Unsupported)));
        assert!(matches!(snap1.io_psi, Err(Error::Unsupported)));

        // Simulate CPU counter reset on second snapshot
        fs::write(
            dir.join("stat"),
            "cpu 100 0 10 500 0 0 0 0 0 0\ncpu0 50 0 5 250 0 0 0 0 0 0\n",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(120));
        let snap2 = collect_multi_resource_from(&dir, None);

        let diag = diagnose_multi_resource(&snap1, &snap2, Instant::now(), Duration::from_secs(5));
        assert_eq!(diag.cpu_psi_some_10, None);
        assert_eq!(diag.io_psi_some_10, None);
        assert_eq!(diag.cpu_utilization_pct, None);
        assert!(!diag.cpu_pressure_reliable);
        assert!(!diag.io_pressure_reliable);
        assert!(!diag.memory_pressure_reliable);
        assert!(diag.limitations.iter().any(|l| l.contains("cpu_psi")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn valid_multi_resource_telemetry_computes_exact_signals() {
        let dir = std::env::temp_dir().join(format!("xynoos_piq_ok_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("pressure")).unwrap();

        fs::write(
            dir.join("pressure/cpu"),
            "some avg10=18.50 avg60=12.00 avg300=5.00 total=1000\n",
        )
        .unwrap();
        fs::write(
            dir.join("pressure/memory"),
            "some avg10=4.00 avg60=2.00 avg300=1.00 total=500\nfull avg10=0.50 avg60=0.10 avg300=0.00 total=100\n",
        )
        .unwrap();
        fs::write(
            dir.join("pressure/io"),
            "some avg10=2.25 avg60=1.00 avg300=0.50 total=300\n",
        )
        .unwrap();
        fs::write(
            dir.join("meminfo"),
            "MemTotal: 4096000 kB\nMemAvailable: 1024000 kB\n",
        )
        .unwrap();
        fs::write(
            dir.join("vmstat"),
            "pgfault 100\npgmajfault 1\npswpin 0\npswpout 0\npgscan_direct 0\n",
        )
        .unwrap();
        fs::write(
            dir.join("stat"),
            "cpu 100 0 0 100 0 0 0 0 0 0\ncpu0 100 0 0 100 0 0 0 0 0 0\n",
        )
        .unwrap();

        let snap1 = collect_multi_resource_from(&dir, None);
        std::thread::sleep(Duration::from_millis(120));
        fs::write(
            dir.join("stat"),
            "cpu 150 0 0 150 0 0 0 0 0 0\ncpu0 150 0 0 150 0 0 0 0 0 0\n",
        )
        .unwrap();
        let snap2 = collect_multi_resource_from(&dir, None);

        let diag = diagnose_multi_resource(&snap1, &snap2, Instant::now(), Duration::from_secs(5));
        assert!(diag.cpu_pressure_reliable);
        assert!(diag.io_pressure_reliable);
        assert!(diag.memory_pressure_reliable);
        assert_eq!(diag.cpu_psi_some_10, Some(18.5));
        assert_eq!(diag.io_psi_some_10, Some(2.25));
        assert_eq!(diag.cpu_utilization_pct, Some(50.0));
        assert_eq!(diag.mem_available_bytes, Some(1024000 * 1024));

        let _ = fs::remove_dir_all(&dir);
    }
}
