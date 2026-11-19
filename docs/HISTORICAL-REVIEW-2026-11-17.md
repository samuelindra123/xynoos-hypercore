# Historical safety review — 2026-11-17

Review baseline: safety `3c3402efad45d23eb9f047d085e21cf05d468a7e`; MVP `ef39a4ddfd35dbda390dcb7457af2b2e45322bbe`; current main `630ca19ae847525e401ddf8e0733bac21105cd1f`.

Status: Ready for Review, **BLOCKED for merge**. No physical test or safety certification is claimed. The fresh pinned historical-review workflow runs all Rust targets, compile-fail doctests, strict Clippy/rustdoc, release build and Python/launcher fixtures on an unprivileged hosted runner. Results must be linked from the PR after completion. No ignored physical test, systemd launcher or actual kernel mutation is run.

## Findings

- Adapter permits are private/non-clone; non-test guard always refuses. Descriptor controller writes, child create/remove, and alternate DisposableCgroup sinks independently refuse through physical_mutation_disabled. Authorization is necessary but cannot open these gates.
- Binding checks unit invocation, manager PID/start identity, held descriptors and membership. No-follow descriptor-relative operations reduce path substitution risks but checks followed by syscalls cannot prove exclusive delegation or eliminate TOCTOU races. Existing #18 owns remaining identity/concurrency/cleanup acceptance.
- Child memory operations latch ambiguous short writes/read-back/identity failure and preserve unresolved state. Coordinator checks before each transition and restores only expected state; unresolved or unavailable evidence stops recovery. Generic public Backend implementations remain caller-controlled and are not a sandbox.
- Controller state/latches are in memory. SIGKILL, panic, power loss and a worker that cannot be reaped lack durable physical recovery. #19 owns durable recovery; #17 owns independent physical admission/safety review; #20 owns unavailable/reset/stale telemetry across hosts.
- The opt-in systemd unit-preflight launcher requests an external transient service: it is **not read-only**, even though Xynoos built-in control writes are disabled. Fixture mocks cannot establish physical cleanup. Do not run it on the owner's laptop as part of CI review.
- PR15's unbounded repeated adapter evidence collection is corrected by backporting PR14's existing 128-record budget and regression. The bound refuses and retains evidence; it does not authorize a write or claim physical recovery.
- PR14 adds bounded private corpus compression and simulation, not arbitrary application memory optimization. Exact equality precedes committing compressed blocks; reversal regenerates and checks owned inputs. CRC32 is not authentication. Resource/time bounds depend on scheduler/OS behavior.
- Simulation-only journals reject physical identity/replay and do not implement physical recovery. No N5000 desktop gain was demonstrated. The laptop report explicitly recommends changing approach; cloud results remain environment-scoped (#21).

## Integration decision

PR15 remains blocked on #17–#20 and integration review against current main. PR14 remains blocked on PR15 and its current-main conflicts in Cargo.lock, README.md, crates/xynoos-cli/Cargo.toml and crates/xynoos-cli/src/main.rs. Preserve current signed installer, Debian packaging, product CLI and Studio; never resolve these conflicts by replacing current main with the old research workspace. Historical packages/evidence stay on their original branches, tied to their original source hashes (#22). PR descriptions link the detailed audit and fresh CI. Nothing is force-pushed or deleted.
