# TASK 018E-2C-2B3 — fresh safety telemetry contract

Implemented a standalone, typed telemetry boundary contract in
`xynoos_linux_control::safety`. TASK 018E remains incomplete and is not production
ready. **Physical kernel status: NOT VERIFIED.** No physical mutation, controller
activation, memory.low write, workload, stress test or sudo command was performed.

## Existing infrastructure inspected

PressureIQ `collect()` already collects `/proc/meminfo`, `/proc/pressure/memory`
and vmstat, with typed parse/read errors, wall and monotonic timestamps and read/
parse durations. `collect_system()` wraps this memory snapshot with other system
readings; `system::contract` supplies status classifications and monotonic age
handling. The new module reuses the narrower `collect()` and the existing
`contract::Status`/error mapping, avoiding another Linux parser or filesystem
collector. The optional vmstat read is incidental to reusing this collector; its
result is not required by this policy.

The current experiment preflight requires MemAvailable >= 200 MiB. Existing
`validate_telemetry` requires memory PSI and meminfo in the broader system contract;
it remains unchanged. `xynoos-core::MemoryPressure` defines Low/Moderate/High, and
the decision engine interprets PSI with workload/scope-specific rules. Those
classifications do not establish mutation safety and are not reused as a universal
PSI cutoff. ControllerSession authorization, strict identity/hierarchy/controller
verification and unresolved guards remain unchanged and mandatory.

## API and policy

`check_boundary(SafetyThresholds) -> SafetyObservation` collects on EVERY call.
It accepts neither a cached observation nor a session, binding or authorization
flag. The injection interface is private and changes only readings/clock evidence;
all production decisions are made by the same `check_with` used in the tests.

`SafetyObservation` retains collection wall timestamp, monotonic collection start
and finish, operation boundary start, decision time, age, configured thresholds,
source/scope/status/value/error for MemAvailable and PSI, `SafetyValidity`, and
`SafetyDecision::Eligible` or `Refused(Vec<RefusalReason>)`. Collection-wide failure
retains typed errors and unavailable validity with no invented collection time.
Parse failure is explicit through PressureIQ's `Status::ParseError` and typed error.

Defaults: minimum available memory 200 MiB (209715200 bytes), maximum age five
seconds (`MAX_AGE`). Equality passes both limits; lower memory or greater age
refuses. Zero minimum or age is invalid configuration. Age is measured from
collection START, including time spent collecting, rather than just from finish.
The collection must start after the boundary begins and finish before the decision;
backwards, future, cached/pre-boundary or inconsistent monotonic evidence refuses.
Wall collection time must lie within the wall boundary interval; a backwards wall
clock also refuses. Instants are process-local evidence, not portable timestamps.

MemAvailable and MemTotal must both parse and exist. A zero MemTotal or available
value exceeding total refuses. Missing, denied, malformed or failed required
memory evidence refuses. A stale observation retains values for diagnosis and marks
successful readings Stale; inconsistent timestamps invalidate the observation even
if individual parses succeeded.

PSI is optional diagnostic evidence: unsupported, permission and I/O errors are
recorded without inventing a value or refusing an otherwise valid memory gate.
Malformed PSI refuses; averages must be finite and within 0..=100. No numeric PSI
pass/fail cutoff is applied. A high valid PSI therefore remains visible without
being translated into a generic safety classification. `Eligible` means only that
THIS configured telemetry policy passed; it is not proof of global kernel safety,
exclusive delegation, target attribution or permission to mutate.

## Deterministic matrix

All 14 tests in `safety::tests` pass, using injected readings and clocks and the real
contract decision path. No live collector is invoked by these focused tests.

| Scenario | Test | Result |
| --- | --- | --- |
| Fresh observation and memory equality | `fresh_valid_and_threshold_equality` | Eligible, Valid, typed bytes/age/source |
| Low memory | `insufficient` | InsufficientAvailableMemory |
| Missing MemAvailable | `missing_available` | MissingMemAvailable, ParseError |
| Malformed required telemetry | `malformed` | MemoryUnavailable with typed Malformed error |
| Stale and freshness equality | `stale_and_age_equality` | Equality eligible; beyond maximum Stale/refused |
| Timestamp inconsistencies | `timestamps` | Cached start, finish before start, future finish and future wall time refuse |
| Collection failure | `collection_failure` | CollectionFailed, Unavailable, no fabricated timestamp |
| PSI present | `psi_present_is_diagnostic_even_when_high` | High PSI retained; no universal cutoff |
| PSI unavailable | `psi_unavailable_is_explicit_optional_evidence` | Explicit Unsupported; optional evidence |
| Malformed PSI | `malformed_psi_refuses` | MalformedPsi |
| Memory consistency | `inconsistent_memory_and_missing_total` | InconsistentMemory or MissingMemTotal |
| Sequential change | `sequential_checks_recollect_and_detect_change` | Two collection calls; second low-memory check refuses |
| No authority from telemetry | `telemetry_grants_no_write_authority` | Eligible evidence still cannot enable execution, even with authorization |
| Invalid configuration | `invalid_thresholds_refuse` | InvalidThresholds |

Quality gates passed on 2026-11-16:

- `cargo test -p xynoos-linux-control safety::`: 14 passed.
- `cargo test -p xynoos-linux-control controller::`: 37 passed.
- `cargo test --workspace`: 171 passed; one opt-in physical kernel test ignored.
- `cargo fmt --all` and `cargo fmt --all -- --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo check --workspace --all-targets`.
- `cargo build --workspace --release` (native compilation only).
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`.
- `git diff --check`.

The existing Cargo duplicate `validation`/`validation.dwp` example-name warnings
remain visible in workspace tests, from aura/decision examples. They were not
suppressed; no new compiler, Clippy or documentation warnings were introduced.

## Future integration and limits

The future coordinator adapter should call `safety::check_boundary` at each relevant
mutation boundary and retain its typed evidence/refusal in its report. It must also
independently enforce explicit authorization and fresh session identity, hierarchy,
controller, child ownership and unresolved-state checks. These checks remain
separate; this task does not connect or enable any mutation entry point, alter the
old preflight, or implement the coordinator adapter.

Measurements are sequential and scoped to the visible system; they do not measure
exclusive target headroom or eliminate collection-to-mutation races. A future
adapter still needs ordering, cancellation, typed uncertainty mapping and reverse
cleanup proofs, independent child absence/controller restoration verification,
worker/service termination checks and physical target validation. There is no
durable crash/power-loss recovery. Physical CLI and memory.low writers remain
disabled. Unrelated untracked files and archives remain untouched. PR #14 stays
draft on `task-018-real-optimization`, based on
`55133e6341f72d04c40b26388f4102888841470c`.
