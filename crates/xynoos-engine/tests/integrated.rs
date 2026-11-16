use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use xynoos_engine::{
    recovery::{self, Journal, Stage, Status},
    replay::{self, CancelPoint},
    Action, Engine, Mode, Outcome, Reason,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "xynoos-mvp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn three_arms_share_trace_and_workload_but_only_simulation_compresses() {
    let t = replay::default_trace();
    let baseline = replay::run(&t, Mode::Baseline, true).unwrap();
    let observe = replay::run(&t, Mode::Observe, true).unwrap();
    let sim = replay::run(&t, Mode::Simulate, true).unwrap();
    assert_eq!(baseline.trace_crc32, sim.trace_crc32);
    assert_eq!(observe.workload_crc32, sim.workload_crc32);
    assert_eq!(sim.safety_refusals, 2);
    assert_eq!(observe.safety_refusals, 2);
    assert_eq!(sim.owned_buffer_verifications, t.steps.len());
    assert_eq!(sim.kernel_mutations, 0);
    assert!(baseline
        .rows
        .iter()
        .all(|r| r.plan.decision.is_none() && r.workload.compressed_pages == 0));
    assert!(observe
        .rows
        .iter()
        .all(|r| r.workload.compressed_pages == 0));
    assert!(sim.rows[2].workload.compressed_pages > 0);
    assert!(sim.rows[2].workload.stored_payload_bytes < sim.rows[2].workload.original_bytes);
    assert_eq!(sim.rows[4].plan.reason, Reason::CpuOrIoPressure);
    assert_eq!(sim.rows[6].plan.reason, Reason::TelemetryUnavailable);
    assert_eq!(sim.rows[7].plan.reason, Reason::TelemetryUnavailable);
    assert_eq!(sim.rows[8].plan.action, Action::KeepOwnedRaw);
    assert_eq!(sim.rows[9].plan.action, Action::CompressOwnedPages);
    assert_eq!(sim.rows[12].plan.action, Action::KeepOwnedRaw);
    assert!(sim.rows.iter().all(|r| r.workload.byte_verification_passed
        && r.workload.accounted_store_bytes <= xynoos_engine::STORE_BUDGET));
}

#[test]
fn authorization_and_physical_lock_are_independent_of_caller_intent() {
    let t = replay::default_trace();
    let denied = replay::run(&t, Mode::Simulate, false).unwrap();
    assert!(denied.rows.iter().all(|r| r.workload.compressed_pages == 0));
    let (s, now) = t.steps[2].snapshot(Instant::now()).unwrap();
    for authorized in [false, true] {
        let p = Engine::default().evaluate(&s, now, Mode::Controlled, authorized, false);
        assert_eq!(p.outcome, Outcome::Refused);
        assert_eq!(p.action, Action::BackOff);
        assert!(!p.kernel_write_available);
        assert_eq!(
            p.reason,
            if authorized {
                Reason::PhysicalMutationLocked
            } else {
                Reason::AuthorizationRequired
            }
        );
        assert!(replay::run(&t, Mode::Controlled, authorized).is_err());
    }
}

#[test]
fn telemetry_refusals_reset_history_and_swap_counter_reset_is_not_activity() {
    let t = replay::default_trace();
    let origin = Instant::now();
    let (s, now) = t.steps[2].snapshot(origin).unwrap();
    let mut engine = Engine::default();
    assert_eq!(
        engine.evaluate(&s, now, Mode::Simulate, true, false).action,
        Action::KeepOwnedRaw
    );
    let mut step = t.steps[3].clone();
    step.swap_out_pages = Some(0);
    let (s, now) = step.snapshot(origin).unwrap();
    assert_eq!(
        engine.evaluate(&s, now, Mode::Simulate, true, false).reason,
        Reason::TelemetryUnavailable
    );
    let mut step = t.steps[3].clone();
    step.at_ms = 4000;
    let (s, now) = step.snapshot(origin).unwrap();
    assert_eq!(
        engine.evaluate(&s, now, Mode::Simulate, true, false).action,
        Action::KeepOwnedRaw
    );
    let mut engine = Engine::default();
    let mut bad = s.clone();
    bad.memory.monotonic = now + Duration::from_secs(1);
    assert_eq!(
        engine
            .evaluate(&bad, now, Mode::Observe, false, false)
            .reason,
        Reason::TelemetryUnavailable
    );
    bad = s.clone();
    bad.memory.vm.result = Err(xynoos_pressureiq::Error::Permission);
    assert_eq!(
        engine
            .evaluate(&bad, now, Mode::Observe, false, false)
            .reason,
        Reason::TelemetryUnavailable
    );
    bad = s.clone();
    bad.memory.source_root = "/tmp/forged".into();
    assert_eq!(
        engine
            .evaluate(&bad, now, Mode::Observe, false, false)
            .reason,
        Reason::TelemetryUnavailable
    );
    assert_eq!(
        engine
            .evaluate(
                &s,
                now + Duration::from_secs(6),
                Mode::Observe,
                false,
                false
            )
            .reason,
        Reason::TelemetryUnavailable
    );
}

