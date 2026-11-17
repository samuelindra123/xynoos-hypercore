#![forbid(unsafe_code)]
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    Permission,
    Io(String),
    Malformed(String),
    Missing(String),
    Duplicate(String),
    Overflow(String),
    Bounds,
    CounterReset(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
fn num(s: &str, key: &str) -> Result<u64, Error> {
    s.parse::<u64>().map_err(|e| match e.kind() {
        std::num::IntErrorKind::PosOverflow => Error::Overflow(key.into()),
        _ => Error::Malformed(key.into()),
    })
}
#[derive(Debug, Clone, PartialEq)]
pub struct PressureLine {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    pub total_us: u64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Psi {
    pub some: PressureLine,
    pub full: Option<PressureLine>,
}
pub fn parse_psi(input: &str) -> Result<Psi, Error> {
    let mut some = None;
    let mut full = None;
    for line in input.lines() {
        let mut words = line.split_whitespace();
        let Some(kind) = words.next() else { continue };
        if kind != "some" && kind != "full" {
            continue;
        }
        let mut fields = BTreeMap::new();
        for item in words {
            let (k, v) = item
                .split_once('=')
                .ok_or_else(|| Error::Malformed(item.into()))?;
            if matches!(k, "avg10" | "avg60" | "avg300" | "total") && fields.insert(k, v).is_some()
            {
                return Err(Error::Duplicate(k.into()));
            }
        }
        let get = |k: &str| {
            fields
                .get(k)
                .copied()
                .ok_or_else(|| Error::Missing(k.into()))
        };
        let avg = |k: &str| -> Result<f64, Error> {
            let v = get(k)?
                .parse::<f64>()
                .map_err(|_| Error::Malformed(k.into()))?;
            if !v.is_finite() || v.is_sign_negative() || !(0.0..=100.0).contains(&v) {
                return Err(Error::Malformed(k.into()));
            }
            Ok(v)
        };
        let value = PressureLine {
            avg10: avg("avg10")?,
            avg60: avg("avg60")?,
            avg300: avg("avg300")?,
            total_us: num(get("total")?, "total")?,
        };
        let slot = if kind == "some" { &mut some } else { &mut full };
        if slot.replace(value).is_some() {
            return Err(Error::Duplicate(kind.into()));
        }
    }
    Ok(Psi {
        some: some.ok_or_else(|| Error::Missing("some".into()))?,
        full,
    })
}
const MEM_FIELDS: [&str; 12] = [
    "MemTotal",
    "MemFree",
    "MemAvailable",
    "Buffers",
    "Cached",
    "SwapTotal",
    "SwapFree",
    "Active",
    "Inactive",
    "Dirty",
    "Writeback",
    "SReclaimable",
];
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemInfo {
    pub bytes: BTreeMap<String, u64>,
}
pub fn parse_meminfo(input: &str) -> Result<MemInfo, Error> {
    let mut bytes = BTreeMap::new();
    for line in input.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if !MEM_FIELDS.contains(&k) {
            continue;
        }
        let mut fields = v.split_whitespace();
        let raw = fields.next().ok_or_else(|| Error::Missing(k.into()))?;
        if fields.next() != Some("kB") || fields.next().is_some() {
            return Err(Error::Malformed(k.into()));
        }
        let value = num(raw, k)?
            .checked_mul(1024)
            .ok_or_else(|| Error::Overflow(k.into()))?;
        if bytes.insert(k.into(), value).is_some() {
            return Err(Error::Duplicate(k.into()));
        }
    }
    if !bytes.contains_key("MemTotal") {
        return Err(Error::Missing("MemTotal".into()));
    }
    Ok(MemInfo { bytes })
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmStat {
    pub counters: BTreeMap<String, u64>,
}
pub fn parse_vmstat(input: &str) -> Result<VmStat, Error> {
    let mut counters = BTreeMap::new();
    for line in input.lines() {
        let mut f = line.split_whitespace();
        let Some(k) = f.next() else { continue };
        if !(matches!(k, "pgfault" | "pgmajfault" | "pswpin" | "pswpout")
            || k.starts_with("allocstall")
            || k.starts_with("pgscan_")
            || k.starts_with("pgsteal_")
            || k.starts_with("workingset_refault_"))
        {
            continue;
        }
        let v = num(f.next().ok_or_else(|| Error::Missing(k.into()))?, k)?;
        if f.next().is_some() {
            return Err(Error::Malformed(k.into()));
        }
        if counters.insert(k.into(), v).is_some() {
            return Err(Error::Duplicate(k.into()));
        }
    }
    if counters.is_empty() {
        return Err(Error::Missing("vmstat counters".into()));
    }
    Ok(VmStat { counters })
}
pub fn delta(old: &VmStat, new: &VmStat) -> Result<VmStat, Error> {
    let mut counters = BTreeMap::new();
    for (k, v) in &new.counters {
        if let Some(previous) = old.counters.get(k) {
            counters.insert(
                k.clone(),
                v.checked_sub(*previous)
                    .ok_or_else(|| Error::CounterReset(k.clone()))?,
            );
        }
    }
    Ok(VmStat { counters })
}
#[derive(Debug, Clone)]
pub struct Source<T> {
    pub result: Result<T, Error>,
    pub read_ns: u128,
    pub parse_ns: u128,
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub source_root: std::path::PathBuf,
    pub timestamp: SystemTime,
    pub monotonic: Instant,
    pub psi: Source<Psi>,
    pub mem: Source<MemInfo>,
    pub vm: Source<VmStat>,
    pub elapsed_ns: u128,
}
impl Snapshot {
    pub fn partial(&self) -> bool {
        self.psi.result.is_err() || self.mem.result.is_err() || self.vm.result.is_err()
    }
}
fn read(path: &Path) -> Result<String, Error> {
    let mut f = File::open(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::Unsupported,
        io::ErrorKind::PermissionDenied => Error::Permission,
        _ => Error::Io(e.kind().to_string()),
    })?;
    let mut text = String::new();
    f.by_ref()
        .take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| Error::Io(e.kind().to_string()))?;
    if text.len() > 1024 * 1024 {
        return Err(Error::Bounds);
    }
    Ok(text)
}
fn source<T>(path: &Path, parser: fn(&str) -> Result<T, Error>) -> Source<T> {
    let now = Instant::now();
    let data = read(path);
    let read_ns = now.elapsed().as_nanos();
    let now = Instant::now();
    let result = data.and_then(|v| parser(&v));
    Source {
        result,
        read_ns,
        parse_ns: now.elapsed().as_nanos(),
    }
}
pub fn collect_from(root: &Path) -> Snapshot {
    let start = Instant::now();
    let timestamp = SystemTime::now();
    let monotonic = Instant::now();
    let psi = source(&root.join("pressure/memory"), parse_psi);
    let mem = source(&root.join("meminfo"), parse_meminfo);
    let vm = source(&root.join("vmstat"), parse_vmstat);
    Snapshot {
        source_root: root.to_path_buf(),
        timestamp,
        monotonic,
        psi,
        mem,
        vm,
        elapsed_ns: start.elapsed().as_nanos(),
    }
}
pub fn collect() -> Snapshot {
    collect_from(Path::new("/proc"))
}
pub fn validate_sampling(count: u32, interval: Duration) -> Result<(), Error> {
    if !(1..=60).contains(&count)
        || interval < Duration::from_millis(100)
        || interval > Duration::from_secs(60)
    {
        Err(Error::Bounds)
    } else {
        Ok(())
    }
}
pub fn sample<F: FnMut(&Snapshot) -> Result<(), Error>>(
    count: u32,
    interval: Duration,
    mut emit: F,
) -> Result<(), Error> {
    validate_sampling(count, interval)?;
    for i in 0..count {
        if i > 0 {
            std::thread::sleep(interval)
        }
        emit(&collect())?;
    }
    Ok(())
}
pub const CSV_HEADER:&str="timestamp_unix_ms,partial,psi_status,psi_some_avg10_pct,psi_full_avg10_pct,psi_some_total_us,mem_status,mem_total_bytes,mem_available_bytes,vm_status,pgfault,pgmajfault,pswpin,pswpout,read_psi_ns,parse_psi_ns,read_mem_ns,parse_mem_ns,read_vm_ns,parse_vm_ns,snapshot_ns";
fn status<T>(s: &Source<T>) -> &'static str {
    match &s.result {
        Ok(_) => "ok",
        Err(Error::Unsupported) => "unsupported",
        Err(Error::Permission) => "permission",
        Err(_) => "error",
    }
}
fn opt<T: fmt::Display>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}
pub fn csv_row(s: &Snapshot) -> String {
    let psi = s.psi.result.as_ref().ok();
    let mem = s.mem.result.as_ref().ok();
    let vm = s.vm.result.as_ref().ok();
    let mb = |k| mem.and_then(|m| m.bytes.get(k)).copied();
    let vc = |k| vm.and_then(|v| v.counters.get(k)).copied();
    format!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        s.timestamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        s.partial(),
        status(&s.psi),
        opt(psi.map(|p| p.some.avg10)),
        opt(psi.and_then(|p| p.full.as_ref().map(|v| v.avg10))),
        opt(psi.map(|p| p.some.total_us)),
        status(&s.mem),
        opt(mb("MemTotal")),
        opt(mb("MemAvailable")),
        status(&s.vm),
        opt(vc("pgfault")),
        opt(vc("pgmajfault")),
        opt(vc("pswpin")),
        opt(vc("pswpout")),
        s.psi.read_ns,
        s.psi.parse_ns,
        s.mem.read_ns,
        s.mem.parse_ns,
        s.vm.read_ns,
        s.vm.parse_ns,
        s.elapsed_ns
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    const PSI:&str="full total=6 avg300=0.03 avg10=2.50 avg60=1.00 extra=8\nsome avg60=2 avg10=4 avg300=1 total=10  \n";
    #[test]
    fn psi_ok_and_optional() {
        let p = parse_psi(PSI).unwrap();
        assert_eq!(p.some.total_us, 10);
        assert_eq!(p.full.unwrap().avg10, 2.5);
        assert!(parse_psi("some avg10=0 avg60=0 avg300=0 total=0")
            .unwrap()
            .full
            .is_none());
    }
    #[test]
    fn psi_rejects_invalid() {
        for s in [
            "",
            "full avg10=0 avg60=0 avg300=0 total=0",
            "some avg10=NaN avg60=0 avg300=0 total=0",
            "some avg10=inf avg60=0 avg300=0 total=0",
            "some avg10=-1 avg60=0 avg300=0 total=0",
            "some avg10=-0.00 avg60=0 avg300=0 total=0",
            "some avg10=101 avg60=0 avg300=0 total=0",
            "some avg10=0 avg10=0 avg60=0 avg300=0 total=0",
            "some avg10=0 avg60=0 avg300=0 total=18446744073709551616",
            "some avg10=0 avg60=0 total=0",
        ] {
            assert!(parse_psi(s).is_err(), "{s}");
        }
        assert!(parse_psi(
            "some avg10=0 avg60=0 avg300=0 total=0\nsome avg10=0 avg60=0 avg300=0 total=0"
        )
        .is_err());
    }
    #[test]
    fn meminfo_units_and_optional() {
        let m = parse_meminfo("Cached: 2 kB\nMemTotal: 4 kB\nunknown: 9 kB\n").unwrap();
        assert_eq!(m.bytes["MemTotal"], 4096);
        assert!(!m.bytes.contains_key("MemAvailable"));
        for s in [
            "",
            "MemTotal: 4 MB",
            "MemTotal: 18014398509481984 kB",
            "MemTotal: 1 kB\nMemTotal: 2 kB",
        ] {
            assert!(parse_meminfo(s).is_err());
        }
    }
    #[test]
    fn vm_delta_and_reset() {
        let a = parse_vmstat("unknown 2\npgfault 9\npgscan_direct 1\n").unwrap();
        let b = parse_vmstat("pgfault 12\npgscan_direct 4\npgsteal_new 8\n").unwrap();
        let d = delta(&a, &b).unwrap();
        assert_eq!(d.counters["pgfault"], 3);
        assert!(!d.counters.contains_key("pgsteal_new"));
        assert!(matches!(delta(&b, &a), Err(Error::CounterReset(_))));
        assert!(parse_vmstat("pgfault -1").is_err());
        assert!(parse_vmstat("pgfault 1\npgfault 2").is_err());
        assert!(parse_vmstat("pgfault 18446744073709551616").is_err());
    }
    #[test]
    fn partial_and_csv() {
        let s = collect_from(Path::new("/nonexistent-pressureiq-fixture"));
        assert!(s.partial());
        assert_eq!(s.psi.result, Err(Error::Unsupported));
        assert_eq!(
            csv_row(&s).split(',').count(),
            CSV_HEADER.split(',').count()
        );
    }
    #[test]
    fn bounds() {
        assert!(validate_sampling(0, Duration::from_secs(1)).is_err());
        assert!(validate_sampling(61, Duration::from_secs(1)).is_err());
        assert!(validate_sampling(1, Duration::from_millis(99)).is_err());
        assert!(validate_sampling(1, Duration::from_secs(61)).is_err());
        assert!(validate_sampling(60, Duration::from_secs(1)).is_ok());
    }
}
#[derive(Debug, Clone)]
pub struct SnapshotDelta {
    pub interval: Duration,
    pub vm: Option<VmStat>,
    pub psi_some_us: Option<u64>,
    pub psi_full_us: Option<u64>,
}
pub fn snapshot_delta(old: &Snapshot, new: &Snapshot) -> Result<SnapshotDelta, Error> {
    if old.source_root != new.source_root {
        return Err(Error::Malformed("source root mismatch".into()));
    }
    let interval = new
        .monotonic
        .checked_duration_since(old.monotonic)
        .filter(|v| !v.is_zero())
        .ok_or_else(|| Error::Malformed("monotonic interval".into()))?;
    let vm = match (&old.vm.result, &new.vm.result) {
        (Ok(a), Ok(b)) => Some(delta(a, b)?),
        _ => None,
    };
    let (some, full) = match (&old.psi.result, &new.psi.result) {
        (Ok(a), Ok(b)) => {
            let some = b
                .some
                .total_us
                .checked_sub(a.some.total_us)
                .ok_or_else(|| Error::CounterReset("psi some".into()))?;
            let full = match (&a.full, &b.full) {
                (Some(x), Some(y)) => Some(
                    y.total_us
                        .checked_sub(x.total_us)
                        .ok_or_else(|| Error::CounterReset("psi full".into()))?,
                ),
                _ => None,
            };
            (Some(some), full)
        }
        _ => (None, None),
    };
    Ok(SnapshotDelta {
        interval,
        vm,
        psi_some_us: some,
        psi_full_us: full,
    })
}
#[cfg(test)]
mod delta_tests {
    use super::*;
    #[test]
    fn monotonic_and_reset() {
        let a = collect_from(Path::new("/missing-pressureiq"));
        let mut b = a.clone();
        assert!(snapshot_delta(&a, &b).is_err());
        b.monotonic = a.monotonic + Duration::from_millis(250);
        let d = snapshot_delta(&a, &b).unwrap();
        assert_eq!(d.interval, Duration::from_millis(250));
        assert!(d.vm.is_none());
        let mk = |n| Source {
            result: Ok(Psi {
                some: PressureLine {
                    avg10: 0.,
                    avg60: 0.,
                    avg300: 0.,
                    total_us: n,
                },
                full: None,
            }),
            read_ns: 0,
            parse_ns: 0,
        };
        let mut c = a.clone();
        c.psi = mk(20);
        b.psi = mk(10);
        assert!(matches!(
            snapshot_delta(&c, &b),
            Err(Error::CounterReset(_))
        ));
    }
}
pub mod system;

pub mod cgroup_pressure;
pub mod workload;

pub mod intelligence;

pub mod hardware;
