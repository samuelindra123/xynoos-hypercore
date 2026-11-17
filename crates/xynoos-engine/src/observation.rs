//! Serialize the existing observation schema without an intermediate JSON tree.
use crate::Plan;
use serde::Serialize;
use xynoos_pressureiq::system::contract::Contract;

#[derive(Serialize)]
struct Observation<'a> {
    schema_version: u32,
    evidence: &'static str,
    sequential: bool,
    scope: &'static str,
    kernel_mutations: u64,
    owned_buffer_mutations: u64,
    plan: &'a Plan,
    decision_ns: u128,
    telemetry: &'a Contract,
}

/// The borrowed report retains every telemetry error and safety field.
/// Streaming avoids cloning telemetry into `serde_json::Value` and a second String.
pub fn write(
    writer: impl std::io::Write,
    plan: &Plan,
    decision_ns: u128,
    telemetry: &Contract,
) -> serde_json::Result<()> {
    serde_json::to_writer(
        writer,
        &Observation {
            schema_version: 1,
            evidence: "live_read_only",
            sequential: true,
            scope: "visible_namespace",
            kernel_mutations: 0,
            owned_buffer_mutations: 0,
            plan,
            decision_ns,
            telemetry,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, Mode};
    use std::time::{Duration, Instant};
    #[test]
    fn streaming_preserves_schema_and_missing_telemetry() {
        let trace = crate::replay::default_trace();
        for step in &trace.steps {
            let (s, now) = step.snapshot(Instant::now()).unwrap();
            for mode in [Mode::Observe, Mode::Controlled] {
                let plan = Engine::default().evaluate(&s, now, mode, true, false);
                let contract = xynoos_pressureiq::system::contract::build(
                    &s,
                    None,
                    None,
                    now,
                    Duration::from_secs(5),
                );
                let old = serde_json::json!({"schema_version":1,"evidence":"live_read_only", "sequential":true,
                    "scope":"visible_namespace","kernel_mutations":0,"owned_buffer_mutations":0,
                    "plan":plan,"decision_ns":123,"telemetry":contract});
                let mut bytes = Vec::new();
                write(&mut bytes, &plan, 123, &contract).unwrap();
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                    old
                );
            }
        }
    }
}
