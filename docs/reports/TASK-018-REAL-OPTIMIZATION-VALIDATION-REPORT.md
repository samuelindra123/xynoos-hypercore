# TASK 018 — Real optimization validation (partial)

## TASK 018E-1A — physical failure and decoding fix

**LIVE SYSTEMD BINDING: NOT VERIFIED.** The reported MX Linux run failed before emitting the manager record: the D-Bus decoder assumed every `data` value was a single-element array, and the launcher then produced a secondary unpacking error when no record existed. Method object replies now require their explicit one-argument framing; properties require their exact scalar or 16-byte-array shapes and signatures. The launcher records the unit name independently, checks termination even on failure, preserves the diagnostic exit status and reports cleanup unverified when no unique path is available. It never removes cgroups.

Realistic D-Bus fixtures cover valid MX reply shapes and malformed, missing and wrong-signature values. Launcher fixtures cover an early exit without JSON, original stderr and status, termination checking and missing cleanup evidence. Focused and workspace tests, all-target Clippy and check, rustdoc, formatting and static musl release build passed. The existing duplicate-example-name workspace warning remains. No physical rerun has been performed. See the [MX retry handoff](TASK-018E-1A-MX-HANDOFF.md).

## TASK 018E-1A — live binding diagnostic prepared

**LIVE SYSTEMD BINDING: NOT VERIFIED.** The existing manager preflight now reports the typed D-Bus identity, invocation, state and delegation, PID/start ticks, actual membership, and held parent/manager device and inode, after final binding revalidation. This remains a sequential, read-only observation and grants no authority for a kernel write. The launcher checks service termination and the reported parent cgroup's disappearance; unavailable state or cleanup evidence fails closed. The service itself is a temporary systemd user unit. [MX handoff](TASK-018E-1A-MX-HANDOFF.md) contains exact steps and pass criteria.

Focused identity and launcher fixtures, workspace tests, all-target Clippy and check, rustdoc and formatting passed. The workspace test emits existing duplicate example-name warnings. An x86_64 static musl release was built; no transient service was started in Codespaces and no physical test has been run. Fixture cleanup is not evidence of live cleanup. Controlled mode and kernel mutation remain disabled.

## TASK 018D continuation — partial, control remains disabled

The manager diagnostic now opens the parent and `manager` directories using the existing descriptor-relative, no-follow helpers, checks cgroup v2 and non-root ownership, refuses the cgroup mount root, and compares the open manager's device/inode with PressureIQ's process-scoped cgroup observation before and after reading the parent files. The systemd unit properties are still checked separately and sequentially: **this is not an authorization binding for a controller write** and does not close races. The launcher checks whether systemd reports the transient service inactive or collected, but does not verify cgroup removal; the unit diagnostic remains untested on systemd 257.

A controller-activation state machine inside `xynoos-linux-control` now has fixture coverage for authorization, populated parent, unavailable or unexpected controllers, identity changes, denied activation and mismatched read-back. It checks the expected enabled-controller set and reports ambiguous write/read-back failures without a blind retry. **There is no real descriptor-backed controller-activation implementation or integration with the gated workload**; the fixture backend only changes memory in a Rust test object. No kernel writes were made. The public controlled command remains disabled.

Full workspace tests and focused controller and example tests passed; formatting, Clippy with warnings denied, workspace check, rustdoc with warnings denied and the release build passed. The real kernel test remained ignored. A read-only Codespaces probe saw PID 1 `docker-init` and `/sys/fs/cgroup/init`; no MX Linux machine was accessed. No performance improvement is established.

Remaining blockers: bind the open parent and manager descriptors to the exact live transient unit and verify that binding at each write; implement descriptor-relative `+memory` activation with real read-back and clear handling of ambiguous writes; integrate it with the worker, cancellation and restoration lifecycle; close the cancellation/release race; inject creation, spawn, placement, signal, timeout, termination, rollback and cleanup failures end to end; verify no restoration or removal while any worker may remain alive; and verify actual cgroup removal and transient-service termination. Keep PR #14 draft until these pass. Real kernel intervention and physical MX Linux validation are separate release gates.

