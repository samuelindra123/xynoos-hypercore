# TASK 018E-2C-1: fixture coordinator

The coordinator in `xynoos-linux-control::empty_child_coordinator` is a generic
state machine with a deterministic in-memory test backend. No Linux implementation
of its `Backend` trait is supplied. The physical command remains disabled; this is
not evidence of kernel behavior or end-to-end Linux integration.

The forward stages authorize and validate the binding and initial hierarchy, record
the controller set, activate and read back memory, create and identify one child,
check inheritance, capture memory.low, apply and verify a bounded value, restore
and verify the original value, verify emptiness, remove and independently inspect
the child, then restore and independently inspect the parent controllers. The
backend contract requires each observation to validate the complete hierarchy and
each operation to revalidate the held binding and target; the fixture cannot prove
that a future Linux adapter satisfies that contract.

Cancellation is sampled before each forward stage. Recovery ignores subsequent
cancellation and takes a bounded reverse-order path: inspect, restore and verify
memory.low if applied, remove and verify the registered empty child, then restore
and verify parent controllers. It never restores controllers with a known or
unresolved child. A known conflicting identity, value or controller set stops
recovery without overwriting it. An ambiguous write or removal is never retried.
These checks are sequential; exclusive control remains necessary.

Reports include the last completed and failed stages, initial and current
observations when available, registered child identity, restoration verification,
child presence certainty, and investigation details. `Complete` requires verified
forward cleanup; `RestoredAfterFailure` requires verified recovery. `Conflict`
means an observed incompatible state; `Ambiguous` means an operation may have
mutated state; `CleanupRequired` means a known-no-effect recovery operation failed;
`RecoveryUnavailable` means the required initial state or recovery observation
could not be obtained. No outcome guarantees recovery after a process crash or
power loss: progress and original values are held only in memory. No automatic
persistent recovery exists.

Fixture tests inject cancellation at all 16 forward stages; errors before every
failable stage; errors after each of the six forward mutations and three recovery
mutations; and errors after read-only stages, where no mutation exists. They cover
controller and low-value changes, stale binding, child replacement, population,
descendants, missing controller/low file, unexpected extra cgroup and removal
read-back mismatch. Recovery tests inject failures at each cleanup stage, including
simultaneous cleanup faults, and check reverse write order and preservation. This
is not an exhaustive scheduler or race proof; the fake backend supplies its own
identity and mutation semantics.

Remaining integration gates include an adapter connecting the coordinator to the
validated systemd binding, `ControllerSession`, registered descriptor-bound child
and existing memory.low transaction backend; fresh memory safety telemetry; typed
mapping of partial/ambiguous Linux operations; signal and process termination
handling; independent transient-service and hierarchy cleanup verification; and
full failure-injection and review at those real interfaces. Existing worker guards
must remain intact. Physical mutation and crash recovery are NOT VERIFIED, and
TASK 018E remains incomplete.
