//! Fresh, visible-system telemetry eligibility. This is never mutation authority.
//! Identity, delegation, controller checks and unresolved guards remain mandatory.
use std::time::{Duration, Instant, SystemTime};
use xynoos_pressureiq::{
    system::contract::{status, Status},
    Error, MemInfo, Psi,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafetyThresholds {
    pub minimum_available_bytes: u64,
    /// Maximum age since collection START, including sequential collection time.
    pub maximum_age: Duration,
}
impl Default for SafetyThresholds {
    fn default() -> Self {
        Self {
            minimum_available_bytes: 200 * 1024 * 1024,
            maximum_age: crate::MAX_AGE,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalReason {
    InvalidThresholds,
    CollectionFailed(Error),
    TimestampInconsistent,
    Stale,
    MemoryUnavailable(Error),
    MissingMemAvailable,
    MissingMemTotal,
    InconsistentMemory,
    InsufficientAvailableMemory,
    MalformedPsi(Error),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyDecision {
    /// Only the configured telemetry policy passed; not permission to write.
    Eligible,
    Refused(Vec<RefusalReason>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyValidity {
    Valid,
    Invalid,
    Unavailable,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SafetyReading<T> {
    pub source: &'static str,
    pub scope: &'static str,
    pub status: Status,
    pub value: Option<T>,
    pub error: Option<Error>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SafetyObservation {
    pub collection_timestamp: Option<SystemTime>,
    pub collection_started: Option<Instant>,
    pub collection_finished: Option<Instant>,
    pub boundary_started: Instant,
    pub checked_at: Instant,
    pub age: Option<Duration>,
    pub thresholds: SafetyThresholds,
    pub mem_available_bytes: SafetyReading<u64>,
    /// Optional diagnostic evidence, without a universal PSI cutoff.
    pub memory_psi: SafetyReading<Psi>,
    pub validity: SafetyValidity,
    pub decision: SafetyDecision,
}
struct Readings {
    timestamp: SystemTime,
    started: Instant,
    finished: Instant,
    memory: Result<MemInfo, Error>,
    psi: Result<Psi, Error>,
}
// Injection is private and substitutes readings/clock only, never policy or authority.
trait Source {
    fn now(&mut self) -> Instant {
        Instant::now()
    }
    fn wall_now(&mut self) -> SystemTime {
        SystemTime::now()
    }
    fn collect(&mut self) -> Result<Readings, Error>;
}
struct PressureIq;
impl Source for PressureIq {
    fn collect(&mut self) -> Result<Readings, Error> {
        let started = Instant::now();
        let snapshot = xynoos_pressureiq::collect();
        Ok(Readings {
            timestamp: snapshot.timestamp,
            started,
            finished: Instant::now(),
            memory: snapshot.mem.result,
            psi: snapshot.psi.result,
        })
    }
}
/// Collect again on every invocation. No API accepts a cached observation as a
/// boundary approval. Call immediately before future operations, alongside the
/// session's independent authorization and verification checks. Races remain.
pub fn check_boundary(thresholds: SafetyThresholds) -> SafetyObservation {
    check_with(&mut PressureIq, thresholds)
}
fn reading<T>(source: &'static str, result: Result<T, Error>) -> SafetyReading<T> {
    match result {
        Ok(value) => SafetyReading {
            source,
            scope: "visible_system",
            status: Status::Available,
            value: Some(value),
            error: None,
        },
        Err(error) => SafetyReading {
            source,
            scope: "visible_system",
            status: status(&error),
            value: None,
            error: Some(error),
        },
    }
}
fn check_with(source: &mut impl Source, thresholds: SafetyThresholds) -> SafetyObservation {
    let boundary_started = source.now();
    let wall_started = source.wall_now();
    let collected = source.collect();
    let checked_at = source.now();
    let wall_checked = source.wall_now();
    let mut reasons = Vec::new();
    if thresholds.maximum_age.is_zero() || thresholds.minimum_available_bytes == 0 {
        reasons.push(RefusalReason::InvalidThresholds);
    }
    let mut o = SafetyObservation {
        collection_timestamp: None,
        collection_started: None,
        collection_finished: None,
        boundary_started,
        checked_at,
        age: None,
        thresholds,
        mem_available_bytes: reading("/proc/meminfo", Err(Error::Unsupported)),
        memory_psi: reading("/proc/pressure/memory", Err(Error::Unsupported)),
        validity: SafetyValidity::Unavailable,
        decision: SafetyDecision::Eligible,
    };
    match collected {
        Err(e) => {
            reasons.push(RefusalReason::CollectionFailed(e.clone()));
            o.mem_available_bytes = reading("/proc/meminfo", Err(e.clone()));
            o.memory_psi = reading("/proc/pressure/memory", Err(e));
        }
        Ok(r) => {
            o.collection_timestamp = Some(r.timestamp);
            o.collection_started = Some(r.started);
            o.collection_finished = Some(r.finished);
            o.age = checked_at.checked_duration_since(r.started);
            o.validity = SafetyValidity::Valid;
            if r.started < boundary_started
                || r.finished < r.started
                || checked_at < r.finished
                || r.timestamp < wall_started
                || r.timestamp > wall_checked
                || wall_checked < wall_started
            {
                reasons.push(RefusalReason::TimestampInconsistent);
            }
            if o.age.is_some_and(|age| age > thresholds.maximum_age) {
                reasons.push(RefusalReason::Stale);
            }
            let available = match r.memory {
                Err(e) => {
                    reasons.push(RefusalReason::MemoryUnavailable(e.clone()));
                    Err(e)
                }
                Ok(mem) => {
                    let available = mem.bytes.get("MemAvailable").copied();
                    let total = mem.bytes.get("MemTotal").copied();
                    if total.is_none() {
                        reasons.push(RefusalReason::MissingMemTotal);
                    }
                    match available {
                        None => {
                            reasons.push(RefusalReason::MissingMemAvailable);
                            Err(Error::Missing("MemAvailable".into()))
                        }
                        Some(n) => {
                            if total.is_some_and(|t| t == 0 || n > t) {
                                reasons.push(RefusalReason::InconsistentMemory);
                            }
                            if n < thresholds.minimum_available_bytes {
                                reasons.push(RefusalReason::InsufficientAvailableMemory);
                            }
                            Ok(n)
                        }
                    }
                }
            };
            o.mem_available_bytes = reading("/proc/meminfo", available);
            let psi = r.psi.and_then(|p| {
                let valid = |v: &xynoos_pressureiq::PressureLine| {
                    [v.avg10, v.avg60, v.avg300]
                        .iter()
                        .all(|n| n.is_finite() && (0.0..=100.0).contains(n))
                };
                if valid(&p.some) && p.full.as_ref().is_none_or(valid) {
                    Ok(p)
                } else {
                    Err(Error::Malformed("PSI range".into()))
                }
            });
            if let Err(e) = &psi {
                if status(e) == Status::ParseError {
                    reasons.push(RefusalReason::MalformedPsi(e.clone()));
                }
            }
            o.memory_psi = reading("/proc/pressure/memory", psi);
            if reasons.contains(&RefusalReason::Stale) {
                for s in [&mut o.mem_available_bytes.status, &mut o.memory_psi.status] {
                    if *s == Status::Available {
                        *s = Status::Stale;
                    }
                }
            }
            if !reasons.is_empty() {
                o.validity = SafetyValidity::Invalid;
            }
        }
    }
    if !reasons.is_empty() {
        o.decision = SafetyDecision::Refused(reasons);
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Fixture {
        times: VecDeque<Instant>,
        wall: SystemTime,
        readings: VecDeque<Result<Readings, Error>>,
        calls: usize,
    }
    impl Source for Fixture {
        fn now(&mut self) -> Instant {
            self.times.pop_front().unwrap()
        }
        fn wall_now(&mut self) -> SystemTime {
            self.wall
        }
        fn collect(&mut self) -> Result<Readings, Error> {
            self.calls += 1;
            self.readings.pop_front().unwrap()
        }
    }
    fn fixture() -> Fixture {
        let start = Instant::now();
        let wall = SystemTime::now();
        Fixture {
            times: VecDeque::from([start, start + Duration::from_millis(1)]),
            wall,
            readings: VecDeque::from([Ok(Readings {
                timestamp: wall,
                started: start,
                finished: start,
                memory: xynoos_pressureiq::parse_meminfo(
                    "MemTotal: 1048576 kB\nMemAvailable: 204800 kB\n",
                ),
                psi: Err(Error::Unsupported),
            })]),
            calls: 0,
        }
    }
    fn change(f: &mut Fixture) -> &mut Readings {
        f.readings.front_mut().unwrap().as_mut().unwrap()
    }
    fn refused(o: &SafetyObservation, reason: RefusalReason) {
        match &o.decision {
            SafetyDecision::Refused(r) => assert!(r.contains(&reason), "{r:?}"),
            _ => panic!("unexpected eligibility"),
        }
    }
    #[test]
    fn fresh_valid_and_threshold_equality() {
        let mut f = fixture();
        let o = check_with(&mut f, SafetyThresholds::default());
        assert_eq!(o.decision, SafetyDecision::Eligible);
        assert_eq!(o.validity, SafetyValidity::Valid);
        assert_eq!(o.mem_available_bytes.value, Some(200 * 1024 * 1024));
        assert_eq!(o.age, Some(Duration::from_millis(1)));
        assert_eq!(o.mem_available_bytes.scope, "visible_system");
        assert_eq!(f.calls, 1);
    }
    #[test]
    fn insufficient() {
        let mut f = fixture();
        change(&mut f)
            .memory
            .as_mut()
            .unwrap()
            .bytes
            .insert("MemAvailable".into(), 1);
        refused(
            &check_with(&mut f, SafetyThresholds::default()),
            RefusalReason::InsufficientAvailableMemory,
        );
    }
    #[test]
    fn missing_available() {
        let mut f = fixture();
        change(&mut f)
            .memory
            .as_mut()
            .unwrap()
            .bytes
            .remove("MemAvailable");
        let o = check_with(&mut f, SafetyThresholds::default());
        refused(&o, RefusalReason::MissingMemAvailable);
        assert_eq!(o.mem_available_bytes.status, Status::ParseError);
    }
    #[test]
    fn malformed() {
        let mut f = fixture();
        let e = Error::Malformed("MemAvailable".into());
        change(&mut f).memory = xynoos_pressureiq::parse_meminfo("MemAvailable: nope kB");
        let o = check_with(&mut f, SafetyThresholds::default());
        assert_eq!(o.mem_available_bytes.status, Status::ParseError);
        refused(&o, RefusalReason::MemoryUnavailable(e));
    }
    #[test]
    fn stale_and_age_equality() {
        for (age, pass) in [(5, true), (6, false)] {
            let mut f = fixture();
            f.times[1] = f.times[0] + Duration::from_secs(age);
            let o = check_with(&mut f, SafetyThresholds::default());
            if pass {
                assert_eq!(o.decision, SafetyDecision::Eligible);
            } else {
                refused(&o, RefusalReason::Stale);
                assert_eq!(o.mem_available_bytes.status, Status::Stale);
            }
        }
    }
    #[test]
    fn timestamps() {
        for case in 0..4 {
            let mut f = fixture();
            let r = change(&mut f);
            match case {
                0 => r.started -= Duration::from_secs(1),
                1 => r.finished -= Duration::from_secs(1),
                2 => r.finished += Duration::from_secs(1),
                _ => r.timestamp += Duration::from_secs(1),
            }
            refused(
                &check_with(&mut f, SafetyThresholds::default()),
                RefusalReason::TimestampInconsistent,
            );
        }
    }
    #[test]
    fn collection_failure() {
        let mut f = fixture();
        f.readings[0] = Err(Error::Permission);
        let o = check_with(&mut f, SafetyThresholds::default());
        refused(&o, RefusalReason::CollectionFailed(Error::Permission));
        assert_eq!(o.validity, SafetyValidity::Unavailable);
        assert_eq!(o.collection_timestamp, None);
    }
    #[test]
    fn psi_present_is_diagnostic_even_when_high() {
        let mut f = fixture();
        change(&mut f).psi =
            xynoos_pressureiq::parse_psi("some avg10=99 avg60=98 avg300=97 total=10");
        let o = check_with(&mut f, SafetyThresholds::default());
        assert_eq!(o.decision, SafetyDecision::Eligible);
        assert_eq!(o.memory_psi.value.unwrap().some.avg10, 99.0);
    }
    #[test]
    fn psi_unavailable_is_explicit_optional_evidence() {
        let mut f = fixture();
        let o = check_with(&mut f, SafetyThresholds::default());
        assert_eq!(o.memory_psi.status, Status::Unsupported);
        assert!(o.memory_psi.value.is_none());
        assert_eq!(o.decision, SafetyDecision::Eligible);
    }
    #[test]
    fn malformed_psi_refuses() {
        let mut f = fixture();
        change(&mut f).psi = Err(Error::Malformed("psi".into()));
        refused(
            &check_with(&mut f, SafetyThresholds::default()),
            RefusalReason::MalformedPsi(Error::Malformed("psi".into())),
        );
    }
    #[test]
    fn inconsistent_memory_and_missing_total() {
        for missing in [true, false] {
            let mut f = fixture();
            let mem = change(&mut f).memory.as_mut().unwrap();
            mem.bytes.insert("MemTotal".into(), 1);
            if missing {
                mem.bytes.remove("MemTotal");
            }
            refused(
                &check_with(&mut f, SafetyThresholds::default()),
                if missing {
                    RefusalReason::MissingMemTotal
                } else {
                    RefusalReason::InconsistentMemory
                },
            );
        }
    }
    #[test]
    fn sequential_checks_recollect_and_detect_change() {
        let mut f = fixture();
        let mut second = fixture();
        change(&mut second).started = f.times[1];
        change(&mut second).finished = f.times[1];
        change(&mut second).timestamp = f.wall;
        change(&mut second)
            .memory
            .as_mut()
            .unwrap()
            .bytes
            .insert("MemAvailable".into(), 0);
        f.readings.push_back(second.readings.pop_front().unwrap());
        f.times.extend([f.times[1], f.times[1]]);
        assert_eq!(
            check_with(&mut f, SafetyThresholds::default()).decision,
            SafetyDecision::Eligible
        );
        refused(
            &check_with(&mut f, SafetyThresholds::default()),
            RefusalReason::InsufficientAvailableMemory,
        );
        assert_eq!(f.calls, 2);
    }
    #[test]
    fn telemetry_grants_no_write_authority() {
        let mut f = fixture();
        assert_eq!(
            check_with(&mut f, SafetyThresholds::default()).decision,
            SafetyDecision::Eligible
        );
        let audit = crate::Audit {
            action: crate::Action::MemoryLow { bytes: 4096 },
            target: String::new(),
            decision: crate::Decision::Inspected,
            original_bytes: None,
            device: None,
            inode: None,
            kernel_write_available: false,
            control_writable: None,
        };
        for authorized in [false, true] {
            assert!(matches!(
                crate::execute(&audit, authorized),
                crate::Decision::Declined(_)
            ));
        }
    }
    #[test]
    fn invalid_thresholds_refuse() {
        let mut f = fixture();
        refused(
            &check_with(
                &mut f,
                SafetyThresholds {
                    minimum_available_bytes: 0,
                    maximum_age: Duration::ZERO,
                },
            ),
            RefusalReason::InvalidThresholds,
        );
    }
}
