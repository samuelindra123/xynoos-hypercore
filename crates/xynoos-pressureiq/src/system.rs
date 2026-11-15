//! Pembacaan userspace; angka bukan observasi atomik dan bukan kontrol sistem.
use crate::{collect, parse_psi, Error, Psi, Snapshot};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};
static MONOTONIC_ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpu {
    pub ticks: [u64; 8],
    pub logical_cpus: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    pub major: u32,
    pub minor: u32,
    pub name: String,
    pub read_sectors: u64,
    pub write_sectors: u64,
}
#[derive(Debug, Clone)]
pub struct Cgroup {
    pub path: PathBuf,
    pub cpu_usage_us: Result<u64, Error>,
    pub memory_current_bytes: Result<u64, Error>,
    pub io_bytes: Result<(u64, u64), Error>,
}
#[derive(Debug, Clone)]
pub struct Capability {
    pub source: String,
    pub value: Result<String, Error>,
    /// Unit nilai numerik bila atribut dapat diukur.
    pub unit: Option<&'static str>,
    pub numeric: Option<Result<i64, Error>>,
}
#[derive(Debug, Clone)]
pub struct SystemSnapshot {
    pub timestamp: SystemTime,
    pub monotonic: Instant,
    /// Durasi pengumpulan berurutan, dalam nanodetik.
    pub elapsed_ns: u128,
    pub memory: Snapshot,
    pub cpu: Result<Cpu, Error>,
    pub cpu_psi: Result<Psi, Error>,
    pub io_psi: Result<Psi, Error>,
    pub disks: Result<Vec<Disk>, Error>,
    pub cgroup: Result<Cgroup, Error>,
    pub cpu_frequency_khz: Result<u64, Error>,
    pub load_one: Result<f64, Error>,
    pub gpu: Vec<Capability>,
    pub thermal: Vec<Capability>,
    pub power: Vec<Capability>,
}
fn u64v(s: &str) -> Result<u64, Error> {
    s.parse::<u64>().map_err(|e| {
        if matches!(e.kind(), std::num::IntErrorKind::PosOverflow) {
            Error::Overflow("counter".into())
        } else {
            Error::Malformed("counter".into())
        }
    })
}
fn file(p: &Path) -> Result<String, Error> {
    let mut f = File::open(p).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::Unsupported,
        std::io::ErrorKind::PermissionDenied => Error::Permission,
        _ => Error::Io(e.kind().to_string()),
    })?;
    let mut s = String::new();
    f.by_ref()
        .take(1_048_577)
        .read_to_string(&mut s)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                Error::Permission
            } else {
                Error::Io(e.kind().to_string())
            }
        })?;
    if s.len() > 1_048_576 {
        return Err(Error::Bounds);
    }
    Ok(s)
}
pub fn parse_cpu(s: &str) -> Result<Cpu, Error> {
    let mut found = None;
    let mut cpus = 0usize;
    for line in s.lines() {
        let mut f = line.split_whitespace();
        let Some(name) = f.next() else { continue };
        if name == "cpu" {
            if found.is_some() {
                return Err(Error::Duplicate("cpu".into()));
            }
            let vals: Vec<_> = f.take(8).map(u64v).collect::<Result<_, _>>()?;
            if vals.len() < 4 {
                return Err(Error::Missing("cpu ticks".into()));
            }
            let mut ticks = [0; 8];
            ticks[..vals.len()].copy_from_slice(&vals);
            found = Some(ticks);
        } else if name
            .strip_prefix("cpu")
            .is_some_and(|x| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit()))
        {
            cpus = cpus
                .checked_add(1)
                .ok_or_else(|| Error::Overflow("cpu count".into()))?;
        }
    }
    Ok(Cpu {
        ticks: found.ok_or_else(|| Error::Missing("cpu".into()))?,
        logical_cpus: cpus,
    })
}
pub fn cpu_utilization(old: &Cpu, new: &Cpu, interval: Duration) -> Result<f64, Error> {
    if interval.is_zero() {
        return Err(Error::Malformed("interval nol".into()));
    }
    let mut total = 0u64;
    let mut idle = 0u64;
    for i in 0..8 {
        let d = new.ticks[i]
            .checked_sub(old.ticks[i])
            .ok_or_else(|| Error::CounterReset("cpu".into()))?;
        total = total
            .checked_add(d)
            .ok_or_else(|| Error::Overflow("cpu total".into()))?;
        if i == 3 || i == 4 {
            idle = idle
                .checked_add(d)
                .ok_or_else(|| Error::Overflow("cpu idle".into()))?;
        }
    }
    if total == 0 {
        return Err(Error::Malformed("cpu delta nol".into()));
    }
    Ok((total - idle) as f64 * 100.0 / total as f64)
}
pub fn parse_diskstats(s: &str) -> Result<Vec<Disk>, Error> {
    let mut disks = Vec::new();
    for line in s.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        if f.len() < 14 {
            return Err(Error::Malformed("diskstats".into()));
        }
        if disks.len() >= 4096 {
            return Err(Error::Bounds);
        }
        let major = f[0].parse().map_err(|_| Error::Malformed("major".into()))?;
        let minor = f[1].parse().map_err(|_| Error::Malformed("minor".into()))?;
        disks.push(Disk {
            major,
            minor,
            name: f[2].into(),
            read_sectors: u64v(f[5])?,
            write_sectors: u64v(f[9])?,
        });
    }
    if disks.is_empty() {
        return Err(Error::Missing("diskstats".into()));
    }
    Ok(disks)
}
pub fn disk_delta(a: &Disk, b: &Disk) -> Result<(u64, u64), Error> {
    if (a.major, a.minor, &a.name) != (b.major, b.minor, &b.name) {
        return Err(Error::Malformed("disk identity".into()));
    }
    let r = b
        .read_sectors
        .checked_sub(a.read_sectors)
        .ok_or_else(|| Error::CounterReset("disk read".into()))?;
    let w = b
        .write_sectors
        .checked_sub(a.write_sectors)
        .ok_or_else(|| Error::CounterReset("disk write".into()))?;
    Ok((
        r.checked_mul(512)
            .ok_or_else(|| Error::Overflow("disk read bytes".into()))?,
        w.checked_mul(512)
            .ok_or_else(|| Error::Overflow("disk write bytes".into()))?,
    ))
}
pub fn parse_key_values(s: &str) -> Result<BTreeMap<String, u64>, Error> {
    let mut map = BTreeMap::new();
    for line in s.lines() {
        let mut f = line.split_whitespace();
        let Some(k) = f.next() else { continue };
        let v = u64v(f.next().ok_or_else(|| Error::Missing(k.into()))?)?;
        if f.next().is_some() {
            return Err(Error::Malformed(k.into()));
        }
        if map.insert(k.into(), v).is_some() {
            return Err(Error::Duplicate(k.into()));
        }
    }
    Ok(map)
}
pub fn parse_io_stat(s: &str) -> Result<(u64, u64), Error> {
    let (mut read, mut write) = (0u64, 0u64);
    let (mut saw_read, mut saw_write) = (false, false);
    for line in s.lines() {
        let mut f = line.split_whitespace();
        if f.next().is_none() {
            continue;
        }
        for token in f {
            let (k, v) = token
                .split_once('=')
                .ok_or_else(|| Error::Malformed("io.stat".into()))?;
            if k == "rbytes" {
                saw_read = true;
                read = read
                    .checked_add(u64v(v)?)
                    .ok_or_else(|| Error::Overflow("io rbytes".into()))?
            }
            if k == "wbytes" {
                saw_write = true;
                write = write
                    .checked_add(u64v(v)?)
                    .ok_or_else(|| Error::Overflow("io wbytes".into()))?
            }
        }
    }
    if !saw_read || !saw_write {
        return Err(Error::Missing("io.stat rbytes/wbytes".into()));
    }
    Ok((read, write))
}
fn cgroup_path() -> Result<PathBuf, Error> {
    let membership = file(Path::new("/proc/self/cgroup"))?;
    let mountinfo = file(Path::new("/proc/self/mountinfo"))?;
    parse_cgroup_path(&membership, &mountinfo)
}
pub fn parse_cgroup_path(membership: &str, mountinfo: &str) -> Result<PathBuf, Error> {
    let m = membership
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or(Error::Unsupported)?;
    for line in mountinfo.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        if !right.starts_with("cgroup2 ") {
            continue;
        }
        let fields: Vec<_> = left.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        let unescape = |s: &str| {
            s.replace("\\040", " ")
                .replace("\\011", "\t")
                .replace("\\012", "\n")
                .replace("\\134", "\\")
        };
        let mountroot = PathBuf::from(unescape(fields[3]));
        let mount = PathBuf::from(unescape(fields[4]));
        let member = Path::new(m);
        let Ok(relative) = member.strip_prefix(&mountroot) else {
            continue;
        };
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(Error::Malformed("cgroup path".into()));
        }
        return Ok(mount.join(relative));
    }
    Err(Error::Unsupported)
}
fn cgroup() -> Result<Cgroup, Error> {
    let p = cgroup_path()?;
    let cpu_usage_us = file(&p.join("cpu.stat"))
        .and_then(|s| parse_key_values(&s))
        .and_then(|m| {
            m.get("usage_usec")
                .copied()
                .ok_or_else(|| Error::Missing("usage_usec".into()))
        });
    let memory_current_bytes = file(&p.join("memory.current")).and_then(|s| u64v(s.trim()));
    let io_bytes = file(&p.join("io.stat")).and_then(|s| parse_io_stat(&s));
    Ok(Cgroup {
        path: p,
        cpu_usage_us,
        memory_current_bytes,
        io_bytes,
    })
}
fn numeric_attribute(
    attribute: &str,
    value: &Result<String, Error>,
) -> (Option<&'static str>, Option<Result<i64, Error>>) {
    let unit = match attribute {
        "temp" => "millicelsius",
        "capacity" => "percent",
        "power_now" => "microwatt",
        "energy_now" => "microwatt_hour",
        _ => return (None, None),
    };
    let number = value.as_ref().map_err(Clone::clone).and_then(|s| {
        let n = s.parse::<i64>().map_err(|e| {
            if matches!(
                e.kind(),
                std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow
            ) {
                Error::Overflow(attribute.into())
            } else {
                Error::Malformed(attribute.into())
            }
        })?;
        if (attribute != "temp" && n < 0) || (attribute == "capacity" && n > 100) {
            return Err(Error::Malformed(attribute.into()));
        }
        Ok(n)
    });
    (Some(unit), Some(number))
}
fn discover(root: &str, prefix: &str, attribute: &str) -> Vec<Capability> {
    let mut output = Vec::new();
    match std::fs::read_dir(root) {
        Ok(entries) => {
            for (index, e) in entries.take(257).enumerate() {
                if index == 256 {
                    output.push(Capability {
                        source: root.into(),
                        value: Err(Error::Bounds),
                        unit: None,
                        numeric: None,
                    });
                    break;
                }
                match e {
                    Ok(entry) => {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.starts_with(prefix) && (prefix != "card" || !name.contains('-')) {
                            let path = entry.path().join(attribute);
                            let value = file(&path).and_then(|s| {
                                if s.len() > 4096 {
                                    Err(Error::Bounds)
                                } else {
                                    Ok(s.trim().to_owned())
                                }
                            });
                            let (unit, numeric) = numeric_attribute(attribute, &value);
                            output.push(Capability {
                                source: format!("{root}/{name}/{attribute}"),
                                value,
                                unit,
                                numeric,
                            });
                        }
                    }
                    Err(e) => output.push(Capability {
                        source: root.into(),
                        unit: None,
                        numeric: None,
                        value: Err(Error::Io(e.kind().to_string())),
                    }),
                }
            }
        }
        Err(e) => output.push(Capability {
            source: root.into(),
            unit: None,
            numeric: None,
            value: Err(match e.kind() {
                std::io::ErrorKind::PermissionDenied => Error::Permission,
                std::io::ErrorKind::NotFound => Error::Unsupported,
                _ => Error::Io(e.kind().to_string()),
            }),
        }),
    }
    output.sort_by(|a, b| a.source.cmp(&b.source));
    output
}
pub fn collect_system() -> SystemSnapshot {
    MONOTONIC_ORIGIN.get_or_init(Instant::now);
    let timestamp = SystemTime::now();
    let monotonic = Instant::now();
    let memory = collect();
    let cpu = file(Path::new("/proc/stat")).and_then(|s| parse_cpu(&s));
    let cpu_psi = file(Path::new("/proc/pressure/cpu")).and_then(|s| parse_psi(&s));
    let io_psi = file(Path::new("/proc/pressure/io")).and_then(|s| parse_psi(&s));
    let disks = file(Path::new("/proc/diskstats")).and_then(|s| parse_diskstats(&s));
    let cgroup = cgroup();
    let cpu_frequency_khz = file(Path::new(
        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
    ))
    .and_then(|s| u64v(s.trim()));
    let load_one = file(Path::new("/proc/loadavg")).and_then(|s| {
        let x = s
            .split_whitespace()
            .next()
            .ok_or_else(|| Error::Missing("loadavg".into()))?
            .parse::<f64>()
            .map_err(|_| Error::Malformed("loadavg".into()))?;
        if !x.is_finite() || x < 0.0 {
            return Err(Error::Malformed("loadavg".into()));
        }
        Ok(x)
    });
    let gpu = discover("/sys/class/drm", "card", "device/vendor");
    let mut thermal = discover("/sys/class/thermal", "thermal_zone", "type");
    thermal.extend(discover("/sys/class/thermal", "thermal_zone", "temp"));
    let mut power = Vec::new();
    for attr in ["type", "status", "capacity", "power_now", "energy_now"] {
        power.extend(discover("/sys/class/power_supply", "", attr));
    }
    SystemSnapshot {
        timestamp,
        monotonic,
        elapsed_ns: monotonic.elapsed().as_nanos(),
        memory,
        cpu,
        cpu_psi,
        io_psi,
        disks,
        cgroup,
        cpu_frequency_khz,
        load_one,
        gpu,
        thermal,
        power,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_delta_and_resets() {
        let a = parse_cpu("cpu 100 5 25 200 10 1 2 3 400 0\ncpu0 1 2 3 4\ncpu1 1 2 3 4\n").unwrap();
        let b = parse_cpu("cpu 110 5 30 205 10 1 2 3 500 0\ncpu0 1 2 3 4\ncpu1 1 2 3 4").unwrap();
        assert_eq!(a.logical_cpus, 2);
        assert_eq!(
            cpu_utilization(&a, &b, Duration::from_secs(1)).unwrap(),
            75.0
        );
        assert!(cpu_utilization(&a, &a, Duration::ZERO).is_err());
        assert!(matches!(
            cpu_utilization(&b, &a, Duration::from_secs(1)),
            Err(Error::CounterReset(_))
        ));
        assert!(parse_cpu("cpu 1 2 xyz 4").is_err());
        assert!(parse_cpu("cpu 1 2 3 4\ncpu 1 2 3 4").is_err());
    }
    #[test]
    fn disk_units_identity_and_reset() {
        let a = parse_diskstats("8 0 sda 10 0 100 0 10 0 200 0 0 0 0")
            .unwrap()
            .remove(0);
        let mut b = a.clone();
        b.read_sectors += 5;
        b.write_sectors += 2;
        assert_eq!(disk_delta(&a, &b).unwrap(), (2560, 1024));
        assert!(disk_delta(&b, &a).is_err());
        b.name = "other".into();
        assert!(disk_delta(&a, &b).is_err());
        assert!(parse_diskstats("8 0 sda 1").is_err());
    }
    #[test]
    fn cgroup_values() {
        assert_eq!(
            numeric_attribute("temp", &Ok("-500".into()))
                .1
                .unwrap()
                .unwrap(),
            -500
        );
        assert!(numeric_attribute("capacity", &Ok("101".into()))
            .1
            .unwrap()
            .is_err());
        assert!(numeric_attribute("power_now", &Ok("x".into()))
            .1
            .unwrap()
            .is_err());
        assert_eq!(
            parse_cgroup_path(
                "0::/init\n",
                "1 2 0:1 / /sys/fs/cgroup rw - cgroup2 cgroup rw"
            )
            .unwrap(),
            PathBuf::from("/sys/fs/cgroup/init")
        );
        assert!(parse_cgroup_path(
            "0::/../escape",
            "1 2 0:1 / /sys/fs/cgroup rw - cgroup2 cgroup rw"
        )
        .is_err());
        assert!(parse_cgroup_path("1:cpu:/", "").is_err());
        assert_eq!(
            parse_key_values("usage_usec 123\nuser_usec 22\n").unwrap()["usage_usec"],
            123
        );
        assert!(parse_key_values("x 1\nx 2").is_err());
        assert_eq!(
            parse_io_stat("8:0 rbytes=12 wbytes=9\n8:16 rbytes=1 wbytes=2\n").unwrap(),
            (13, 11)
        );
        assert!(parse_io_stat("").is_err());
        assert!(
            parse_io_stat("8:0 rbytes=18446744073709551615 wbytes=1\n8:1 rbytes=1 wbytes=0")
                .is_err()
        );
    }
}

/// Version 1 is additive: consumers must ignore unknown fields and statuses.
/// All observations are sequential and their age is conservatively measured
/// from the beginning of collection, not from an atomic kernel snapshot.
pub mod contract {
    use super::*;
    use serde::Serialize;
    use serde_json::{json, Value};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Status {
        Available,
        Unsupported,
        PermissionDenied,
        ReadError,
        ParseError,
        Stale,
        NotYetSampled,
        Unavailable,
        Truncated,
    }

    #[derive(Debug, Serialize)]
    pub struct Reading {
        pub source: String,
        pub scope: String,
        pub unit: String,
        pub status: Status,
        pub value: Option<Value>,
        pub error: Option<String>,
        pub age_ns: u128,
    }
    #[derive(Debug, Serialize)]
    pub struct Contract {
        pub schema_version: u32,
        pub timestamp_unix_ms: Option<u128>,
        pub collection_duration_ns: u128,
        /// Process-local monotonic offset. It cannot be compared across processes.
        pub monotonic_start_ns: Option<u128>,
        pub sequential: bool,
        pub freshness_limit_ns: u128,
        pub readings: BTreeMap<String, Reading>,
    }
    pub fn status(error: &Error) -> Status {
        match error {
            Error::Unsupported => Status::Unsupported,
            Error::Permission => Status::PermissionDenied,
            Error::Io(_) => Status::ReadError,
            _ => Status::ParseError,
        }
    }
    fn reading<T>(
        source: &str,
        scope: &str,
        unit: &str,
        result: Result<T, Error>,
        age: Duration,
        limit: Duration,
        convert: impl FnOnce(T) -> Value,
    ) -> Reading {
        let (status, value, error) = match result {
            Ok(v) if age > limit => (Status::Stale, Some(convert(v)), None),
            Ok(v) => (Status::Available, Some(convert(v)), None),
            Err(e) => (status(&e), None, Some(e.to_string())),
        };
        Reading {
            source: source.into(),
            scope: scope.into(),
            unit: unit.into(),
            status,
            value,
            error,
            age_ns: age.as_nanos(),
        }
    }
    /// Uses the caller's monotonic clock and threshold; backwards clocks are stale.
    pub fn build(
        s: &SystemSnapshot,
        previous: Option<&SystemSnapshot>,
        disk_name: Option<&str>,
        now: Instant,
        limit: Duration,
    ) -> Contract {
        let age = now
            .checked_duration_since(s.monotonic)
            .unwrap_or(Duration::MAX);
        let mut readings = BTreeMap::new();
        macro_rules! add {
            ($key:expr,$source:expr,$scope:expr,$unit:expr,$result:expr,$convert:expr) => {
                readings.insert(
                    $key.into(),
                    reading($source, $scope, $unit, $result, age, limit, $convert),
                );
            };
        }
        add!(
            "cpu_ticks",
            "/proc/stat",
            "visible_system",
            "USER_HZ ticks",
            s.cpu.clone(),
            |v: Cpu| json!({"ticks":v.ticks,"logical_cpus":v.logical_cpus})
        );
        add!(
            "cpu_psi",
            "/proc/pressure/cpu",
            "visible_system",
            "percent avg10",
            s.cpu_psi.clone(),
            |v: Psi| json!(v.some.avg10)
        );
        add!(
            "io_psi",
            "/proc/pressure/io",
            "visible_system",
            "percent avg10",
            s.io_psi.clone(),
            |v: Psi| json!(v.some.avg10)
        );
        add!(
            "memory_psi",
            "/proc/pressure/memory",
            "visible_system",
            "percent avg10",
            s.memory.psi.result.clone(),
            |v: Psi| json!(v.some.avg10)
        );
        add!(
            "meminfo",
            "/proc/meminfo",
            "visible_system",
            "bytes",
            s.memory.mem.result.clone(),
            |v: crate::MemInfo| json!(v.bytes)
        );
        add!(
            "vmstat",
            "/proc/vmstat",
            "visible_system",
            "kernel counters",
            s.memory.vm.result.clone(),
            |v: crate::VmStat| json!(v.counters)
        );
        let cgpath = s
            .cgroup
            .as_ref()
            .map(|c| c.path.display().to_string())
            .unwrap_or_else(|_| "cgroup2 (unresolved)".into());
        let cg = |f: fn(&Cgroup) -> Result<u64, Error>| {
            s.cgroup.as_ref().map_err(Clone::clone).and_then(f)
        };
        add!(
            "cgroup_cpu",
            &format!("{cgpath}/cpu.stat"),
            "current_cgroup",
            "microseconds",
            cg(|c| c.cpu_usage_us.clone()),
            |v| json!(v)
        );
        add!(
            "cgroup_memory",
            &format!("{cgpath}/memory.current"),
            "current_cgroup",
            "bytes",
            cg(|c| c.memory_current_bytes.clone()),
            |v| json!(v)
        );
        add!(
            "cgroup_io",
            &format!("{cgpath}/io.stat"),
            "current_cgroup",
            "bytes",
            s.cgroup
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|c| c.io_bytes.clone()),
            |v: (u64, u64)| json!({"read":v.0,"write":v.1})
        );
        let selected = || -> Result<Disk, Error> {
            let name = disk_name
                .ok_or_else(|| Error::Missing("explicit disk selection required".into()))?;
            s.disks
                .as_ref()
                .map_err(Clone::clone)?
                .iter()
                .find(|d| d.name == name)
                .cloned()
                .ok_or_else(|| Error::Missing(format!("disk {name}")))
        };
        let disk = selected();
        let kind = disk
            .as_ref()
            .ok()
            .map(|d| {
                let path = PathBuf::from(format!("/sys/dev/block/{}:{}", d.major, d.minor));
                match std::fs::canonicalize(path) {
                    Ok(p) if p.starts_with("/sys/devices/virtual") => "virtual",
                    Ok(p) if p.starts_with("/sys/devices/") => "device_backed",
                    _ => "unknown",
                }
            })
            .unwrap_or("unknown");
        add!(
            "disk",
            "/proc/diskstats",
            "container_visible_devices",
            "512-byte sectors",
            disk.clone(),
            |v: Disk| json!({"name":v.name,"major":v.major,"minor":v.minor,"read_sectors":v.read_sectors,"write_sectors":v.write_sectors,"sysfs_class":kind})
        );
        if disk_name.is_some() && matches!(disk, Err(Error::Missing(_))) {
            readings.get_mut("disk").unwrap().status = Status::Unavailable;
        }
        let mut delta = reading(
            "/proc/diskstats",
            "container_visible_devices",
            "bytes",
            disk.clone().and_then(|d| {
                let p = previous.ok_or_else(|| Error::Missing("previous sample".into()))?;
                let elapsed = s
                    .monotonic
                    .checked_duration_since(p.monotonic)
                    .filter(|d| !d.is_zero())
                    .ok_or_else(|| Error::Malformed("sample interval".into()))?;
                if now
                    .checked_duration_since(p.monotonic)
                    .is_none_or(|a| a > limit)
                    || elapsed > limit
                {
                    return Err(Error::Malformed("previous sample stale".into()));
                }
                let old = p
                    .disks
                    .as_ref()
                    .map_err(Clone::clone)?
                    .iter()
                    .find(|x| x.name == d.name)
                    .ok_or_else(|| Error::Missing("previous disk".into()))?;
                disk_delta(old, &d)
            }),
            age,
            limit,
            |v: (u64, u64)| json!({"read":v.0,"write":v.1}),
        );
        if previous.is_none() && disk.is_ok() {
            delta.status = Status::NotYetSampled;
            delta.error = None;
        }
        if previous.is_some_and(|p| {
            now.checked_duration_since(p.monotonic)
                .is_none_or(|a| a > limit)
        }) && disk.is_ok()
        {
            delta.status = Status::Stale;
            delta.value = None;
            delta.error = Some("previous sample stale".into());
        }
        readings.insert("disk_delta".into(), delta);
        if disk_name.is_some() && matches!(disk, Err(Error::Missing(_))) {
            readings.get_mut("disk_delta").unwrap().status = Status::Unavailable;
        }
        if disk_name.is_none() {
            for name in ["disk", "disk_delta"] {
                let r = readings.get_mut(name).unwrap();
                r.status = Status::Unavailable;
            }
        }
        let cpu_delta = previous.map(|p| {
            let elapsed = s
                .monotonic
                .checked_duration_since(p.monotonic)
                .ok_or_else(|| Error::Malformed("sample interval".into()))?;
            cpu_utilization(
                p.cpu.as_ref().map_err(Clone::clone)?,
                s.cpu.as_ref().map_err(Clone::clone)?,
                elapsed,
            )
        });
        let mut cpu_rate = reading(
            "/proc/stat",
            "visible_system",
            "percent",
            cpu_delta.unwrap_or_else(|| {
                s.cpu
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|_| Err(Error::Missing("previous sample".into())))
            }),
            age,
            limit,
            |v| json!(v),
        );
        if previous.is_none() && s.cpu.is_ok() {
            cpu_rate.status = Status::NotYetSampled;
            cpu_rate.error = None;
        }
        if s.cpu.is_ok()
            && previous.is_some_and(|p| {
                now.checked_duration_since(p.monotonic)
                    .is_none_or(|a| a > limit)
            })
        {
            cpu_rate.status = Status::Stale;
            cpu_rate.value = None;
            cpu_rate.error = Some("previous sample stale".into());
        }
        readings.insert("cpu_utilization".into(), cpu_rate);
        for (group, caps) in [
            ("gpu", &s.gpu),
            ("thermal", &s.thermal),
            ("power", &s.power),
        ] {
            let root = match group {
                "gpu" => "/sys/class/drm",
                "thermal" => "/sys/class/thermal",
                _ => "/sys/class/power_supply",
            };
            let mut summary = reading(
                root,
                "visible_devices",
                "attributes",
                Ok(caps.len()),
                age,
                limit,
                |v| json!(v),
            );
            if let Some(failure) = caps.iter().find(|c| c.source == root && c.value.is_err()) {
                let error = failure.value.as_ref().unwrap_err();
                summary.status = if matches!(error, Error::Bounds) {
                    Status::Truncated
                } else {
                    status(error)
                };
                summary.error = Some(error.to_string());
            }
            readings.insert(format!("{group}_discovery"), summary);
            for (i, cap) in caps.iter().enumerate() {
                let result = match &cap.numeric {
                    Some(number) => number.clone().map(|n| json!(n)),
                    None => cap.value.clone().map(|s| json!(s)),
                };
                add!(
                    &format!("{group}_{i}"),
                    &cap.source,
                    "visible_devices",
                    cap.unit.unwrap_or("text"),
                    result,
                    |v| v
                );
                if matches!(cap.value, Err(Error::Bounds))
                    && [
                        "/sys/class/drm",
                        "/sys/class/thermal",
                        "/sys/class/power_supply",
                    ]
                    .contains(&cap.source.as_str())
                {
                    readings.get_mut(&format!("{group}_{i}")).unwrap().status = Status::Truncated;
                }
            }
        }
        Contract {
            schema_version: 1,
            timestamp_unix_ms: s
                .timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis()),
            collection_duration_ns: s.elapsed_ns,
            monotonic_start_ns: MONOTONIC_ORIGIN
                .get()
                .and_then(|origin| s.monotonic.checked_duration_since(*origin))
                .map(|d| d.as_nanos()),
            sequential: true,
            freshness_limit_ns: limit.as_nanos(),
            readings,
        }
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use contract::{build, Status};
    #[test]
    fn freshness_and_readiness_preserve_errors_and_identity() {
        let mut a = collect_system();
        a.cpu = Ok(parse_cpu("cpu 10 0 0 10\ncpu0 1 1 1 1").unwrap());
        a.disks = Ok(vec![parse_diskstats("8 0 sda 1 0 10 0 1 0 20 0 0 0 0")
            .unwrap()
            .remove(0)]);
        let first = build(&a, None, Some("sda"), a.monotonic, Duration::from_secs(2));
        assert_eq!(first.readings["disk_delta"].status, Status::NotYetSampled);
        assert_eq!(
            first.readings["cpu_utilization"].status,
            Status::NotYetSampled
        );
        assert_eq!(
            build(&a, None, None, a.monotonic, Duration::from_secs(1)).readings["disk"].status,
            Status::Unavailable
        );
        assert_eq!(
            build(
                &a,
                None,
                Some("sda"),
                a.monotonic + Duration::from_secs(3),
                Duration::from_secs(2)
            )
            .readings["disk"]
                .status,
            Status::Stale
        );
        let mut b = a.clone();
        b.monotonic += Duration::from_secs(1);
        b.disks.as_mut().unwrap()[0].minor = 1;
        assert_eq!(
            build(
                &b,
                Some(&a),
                Some("sda"),
                b.monotonic,
                Duration::from_secs(2)
            )
            .readings["disk_delta"]
                .status,
            Status::ParseError
        );
        b.cpu = Err(Error::Permission);
        assert_eq!(
            build(&b, None, None, b.monotonic, Duration::from_secs(2)).readings["cpu_utilization"]
                .status,
            Status::PermissionDenied
        );
        b.cpu = a.cpu.clone();
        b.monotonic = a.monotonic;
        assert_eq!(
            build(
                &b,
                Some(&a),
                Some("sda"),
                b.monotonic,
                Duration::from_secs(2)
            )
            .readings["cpu_utilization"]
                .status,
            Status::ParseError
        );
    }
    #[test]
    fn discovery_preserves_partial_and_marks_truncation() {
        let root = std::env::temp_dir().join(format!("xynoos-discovery-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..257 {
            std::fs::create_dir(root.join(format!("item{i}"))).unwrap();
        }
        let found = discover(root.to_str().unwrap(), "item", "type");
        assert_eq!(found.len(), 257);
        assert!(found.iter().any(|c| matches!(c.value, Err(Error::Bounds))));
        std::fs::remove_dir_all(root).unwrap();
        assert!(matches!(
            discover("/definitely/missing/xynoos", "", "type")[0].value,
            Err(Error::Unsupported)
        ));
    }
    #[cfg(unix)]
    #[test]
    fn filesystem_permission_and_missing_are_distinct() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("xynoos-private-{}", std::process::id()));
        std::fs::write(&path, "private").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
        let result = file(&path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::remove_file(&path).unwrap();
        if std::env::var("USER").ok().as_deref() != Some("root") {
            assert!(matches!(result, Err(Error::Permission)));
        }
        assert!(matches!(file(&path), Err(Error::Unsupported)));
    }
}