## TASK 018D — delegated manager preflight (PARTIAL)

This update supersedes previous suggestions to run controlled mode. The standalone controlled command now refuses before creating a cgroup or writing any control: transient-unit authorization, safe controller activation and lifecycle failure coverage are not complete. **Manager/workload separation, controller activation, empty-child and workload intervention remain NOT VERIFIED and not ready for execution.**

The new `scripts/xynoos-018d-launcher.sh unit-preflight` starts a transient user service using `Delegate=cpu memory pids` and `DelegateSubgroup=manager`, then invokes the experiment's read-only `manager-preflight`. The systemd unit setting for a restricted controller list is `Delegate=`; `DelegateControllers` is a D-Bus property, not the unit-file assignment. See [systemd 257 resource control](https://www.freedesktop.org/software/systemd/man/257/systemd.resource-control.html) and [systemd D-Bus properties](https://freedesktop.org/software/systemd/man/latest/org.freedesktop.systemd1.html). The preflight dynamically maps `/proc/self/cgroup` and mountinfo, checks the unit's `MainPID`, `ControlGroup`, `Delegate`, and `DelegateSubgroup` with `systemctl --user show`, examines parent owner/mode, direct population and controllers, and emits JSON. These pathname-based sequential checks are only diagnostic: they are **not** an authorization boundary or race-free proof of ownership. No subtree control write is implemented. Starting the transient service itself creates/manages systemd cgroups; it is distinct from the fully read-only `preflight` command. Unit termination and cleanup are not independently verified.

A 200 MiB `MemAvailable` guard and five-second spawned-worker deadline were added to the dormant controlled implementation; the threshold is rechecked before release. Visible-system MemAvailable is not a guarantee of cgroup headroom and can change immediately. The existing worker allocation is bounded to 32 MiB. Fixture tests cover the exact low-memory threshold and parent populated, memory-controller-unavailable and unexpected-controller preflight cases. Existing transaction fixtures exercise rollback checks. They do not cover controller activation denial/read-back, unit authority, placement, cancellation boundaries, timeout, cleanup and service termination end to end. No physical-host kernel writes or service launch were performed in Codespaces.

Codespaces read-only preflight: PID 1 `docker-init`; `/proc/self/cgroup` `0::/init`; cgroup v2 visible at `/sys/fs/cgroup`; `/sys/fs/cgroup/init` is root-owned. `MemAvailable` was 13,455,589,376 bytes during this particular probe, not a target-host measurement. `cargo fmt --all -- --check`, `cargo test --workspace`, targeted example tests (4), Clippy with warnings denied, `cargo check --workspace`, release build, and rustdoc with warnings denied passed. The real-kernel test remains ignored. No claim of optimization is made.

### Safe physical preparation

Build on a compatible x86_64 Linux/glibc environment and copy the two files to the target. The release artifact is dynamically linked; a Codespaces build may require a newer glibc than MX Linux, so inspect with `ldd` and use a compatible build environment if needed. No Rust installation is needed on the target for a compatible binary:

```sh
cargo build --release -p xynoos-decision --example optimization_experiment
mkdir -p xynoos-018d-dist
cp target/release/examples/optimization_experiment scripts/xynoos-018d-launcher.sh xynoos-018d-dist/
ldd xynoos-018d-dist/optimization_experiment
# Transfer xynoos-018d-dist/ to the MX Linux host, then:
cd xynoos-018d-dist
ldd ./optimization_experiment
./xynoos-018d-launcher.sh preflight | tee preflight.json
```

