# TASK 018E — transient cleanup verification

Baseline: `2f01e496a876573838c20180c62a4886fc22de86`.
Physical MX Linux result: **NOT VERIFIED**. Kernel mutation remains disabled.
This is fixture and compilation evidence, not production readiness or a performance result.

## Trust model

The local diagnostic executable, Python observer, user systemd manager, D-Bus,
procfs and kernel are trusted. An actor with the same user's privileges can
replace binaries, forge output or alter the hierarchy; this is not a sandbox
against that actor. Root or namespace manipulation is outside this model.

A UUID unit name reduces accidental collisions; it is not identity evidence.
The manager's existing binding establishes unit Id, D-Bus object, nonzero
InvocationID, MainPID/start ticks, active/loaded transient delegation, membership,
and original parent/manager device/inode pairs. The new stdin handshake holds
that exact manager alive while the observer independently reads systemctl's
D-Bus-backed properties and compares Id/invocation/MainPID/ControlGroup.
Two identical active property snapshots bracket descriptor capture. Properties
are sequential observations, not an atomic transaction.

The observer opens every absolute path component without following symlinks,
retaining the root, containing ancestors, delegated parent and manager descriptors.
Device, inode and procfs fdinfo mount ID identify each held directory. The
ancestor, parent and manager must be cgroup v2 on the same mount. Parent and
manager device/inode pairs must match the manager's diagnostic. Holding the
objects prevents ordinary inode/mount-ID recycling from impersonating them.
Named ancestors are reopened and compared before and after removal checks.
Observed replacements are FAIL; unavailable evidence is UNVERIFIED.

The observer also holds read-only `cgroup.events` descriptors for both original
cgroups and successfully reads them while alive. Removal requires both fresh
**offset-zero pread calls to return ENODEV**, plus intended-name absence under
the retained hierarchy. Ordinary file EOF, ENOENT on an events read, zero link
counts, a missing pathname, or systemd collection are insufficient.

