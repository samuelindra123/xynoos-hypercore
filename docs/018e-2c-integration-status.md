# TASK 018E-2C integration status

**Incomplete. Physical transaction disabled; kernel mutation NOT VERIFIED.**
No end-to-end empty-child transaction or transaction report is implemented here.
No physical testing is authorized by this document.

The controller session can now be prepared with a validated binding before an
activation attempt. The tracked activation path records the observed controller
set before invoking the existing activation function. On an activation error, or a
change in the captured state between inspection and successful activation, the
session retains the binding and marks activation unresolved. It does not retry,
create a child or restore automatically. A failed controller restoration also marks
the session unresolved. The old one-shot constructor still consumes its session on
failure, so integration must use the prepared session. The recorded observation is
not authority to overwrite a competing change. A failed pre-attempt inspection
occurs before the attempted-write marker; subsequent attempts must still revalidate.

Focused controller fixtures cover successful activation/restoration, failed
activation write, activation read-back mismatch, recorded original state and
refusal to retry after an unresolved attempt. These are isolated controller
fixtures, not the requested end-to-end failure matrix. Existing child fixtures
cover identity, population, hierarchy and cleanup independently; existing
memory.low fixtures cover its isolated apply/restore behavior. Cancellation at
every lifecycle stage, cross-component cleanup ordering, live session report,
child memory.low access and the requested structured recovery outcomes remain
unimplemented and untested. In particular, existing fixture PASS results must not
be interpreted as proof of integration or physical behavior.

Before integration can be considered complete, it must connect the held child
and control-file identity to the transaction backend, validate fresh safety
telemetry at the write boundaries, and prove rollback and cancellation across the
entire lifecycle. It must capture original controller and child values, distinguish
pre-write failures from partial writes and competing values, and preserve the
child and parent when cleanup is ambiguous. It must not restore the parent while
child cleanup is unresolved. Independent removal and final controller read-back
must precede any success report. Service termination and hierarchy removal still
require external verification. No durable crash or power-loss recovery exists;
in-memory state and diagnostics may be lost and systemd may clean up resources.
Checks and writes are sequential and require exclusive delegation; concurrent
replacement between checks remains possible. The physical command must remain
disabled until these issues and the complete fixture matrix are reviewed.
