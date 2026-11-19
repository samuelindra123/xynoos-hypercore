# TASK 018E-2C-2B2B — injectable session I/O boundary

The narrow session I/O boundary is implemented and exercised by 19 new deterministic
unit tests. This completes this task's seam and session test scope. The coordinator
adapter and physical transaction remain incomplete. **Physical kernel status: NOT
VERIFIED.** No live controller activation or memory.low write was performed.

## Starting state and scope

PR #14 was confirmed open and draft with head branch `task-018-real-optimization`.
Local HEAD and the remote PR head both equaled the requested baseline
`6a7e6e108c4fcffbdca9eb0cde3c10f85fd61cbc` before editing. No AGENTS.md was found.
Existing untracked `SHA256SUMS`, `ss` and the four distribution archives were left
untouched and excluded from the commit.

## Interface and integration

`controller/session_io.rs` contains the private `SessionIo` trait and its sole
production implementation, `DescriptorIo`. The latter owns the existing live
`Binding` (held parent and manager descriptors) and the registered `BoundChild`.
It delegates controller reads to `transaction::read_at`, controller writes to
`transaction::openat` plus the same checked `write_at`, and child operations to
`BoundChild`. Existing `O_NOFOLLOW`/`O_CLOEXEC`, child ownership, population,
descendant, inode and removal read-back checks remain intact. Live revalidation
still calls `Binding::revalidate`, including systemd invocation and process checks.

`LinuxController` privately holds `Box<dyn SessionIo>`; its authorization check and
strict `verify_unit` remain above the boundary. The latter still requires the real
manager process ID, an empty parent, the permitted descendant counts, a validated
child and no unresolved child state. `ControllerSession::verify_for_mutation`
retains its activation/memory.low latches and calls that same verifier. Observation,
controller activation/restoration, child creation/removal and `SessionLow` reads
now reach the interface through their existing session orchestration. The existing
typed observation and mutation algorithms and restoration guards are unchanged.
`Send + Sync` on the private interface preserves those session capabilities.

Production constructors still take a discovered `Binding`, never a path or a
caller-supplied implementation. Explicit authorization is still required for
preparation/activation and restoration. There is no public fixture constructor,
trait or generic session parameter. `FixtureIo` and the downcast used to retain
existing descriptor observation fixtures compile only under `cfg(test)`. A
simulated successful identity check supplies test evidence over in-memory state;
it cannot create a live binding or authorize production I/O.

`SessionLow::write_enabled` remains false, and its write and prepare methods still
refuse physical memory.low writes, including in injected sessions. The injected
write failures below exercise controller writes. This task does not claim to
exercise a session memory.low write protocol through a newly enabled writer.
Existing isolated memory.low protocol fixtures remain separate evidence.

## Evidence of real session execution

`controller/session_io_tests.rs` constructs the actual `ControllerSession` with the
actual `LinuxController`, substituting only its private I/O object. It calls
`observe`, `verify_for_mutation`, `activate_prepared`, `create_empty_child`,
`registered_child_identity`, `child_controllers`, `read_child_memory_low`,
`change_child_memory_low`, `remove_empty_child` and `restore` as appropriate.
It does not implement `ControllerBackend::verify_unit` or copy the observation,
mutation or latch algorithms. Thus parent/manager hierarchy refusal is checked by
production code, rather than supplied as a successful fixture policy decision.

The fixture has no filesystem or systemd access. Its read/write/identity/cleanup
faults and operation counters supply deterministic I/O. Assertions cover typed
validity/completeness, activation state, per-read results, mutation operation and
status, attempted/verified certainty, expected/last-verified/observed state,
registered ownership, and counts proving retries or restoration did not write.
The four existing descriptor-backed observation tests also continue to pass.

## Failure-injection matrix

All rows PASS in `cargo test -p xynoos-linux-control controller::session_io_tests`.
Test names below are in that module; several rows have additional cases.

