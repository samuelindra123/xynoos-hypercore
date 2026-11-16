//! Identical bounded workload and telemetry replay for all three benchmark arms.
use crate::{Action, Engine, Mode, Outcome, Plan, PAGES, PAGE_SIZE, STORE_BUDGET};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    hint::black_box,
    path::PathBuf,
    time::{Duration, Instant, UNIX_EPOCH},
};
use xynoos_pressureiq::{system::SystemSnapshot, Source};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub at_ms: u64,
    pub age_ms: u64,
    pub cpu_psi: Option<f64>,
    pub memory_psi: Option<f64>,
    pub io_psi: Option<f64>,
    pub total_kib: Option<u64>,
    pub available_kib: Option<u64>,
    pub swap_in_pages: Option<u64>,
    pub swap_out_pages: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trace {
    pub schema_version: u32,
    pub steps: Vec<Step>,
}
impl Trace {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 65536 {
            return Err("trace exceeds 64 KiB".into());
        }
        let trace: Self = serde_json::from_slice(bytes)?;
        trace.validate()?;
        Ok(trace)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.steps.is_empty() || self.steps.len() > crate::MAX_STEPS
        {
            return Err("unsupported trace version or step count (1..60)".into());
        }
        let mut previous = None;
        for s in &self.steps {
            if s.at_ms > 300_000
                || s.age_ms > 60_000
                || previous.is_some_and(|p| s.at_ms <= p || s.at_ms - p > 5000)
            {
                return Err("trace clocks must increase with gaps <= 5 s; total <= 300 s".into());
            }
            for n in [s.cpu_psi, s.memory_psi, s.io_psi].into_iter().flatten() {
                if !n.is_finite() || !(0.0..=100.0).contains(&n) {
                    return Err("invalid PSI".into());
                }
            }
            previous = Some(s.at_ms);
        }
        Ok(())
    }
}
pub fn default_trace() -> Trace {
    Trace::parse(include_bytes!("../../../fixtures/mvp-trace.json")).expect("built-in trace")
}
fn source<T>(result: std::result::Result<T, xynoos_pressureiq::Error>) -> Source<T> {
    Source {
        result,
        read_ns: 0,
        parse_ns: 0,
    }
}
impl Step {
    /// Fixture source labels match PressureIQ's contract, but the outer report always
    /// marks replay. No real collection or sleep occurs in this conversion.
    pub fn snapshot(&self, origin: Instant) -> Result<(SystemSnapshot, Instant)> {
        use xynoos_pressureiq::{parse_meminfo, parse_psi, parse_vmstat, Error};
        if self.at_ms > 300_000 || self.age_ms > 60_000 {
            return Err("trace clock bounds".into());
        }
        // Evaluation ticks always increase. Stale samples were collected earlier;
        // their age must not make the simulated clock jump forward and then back.
        let now = origin
            .checked_add(Duration::from_millis(60_000 + self.at_ms))
            .ok_or("trace clock overflow")?;
        let at = now
            .checked_sub(Duration::from_millis(self.age_ms))
            .ok_or("trace clock underflow")?;
        let missing = || Error::Missing("trace field".into());
        let psi = |n: Option<f64>| {
            n.ok_or_else(missing)
                .and_then(|n| parse_psi(&format!("some avg10={n} avg60={n} avg300={n} total=0")))
        };
        let mem = self
            .total_kib
            .zip(self.available_kib)
            .ok_or_else(missing)
            .and_then(|(t, a)| parse_meminfo(&format!("MemTotal: {t} kB\nMemAvailable: {a} kB\n")));
        let vm = self
            .swap_in_pages
            .zip(self.swap_out_pages)
            .ok_or_else(missing)
            .and_then(|(i, o)| parse_vmstat(&format!("pswpin {i}\npswpout {o}\n")));
        Ok((
            SystemSnapshot {
                timestamp: UNIX_EPOCH + Duration::from_millis(60_000 + self.at_ms - self.age_ms),
                monotonic: at,
                elapsed_ns: 0,
                memory: xynoos_pressureiq::Snapshot {
                    source_root: PathBuf::from("/proc"),
                    timestamp: UNIX_EPOCH
                        + Duration::from_millis(60_000 + self.at_ms - self.age_ms),
                    monotonic: at,
                    elapsed_ns: 0,
                    psi: source(psi(self.memory_psi)),
                    mem: source(mem),
                    vm: source(vm),
                },
                cpu: Err(Error::Unsupported),
                cpu_psi: psi(self.cpu_psi),
                io_psi: psi(self.io_psi),
                disks: Err(Error::Unsupported),
                cgroup: Err(Error::Unsupported),
                cpu_frequency_khz: Err(Error::Unsupported),
                load_one: Err(Error::Unsupported),
                gpu: vec![],
                thermal: vec![],
                power: vec![],
            },
            now,
        ))
    }
}
#[derive(Debug, Serialize)]
pub struct Workload {
    pub original_bytes: usize,
    pub stored_payload_bytes: usize,
    pub accounted_store_bytes: usize,
    pub compressed_pages: usize,
    pub raw_pages: usize,
    pub byte_verification_passed: bool,
    pub sampled_process_rss_kib: Option<u64>,
    pub workload_ns: u128,
}
fn pages() -> Vec<Vec<u8>> {
    let mut seed = 0x12345678u32;
    (0..PAGES)
        .map(|p| {
            (0..PAGE_SIZE)
                .map(|i| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    match p % 4 {
                        0 => 0,
                        1 => (i % 13) as u8,
                        2 if i % 64 < 48 => ((i / 64 + p) % 16) as u8,
                        _ => seed as u8,
                    }
                })
                .collect()
        })
        .collect()
}
// Private executor: it never receives a user-constructed Plan or a kernel path.
fn workload(data: &[Vec<u8>], compress: bool) -> Result<Workload> {
    let start = Instant::now();
    let (stored, accounted, compressed_pages, raw_pages, sampled_process_rss_kib) = if compress {
        let mut store = xynoos_hypermemory::PageStore::adaptive(PAGE_SIZE, PAGES, STORE_BUDGET)?;
        let mut ids = Vec::with_capacity(PAGES);
        for page in data {
            ids.push(store.insert(black_box(page))?);
        }
        for (id, page) in ids.into_iter().zip(data) {
            if store.get(black_box(id))? != *page {
                return Err("owned page verification failed".into());
            }
        }
        let s = store.stats();
        (
            s.stored_bytes,
            s.accounted_bytes,
            s.compressed_pages,
            s.raw_pages,
            rss_kib(),
        )
    } else {
        let raw = black_box(data).to_vec();
        if raw != data {
            return Err("raw page verification failed".into());
        }
        black_box(&raw);
        let bytes = raw.iter().map(Vec::len).sum();
        (bytes, bytes, 0, PAGES, rss_kib())
    };
    Ok(Workload {
        original_bytes: PAGES * PAGE_SIZE,
        stored_payload_bytes: stored,
        accounted_store_bytes: accounted,
        compressed_pages,
        raw_pages,
        byte_verification_passed: true,
        sampled_process_rss_kib,
        workload_ns: start.elapsed().as_nanos(),
    })
}
fn rss_kib() -> Option<u64> {
    status_kib("VmRSS:")
}
fn status_kib(key: &str) -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|line| line.starts_with(key))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
#[derive(Debug, Serialize)]
pub struct Row {
    pub step: usize,
    pub plan: Plan,
    pub decision_ns: u128,
    pub response_ns: u128,
    pub workload: Workload,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub evidence: &'static str,
    pub telemetry: &'static str,
    pub mode: Mode,
    pub kernel_mutations: u64,
    pub trace_crc32: u32,
    pub workload_crc32: u32,
    pub rows: Vec<Row>,
    pub safety_refusals: usize,
    pub owned_buffer_verifications: usize,
    pub cancelled_at_step: Option<usize>,
    pub process_high_water_rss_kib: Option<u64>,
    pub total_ns: u128,
}
pub fn run(trace: &Trace, mode: Mode, authorized: bool) -> Result<Report> {
    run_with_evidence(trace, mode, authorized, |_| Ok(()))
}
pub fn run_with_evidence(
    trace: &Trace,
    mode: Mode,
    authorized: bool,
    evidence: impl FnMut(crate::recovery::Entry) -> Result<()>,
) -> Result<Report> {
    run_with_control(trace, mode, authorized, evidence, |_, _| false)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelPoint {
    BeforeDecision,
    BeforeWork,
}
pub fn run_with_control(
    trace: &Trace,
    mode: Mode,
    authorized: bool,
    mut evidence: impl FnMut(crate::recovery::Entry) -> Result<()>,
    mut cancelled: impl FnMut(usize, CancelPoint) -> bool,
) -> Result<Report> {
    trace.validate()?;
    if mode == Mode::Controlled {
        return Err("physical mode cannot execute replay".into());
    }
    let data = pages(); // Same dataset, generated outside the measured workload interval.
    let workload_crc32 = {
        let mut h = crc32fast::Hasher::new();
        for p in &data {
            h.update(p);
        }
        h.finalize()
    };
    let start = Instant::now();
    let origin = Instant::now();
    let mut engine = Engine::default();
    let mut rows = Vec::with_capacity(trace.steps.len());
    let mut cancelled_at_step = None;
    for (step, s) in trace.steps.iter().enumerate() {
        if cancelled(step, CancelPoint::BeforeDecision) {
            cancelled_at_step = Some(step);
            break;
        }
        let response = Instant::now();
        let (snapshot, now) = s.snapshot(origin)?;
        let decision = Instant::now();
        let plan = engine.evaluate(&snapshot, now, mode, authorized, false);
        let decision_ns = decision.elapsed().as_nanos();
        let compress = mode == Mode::Simulate
            && plan.outcome == Outcome::Simulated
            && plan.action == Action::CompressOwnedPages;
        for stage in [
            crate::recovery::Stage::Captured,
            crate::recovery::Stage::ApplyIntent,
        ] {
            evidence(crate::recovery::Entry::owned(
                step,
                trace.steps.len(),
                stage,
                compress,
            ))?;
        }
        if cancelled(step, CancelPoint::BeforeWork) {
            cancelled_at_step = Some(step);
            break;
        }
        let work = workload(&data, compress)?;
        // Workload verifies every page and releases the private store before return.
        // Only the terminal record certifies that return; a crash before it is unresolved.
        evidence(crate::recovery::Entry::owned(
            step,
            trace.steps.len(),
            crate::recovery::Stage::RestoredVerified,
            compress,
        ))?;
        rows.push(Row {
            step,
            plan,
            decision_ns,
            response_ns: response.elapsed().as_nanos(),
            workload: work,
        });
    }
    if cancelled_at_step.is_none() {
        let last = rows.last().expect("validated nonempty trace");
        evidence(crate::recovery::Entry::owned(
            last.step,
            trace.steps.len(),
            crate::recovery::Stage::RunComplete,
            mode == Mode::Simulate
                && last.plan.outcome == Outcome::Simulated
                && last.plan.action == Action::CompressOwnedPages,
        ))?;
    }
    let safety_refusals = rows
        .iter()
        .filter(|r| r.plan.outcome == Outcome::Refused)
        .count();
    Ok(Report {
        schema_version: 1,
        evidence: "measured_userspace_work_with_simulated_pressure",
        telemetry: "deterministic_replay",
        mode,
        kernel_mutations: 0,
        trace_crc32: crc32fast::hash(&serde_json::to_vec(trace)?),
        workload_crc32,
        owned_buffer_verifications: rows.len(),
        cancelled_at_step,
        process_high_water_rss_kib: status_kib("VmHWM:"),
        safety_refusals,
        rows,
        total_ns: start.elapsed().as_nanos(),
    })
}
