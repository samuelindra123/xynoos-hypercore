//! Deterministic advisory policy; untrusted replay cannot authorize control.
use serde::Serialize;
use xynoos_pressureiq::{
    hardware::{Hardware, Status},
    system::contract::{Contract, Status as TelemetryStatus},
};
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Plan {
    pub mode: &'static str,
    pub confidence: &'static str,
    pub evidence: Vec<String>,
    pub recommendations: Vec<String>,
    pub limitations: Vec<String>,
    pub estimated_cost: &'static str,
    pub execution_available: bool,
}
pub fn plan(h: &Hardware, system: &Contract, age_ms: u64) -> Plan {
    let mut p = Plan {
        mode: "read_only", confidence: "moderate", evidence: vec![], recommendations: vec![],
        limitations: vec![
            "Sequential visible-namespace observations; no causal attribution or host-wide completeness.".into(),
            "Temperature alone does not establish throttling; cumulative throttle counts require a fresh delta.".into(),
            "No writable power backend; service ownership, authorization and rollback are not verified.".into(),
        ],
        estimated_cost: "Bounded on-demand procfs/sysfs reads; no subprocesses or background worker.",
        execution_available: false,
    };
    let mut incomplete = age_ms > 5000
        || h.duration_ns > 5_000_000_000
        || system.collection_duration_ns > 5_000_000_000;
    if incomplete {
        p.limitations
            .push("Observation stale or collection exceeded five seconds.".into());
    }
    for (key, r) in &system.readings {
        if r.status != TelemetryStatus::Available {
            incomplete = true;
            continue;
        }
        if ["cpu_psi", "memory_psi", "io_psi"].contains(&key.as_str()) {
            if let Some(n) = r
                .value
                .as_ref()
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
            {
                p.evidence
                    .push(format!("{key}: {n}% avg10 from {}", r.source));
                if n >= 10.0 && age_ms <= 5000 {
                    p.recommendations.push(format!(
                        "Investigate {key} with repeated observations and application latency."
                    ));
                }
            } else {
                incomplete = true;
            }
        }
    }
    let mut available = 0;
    let mut unavailable = 0;
    for d in h.cpu.iter().chain(&h.gpu).chain(&h.stability) {
        for r in &d.readings {
            if r.status == Status::Available {
                available += 1;
            } else {
                unavailable += 1;
                incomplete = true;
            }
        }
    }
    p.evidence.push(format!("Hardware attributes available={available}, unavailable={unavailable}; raw evidence accompanies this plan."));
    if available == 0 {
        incomplete = true;
    }
    if incomplete {
        p.confidence = "low";
        p.recommendations.insert(
            0,
            "Gather fresh evidence and inspect missing or restricted sensors.".into(),
        );
    }
    if p.recommendations.is_empty() {
        p.recommendations
            .push("Continue bounded observation under a representative workload.".into());
    }
    p
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_and_incomplete_evidence_never_authorizes_execution() {
        let hardware = Hardware {
            timestamp_unix_ms: None,
            duration_ns: 0,
            cpu: vec![],
            gpu: vec![],
            stability: vec![],
        };
        let snapshot = xynoos_pressureiq::system::collect_system();
        let contract = xynoos_pressureiq::system::contract::build(
            &snapshot,
            None,
            None,
            snapshot.monotonic,
            std::time::Duration::from_secs(5),
        );
        for age in [0, 5000, 5001, u64::MAX] {
            let a = plan(&hardware, &contract, age);
            assert_eq!(a, plan(&hardware, &contract, age));
            assert!(!a.execution_available);
            assert_eq!(a.confidence, "low");
        }
    }
}