| Required scenario | Test(s) | Verified typed result / protection |
| --- | --- | --- |
| Valid observation | `valid_session_observation_and_verified_lifecycle` | `Valid` + `Complete`; full simulated activate/create/read/remove/restore lifecycle returns `AppliedAndVerified` |
| Controller read failure | `controller_read_failure_is_unknown_and_refuses_activation` | Failed controller field; `Unknown` + `Incomplete`; `RefusedBeforeMutation`, attempted false |
| Controller state changes | `controller_change_between_observations_is_not_cached_authority`; `controller_change_during_observation_is_invalid` | `Invalid` + `Complete`; no cached observation authorizes activation; no writes |
| Parent identity mismatch | `parent_identity_mismatch_invalidates_and_refuses` | Both binding checks fail; historical identity retained; `Invalid` + `Incomplete`; verification and activation refused |
| Registered-child inode mismatch | `registered_child_inode_mismatch_preserves_child` | Failed child reads, `Unknown` + `Incomplete`; low read `IdentityConflict`; removal `Conflict`; controller restoration `CleanupRequired`; child retained |
| memory.low read failure | `memory_low_read_failure_is_typed_and_incomplete` | Failed low field; `Unknown` + `Incomplete`; low read `RefusedBeforeWrite`; no low latch or write |
| Pre-write refusal | `pre_write_refusal_uses_real_hierarchy_and_population_checks` | Parent population, wrong manager PID, extra parent descendants and manager descendants independently fail actual verifier; `RefusedBeforeMutation`; zero writes |
| Ambiguous write | `ambiguous_controller_write_retains_expected_and_observed_certainty` | Error after simulated effect; `AmbiguousMutation`, attempted true; expected/original retained, observed unknown; unresolved latch |
| Write read-back mismatch | `controller_write_readback_mismatch_is_ambiguous_with_observed_state` | `AmbiguousMutation`; typed observed controller set differs from expected; unresolved latch |
| Child cleanup failure | `child_cleanup_failure_latches_and_blocks_controller_restoration` | `AmbiguousMutation`, attempted unknown; child retained; retry and restoration `CleanupRequired`; exactly one removal attempt |
| Controller restoration failure | `controller_restoration_failure_before_write_is_not_retried`; `controller_restoration_write_failure_or_mismatch_stays_ambiguous` | Before write: `RestorationFailure`, attempted false; write/error/mismatch/read-back failure: `AmbiguousMutation`, attempted true; retry `CleanupRequired` |
| Unresolved mutation blocks retry | `unresolved_mutation_blocks_retry_even_after_valid_io_returns` | Successful later identity reads do not clear latch; activation/creation `RefusedBeforeMutation`; restoration `CleanupRequired`; one write only |

Additional tests inject controller read-back failure, post-write binding failure,
child creation failure after a possible effect, and child population. They also
prove observation/verification does not grant write authorization and that an
authorized memory.low request remains `RefusedBeforeWrite` with the original
simulated value unchanged. These are 19 tests total, including the multi-case rows.

Observation retains the existing conservative semantics: a child read failure
(including inode mismatch reported by that read) gives `Unknown`/`Incomplete` unless
other evidence proves invalidity. Mutation verification independently rejects the
child mismatch. Read errors are not fabricated into observed values or authority.

## Quality gates

All commands exited successfully on 2026-11-16 in Codespaces:

- `cargo test -p xynoos-linux-control controller::session_io_tests`: 19 passed.
- `cargo test -p xynoos-linux-control controller::observation_tests`: 4 passed.
- `cargo test --workspace`: 157 passed, 1 ignored (the opt-in physical kernel test).
- `cargo test -p xynoos-decision --example optimization_experiment`: 5 passed,
  including physical CLI refusal, memory guards and worker termination/reaping.
- `cargo fmt --all` and `cargo fmt --all -- --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo check --workspace --all-targets`.
- `cargo build --workspace --release` (native target; compilation only).
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`.
- `git diff --check`.

The workspace test still emits Cargo's existing output filename collision warning
for `validation` (and `validation.dwp`): both `xynoos-aura` and `xynoos-decision`
have an example named `validation`. Those files predate this change and are
unchanged. No warning was suppressed or allowed; no new compiler/Clippy/rustdoc
warning was found. Renaming unrelated examples is outside this task.

An initial new fixture test exposed an injection timing bug: the read-back error
was initially activated before the restoration write because activation had already
written once. The fixture now arms that failure only on entry to the selected
write, and verifies both pre-write and post-write restoration outcomes separately.
No production checks were relaxed to make the matrix pass.

## Remaining blockers

Fresh safety telemetry still needs integration at physical write boundaries,
including freshness, scope, memory availability and required pressure readings.
These diagnostic observations supply neither telemetry freshness nor exclusive
delegation and do not authorize mutation.

The coordinator still lacks a Linux adapter connecting its lifecycle to this
session, descriptor-bound child and memory.low transaction backend. That work must
map typed uncertainty without retries, prove cancellation and reverse cleanup
across components, independently verify child absence and controller restoration,
and retain the existing worker-termination guards. External transient-service
termination and hierarchy cleanup also require independent verification. No
coordinator code, CLI gate or worker guard was changed here.

Physical memory.low writes and the physical CLI remain disabled. Sequential checks
cannot remove concurrent replacement races; exclusive delegation remains required.
State is held only in memory, with no durable crash/power-loss recovery. Live
systemd binding on the physical target, kernel mutation and physical cleanup are
**NOT VERIFIED**. This test result does not complete TASK 018E or authorize a
physical run, workload, stress test or benchmark.