#[test]
fn policy_threshold_sweep_uses_real_parsers_and_aura_decisions() {
    let t = replay::default_trace();
    let origin = Instant::now();
    for percent in (0..=100).step_by(5) {
        let mut engine = Engine::default();
        let mut step = t.steps[2].clone();
        step.at_ms = 0;
        step.memory_psi = Some(f64::from(percent));
        let (s, now) = step.snapshot(origin).unwrap();
        assert_eq!(
            engine.evaluate(&s, now, Mode::Simulate, true, false).action,
            Action::KeepOwnedRaw
        );
        step.at_ms = 1000;
        let (s, now) = step.snapshot(origin).unwrap();
        let p = engine.evaluate(&s, now, Mode::Simulate, true, false);
        assert_eq!(
            p.action,
            if percent >= 10 {
                Action::CompressOwnedPages
            } else {
                Action::KeepOwnedRaw
            }
        );
        step.at_ms = 2000;
        step.io_psi = Some(10.0);
        let (s, now) = step.snapshot(origin).unwrap();
        assert_eq!(
            engine.evaluate(&s, now, Mode::Simulate, true, false).reason,
            Reason::CpuOrIoPressure
        );
    }
}

#[test]
fn low_memory_and_cancellation_back_off_without_owned_mutations() {
    let t = replay::default_trace();
    let mut step = t.steps[2].clone();
    step.available_kib = Some(1024);
    let (s, now) = step.snapshot(Instant::now()).unwrap();
    let mut e = Engine::default();
    assert_eq!(
        e.evaluate(&s, now, Mode::Simulate, true, false).reason,
        Reason::InsufficientAvailableMemory
    );
    assert_eq!(
        e.evaluate(&s, now, Mode::Simulate, true, true).outcome,
        Outcome::Cancelled
    );
    assert_eq!(
        e.evaluate(&s, now, Mode::Simulate, true, false).outcome,
        Outcome::Cancelled
    );
    let mut entries = vec![];
    let r = replay::run_with_control(
        &t,
        Mode::Simulate,
        true,
        |entry| {
            entries.push(entry);
            Ok(())
        },
        |step, point| step == 2 && point == CancelPoint::BeforeWork,
    )
    .unwrap();
    assert_eq!(r.cancelled_at_step, Some(2));
    assert_eq!(r.rows.len(), 2);
    assert_eq!(entries.last().unwrap().stage, Stage::ApplyIntent);
    assert!(!entries.iter().any(|e| e.stage == Stage::RunComplete));
}

#[test]
fn durable_evidence_restart_and_every_interrupted_stage_fail_closed() {
    let temp = Temp::new();
    let path = temp.0.join("evidence.jsonl");
    let t = replay::default_trace();
    let mut journal = Journal::create(&path).unwrap();
    replay::run_with_evidence(&t, Mode::Simulate, true, |e| journal.append(e)).unwrap();
    drop(journal);
    let bytes = fs::read(&path).unwrap();
    let d = recovery::inspect_file(&path).unwrap();
    assert_eq!(d.status, Status::SimulationRestored);
    assert_eq!(d.valid_prefix_records, t.steps.len() * 3 + 1);
    assert!(!d.physical_mutation_available && !d.automatic_recovery_available);
    assert_eq!(
        recovery::inspect(&bytes, Some("different-restart-identity")).status,
        Status::IdentityMismatch
    );
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && i + 1 < bytes.len() {
            let prefix = recovery::inspect(&bytes[..i + 1], None);
            assert_eq!(prefix.status, Status::Unresolved);
            assert!(prefix.manual_intervention_required);
            if i > 0 {
                assert_eq!(recovery::inspect(&bytes[..i], None).status, Status::Corrupt);
            }
        }
    }
    assert_eq!(fs::read(&path).unwrap(), bytes); // Inspection never repairs or deletes.
    assert!(Journal::create(&path).is_err());
}

