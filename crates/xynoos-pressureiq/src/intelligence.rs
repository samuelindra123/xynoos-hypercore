//! Read-only, bounded memory observations and deterministic interpretation.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Zram {
    pub name: String,
    /// Kernel mm_stat: original bytes, compressed bytes, allocator bytes.
    pub mm_stat: Option<Vec<u64>>,
    pub io_stat: Option<Vec<u64>>,
    pub stat: Option<Vec<u64>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub timestamp_ms: Option<u64>,
    pub collection_ms: u64,
    pub mem_total: Option<u64>,
    pub mem_available: Option<u64>,
    pub swap_total: Option<u64>,
    pub swap_free: Option<u64>,
    pub memory_some_10: Option<f64>,
    pub memory_some_60: Option<f64>,
    pub memory_full_10: Option<f64>,
    pub io_some_10: Option<f64>,
    pub vm: Option<BTreeMap<String, u64>>,
    pub disks: Option<BTreeMap<String, (u64, u64)>>,
    /// None means discovery failed; an empty list means discovery succeeded with no devices.
    pub zram: Option<Vec<Zram>>,
}
fn read(path: &str) -> Option<String> {
    let mut s = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(1_048_577)
        .read_to_string(&mut s)
        .ok()?;
    (s.len() <= 1_048_576).then_some(s)
}
fn numbers(path: &str, minimum: usize) -> Option<Vec<u64>> {
    let text = read(path)?;
    let values: Vec<u64> = text
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (values.len() >= minimum && values.len() <= 64).then_some(values)
}
pub fn collect() -> Observation {
    let start = Instant::now();
    let m = crate::collect();
    let mem = m.mem.result.as_ref().ok();
    let psi = m.psi.result.as_ref().ok();
    let io = read("/proc/pressure/io").and_then(|s| crate::parse_psi(&s).ok());
    let disks = read("/proc/diskstats")
        .and_then(|s| crate::system::parse_diskstats(&s).ok())
        .map(|ds| {
            ds.into_iter()
                .map(|d| {
                    (
                        format!("{}:{}:{}", d.major, d.minor, d.name),
                        (d.read_sectors, d.write_sectors),
                    )
                })
                .collect()
        });
    let zram = std::fs::read_dir("/sys/block").ok().and_then(|entries| {
        let mut out = Vec::new();
        for (index, e) in entries.take(4097).enumerate() {
            if index == 4096 {
                return None;
            }
            let e = e.ok()?;
            let name = e.file_name().into_string().ok()?;
            if name
                .strip_prefix("zram")
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            {
                let base = format!("/sys/block/{name}");
                out.push(Zram {
                    name,
                    mm_stat: numbers(&format!("{base}/mm_stat"), 3),
                    io_stat: numbers(&format!("{base}/io_stat"), 4),
                    stat: numbers(&format!("{base}/stat"), 11),
                });
            }
            if out.len() > 256 {
                return None;
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Some(out)
    });
    let get = |key| mem.and_then(|m| m.bytes.get(key).copied());
    Observation {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_millis()).ok()),
        collection_ms: start.elapsed().as_millis() as u64,
        mem_total: get("MemTotal"),
        mem_available: get("MemAvailable"),
        swap_total: get("SwapTotal"),
        swap_free: get("SwapFree"),
        memory_some_10: psi.map(|p| p.some.avg10),
        memory_some_60: psi.map(|p| p.some.avg60),
        memory_full_10: psi.and_then(|p| p.full.as_ref().map(|f| f.avg10)),
        io_some_10: io.map(|p| p.some.avg10),
        vm: m.vm.result.ok().map(|v| v.counters),
        disks,
        zram,
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    pub classes: Vec<String>,
    pub evidence: Vec<String>,
    pub limitations: Vec<String>,
    pub available_pct: Option<f64>,
    pub rates_per_second: BTreeMap<String, Option<f64>>,
    pub disk_bytes_per_second: BTreeMap<String, Option<(f64, f64)>>,
    pub zram_ratios: BTreeMap<String, Option<(f64, f64)>>,
    pub interval_seconds: Option<f64>,
    pub reliable: bool,
}
/// `age` must be measured by the caller; timestamps alone cannot establish freshness.
pub fn diagnose(a: &Observation, b: &Observation, interval: Duration, age: Duration) -> Diagnosis {
    let seconds = interval.as_secs_f64();
    let valid_interval = (0.1..=60.0).contains(&seconds);
    let valid_psi = [
        b.memory_some_10,
        b.memory_some_60,
        b.memory_full_10,
        b.io_some_10,
    ]
    .into_iter()
    .flatten()
    .all(|v| v.is_finite() && (0.0..=100.0).contains(&v));
    let mut d = Diagnosis {
        classes: vec![],
        evidence: vec![],
        limitations: vec![],
        available_pct: None,
        rates_per_second: BTreeMap::new(),
        disk_bytes_per_second: BTreeMap::new(),
        zram_ratios: BTreeMap::new(),
        interval_seconds: valid_interval.then_some(seconds),
        reliable: true,
    };
    if !valid_interval
        || !valid_psi
        || age > Duration::from_secs(5)
        || a.collection_ms > 5000
        || b.collection_ms > 5000
    {
        d.limitations
            .push("stale observation or invalid sampling interval".into());
        d.reliable = false;
    }
    if let (Some(t), Some(v)) = (b.mem_total, b.mem_available) {
        if t > 0 && v <= t {
            d.available_pct = Some(v as f64 * 100.0 / t as f64);
        }
    }
    for key in [
        "pgscan_direct",
        "pgsteal_direct",
        "pgscan_kswapd",
        "pgsteal_kswapd",
        "pswpin",
        "pswpout",
    ] {
        let rate =
            a.vm.as_ref()
                .and_then(|v| v.get(key))
                .zip(b.vm.as_ref().and_then(|v| v.get(key)))
                .and_then(|(x, y)| y.checked_sub(*x))
                .filter(|_| valid_interval)
                .map(|v| v as f64 / seconds);
        if rate.is_none() {
            d.limitations
                .push(format!("{key}: unavailable or counter reset"));
        }
        d.rates_per_second.insert(key.into(), rate);
    }
    // allocstall is split by zone on many kernels; aggregate only matching complete key sets.
    let alloc =
        a.vm.as_ref()
            .zip(b.vm.as_ref())
            .and_then(|(x, y)| {
                let keys: Vec<_> = x.keys().filter(|k| k.starts_with("allocstall")).collect();
                if keys.is_empty()
                    || keys.len() != y.keys().filter(|k| k.starts_with("allocstall")).count()
                {
                    return None;
                }
                keys.into_iter()
                    .try_fold(0u64, |sum, k| sum.checked_add(y.get(k)?.checked_sub(x[k])?))
            })
            .filter(|_| valid_interval)
            .map(|v| v as f64 / seconds);
    d.rates_per_second.insert("allocstall".into(), alloc);
    if alloc.is_none() {
        d.limitations
            .push("allocstall: unavailable or counter reset".into());
    }
    if let (Some(x), Some(y)) = (&a.disks, &b.disks) {
        for (k, (r, w)) in y {
            let rate = x
                .get(k)
                .and_then(|(or, ow)| {
                    Some((
                        r.checked_sub(*or)? as f64 * 512.0 / seconds,
                        w.checked_sub(*ow)? as f64 * 512.0 / seconds,
                    ))
                })
                .filter(|_| valid_interval);
            d.disk_bytes_per_second.insert(k.clone(), rate);
        }
    } else {
        d.limitations
            .push("block device statistics unavailable".into());
    }
    if let Some(zs) = &b.zram {
        for z in zs {
            let ratio = z.mm_stat.as_ref().and_then(|v| {
                if v.len() >= 3 && v[1] > 0 && v[2] > 0 {
                    Some((v[0] as f64 / v[1] as f64, v[0] as f64 / v[2] as f64))
                } else {
                    None
                }
            });
            d.zram_ratios.insert(z.name.clone(), ratio);
        }
    } else {
        d.limitations.push("ZRAM discovery unavailable".into());
    }
    let rate = |k: &str| d.rates_per_second.get(k).copied().flatten();
    let direct = rate("pgscan_direct").is_some_and(|v| v > 0.0) || alloc.is_some_and(|v| v > 0.0);
    let swap = rate("pswpin")
        .zip(rate("pswpout"))
        .is_some_and(|(i, o)| i >= 10.0 && o >= 10.0);
    let psi_at_least = |value: Option<f64>, threshold: f64| {
        value.is_some_and(|v| v.is_finite() && (threshold..=100.0).contains(&v))
    };
    let stalled = psi_at_least(b.memory_some_10, 1.0) || psi_at_least(b.memory_full_10, 0.2);
    let reclaim = direct || rate("pgscan_kswapd").is_some_and(|v| v > 0.0);
    if d.available_pct.is_none()
        || b.memory_some_10.is_none()
        || rate("pgscan_direct").is_none()
        || rate("pswpin").is_none()
        || rate("pswpout").is_none()
    {
        d.reliable = false;
        d.limitations
            .push("core memory, PSI or counter evidence incomplete".into());
    }
    if direct {
        d.classes.push("active_direct_reclaim".into());
        d.evidence
            .push("direct scan or allocation stall counter increased".into());
    }
    if rate("pswpin").is_some_and(|v| v > 0.0) || rate("pswpout").is_some_and(|v| v > 0.0) {
        d.classes.push("swap_activity".into());
        d.evidence
            .push("swap page counters increased during the interval".into());
    }
    if swap && stalled {
        d.classes.push("swap_thrashing".into());
        d.evidence
            .push("swap-in and swap-out both at least 10 pages/s with memory PSI stalls".into());
    }
    if b.zram.as_ref().is_some_and(|zs| {
        zs.iter().any(|z| {
            z.mm_stat.as_ref().is_some_and(|v| {
                v.first().is_some_and(|n| {
                    b.mem_total
                        .is_some_and(|t| t > 0 && *n as f64 / t as f64 >= 0.10)
                })
            })
        })
    }) {
        d.classes.push("zram_heavy".into());
        d.evidence
            .push("ZRAM original data is at least 10% of RAM on a device".into());
    }
    if stalled && reclaim && psi_at_least(b.io_some_10, 5.0) {
        d.classes.push("io_amplified_memory_stalls".into());
        d.evidence.push(
            "memory stalls and reclaim coincide with I/O PSI >= 5%; causality is not established"
                .into(),
        );
    }
    if psi_at_least(b.memory_some_60, 1.0) && reclaim {
        d.classes.push("sustained_pressure".into());
        d.evidence
            .push("memory PSI avg60 >= 1% with current reclaim".into());
    } else if stalled && (reclaim || swap) {
        d.classes.push("elevated_pressure".into());
        d.evidence
            .push("memory PSI stalls coincide with reclaim or swap churn".into());
    }
    if !d.reliable {
        d.classes.push("incomplete_or_unreliable".into());
    }
    if d.classes.is_empty() {
        d.classes.push("normal".into());
        d.evidence
            .push("core evidence shows no threshold-level stalls or direct reclaim".into());
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Observation {
        Observation {
            timestamp_ms: Some(1),
            collection_ms: 1,
            mem_total: Some(1000),
            mem_available: Some(100),
            swap_total: Some(100),
            swap_free: Some(50),
            memory_some_10: Some(0.0),
            memory_some_60: Some(0.0),
            memory_full_10: Some(0.0),
            io_some_10: Some(0.0),
            vm: Some(
                [
                    ("pgscan_direct", 100),
                    ("pgsteal_direct", 50),
                    ("pgscan_kswapd", 100),
                    ("pgsteal_kswapd", 50),
                    ("pswpin", 100),
                    ("pswpout", 100),
                    ("allocstall_normal", 0),
                ]
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
            ),
            disks: Some(BTreeMap::new()),
            zram: Some(vec![]),
        }
    }
    #[test]
    fn independent_signals_and_rates() {
        let a = sample();
        let mut b = a.clone();
        let d = diagnose(&a, &b, Duration::from_secs(1), Duration::ZERO);
        assert_eq!(d.classes, ["normal"]); // low available memory alone is not starvation
        let vm = b.vm.as_mut().unwrap();
        *vm.get_mut("pgscan_direct").unwrap() += 20;
        *vm.get_mut("pswpin").unwrap() += 30;
        *vm.get_mut("pswpout").unwrap() += 40;
        b.memory_some_10 = Some(3.0);
        b.memory_some_60 = Some(2.0);
        b.io_some_10 = Some(6.0);
        b.zram = Some(vec![Zram {
            name: "zram0".into(),
            mm_stat: Some(vec![200, 50, 80]),
            io_stat: None,
            stat: None,
        }]);
        let d = diagnose(&a, &b, Duration::from_secs(1), Duration::ZERO);
        for c in [
            "active_direct_reclaim",
            "swap_activity",
            "swap_thrashing",
            "sustained_pressure",
            "io_amplified_memory_stalls",
            "zram_heavy",
        ] {
            assert!(d.classes.contains(&c.to_string()), "{c}");
        }
        assert_eq!(d.rates_per_second["pswpin"], Some(30.0));
        assert_eq!(d.zram_ratios["zram0"], Some((4.0, 2.5)));
    }
    #[test]
    fn invalid_psi_and_slow_first_collection_cannot_produce_pressure_evidence() {
        let mut a = sample();
        let mut b = a.clone();
        a.collection_ms = 5001;
        *b.vm.as_mut().unwrap().get_mut("pgscan_direct").unwrap() += 1;
        b.memory_some_10 = Some(101.0);
        b.memory_some_60 = Some(101.0);
        b.io_some_10 = Some(101.0);
        let d = diagnose(&a, &b, Duration::from_secs(1), Duration::ZERO);
        assert!(!d.reliable);
        assert!(!d.classes.contains(&"sustained_pressure".into()));
        assert!(!d.classes.contains(&"elevated_pressure".into()));
        assert!(!d.classes.contains(&"io_amplified_memory_stalls".into()));
        b.memory_some_10 = Some(0.0);
        b.memory_some_60 = Some(0.0);
        b.io_some_10 = Some(0.0);
        assert!(!diagnose(&a, &b, Duration::from_secs(1), Duration::ZERO).reliable);
    }
    #[test]
    fn missing_stale_and_reset_are_not_zero() {
        let a = sample();
        let mut b = a.clone();
        b.vm.as_mut().unwrap().insert("pswpin".into(), 0);
        b.memory_some_10 = None;
        let d = diagnose(&a, &b, Duration::from_secs(1), Duration::from_secs(6));
        assert!(!d.reliable);
        assert_eq!(d.rates_per_second["pswpin"], None);
        assert!(d.classes.contains(&"incomplete_or_unreliable".into()));
        assert!(!d.classes.contains(&"normal".into()));
    }
}