After reviewing the non-mutating output, the separate diagnostic `./xynoos-018d-launcher.sh unit-preflight` intentionally creates a short-lived systemd user service. This diagnostic has not been tested on MX Linux. The launcher does not use sudo or enable controllers; do not use the desktop scope, alter swap/zram, or change ancestor cgroups. No safe standalone empty-child or controlled workload command is supplied yet. If a transient unit remains, inspect it using the unit name in the output and the user's `systemctl --user` and journal; do not remove or alter a populated or unverified cgroup by hand.

### Blocking implementation gates

Implement descriptor-relative verification binding the parent to the designated transient service, safely activate and read back only `+memory`, integrate the existing adapter and gated worker, and prove cleanup and transient-unit termination. Add deterministic activation denied/read-back, creation, spawn, placement, identity, cancellation, timeout, termination, rollback, cleanup and service-termination tests. Resolve the release/cancellation race and prove no restore/removal while a worker might be alive. Add explicit opt-in empty-child and separate workload commands only after those gates. Real MX kernel operation and performance remain separate release gates.

## TASK 018B finalization update — still partial

The runner now installs SIGINT/SIGTERM handlers for controlled mode, checks a cancellation flag around startup and while waiting, and attempts kill and reap within two seconds. If termination cannot be verified it marks the target unresolved and blocks restoration and automatic removal. The real backend also independently refuses restoration while its cgroup is populated. These checks are not atomic with worker release or external migration. Signal handling, real placement, creation and cleanup failures have not been exercised end-to-end; an uninterruptible process, races, SIGKILL, power loss and privileged concurrent writers remain limits. In particular, cancellation can arrive between the last flag check and release, and the correctness of real cgroup termination and restoration has not been verified. **Keep the PR draft; do not run controlled mode on a physical host yet.**

Fixture tests: 14 transaction tests passed, including refusal to restore an injected unresolved worker, alongside authorization, write, read-back, identity, conflict and rollback coverage. A real subprocess test verifies stop/reap and repeated stop. Full deterministic injection of creation, spawn, placement, PID change, cancellation at each lifecycle boundary, worker timeout and cleanup failure remains outstanding. Fixture tests alone do not prove the production lifecycle. Workspace tests, targeted example tests, format, Clippy, check and rustdoc passed. A real read-only cgroup probe succeeded; no real control write was attempted.

`optimization_experiment memory-repeatability` adds three alternating sampled/unsampled observation pairs. Each uses a 32 MiB fixed-work worker (128 passes) and a 32 MiB fixed-work competitor (512 passes), both bounded by a 10-second timeout. Raw records are in [memory-repeatability.jsonl](task-018/memory-repeatability.jsonl). Worker completion ranged 36.97–38.48 ms, and the competitor was still running when the worker completed in all six runs. Two visible-system PSI collections cost 1.62–2.00 ms per sampled run. Visible-system memory PSI `some` totals did not increase during the sampled intervals; these shared observations cannot establish workload pressure or causation. These are observation-only comparisons: no cgroup memory limit, scoped PSI, intervention, policy A/B, controlled sampling fixtures or statistical result is implemented. **Benchmark foundation remains PARTIAL; performance benefit NOT ESTABLISHED.**

Before merge: complete deterministic end-to-end lifecycle failure injection, resolve and test the cancellation/release race and cleanup/reporting paths, and implement an isolated limited memory-contention trial with alternating baseline/intervention and scoped telemetry. Physical MX Linux capability discovery, explicit exclusive delegation, real read-back/restoration/cleanup, and repeatable performance measurements are additional release gates.

## TASK 018B update — partial, not ready for real intervention

The TASK 018 observations below are historical; statements there that controlled mode always refuses describe the earlier commit. The new experimental path creates a random disposable child only under an explicitly authorized parent and launches its own worker blocked on a release pipe. It checks process start time, membership and cgroup device/inode, checks `memory.low` and fresh telemetry before releasing the worker, then waits with a timeout. The transaction now spans the callback and checks identity and value again before restoration. Worker errors are reported separately; a known conflict or rollback failure preserves the cgroup for inspection rather than deleting it. Unexpected membership and descendants cause refusal. Each experiment owns a unique target; there is no API to reopen an existing target. These checks are sequential, not atomic.

