# TASK 018E-2C-2C — integrated transaction adapter

The concrete `controller::ControllerSessionAdapter` now implements the existing
`empty_child_coordinator::Backend`. Sixteen new integration tests execute the
adapter, coordinator and actual ControllerSession together. This completes the
injected integration path, not TASK 018E or production readiness. **Physical kernel
status: NOT VERIFIED.** Production adapter mutations, physical CLI and physical
memory.low writes remain disabled.

## Implementation and authority

`controller/session_adapter.rs` owns a prepared ControllerSession and implements
each coordinator backend method using its public session methods. Construction
requires that session; it accepts no filesystem paths or replacement I/O. Binding
preparation still requires explicit authorization and live systemd revalidation.
The coordinator independently checks explicit authorization and bounded test value.
The adapter also checks the session's existing authorization before mutations.

`inspect` calls `observe`, retains the typed result, rejects incomplete/invalid or
unresolved state, and calls `verify_for_mutation`. Thus manager PID, hierarchy,
child ownership/population/descendants and identity checks remain mandatory. Child
inheritance and low values are read through the registered-child session methods.
The opaque coordinator binding key includes the complete BoundIdentity; child keys
include the device and inode. Zero child descendants are reported only after the
strict session verifier has established that condition. The expected controller
capture is checked again before activation; original controller records must match
before restoration. Removal/final inspections independently revalidate the session
hierarchy after each operation.

Fresh safety observations are collected by `safety::check_boundary` during
inspection and separately at every mutation entry, including low restoration,
child removal, controller restoration and recovery operations. The test telemetry
collector feeds the actual safety decision function with injected readings/clocks;
it does not fabricate passing decisions. A safety refusal latches this adapter
against ALL later mutations, including recovery; read-only evidence collection is
still permitted. Recovery may therefore require manual investigation rather than
writing under failed telemetry. A passing observation never authorizes a write.

The public constructor always disables mutation entry points. Only a cfg(test)
constructor plus a cfg(test) I/O capability allow the simulated path to proceed.
DescriptorIo never supplies that capability. Private SessionIo test hooks allow
SessionLow to reach fixture prepare/write operations; these hooks do not exist in
production builds. The existing low protocol still enforces authorization, bounds,
expected values, repeated verification/read-back and unresolved latches. No live
writer was added. Existing registered-child, controller and worker guards and the
physical CLI are unchanged.

The coordinator remains the sole lifecycle/recovery state machine. Callers run
`empty_child_coordinator::run(&mut adapter, authorized, requested, cancelled)` and
receive the existing structured Report. The adapter also retains stage-tagged
SafetyObservation, SessionObservation, MutationOutcome and MemoryLowError evidence.
These diagnostics are needed because the existing coordinator FaultKind is coarser
than session statuses. No diagnostic string is parsed to decide recovery.

## Typed mapping

| Session evidence | Coordinator fault | Retained protection |
| --- | --- | --- |
| RefusedBeforeMutation | Before | No assumed syscall effect |
| Conflict | Conflict | No overwrite/recovery through known competition |
| AmbiguousMutation | Ambiguous | Stop; no retry |
| CleanupRequired / RestorationFailure | Before | Exact typed status retained; unresolved session blocks subsequent recovery inspection |
| Low RefusedBeforeWrite | Before | Known pre-write refusal can recover |
| Low CompetingValue / IdentityConflict before write | Conflict | Preserve competing state |
| Low WriteOutcomeAmbiguous / ReadBackMismatch | Ambiguous | Exact error and observed value retained; low latch remains |
| Low RestorationFailure | Typed cause mapping | Wrapper and cause retained |
| Any low failure with unresolved latch | Ambiguous | Includes post-write identity failure; never misreported as safe conflict recovery |
| Invalid/incomplete observation | Conflict / Unavailable | No manufactured valid observation |
| Any unresolved observation | Ambiguous | Never clears a latch from later reads |

