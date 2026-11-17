# Safety and recovery contract

Physical mutation: **NO-GO, NOT VERIFIED**. Production readiness: **NO**.
The delivered MVP is suitable for read-only observation and bounded development
experiments with private buffers. Useful desktop performance benefit is unproven.

## Physical boundary

Existing private, consumed, non-Clone mutation permits, private SessionIo, and
descriptor-relative `openat`/`O_NOFOLLOW` controls remain in place. Exact systemd
InvocationID, controller/manager descriptor identities, PID/start ticks,
registered child identity, hierarchy and expected control values are checked by
the existing session adapter. Ambiguous/short writes retain unresolved latches;
successful-looking readback after an error is not proof of ownership. No public
constructor can obtain a built-in live writer. Eight external rustdoc compile-fail
cases and unconditional alternate-constructor refusal cover the API boundary.

Exclusive delegation cannot be inferred from Delegate=yes, ownership or a path.
The admission policy refuses physical execution for every authorization value.
The private adapter, descriptor writer, child writer and legacy disposable writer
also retain their independent unconditional locks. There is no operational claim
that competing same-UID writers can be excluded. Observable conflicts are refused;
sequential reads and writes cannot prevent TOCTOU/ABA or provide kernel atomicity.

The existing coordinator requires fresh safety telemetry at operation boundaries,
records original controllers/low and identities, and restores in the order low →
verified empty child removal → controllers. Unavailable telemetry, an uncertain
write, changed identity/value, failed child cleanup or uncertain worker termination
preserves evidence and blocks unsafe subsequent restoration. Cancellation is
checked at forward stages; bounded recovery defers further cancellation. Tests
exercise this actual adapter/session/coordinator code through private injected I/O.
Those results are fixture evidence, not physical mutation validation.

Adapter diagnostic vectors are now capped at 128 entries per kind. Once a cap is
reached, inspection/admission refuse further work without dropping retained history.
Reusing an exhausted adapter for cleanup is refused; operator investigation is
required. This bounds normal diagnostic growth and preserves unresolved evidence.

## Durable simulation evidence

`mvp simulate --journal NEW_FILE` records each private-buffer step:

1. `captured`: fixed simulation identity, declared total step count, original
   bytes and requested representation.
2. `apply_intent`: synced before private store/copy execution.
3. `restored_verified`: all page contents checked and private storage released.
4. `run_complete`: only after every declared step returns with verified contents.

Each JSONL record has schema/scope, sequence, previous checksum and CRC32 of its
canonical typed payload. Size, count, identity, original size and transition order
are checked. New regular files only, mode 0600, final-component symlinks rejected;
file and parent directory are synced on creation, and every record is synced.
A write/sync error poisons that journal instance, stops the caller and preserves
the file; it cannot authorize a retry. CRC is accidental-corruption detection,
not cryptographic authentication. The simulation identity identifies the fixture
type, not a trusted live process or kernel owner. Records never grant authority.

`mvp recovery --journal FILE` is read-only. Missing terminal records, incomplete
stages, truncated JSON, checksum/sequence conflicts, excessive input or mismatched
restart identity remain unresolved/corrupt and preserve the valid prefix in the
diagnostic. It never edits the input. A complete simulation record certifies only
the recorded private-buffer experiment. It cannot prove that records were not
forged/replaced or that a later unrecorded action did not occur. Filesystem sync
semantics and the storage device determine actual power-loss durability.

Cancellation before a decision ends replay. Cancellation after a durable intent
retains that unresolved intent. After process death, the OS reclaims private
userspace allocations. This is independent of any kernel-resource restoration.

## Physical crash recovery alternative

There is **no physical mutation journal, automatic physical cleanup or safe
physical restart protocol** in this build. Existing activation/child ownership
records are in memory. A stale persisted inode/value would not establish current
ownership or exclusive delegation, and no writes can occur through the locked
built-in physical APIs. Durable simulation records must never be repurposed as
physical transaction records.

`mvp recovery --human` is the runnable non-mutating alternative. It reports current
visible cgroup capabilities, the physical lock, and manual intervention requirements.
It does not validate a historical unit invocation, reap an unknown worker, restore
a control or remove a cgroup. Preserve any older executable's logs and resources;
use the separately documented exact-invocation diagnostic only in an explicitly
authorized local environment. Absence of a resource alone does not prove cleanup.

Physical enablement requires further engineering of exclusive ownership and a
durable physical protocol, plus independent isolated validation. Authorization alone
cannot supply those missing guarantees. No automatic cleanup from stale data is
included in the package.
