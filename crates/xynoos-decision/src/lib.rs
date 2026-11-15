//! Read-only policy for sequential, visible-namespace PressureIQ observations.
#![forbid(unsafe_code)]
use serde::Serialize;
use std::{
    collections::VecDeque,
    path::Path,
    time::{Duration, Instant},
};
use xynoos_linux_control::{self as control, Action, Audit};
use xynoos_pressureiq::system::{
    contract::{self, Contract, Status},
    SystemSnapshot,
};

const MAX_AGE: Duration = Duration::from_secs(5);
const HISTORY: usize = 8;
const ENTER: f64 = 10.0;
const EXIT: f64 = 5.0;
const COOLDOWN: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Observe,
    Recommend,
    Decline,
    RequestAction,
}
#[derive(Debug, Serialize)]
pub struct Decision {
    pub kind: Kind,
    pub explanation: String,
    pub pressure_percent: Option<[f64; 3]>,
    pub cgroup_memory_bytes: Option<u64>,
    pub cgroup_cpu_usage_us: Option<u64>,
    pub cgroup_io_bytes: Option<serde_json::Value>,
    pub cgroup_cpu_delta_us: Option<u64>,
    pub cgroup_io_delta_bytes: Option<[u64; 2]>,
    pub adapter: Option<Audit>,
}
#[derive(Default)]
pub struct Engine {
    history: VecDeque<(Instant, [f64; 3])>,
    active: [bool; 3],
    last_change: Option<Instant>,
    cgroup_previous: Option<(std::path::PathBuf, u64, [u64; 2])>,
}
impl Engine {
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
    /// Rebuilds freshness at invocation. Target is an explicit current-cgroup identity,
    /// never inferred from visible-system pressure. This function does not execute writes.
    pub fn evaluate(
        &mut self,
        s: &SystemSnapshot,
        now: Instant,
        target: Option<&Path>,
    ) -> Decision {
        let c = contract::build(s, None, None, now, MAX_AGE);
        self.evaluate_contract(s, &c, now, target)
    }
    fn evaluate_contract(
        &mut self,
        s: &SystemSnapshot,
        c: &Contract,
        now: Instant,
        target: Option<&Path>,
    ) -> Decision {
        let mut out = Decision {
            kind: Kind::Decline,
            explanation: String::new(),
            pressure_percent: None,
            cgroup_memory_bytes: None,
            cgroup_cpu_usage_us: None,
            cgroup_io_bytes: None,
            cgroup_cpu_delta_us: None,
            cgroup_io_delta_bytes: None,
            adapter: None,
        };
        let read = |key: &str,
                    source: &str,
                    scope: &str,
                    unit: &str|
         -> Result<&serde_json::Value, String> {
            let r = c
                .readings
                .get(key)
                .ok_or_else(|| format!("missing {key}"))?;
            if r.status != Status::Available
                || r.error.is_some()
                || r.age_ns > MAX_AGE.as_nanos()
                || r.source != source
                || r.scope != scope
                || r.unit != unit
            {
                return Err(format!("unusable or ambiguous {key}"));
            }
            r.value
                .as_ref()
                .ok_or_else(|| format!("missing value {key}"))
        };
        let result = (|| -> Result<[f64; 3], String> {
            if s.memory.source_root != Path::new("/proc") {
                return Err("invalid memory source root".into());
            }
            if c.schema_version != 1 || !c.sequential || c.freshness_limit_ns > MAX_AGE.as_nanos() {
                return Err("unsupported contract".into());
            }
            if self.history.back().is_some_and(|(t, _)| s.monotonic <= *t)
                || now
                    .checked_duration_since(s.monotonic)
                    .is_none_or(|d| d > MAX_AGE)
            {
                return Err("stale or non-increasing observation".into());
            }
            let mut values = [0.; 3];
            for (i, (key, source)) in [
                ("cpu_psi", "/proc/pressure/cpu"),
                ("memory_psi", "/proc/pressure/memory"),
                ("io_psi", "/proc/pressure/io"),
            ]
            .iter()
            .enumerate()
            {
                let v = read(key, source, "visible_system", "percent avg10")?
                    .as_f64()
                    .ok_or_else(|| format!("invalid {key}"))?;
                if !v.is_finite() || !(0.0..=100.0).contains(&v) {
                    return Err(format!("invalid {key}"));
                }
                values[i] = v;
            }
            let mem = read("meminfo", "/proc/meminfo", "visible_system", "bytes")?;
            let total = mem
                .get("MemTotal")
                .and_then(|v| v.as_u64())
                .filter(|v| *v > 0)
                .ok_or("missing MemTotal")?;
            let available = mem
                .get("MemAvailable")
                .and_then(|v| v.as_u64())
                .ok_or("missing MemAvailable")?;
            if available > total {
                return Err("ambiguous memory availability".into());
            }
            Ok(values)
        })();
        let values = match result {
            Ok(v) => v,
            Err(e) => {
                self.history.clear();
                self.active = [false; 3];
                self.last_change = None;
                self.cgroup_previous = None;
                out.explanation = e;
                return out;
            }
        };
        out.pressure_percent = Some(values);
        // Cgroup context is optional and must retain the source identity of this snapshot.
        if let Ok(cg) = &s.cgroup {
            let p = cg.path.display().to_string();
            out.cgroup_memory_bytes = read(
                "cgroup_memory",
                &format!("{p}/memory.current"),
                "current_cgroup",
                "bytes",
            )
            .ok()
            .and_then(|v| v.as_u64());
            out.cgroup_cpu_usage_us = read(
                "cgroup_cpu",
                &format!("{p}/cpu.stat"),
                "current_cgroup",
                "microseconds",
            )
            .ok()
            .and_then(|v| v.as_u64());
            out.cgroup_io_bytes = read(
                "cgroup_io",
                &format!("{p}/io.stat"),
                "current_cgroup",
                "bytes",
            )
            .ok()
            .cloned();
        }
        if self
            .history
            .back()
            .is_some_and(|(t, _)| s.monotonic.duration_since(*t) > MAX_AGE)
        {
            self.history.clear();
            self.active = [false; 3];
            self.last_change = None;
            self.cgroup_previous = None;
        }
        if let (Ok(cg), Some(cpu), Some(io)) =
            (&s.cgroup, out.cgroup_cpu_usage_us, &out.cgroup_io_bytes)
        {
            if let (Some(r), Some(w)) = (
                io.get("read").and_then(|v| v.as_u64()),
                io.get("write").and_then(|v| v.as_u64()),
            ) {
                if let Some((path, old_cpu, old_io)) = &self.cgroup_previous {
                    if *path == cg.path {
                        out.cgroup_cpu_delta_us = cpu.checked_sub(*old_cpu);
                        out.cgroup_io_delta_bytes = r
                            .checked_sub(old_io[0])
                            .zip(w.checked_sub(old_io[1]))
                            .map(|(a, b)| [a, b]);
                    }
                }
                self.cgroup_previous = Some((cg.path.clone(), cpu, [r, w]));
            }
        } else {
            self.cgroup_previous = None;
        }
        self.history.push_back((s.monotonic, values));
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }
        if self.history.len() < 2 {
            out.kind = Kind::Observe;
            out.explanation = "need two fresh observations".into();
            return out;
        }
        let previous = self.history[self.history.len() - 2].1;
        let cooling = self
            .last_change
            .is_some_and(|t| now.checked_duration_since(t).is_none_or(|d| d < COOLDOWN));
        if !cooling {
            let mut next = self.active;
            for i in 0..3 {
                if !next[i] && values[i] >= ENTER && previous[i] >= ENTER {
                    next[i] = true;
                } else if next[i] && values[i] <= EXIT && previous[i] <= EXIT {
                    next[i] = false;
                }
            }
            if next != self.active {
                self.active = next;
                self.last_change = Some(now);
            }
        }
        let names: Vec<_> = ["cpu", "memory", "io"]
            .into_iter()
            .enumerate()
            .filter_map(|(i, n)| self.active[i].then_some(n))
            .collect();
        if names.is_empty() {
            out.kind = Kind::Observe;
            out.explanation = if cooling {
                "cooldown; no active pressure"
            } else {
                "no sustained pressure above entry threshold"
            }
            .into();
            return out;
        }
        out.kind = Kind::Recommend;
        out.explanation = format!(
            "sustained visible-system {} pressure; target impact and benefit unproven{}",
            names.join(", "),
            if cooling { "; cooldown active" } else { "" }
        );
        if self.active[1] {
            if let (Some(t), Ok(cg), Some(bytes)) = (target, &s.cgroup, out.cgroup_memory_bytes) {
                if t == cg.path && bytes >= 4096 {
                    let audit = control::prepare(
                        s,
                        now,
                        t,
                        Action::MemoryLow {
                            bytes: bytes.min(control::MAX_LOW_BYTES) / 4096 * 4096,
                        },
                    );
                    if audit.decision == control::Decision::Inspected
                        && audit.control_writable == Some(true)
                        && audit.kernel_write_available
                    {
                        out.kind = Kind::RequestAction;
                    } else {
                        out.explanation
                            .push_str("; adapter did not establish an eligible action");
                    }
                    out.adapter = Some(audit);
                } else {
                    out.explanation
                        .push_str("; target identity or cgroup memory insufficient");
                }
            } else {
                out.explanation
                    .push_str("; no explicit verified current-cgroup target");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xynoos_pressureiq::{parse_psi, Error};
    fn sample(t: Instant, cpu: f64, mem: f64, io: f64) -> SystemSnapshot {
        let mut s = xynoos_pressureiq::system::collect_system();
        s.monotonic = t;
        let psi = |n| parse_psi(&format!("some avg10={n} avg60=0 avg300=0 total=1")).unwrap();
        s.cpu_psi = Ok(psi(cpu));
        s.memory.psi.result = Ok(psi(mem));
        s.io_psi = Ok(psi(io));
        s.memory.mem.result = Ok(xynoos_pressureiq::parse_meminfo(
            "MemTotal: 100 kB\nMemAvailable: 50 kB\n",
        )
        .unwrap());
        s
    }
    #[test]
    fn boundaries_hysteresis_and_cooldown() {
        let t = Instant::now();
        let mut e = Engine::default();
        let run = |e: &mut Engine, seconds, v| {
            let at = t + Duration::from_secs(seconds);
            e.evaluate(&sample(at, v, 0., 0.), at, None).kind
        };
        assert_eq!(run(&mut e, 0, 10.), Kind::Observe);
        assert_eq!(run(&mut e, 1, 10.), Kind::Recommend);
        assert_eq!(run(&mut e, 2, 5.), Kind::Recommend);
        assert_eq!(run(&mut e, 3, 5.), Kind::Recommend);
        for i in 4..11 {
            assert_eq!(run(&mut e, i, 5.), Kind::Recommend);
        }
        assert_eq!(run(&mut e, 11, 5.), Kind::Observe);
    }
    #[test]
    fn rejects_invalid_and_resets_history() {
        let t = Instant::now();
        let s = sample(t, 0., 0., 0.);
        let mut e = Engine::default();
        assert_eq!(
            e.evaluate(&s, t + Duration::from_secs(6), None).kind,
            Kind::Decline
        );
        let mut c = contract::build(&s, None, None, t, MAX_AGE);
        c.readings.get_mut("cpu_psi").unwrap().scope = "current_cgroup".into();
        assert_eq!(e.evaluate_contract(&s, &c, t, None).kind, Kind::Decline);
        c.readings.remove("io_psi");
        assert_eq!(e.evaluate_contract(&s, &c, t, None).kind, Kind::Decline);
        let mut bad = s.clone();
        bad.io_psi = Err(Error::Permission);
        assert_eq!(e.evaluate(&bad, t, None).kind, Kind::Decline);
        assert_eq!(e.history_len(), 0);
    }
    #[test]
    fn sustained_memory_routes_to_adapter_without_execution() {
        let t = Instant::now();
        let mut engine = Engine::default();
        let mut first = sample(t, 0., 20., 0.);
        first.cgroup = Ok(xynoos_pressureiq::system::Cgroup {
            path: "/sys/fs/cgroup/definitely-not-a-real-xynoos-target".into(),
            cpu_usage_us: Ok(0),
            memory_current_bytes: Ok(8192),
            io_bytes: Ok((0, 0)),
        });
        let target = first.cgroup.as_ref().unwrap().path.clone();
        engine.evaluate(&first, t, Some(&target));
        let mut second = first.clone();
        second.monotonic += Duration::from_secs(1);
        let result = engine.evaluate(&second, second.monotonic, Some(&target));
        assert_eq!(result.kind, Kind::Recommend);
        assert!(matches!(
            result.adapter.unwrap().decision,
            control::Decision::Declined(_)
        ));
    }
    #[test]
    fn bounded_deterministic_and_no_action_without_capability() {
        let t = Instant::now();
        let mut a = Engine::default();
        let mut b = Engine::default();
        for i in 0..1000 {
            let at = t + Duration::from_millis(i * 100);
            let s = sample(at, 0., 20., 0.);
            let target = s.cgroup.as_ref().ok().map(|c| c.path.as_path());
            let x = a.evaluate(&s, at, target);
            let y = b.evaluate(&s, at, target);
            assert_eq!(x.kind, y.kind);
            assert_ne!(x.kind, Kind::RequestAction);
            assert!(a.history_len() <= HISTORY);
        }
    }
}
