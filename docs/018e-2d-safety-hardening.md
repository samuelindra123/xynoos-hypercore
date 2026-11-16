# TASK 018E-2D safety hardening and recovery

Source: `4fbf6b39115f824c2424811eaf1716bcbed3a2aa`, branch
`task-018-real-optimization`, existing draft PR #14. Verified in Codespaces on
2026-11-16. The initial defect report is the Terra review findings reproduced in
the task request; no separate Terra report was present in the repository.

## Mutation boundary (implementation evidence)

The public `ControllerSession::activate` convenience constructor is removed.
`activate_prepared`, `restore`, `create_empty_child`, `remove_empty_child` and
`change_child_memory_low` are private and consume a non-Clone private permit.
Only the private adapter module can construct production permits. Its guard
checks authorization, the build's mutation gate, telemetry and current session
identity/hierarchy before issuing a permit. The controller backend trait and
standalone activation/restoration helpers are private, with no public re-export.
Observation, binding diagnostics, readiness and child/control reads remain public.
`verify_for_mutation` is a read-only check, not authority.

There is no production permit issuance: the adapter build gate is always false.
The fixture constructor, permit constructor and writer opt-in hooks exist only
under `cfg(test)`. `DescriptorIo` separately denies controller writes and child
creation/removal. `SessionLow` separately denies physical low writes. Crate-visible
`BoundChild::create/remove` also deny physical changes. These are unconditional
code restrictions, with no Cargo feature, environment variable or CLI override.

The audit also found the alternate public `DisposableCgroup` path. Its constructor,
process placement, low writer, explicit cleanup and Drop cleanup now deny physical
mutation. The old ignored kernel opt-in test is replaced with an external-consumer
test of unconditional refusal, including an authorized caller. The generic
transaction/coordinator Backend traits can execute caller-owned fixture code;
they do not expose a constructible live writer or grant kernel authority. As with
any Rust library, this boundary does not prevent a consumer writing its own syscalls.

## Recovery policy

Forward mutation and compensating recovery have separate telemetry policy.
A refusal permanently closes forward authority on that adapter. Later eligible
telemetry never clears that latch. Recovery is permitted only with a fresh eligible
sample at its mutation guard and all independent binding, hierarchy, registered
identity, expected-value and unresolved-effect checks satisfied. Recovery stage
names are restricted to their corresponding low/removal/controller operations.
Read-only recovery inspection remains available even with refused telemetry and
retains evidence; inspection itself grants no permit.

All recovery writes remain disabled for live descriptors. The following protocol
is exercised through the real session, adapter and coordinator with injected I/O:

1. The empty-child coordinator never starts or migrates workloads and requires an
   empty child and zero descendants throughout. It cannot be used for a running
   workload. The separate legacy worker transaction calls `ready_to_restore`
   before restoration; unconfirmed termination preserves its target. The example
   worker runner requires bounded stop/reap confirmation and marks uncertainty.
2. Restore low only from the recorded requested value to the recorded original,
   with stable registered identity and repeated expected-value checks. Verify it.
3. Verify empty child and owned hierarchy, remove only the registered child, and
   verify absence. A failed or uncertain removal latches child unresolved.
4. Only with no registered or unresolved child and no unresolved low write,
   restore the recorded original controller set from exactly the expected active
   set. Verify the controller set and final hierarchy.

A refusal before a forward syscall is `FaultKind::Before`, allowing the coordinator
to enter recovery. It is not mislabeled as a competing-state conflict. Persistent
or unavailable telemetry at a recovery write is `FaultKind::Unavailable`, reported
as `Outcome::RecoveryUnavailable`. Nothing is removed or restored in that case.
Competing identities, values or population are conflicts and are not deliberately
overwritten. Unknown/short writes, post-write identity failure and read-back errors
retain unresolved latches and stop recovery. A second failure stops reverse cleanup;
there is no blind retry. In particular the parent is never restored through an
unresolved child. Cancellation is checked at every forward stage. Once recovery
starts, further cancellation is deferred until its bounded protocol returns; no
asynchronous cancellation or power-loss guarantee is implied.

The legacy generic transaction previously restored after an apply error if a
later read happened to match the requested value. It now reports `Ambiguous`,
does not release the worker or perform rollback, and its example caller preserves
the target. Matching a value after an error does not prove who wrote it.

## Concurrency and durability limits