#[test]
fn corruption_reordering_and_changed_originals_preserve_valid_prefix() {
    let temp = Temp::new();
    let path = temp.0.join("journal");
    let t = replay::default_trace();
    let mut journal = Journal::create(&path).unwrap();
    replay::run_with_evidence(&t, Mode::Simulate, true, |e| journal.append(e)).unwrap();
    drop(journal);
    let bytes = fs::read(&path).unwrap();
    let lines: Vec<_> = bytes.split_inclusive(|b| *b == b'\n').collect();
    let corrupt = [lines[0], b"{broken}\n", lines[2]].concat();
    let d = recovery::inspect(&corrupt, None);
    assert_eq!(d.status, Status::Corrupt);
    assert_eq!(d.valid_prefix_records, 1);
    let reordered = [lines[0], lines[2], lines[1]].concat();
    assert_eq!(recovery::inspect(&reordered, None).status, Status::Corrupt);
    let mut altered = bytes.clone();
    let position = altered.iter().position(|b| *b == b'1').unwrap();
    altered[position] = b'2';
    assert_eq!(recovery::inspect(&altered, None).status, Status::Corrupt);
    assert_eq!(
        recovery::inspect(&vec![b' '; recovery::MAX_BYTES + 1], None).status,
        Status::Corrupt
    );
    assert_eq!(recovery::inspect(&[], None).status, Status::Unresolved);
}

#[test]
fn evidence_io_failure_stops_execution_and_preserves_durable_intent() {
    let temp = Temp::new();
    let path = temp.0.join("journal");
    let mut j = Journal::create(&path).unwrap();
    let r = replay::run_with_evidence(&replay::default_trace(), Mode::Simulate, true, |e| {
        if e.stage == Stage::RestoredVerified {
            return Err("injected evidence sync failure".into());
        }
        j.append(e)
    });
    assert!(r.is_err());
    drop(j);
    let d = recovery::inspect_file(&path).unwrap();
    assert_eq!(d.status, Status::Unresolved);
    assert_eq!(d.last.unwrap().stage, Stage::ApplyIntent);
}

#[test]
fn evidence_paths_refuse_symlinks_and_special_files() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let target = temp.0.join("target");
    fs::write(&target, b"preserve").unwrap();
    let link = temp.0.join("link");
    symlink(&target, &link).unwrap();
    assert!(Journal::create(&link).is_err());
    assert!(recovery::inspect_file(&link).is_err());
    assert!(recovery::inspect_file(&temp.0).is_err());
    assert!(recovery::inspect_file(std::path::Path::new("/dev/null")).is_err());
    assert_eq!(fs::read(target).unwrap(), b"preserve");
}

#[test]
fn traces_are_bounded_and_malformed_input_never_reaches_executor() {
    let mut step = replay::default_trace().steps[0].clone();
    step.at_ms = u64::MAX;
    assert!(step.snapshot(Instant::now()).is_err());
    let mut t = replay::default_trace();
    t.steps[1].at_ms = 0;
    assert!(t.validate().is_err());
    let mut t = replay::default_trace();
    t.steps[0].memory_psi = Some(f64::NAN);
    assert!(t.validate().is_err());
    let mut t = replay::default_trace();
    t.steps[0].age_ms = 60001;
    assert!(t.validate().is_err());
    let mut t = replay::default_trace();
    t.steps = t.steps.iter().cycle().take(61).cloned().collect();
    assert!(t.validate().is_err());
    assert!(replay::Trace::parse(&vec![b' '; 65537]).is_err());
    assert!(replay::Trace::parse(br#"{"schema_version":1,"steps":[],"unknown":true}"#).is_err());
}
