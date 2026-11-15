//! Pembacaan userspace; angka bukan observasi atomik dan bukan kontrol sistem.
use crate::{collect, parse_psi, Error, Psi, Snapshot};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};
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
        .map_err(|e| Error::Io(e.kind().to_string()))?;
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
            for e in entries.take(256) {
                match e {
                    Ok(entry) => {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.starts_with(prefix) && (prefix != "card" || !name.contains('-')) {
                            let path = entry.path().join(attribute);
                            let value = file(&path).map(|s| s.trim().to_owned());
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
            value: Err(if e.kind() == std::io::ErrorKind::PermissionDenied {
                Error::Permission
            } else {
                Error::Unsupported
            }),
        }),
    }
    output.sort_by(|a, b| a.source.cmp(&b.source));
    output
}
pub fn collect_system() -> SystemSnapshot {
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