**Lifecycle: PARTIAL; placement: PARTIAL; transaction spanning execution: PARTIAL.** The transaction callback is exercised in fixtures, including authorization, read-back and write refusal, operation failure, conflicts and rollback failure. Creation, process placement, timeout/cancellation and cleanup fault injection are incomplete, and real placement and the transaction around a real worker have not been validated. SIGTERM cancellation is not handled; abrupt termination, SIGKILL, power loss and concurrent writers can leave a child or a modified control. The process identity and membership checks are point-in-time and cannot establish continuous isolation. Do not run the controlled command until the missing lifecycle and fault-injection gates have been completed and reviewed. No claims of real writes or improvement are made. The controlled workload is fixed work (32 MiB, 128 passes), but no bounded competitor, repeated baseline/intervention comparison, scoped PSI or CPU accounting is implemented. It does not establish meaningful memory pressure.

Codespaces recheck: PID 1 is `docker-init`; `/proc/self/cgroup` is `0::/init`. The mount root remains codespace:root mode 755 and `/sys/fs/cgroup/init` root:root mode 755. No safe delegated parent was identified, no write attempted. A live read-only cgroup probe succeeded and classified the cgroup as shared. Workspace tests, the 13 transaction fixtures, focused runner test, formatting, Clippy, check and rustdoc passed. Physical MX Linux validation remains **NOT VERIFIED**.

### Standalone artifact and MX Linux preparation

Build on a compatible x86-64 Linux machine with a glibc version no newer than the target, then copy the executable to the N5000 (or build in a compatible older distribution/container):

```sh
cargo build --release -p xynoos-decision --example optimization_experiment
ldd target/release/examples/optimization_experiment
# Copy target/release/examples/optimization_experiment to the test machine.
./optimization_experiment observe > observe.jsonl
./optimization_experiment dry-run > dry-run.jsonl
```

The binary is dynamically linked; match architecture and libc, and check `ldd` on the target. These read-only commands need no Rust installation. Detect actual service environment and delegation rather than assuming systemd:

```sh
cat /proc/1/comm
findmnt -t cgroup2 -o TARGET,OPTIONS
cat /proc/self/cgroup
cat /sys/fs/cgroup/cgroup.controllers
```

An administrator must explicitly prepare an exclusive disposable cgroup v2 parent for the non-root test user, distinct from the mount root, with memory enabled in its `cgroup.subtree_control`, owner-only write access and no unrelated workloads or competing writers. On systemd, delegation may be provided by an administrator-managed delegated unit; MX Linux may instead use SysV init, in which case arrange delegation through that environment or use an explicitly prepared parent. The runner does not create privileged delegation or alter boot, swap, zram or CPU settings. Verify ownership, mount and controller files read-only, and keep a recovery session. The eventual opt-in syntax is `./optimization_experiment controlled --authorize --parent /absolute/delegated/parent`; it is **not yet approved for physical testing** because the safety gates above remain open. After a future run, verify no `xynoos-test-*` directory remains; investigate preserved targets and original values before any manual cleanup. Remove the administrator-created parent only after all child processes are gone and state has been checked; follow the host's delegation manager procedure. No automated privileged setup or teardown is supplied.

Baseline `aa831f9a1206307c6280ac352401ce0a6b36afa2`. **Integration: PARTIAL. Real kernel write: NOT VERIFIED. Workload intervention: NOT VERIFIED. Performance improvement: NOT ESTABLISHED.** This is an observation and dry-run foundation, not an optimizer. Controlled mode deliberately refuses before any write; safe process placement and a transaction that remains applied while a workload runs have not been implemented.

## Environment and feasibility