The coordinator's initial unsafe observation classifier may report Conflict for a
telemetry refusal; the exact Refused safety decision remains in adapter evidence.
There is no claim that the four coordinator fault kinds represent every session
status directly. The original typed statuses and causes are preserved alongside
the report, including cleanup and restoration failures.

## Integrated deterministic evidence

All tests are in `controller::session_io_tests::adapter_integration`, reusing the
private I/O fixture from session tests. They execute no filesystem operations,
systemd calls, workloads or kernel writes. Successful mutation order is asserted:

`+memory -> create -> low:8192 -> low:4096 -> remove -> -memory`

The coordinator returns Complete, original_low=4096, low/controller restoration
certainty Yes and child_remains No. Final fixture state is original controllers,
no child and original low. Every mutation has fresh passing safety evidence; every
session mutation outcome is AppliedAndVerified.

| Integrated scenario | Evidence |
| --- | --- |
| Successful lifecycle | Actual adapter/session methods and coordinator Report; exact six-operation order/final state |
| Authorization/live gate | Unauthorized coordinator writes nothing; public adapter constructor refuses mutation |
| Telemetry refusal | Low/stale readings; failures at all six mutation stages leave trace unchanged after refusal |
| Stale binding | Invalid binding observation; zero mutations |
| Activation failure | Ambiguous typed outcome, activation latch, exactly one activation attempt |
| Child creation failure | Ambiguous creation, registered child retained, unresolved child latch |
| Low write failure | Error-after-effect, short write and failed read-back all ambiguous; one attempt, no cleanup |
| Read-back mismatch | Exact ReadBackMismatch retained, low unresolved; no retry |
| Low pre-write refusal | RefusedBeforeWrite, then verified child/controller cleanup in order |
| Competing low value | CompetingValue; preserves 12288 and registered child without overwrite |
| Low restoration failure | RestorationFailure wrapping WriteOutcomeAmbiguous; child preserved |
| Child cleanup failure | Ambiguous removal, child unresolved; parent restoration blocked |
| Controller restoration refusal | CleanupRequired with child; RestorationFailure before write on controller conflict; typed attempted=false retained |
| Controller restoration write failure | Ambiguous outcome and activation latch, no retry |
| Cancellation | All 16 forward stages; bounded existing coordinator recovery; exact reverse order after apply |
| Recovery failures | Errors during recovery low restoration, child removal or controller restoration stop subsequent cleanup and preserve typed ambiguity |

Quality gates passed in Codespaces on 2026-11-16:

- Focused integration tests: 16 passed (run before broader gates).
- `cargo test --workspace`: 187 passed, one opt-in physical kernel test ignored.
- `cargo fmt --all` and `cargo fmt --all -- --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo check --workspace --all-targets`.
- `cargo build --workspace --release` (native compilation only).
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`.
- `git diff --check`.

Initial Clippy findings (fixture tuple type complexity, a boolean expression and
test helper placement) were fixed without suppressions. The existing duplicate
`validation`/`validation.dwp` example-name warnings remain visible and unchanged.

## Remaining physical gates

No production low writer or enabled live adapter exists. The integrated fixtures
prove orchestration and typed failure handling, not real kernel syscall semantics,
exclusive delegation, race freedom, target headroom or live service cleanup.
Telemetry is sequential/visible-system with optional diagnostic PSI; collection
and verification do not eliminate subsequent races. The conservative safety latch
can leave cleanup outstanding. A physical release still requires review of the
live writer and its descriptor/partial-write semantics, exclusive ownership,
physical systemd binding and cgroup validation, external service termination and
hierarchy cleanup evidence, and signal/process lifecycle validation. Existing
worker protections were preserved, not replaced by these fixtures. State and
original values remain in memory; crash/power-loss recovery is not implemented.

Baseline was `1c27a4ee0acdabb1b7872a37a8e96d2a3a5fed33` on
`task-018-real-optimization`, confirmed as draft PR #14's head. Unrelated untracked
files/archives were preserved. No physical test or mutation is authorized by this
report. **Physical kernel status: NOT VERIFIED.**
