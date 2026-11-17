//! Advisory policy: no action here mutates a workload or kernel state.
use serde::Serialize;
use xynoos_pressureiq::intelligence::Diagnosis;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    GatherEvidence,
    InvestigateReclaim,
    InvestigateSwap,
    Observe,
}
#[derive(Debug, Serialize)]
pub struct Decision {
    pub action: Action,
    pub confidence: &'static str,
    pub reasons: Vec<String>,
    pub recommendations: Vec<&'static str>,
    pub rejected_actions: Vec<&'static str>,
    pub resource_cost: &'static str,
    pub cooldown: &'static str,
}
pub fn decide(d: &Diagnosis) -> Decision {
    let has = |s: &str| d.classes.iter().any(|c| c == s);
    let (action, recommendation) = if !d.reliable {
        (
            Action::GatherEvidence,
            "Repeat a fresh bounded observation and inspect unavailable sources.",
        )
    } else if has("swap_thrashing") {
        (Action::InvestigateSwap, "Inspect workload and swap activity over a longer interval; correlate with application latency.")
    } else if has("active_direct_reclaim") || has("sustained_pressure") || has("elevated_pressure")
    {
        (
            Action::InvestigateReclaim,
            "Inspect memory consumers and reclaim over time before changing system configuration.",
        )
    } else {
        (
            Action::Observe,
            "Continue observing under a representative workload.",
        )
    };
    Decision {
        action,
        confidence: if d.reliable { "moderate" } else { "low" },
        reasons: d
            .evidence
            .iter()
            .chain(d.limitations.iter())
            .cloned()
            .collect(),
        recommendations: vec![recommendation],
        rejected_actions: vec![
            "Kernel, swap, ZRAM, process and cgroup mutations: read-only phase",
            "Private-buffer compression: no authorization or workload-specific cost evidence",
        ],
        resource_cost:
            "Two bounded sequential telemetry reads and one sampling interval; no background work",
        cooldown: "No automatic action or retry; operator chooses when to resample",
    }
}
