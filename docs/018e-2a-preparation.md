# TASK 018E-2A: physical transaction remains disabled

Status: **partial preparation; physical mutation NOT VERIFIED**. The separately
named `empty-child-transaction --authorize-physical-mutation --unit UNIT.service`
request always refuses before unit discovery, service launch, cgroup creation or
control writes. It is not a physical-test command. The existing read-only
`unit-preflight` remains separate. TASK 018E is not complete or production-ready.

The 018E-1C physical read-only results do not verify controller or child writes.
The existing controller activation/restoration fixtures test those operations in
isolation; they do not test an end-to-end empty-child transaction. Existing
`DisposableCgroup` opens a parent by pathname and cannot serve as the required
held-binding implementation without further work. `LinuxController::verify_unit`
currently expects exactly the manager descendant, so it cannot validate the
hierarchy while a test child exists. No child lifecycle has been connected to
`ControllerSession`, and the disabled request does not bind its unit argument to
any descriptors. Existing fixture tests cannot establish real kernel behavior.

Before enabling physical testing, implement and review all of the following:

- Bind the authorized request to the active unit's invocation and held parent and
  manager descriptors, revalidating identity and the exact allowed hierarchy at
  each transition. Eliminate pathname reopening as authority for child creation.
- Check fresh memory safety telemetry immediately before mutation. Check empty
  parent, expected controller set and manager membership, activate `+memory`
  descriptor-relatively and verify read-back. Record enough recovery information
  before each potentially ambiguous operation.
- Create a unique restrictive empty child relative to the held parent; verify its
  identity, emptiness, absence of descendants, memory controller inheritance and
  `memory.low`. Apply a bounded value, verify it, restore its original value and
  verify again. Remove only the same verified empty child, then restore and verify
  the original parent controller state. Never overwrite a competing change.
- Handle cancellation, short/failed writes and ambiguous reads, stale identities,
  unexpected population, child removal failure and controller rollback failure.
  Preserve unresolved resources and report precise recovery state. Never release
  or destroy a worker target while termination is unconfirmed. This empty-child
  transaction must not launch or migrate a worker.
- Cover every transition and failure with deterministic end-to-end injections,
  including failures during cleanup and service termination. Independently verify
  service termination and hierarchy removal. Sequential checks cannot exclude a
  concurrent change between check and write; exclusive control remains a premise.
- Design and review crash recovery. Current controller state is held in memory;
  process death, systemd cleanup, or host failure can interrupt restoration.
  Automated cleanup cannot be guaranteed, and no unattended manual removal or
  restoration is authorized by this document.

## Future physical-test review procedure (do not execute yet)

After the implementation and its gates are independently reviewed, prepare a
separate test handoff tied to the exact reviewed commit. On the isolated MX Linux
machine, use a normal non-root graphical user session and a recovery session;
confirm the user bus, cgroup v2, telemetry thresholds, available memory controller,
exclusive transient hierarchy and lack of competing writers. First run and record
only the established read-only 018E-1C preflight. Review its unit identity,
controller state and cleanup. The later handoff must specify the exact new
launcher command, bounded value, cancellation behavior, expected records and
stop conditions; no such command is approved here. Capture logs and exit status,
verify unit termination and cgroup disappearance independently, and report any
ambiguous state for investigation without manual cgroup writes or deletion.
Never use sudo, migrate an existing workload, or run stress/performance tests.
