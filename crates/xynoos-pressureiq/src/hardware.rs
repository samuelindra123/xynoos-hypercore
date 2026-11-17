//! Bounded, read-only sysfs adapters. A snapshot is sequential and namespace visible.
use serde::Serialize;
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const MAX_ENTRIES: usize = 256;
const MAX_TEXT: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Available,
    Unsupported,
    PermissionDenied,
    TemporarilyUnavailable,
    Stale,
    Malformed,
    CounterReset,
    HardwareRemoved,
    PartialCollection,
}
#[derive(Debug, Clone, Serialize)]
pub struct Reading {
    pub source: String,
    pub unit: &'static str,
    pub status: Status,
    pub value: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Device {
    pub identity: String,
    pub readings: Vec<Reading>,
}
#[derive(Debug, Serialize)]
pub struct Hardware {
    pub timestamp_unix_ms: Option<u128>,
    pub duration_ns: u128,
    pub cpu: Vec<Device>,
    pub gpu: Vec<Device>,
    pub stability: Vec<Device>,
}
fn status(e: &io::Error) -> Status {
    match e.kind() {
        io::ErrorKind::NotFound => Status::Unsupported,
        io::ErrorKind::PermissionDenied => Status::PermissionDenied,
        io::ErrorKind::InvalidData => Status::Malformed,
        _ => Status::TemporarilyUnavailable,
    }
}
fn failed(p: &Path, unit: &'static str, status: Status) -> Reading {
    Reading {
        source: p.display().to_string(),
        unit,
        status,
        value: None,
    }
}
fn read(p: &Path, unit: &'static str) -> Reading {
    let result = (|| {
        let mut s = String::new();
        fs::File::open(p)?
            .take(MAX_TEXT + 1)
            .read_to_string(&mut s)?;
        if s.len() as u64 > MAX_TEXT || s.trim().is_empty() {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        Ok(s.trim().to_string())
    })();
    match result {
        Ok(value) => Reading {
            source: p.display().to_string(),
            unit,
            status: Status::Available,
            value: Some(value),
        },
        Err(e) => failed(p, unit, status(&e)),
    }
}
fn numeric(p: &Path, unit: &'static str, signed: bool, max: Option<u64>) -> Reading {
    let mut r = read(p, unit);
    if let Some(v) = &r.value {
        let valid = if signed {
            v.parse::<i64>().is_ok()
        } else {
            v.parse::<u64>().is_ok_and(|n| max.is_none_or(|m| n <= m))
        };
        if !valid {
            r.status = Status::Malformed;
            r.value = None;
        }
    }
    r
}
fn numbered(s: &str, prefix: &str) -> bool {
    s.strip_prefix(prefix)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}
fn devices(
    root: &Path,
    accept: impl Fn(&str) -> bool,
    build: impl Fn(PathBuf) -> Device,
) -> Vec<Device> {
    let mut paths = Vec::new();
    let mut errors = Vec::new();
    match fs::read_dir(root) {
        Err(e) => errors.push(failed(root, "discovery", status(&e))),
        Ok(entries) => {
            for (i, e) in entries.take(MAX_ENTRIES + 1).enumerate() {
                if i == MAX_ENTRIES {
                    errors.push(failed(root, "discovery", Status::PartialCollection));
                    break;
                }
                match e {
                    Ok(e) if accept(&e.file_name().to_string_lossy()) => paths.push(e.path()),
                    Ok(_) => {}
                    Err(e) => errors.push(failed(root, "discovery", status(&e))),
                }
            }
        }
    }
    paths.sort();
    let mut out: Vec<_> = paths.into_iter().map(build).collect();
    if !errors.is_empty() {
        out.push(Device {
            identity: root.display().to_string(),
            readings: errors,
        });
    }
    out
}
fn device(p: &Path, readings: Vec<Reading>) -> Device {
    Device {
        identity: p.display().to_string(),
        readings,
    }
}
/// Alternate roots support fixtures. Production callers use /sys; no subprocesses or writes.
pub fn collect(root: &Path) -> Hardware {
    let start = Instant::now();
    let cpu_root = root.join("devices/system/cpu");
    let mut cpu = devices(
        &cpu_root,
        |n| numbered(n, "cpu"),
        |p| {
            device(
                &p,
                vec![
                    read(&p.join("topology/core_id"), "identifier"),
                    read(&p.join("topology/physical_package_id"), "identifier"),
                    read(&p.join("topology/thread_siblings_list"), "cpu_list"),
                    numeric(
                        &p.join("thermal_throttle/core_throttle_count"),
                        "events",
                        false,
                        None,
                    ),
                    numeric(
                        &p.join("thermal_throttle/package_throttle_count"),
                        "events",
                        false,
                        None,
                    ),
                ],
            )
        },
    );
    cpu.extend(devices(
        &cpu_root.join("cpufreq"),
        |n| numbered(n, "policy"),
        |p| {
            let mut rs = vec![];
            for (name, unit) in [
                ("scaling_driver", "driver"),
                ("scaling_governor", "governor"),
                ("scaling_available_governors", "governor_list"),
                ("related_cpus", "cpu_list"),
                ("scaling_available_frequencies", "kHz_list"),
            ] {
                rs.push(read(&p.join(name), unit));
            }
            for name in [
                "scaling_cur_freq",
                "cpuinfo_cur_freq",
                "cpuinfo_min_freq",
                "cpuinfo_max_freq",
            ] {
                rs.push(numeric(&p.join(name), "kHz", false, None));
            }
            device(&p, rs)
        },
    ));
    let gpu = devices(
        &root.join("class/drm"),
        |n| numbered(n, "card"),
        |p| {
            let vendor = read(&p.join("device/vendor"), "pci_vendor_id");
            let driver_path = p.join("device/driver");
            let driver = match fs::read_link(&driver_path) {
                Ok(link) => Reading {
                    source: driver_path.display().to_string(),
                    unit: "driver",
                    status: Status::Available,
                    value: link.file_name().map(|s| s.to_string_lossy().into_owned()),
                },
                Err(e) => failed(&driver_path, "driver", status(&e)),
            };
            let mut rs = vec![read(&p.join("device/device"), "pci_device_id"), driver];
            match vendor.value.as_deref() {
                Some("0x8086") => {
                    for name in ["gt_cur_freq_mhz", "gt_min_freq_mhz", "gt_max_freq_mhz"] {
                        rs.push(numeric(&p.join(name), "MHz", false, None));
                    }
                    // i915 busy requires PMU/perf sampling, not frequency inference.
                    rs.push(failed(
                        &p.join("device/gpu_busy_percent"),
                        "percent",
                        Status::Unsupported,
                    ));
                    rs.push(failed(
                        &p.join("device/mem_info_vram_used"),
                        "bytes",
                        Status::Unsupported,
                    ));
                }
                Some("0x1002") => {
                    rs.push(numeric(
                        &p.join("device/gpu_busy_percent"),
                        "percent",
                        false,
                        Some(100),
                    ));
                    for name in ["mem_info_vram_used", "mem_info_vram_total"] {
                        rs.push(numeric(&p.join("device").join(name), "bytes", false, None));
                    }
                }
                _ => rs.push(failed(
                    &p.join("device/gpu_busy_percent"),
                    "percent",
                    Status::Unsupported,
                )),
            }
            rs.push(vendor);
            device(&p, rs)
        },
    );
    let mut stability = devices(
        &root.join("class/thermal"),
        |n| numbered(n, "thermal_zone"),
        |p| {
            device(
                &p,
                vec![
                    read(&p.join("type"), "sensor_label"),
                    numeric(&p.join("temp"), "millicelsius", true, None),
                ],
            )
        },
    );
    stability.extend(devices(
        &root.join("class/hwmon"),
        |n| numbered(n, "hwmon"),
        |p| {
            let mut readings = vec![read(&p.join("name"), "sensor_label")];
            let sensors = devices(
                &p,
                |n| {
                    n.strip_suffix("_input")
                        .is_some_and(|s| numbered(s, "temp"))
                },
                |f| device(&f, vec![numeric(&f, "millicelsius", true, None)]),
            );
            for sensor in sensors {
                readings.extend(sensor.readings);
            }
            device(&p, readings)
        },
    ));
    stability.extend(devices(
        &root.join("class/power_supply"),
        |_| true,
        |p| {
            let mut rs = vec![
                read(&p.join("type"), "supply_type"),
                read(&p.join("status"), "supply_status"),
            ];
            for (name, unit, max) in [
                ("online", "boolean", Some(1)),
                ("capacity", "percent", Some(100)),
                ("power_now", "microwatt", None),
                ("energy_now", "microwatt_hour", None),
            ] {
                rs.push(numeric(&p.join(name), unit, false, max));
            }
            device(&p, rs)
        },
    ));
    Hardware {
        timestamp_unix_ms: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis()),
        duration_ns: start.elapsed().as_nanos(),
        cpu,
        gpu,
        stability,
    }
}
/// Lifecycle comparison must use the same source identity, never a replacement device.
pub fn counter_delta(a: &Reading, b: &Reading) -> Result<u64, Status> {
    if a.source != b.source || a.unit != b.unit {
        return Err(Status::HardwareRemoved);
    }
    if a.status != Status::Available {
        return Err(a.status);
    }
    if b.status != Status::Available {
        return Err(b.status);
    }
    let number = |r: &Reading| {
        r.value
            .as_ref()
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or(Status::Malformed)
    };
    number(b)?
        .checked_sub(number(a)?)
        .ok_or(Status::CounterReset)
}
pub fn freshness(collected: Instant, now: Instant, limit: Duration) -> Status {
    match now.checked_duration_since(collected) {
        Some(age) if age <= limit => Status::Available,
        _ => Status::Stale,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn errors_and_resets() {
        assert_eq!(
            status(&io::Error::from(io::ErrorKind::PermissionDenied)),
            Status::PermissionDenied
        );
        let a = Reading {
            source: "cpu0".into(),
            unit: "events",
            value: Some("9".into()),
            status: Status::Available,
        };
        let mut b = a.clone();
        b.value = Some("8".into());
        assert_eq!(counter_delta(&a, &b), Err(Status::CounterReset));
        b.source = "cpu1".into();
        assert_eq!(counter_delta(&a, &b), Err(Status::HardwareRemoved));
        let now = Instant::now();
        assert_eq!(
            freshness(now, now + Duration::from_secs(6), Duration::from_secs(5)),
            Status::Stale
        );
    }
    #[test]
    fn synthetic_gpu_and_cpu() {
        let root = std::env::temp_dir().join(format!("xynoos-hardware-{}", std::process::id()));
        let card = root.join("class/drm/card0/device");
        fs::create_dir_all(&card).unwrap();
        fs::write(card.join("vendor"), "0x1002").unwrap();
        fs::write(card.join("gpu_busy_percent"), "101").unwrap();
        let h = collect(&root);
        assert_eq!(h.gpu.len(), 1);
        assert!(h.gpu[0]
            .readings
            .iter()
            .any(|r| r.unit == "percent" && r.status == Status::Malformed));
        assert_eq!(h.cpu[0].readings[0].status, Status::Unsupported);
        fs::write(card.join("vendor"), "0x8086").unwrap();
        fs::write(card.join("gpu_busy_percent"), "73").unwrap();
        let h = collect(&root);
        assert!(h.gpu[0]
            .readings
            .iter()
            .any(|r| r.unit == "percent" && r.status == Status::Unsupported && r.value.is_none()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn bounds_numeric_and_partial_discovery() {
        let root = std::env::temp_dir().join(format!("xynoos-bounds-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let input = root.join("reading");
        for text in ["", "NaN", "-1", "18446744073709551616"] {
            fs::write(&input, text).unwrap();
            assert_eq!(
                numeric(&input, "events", false, None).status,
                Status::Malformed
            );
        }
        fs::write(&input, "-1000").unwrap();
        assert_eq!(
            numeric(&input, "millicelsius", true, None).value.as_deref(),
            Some("-1000")
        );
        fs::write(&input, "a".repeat(4097)).unwrap();
        assert_eq!(read(&input, "text").status, Status::Malformed);
        for n in 0..257 {
            fs::write(root.join(format!("cpu{n}")), "1").unwrap();
        }
        let found = devices(&root, |n| numbered(n, "cpu"), |p| device(&p, vec![]));
        assert!(found
            .iter()
            .flat_map(|d| &d.readings)
            .any(|r| r.status == Status::PartialCollection));
        fs::remove_dir_all(root).unwrap();
    }
}