Linux 6.8.0-1070-azure x86_64; effective UID/GID 1000. Cgroup v2 is mounted read-write at `/sys/fs/cgroup` with `nsdelegate,memory_recursiveprot`; `/proc/self/cgroup` is `0::/init`, cgroup namespace `cgroup:[4026532226]`. Available and enabled subtree controllers: `cpuset cpu io memory hugetlb pids rdma misc`. The mount root is owned by codespace:root mode 755, and its `cgroup.procs` and `cgroup.subtree_control` are codespace:root mode 644. Its only visible child `/sys/fs/cgroup/init` is root:root mode 755. The writable mount root is excluded by the existing disposable-backend policy, and ownership alone cannot establish exclusive delegation. No eligible exclusive delegated parent was established; no cgroup was created or written. We did not probe host-owned controls for write access.

## Implementation and experiment

`cargo run --release -p xynoos-decision --example optimization_experiment -- observe` runs four bounded workloads, three alternating-order unsampled/sampled pairs each. `dry-run` additionally records Linux Control Adapter `memory.low` inspections (32 MiB proposed) against the observed shared cgroup, never authorization or a transaction. `controlled` fails closed. The runner uses PressureIQ process identity and samples, shared-cgroup PSI and visible-system telemetry, both decision engines, explicit PID/start-time, per-run timing, sampled peak RSS, available process CPU/I/O deltas and JSON-lines records. Child lifetime is guarded, with a five-second timeout; temporary I/O files are unlinked after opening. Mode parsing has a focused refusal test. These runs have no kernel policy changes. A dry-run inspection is not proof that a target is safe or writable.

Each workload runs for 1.2 seconds; samples occur approximately every 200 ms. Memory allocation is bounded to 32 MiB and I/O to 8 MiB. CPU is a single busy process, memory repeatedly touches pages, I/O writes a bounded temporary file, and idle sleeps. These are **not controlled CPU, memory, or I/O contention experiments**: no competing workload is established, timed work is not a fixed amount of work, storage may be cached, and there is no isolated workload cgroup. Process counters exclude children and cover only the sampled interval; peak RSS is sampled, not kernel high-water RSS. Decisions include shared cgroup and visible-system pressure; neither can be attributed exclusively to the worker. Page faults, swap deltas, full-run CPU time, scoped PSI deltas, intervention/restore measurements and rigorous statistical inference remain unimplemented. Failed collection sources may not be individually represented in every record. See raw [observe JSONL](task-018/observe.jsonl) and [dry-run JSONL](task-018/dry-run.jsonl). Dry-run produced 72 adapter inspections, all `Inspected`; this means read-only inspection succeeded, not that delegation or writes were authorized.

## Measurements

Release build, no concurrent workspace build during measurement. Raw wall times (ms) and total telemetry/decision collection time per sampled run (ms):

| Workload | Unsampled wall | Sampled wall | Collection total |
|---|---|---|---|
| Idle | 1211.77, 1219.67, 1219.78 | 1215.82, 1213.80, 1212.83 | 15.43, 14.24, 13.34 |
| CPU | 1201.42, 1209.51, 1210.02 | 1206.98, 1208.38, 1204.63 | 14.44, 17.56, 14.55 |
| Memory | 1211.27, 1209.92, 1212.77 | 1204.80, 1204.89, 1209.45 | 15.23, 13.51, 17.48 |
| I/O | 1219.20, 1219.41, 1219.32 | 1213.78, 1216.21, 1214.92 | 14.34, 16.47, 14.89 |

Collection cost is wall-clock time in the parent, not CPU time or a demonstrated end-to-end slowdown. Fixed-duration workers, 10 ms polling, scheduling noise, shared resources, and only three pairs preclude a meaningful overhead percentage or statistical claim. The dry-run measurements are separately recorded and include adapter inspection. Observation A/B compares instrumentation, not policy.

## Comparison and safety evidence