Descriptor-relative `openat` with `O_NOFOLLOW`, held parent/manager/child identities,
systemd invocation and PID/start-time comparison, expected-value reads, hierarchy
checks and read-back verification detect many conflicts. They cannot make the
final observation and cgroup write/removal atomic. Linux cgroup v2 offers no
compare-and-swap for these operations. Repeated checks do not eliminate TOCTOU,
ABA values, or replacement between final check and unlink. A same-UID actor or
systemd can still compete; permissions and `Delegate=yes` do not establish
exclusive use. Exclusive delegation remains an external operational precondition
that this code cannot enforce. Therefore physical mutation remains disabled.

Systemd unit disappearance, changed invocation, changed manager descriptor, PID
reuse or process termination uncertainty revoke the binding. A successful stop
request would not prove worker reaping, cgroup emptiness or successful cleanup.
No systemd stop/kill or automatic transient-service cleanup was added or assumed.

Activation ownership and unresolved evidence are in memory. There is no durable
intent journal, restart cleanup, power-loss recovery or safe resumption from a
pathname. A persisted inode/value alone would not prove continuing ownership or
exclusive delegation across reboot. Adding an automatic replay journal without a
trusted exclusive owner could overwrite another actor. This task therefore does
not add persistence or automatic cleanup: after loss of session state, only fresh
read-only inspection is justified; an operator must establish ownership/lifecycle
before any out-of-band action. A crash in this build cannot leave a new physical
mutation made by these disabled APIs. This does not recover resources from an
older or different executable, and does not guarantee future physical recovery.

## Deterministic evidence and adversarial review

Tests use actual session/adapter/coordinator code with private injected I/O,
including exact traces/counts, typed results and retained latch/diagnostic evidence.
Existing identity comparison tests exercise the production systemd comparison
function for unit identity, invocation changes, PID reuse/start ticks, membership,
parent and manager inode changes. BoundChild filesystem fixtures cover replaced
child inodes, symlinks, descendants, population and removal verification. These
are fixture evidence, not successful live systemd or kernel mutation evidence.

| Area | Regression evidence |
| --- | --- |
| Direct API bypass | Eight external rustdoc compile-fail cases: activation, restoration, child create/remove, low, permit, fixture permit, removed convenience API |
| Alternate writer | External integration test refuses physical construction for either authorization value before inspecting any supplied path |
| Authorization | Every adapter forward/recovery mutation method refuses with zero I/O trace when session authorization is absent |
| Telemetry | Initial/persistent/stale refusal; transient refusal at activation guard and after activation, child create and low apply; later eligible recovery; forward latch remains closed |
| Mutation effects | Pre-write refusal, low short/error/mismatch/read-back and post-write child identity replacement; controller short/error/read-back/post-write identity failures |
| Recovery | Exact low/remove/controller ordering; competing values; unexpected population; child removal failure; controller restoration failure; second failure stops cleanup |
| Lifecycle | Every forward cancellation stage, deferred recovery cancellation, unconfirmed worker termination, unresolved retries do not write |

Adversarial review fixes beyond the initial report: alternate disposable writer,
physical descriptor entry points, obsolete physical opt-in test, incorrect telemetry
conflict classification, recovery blocked by the permanent forward latch, ambiguous
legacy rollback and its example cleanup policy, and unrestricted stage labels on
adapter mutation methods. The final review also found that child creation/removal
relied on the coordinator's earlier controller observation, and removal lacked an
independent original-low check. The adapter now checks active controllers at those
operation boundaries, captures the child's initial low independently and refuses
removal unless low equals that record. Integrated late-change tests preserve the
competing state with zero create/remove attempts. These checks still cannot close
the final syscall race. A new authorization test initially tried accessing the
adapter's private session and failed compilation; the fixture now sets authorization
before ownership transfers to the adapter. Privacy was preserved.

The existing Cargo duplicate `validation` example collision is resolved explicitly:
`aura_validation` and `decision_validation` retain the original source paths. Both
manifests enumerate all existing examples. No warning was suppressed; historical
reports using `--example validation` now need the corresponding qualified name.

## Validation and physical blockers

The companion `018e-2d-quality-gates.txt` records the final commands and exits.
No benchmark, workload stress invocation, sudo, live controller activation, live
low write, process migration or host configuration change was performed. Release
builds and documentation generation are compilation evidence only. Ordinary
bounded regression tests are not performance measurements.

Physical kernel status: **NOT VERIFIED**. Real exclusive delegation, kernel write
semantics, live systemd lifecycle behavior, effects under concurrent actors and
physical crash recovery require an isolated authorized environment and further
engineering before any enablement. This change neither enables that validation
nor claims production readiness, kernel optimization or performance improvement.
PR #14 is to remain draft. The six pre-existing untracked files/archives are not
part of this change and are preserved.
