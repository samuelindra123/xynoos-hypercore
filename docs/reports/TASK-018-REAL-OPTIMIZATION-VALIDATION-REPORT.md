# TASK 018 — Real optimization validation (partial)

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