This stronger mechanism relies on Linux kernfs and the core cgroup.events
implementation: kernfs read startup reports ENODEV when its active reference
cannot be acquired; cgroup.events' show callback itself returns success and the
core interface is removed during cgroup teardown. It does not depend on
filesystem-directory nlink behavior, which kernfs refreshes differently from
ordinary files. Source basis: Linux v6.1 [kernfs file reads](https://github.com/torvalds/linux/blob/v6.1/fs/kernfs/file.c),
[core cgroup events and destruction](https://github.com/torvalds/linux/blob/v6.1/kernel/cgroup/cgroup.c),
and [kernfs inode refresh](https://github.com/torvalds/linux/blob/v6.1/fs/kernfs/inode.c).
This is a source-based inference for this specific core interface, not a generic
ENODEV guarantee for arbitrary files. If a target kernel lacks these semantics,
retention prevents collection, access is denied, or evidence differs, the
observer must remain UNVERIFIED. Fixtures cannot validate a target kernel.
Removal here means disappearance/deactivation of the original userspace-visible
hierarchy, not release of every kernel reference or completion of asynchronous
kernel reclamation.

## Algorithm and outcomes

1. Start only a transient user service running manager-preflight. Request no
   restart and a 20-second runtime ceiling. No live service was run in Codespaces.
2. Parse exactly one manager record while it waits (15-second stdin timeout).
   Capture active unit properties, hierarchy and events descriptors; compare a
   second active property snapshot. Reject incomplete, malformed or stale data.
3. Release with the byte `1`. EOF or any other byte fails the diagnostic. Wait
   for systemd-run, preserving a nonzero diagnostic/runner exit code and stderr.
4. Classify terminal properties with the original identity. A loaded/inactive
   service must still identify the exact invocation and have MainPID=0. Report
   `inactive_awaiting_collection`. A not-found/inactive unit must have MainPID=0,
   empty invocation and ControlGroup; report `collected`. Contradictions, an
   active service, different Id/invocation or changed cgroup are FAIL. Collection
   establishes no identity by itself: the pre-exit retained evidence is required.
5. For up to five seconds, verify all named ancestor identities, reject any
   parent/manager replacement, and require bound absence and both retained
   events descriptors' ENODEV. Recheck ancestors and identical terminal unit
   properties. Missing evidence/timeouts are UNVERIFIED. Exit zero only if the
   diagnostic exited zero and all evidence passed.
6. Emit one `unit_cleanup` JSON record with exact status/reason, diagnostic exit
   and accumulated evidence. Close descriptors on ordinary completion/failure.
   Failure before manager JSON leaves cleanup UNVERIFIED and preserves the
   diagnostic's nonzero exit status. No success message is inferred from absence.

A different invocation observed under the requested name is always rejected.
Sequential snapshots cannot exclude an entirely unobserved reuse followed by
collection between reads. They prove the recorded original hierarchy's teardown
under the trusted-manager assumptions, not a continuous history of the unit name.
No exclusive control, atomic kernel transaction, crash recovery, or future
absence guarantee is claimed. SIGKILL/crash can prevent final JSON; the bounded
service timeout is a fallback, not proof of cleanup. If systemctl cannot provide
a complete collected-unit property record, collection stays UNVERIFIED.

## Deterministic failure matrix

`./scripts/tests/test-018e-launcher.sh` runs 20 Python tests plus shell routing.
Fixtures use temporary ordinary directories, fake kernfs responses and mocked
systemd processes only. No production fixture override exists.

| Scenario | Required result / evidence |
| --- | --- |
| Active pre-exit snapshot | Exact Id, invocation, MainPID, membership match |
| Normal termination / removed hierarchy | PASS / cleanup_verified, retained hierarchy and ENODEV evidence |
| Inactive awaiting collection | Exact invocation, inactive_awaiting_collection; PASS only with removal evidence |
| Collected with retained evidence | collected; PASS only with original descriptors and removal evidence |
| Still active | FAIL / service_not_inactive |
| Contradictory not-found + active | FAIL / contradictory_collected_state |
| Missing / duplicate / malformed bus properties | UNVERIFIED / missing_unit_information or malformed_unit_information |
| Bus failure or timeout | UNVERIFIED / unit_information_unavailable |
| Malformed or zero invocation | UNVERIFIED / malformed_invocation or missing_invocation_identity |
| Different invocation / stale diagnostic | FAIL / invocation_mismatch |
| Unit Id reuse | FAIL / unit_name_reuse |
| Loaded inactive loses invocation | UNVERIFIED / inactive_identity_unavailable |
| Cgroup present / cleanup timeout | UNVERIFIED / cleanup_timeout; no removal evidence |
| Path absent, ancestor uncertain | UNVERIFIED; no removal evidence |
| Replaced ancestor inode | FAIL / ancestor_replaced |
| Replaced parent or manager inode | FAIL / parent_replaced or manager_replaced |
| Ordinary filesystem absence, retained events readable | No removal proof; timeout remains UNVERIFIED |
| Events read error other than ENODEV | UNVERIFIED / events_removal_ambiguous |
| Malformed / duplicate / partial manager JSON | UNVERIFIED / observer_error, duplicate_manager_record or unexpected_trailing_output |
| Missing manager output | UNVERIFIED / missing_manager_record, no hierarchy evidence |
| Failure before JSON (37) | UNVERIFIED / missing_manager_record, exit 37 retained |
| Failed diagnostic with verified cleanup | FAIL / diagnostic_failed, exit 37 retained |

## Adversarial review

Review performed locally without subagents or live systemd execution.

- Fixed loss of evidence before capture: manager now explicitly waits; observer
  rejects EOF/timeouts, captures while alive and brackets capture with snapshots.
- Fixed ambiguous collected-state acceptance: inactive and collected are distinct;
  not-found cannot pass alongside active, a PID, invocation or ControlGroup.
- Fixed hierarchy substitution: no-follow component opens, held identities and
  mount IDs replace post-exit absolute-path inference.
- Fixed a review-discovered short-circuit: the parent being present must not
  skip checking a replaced manager child; both named identities are checked.
- Avoided the renamed-directory false positive: absence is combined with core
  interface deactivation on both original retained objects.
- Fixed exceptional pipe close masking final reporting: cleanup tolerates pipe
  close errors and records a bounded runner wait failure.
- Diagnostic failure is independent of cleanup success; zero runner status alone
  cannot pass. Duplicate manager records and trailing partial data are rejected.
- Remaining limits are sequential-observation races, trusted local components,
  kernel-specific interface semantics, and absence of durable recovery. None
  authorizes enabling physical mutation.

## Validation and remaining transaction blockers

See [quality gates](TASK-018E-CLEANUP-GATES.txt). All pass; static x86_64 musl
build is packaging evidence only. Archive SOURCE_COMMIT identifies the exact
committed source used; README contains the safe Antigravity handoff.

Physical kernel transactions remain blocked on real MX Linux delegation and
cleanup validation, exclusive hierarchy control/TOCTOU handling, ambiguous and
partial write recovery, unavailable-telemetry recovery, durable crash journaling,
worker termination/reap guarantees and a separately authorized mutation review.
All existing production mutation guards remain unchanged. No controller
activation, memory.low writes, manual deletion, sudo, migration or stress tests
were performed. Systemd itself creates/removes the transient service hierarchy;
thus unit-preflight is a read-only XYNOOS diagnostic, not zero host-side mutation.