1. Actual kernel control modified: **no**.
2. Modified value read back: **not applicable**.
3. Effect on controlled workload: **not verified**.
4. Original configuration restored: **not applicable**; no control was changed.
5. Statistical or practical performance improvement: **not established**; no policy comparison exists.
6. Observed instrumentation cost: 13.34–17.56 ms total per sampled 1.2-second run; end-to-end effect uncertain.
7. Regressions and uncertainty: no policy regression can be measured; timed workers and shared PSI limit interpretation.

Existing transaction fixture tests cover authorization, stale input, identity changes, write failures, read-back mismatch, conflicts and rollback failures; they exercise an empty fixture target, not this runner with a real workload. The existing real-kernel test is ignored and was not run. Workspace tests passed, including nine transaction fixtures; focused runner mode test, formatting, Clippy with warnings denied, workspace check and rustdoc with warnings denied passed. Cargo reports existing example-name collision warnings. Cleanup failure injection for this runner and safe workload-placement fault injection remain missing. The worker guard attempts kill/wait on error but cannot guarantee cleanup on SIGKILL or power loss. Existing transactions have no atomic compare-and-swap, durable recovery, or guarantees against concurrent privileged writes, SIGKILL, power loss or kernel failure.

## Remaining isolated-host procedure and gates

On a disposable Linux host, obtain explicit authorization and an exclusive, non-root-owned delegated cgroup v2 parent distinct from the mount root with `memory` already enabled for children. Coordinate all other writers. First run the existing empty-child test documented in TASK 015C using `XYNOOS_KERNEL_TEST=1 XYNOOS_CGROUP_PARENT=/absolute/path` as specified there, and record apply/read-back/restore/removal separately. **That test does not control a workload.** Do not run it against a shared or host-owned cgroup. Inspect and resolve any cleanup failure before further attempts.

Before a real workload experiment can run, implement and test safe bounded child placement with verified membership and stable cgroup identity; a transaction lifetime spanning workload execution with explicit authorization, read-back and restoration; stop-on-conflict and cleanup failure handling; and fault injection for every step. Then design fixed-work repeated alternating A/B trials with a bounded competitor, explicit resource limits and scoped PSI/counters, confidence intervals and a predeclared practical threshold. Record actual read-back, original-state verification, child exit and subtree removal; report failures without retrying blind writes. The existing empty-child backend must not be presented as workload control. These and real-host validation, crash recovery, stronger concurrency guarantees, and evidence of user benefit are separate gates before any autonomous optimizer or production activation.

### 018E-1A identity binding (partial)

The manager preflight now uses the read-only `systemd_identity::Binding` before its existing diagnostic checks. It resolves the named user service through systemd's D-Bus `GetUnit` and reads typed properties using `busctl --json=short` ([systemd D-Bus interface](https://www.freedesktop.org/software/systemd/man/255/org.freedesktop.systemd1.html)). It requires the exact canonical unit ID and object, a nonzero 16-byte invocation ID, loaded/active/transient state, delegation and the `manager` subgroup, the control-group path and the current process as `MainPID`. It binds PID plus `/proc` start ticks and actual unified cgroup membership to cgroup-v2 parent and manager descriptors, including filesystem device and inode, and reopens/rechecks them together with systemd properties. Missing properties, bus errors and identity changes fail closed. The binding requires a trustworthy user bus and visible, matching cgroup namespace and mount; unsupported configurations decline. `DelegateSubgroup` requires systemd 254 or newer.

This binding is read-only and no real controller backend is connected to it. Any future backend must hold the binding and revalidate immediately before every operation, use the held parent descriptor, and address authorization and isolation separately. Property reads and kernel observations are sequential and cannot prevent changes between validation and an operation or detect a change and reversal between observations. A successful real transient-service binding has not been established in the present environment: no user bus is available. Real read-only discovery observed the test process and unified membership `/init`, and rejected a unit lookup; fixtures exercise stale invocation, process and cgroup identities and malformed or missing replies. No transient unit was started for this task. Controller activation and cleanup remain unimplemented, and original controller state is retained in memory only; crash-safe recovery is not provided. TASK 018E remains PARTIAL.
