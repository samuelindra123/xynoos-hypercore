# TASK 017 — Real workload validation

Baseline: main `adb37d2986054c03394599ea3d8453521d5cb0d6`.

- **REAL WORKLOAD TELEMETRY: PARTIAL.** Process identity and counters were observed; workload-specific PSI and cgroup exclusivity are not established.
- **DECISION VALIDATION: PARTIAL.** Real low and elevated visible-system pressure was evaluated. The policy does not consume workload-scoped pressure and its recommendations do not establish causation or benefit.
- **REAL KERNEL WRITE: NOT VERIFIED.** No safely delegated parent was established and no cgroup write was attempted.
- **PERFORMANCE IMPROVEMENT: NOT ESTABLISHED.** These are observation-only experiments.
- **WORKSPACE QUALITY GATES: PASS.** Tests, formatting, Clippy, check and documentation passed. The pre-existing duplicate example-name warning remains. The kernel test is ignored by default.

## Environment and scopes

Linux Codespaces `6.8.0-1070-azure`, x86_64, UID 1000, cgroup v2, `/proc/self/cgroup` `0::/init`. The visible cgroup mount is `rw`, but that does not prove exclusive delegation. `/proc/pressure` and `/proc/meminfo` describe the visible system, which may not be the whole host; the container and current cgroup can contain other processes. Do not treat either as exclusively belonging to the child process.

The new process API requires PID and `/proc/<pid>/stat` start time, checks identity around each read, and reports CPU ticks, resident memory bytes, and kernel-accounted I/O bytes with individual monotonic source timestamps and errors. It reports process PSI unsupported; Linux does not provide per-process PSI. CPU and I/O deltas reject mismatched identity, non-increasing time and counter resets; a missing source causes the combined delta to fail explicitly. Counters exclude children. PID start ticks are not a race-free pidfd identity and access restrictions or process exit can prevent reads. This is not a general cgroup workload discovery mechanism; cgroup-specific PSI and exclusive attribution remain future work. No background polling is introduced.

## Measurements

`cargo run --release -p xynoos-decision --example workload_validation` runs three alternating-order A/B pairs per workload, each child for 1.2 s, sampling every 200 ms in B; A performs no telemetry sampling. Memory is 32 MiB; temporary I/O is capped at 8 MiB. Five-second timeout and process guard bound child lifetime. Wall times include launch and exit polling. The following raw wall times are milliseconds; collection totals are microseconds for all six process/system/decision samples in each B run:

| workload | A wall ms | B wall ms | B collection µs |
|---|---|---|---|
| idle | 1219, 1203, 1220 | 1216, 1214, 1215 | 6143, 5053, 6128 |
| CPU | 1210, 1210, 1210 | 1205, 1205, 1207 | 5638, 5397, 7278 |
| memory | 1210, 1207, 1210 | 1205, 1206, 1208 | 7069, 6854, 5392 |
| I/O | 1219, 1219, 1220 | 1215, 1215, 1205 | 4960, 5457, 5836 |

Representative B process deltas over approximately 1.01–1.02 s: idle 0 ticks and 2,453,504 RSS bytes; CPU 101 ticks and 2,318,336 RSS bytes; memory 101 ticks and 35,983,360 RSS bytes; I/O 0 ticks and 8,388,608 written bytes. Tick frequency is kernel-defined. These totals include process and system sampling plus policy evaluation: approximately 0.83–1.21 ms per sample. Scheduling and 10 ms exit polling dominate small wall-time differences, so the pairs cannot establish a reliable end-to-end overhead percentage or improvement. This run used a named temporary file removed at worker exit; afterward cleanup was hardened to unlink immediately after opening, and the final release I/O worker was separately run successfully under a five-second timeout with no named file left. A kill between creation and unlink can still leave a file; inspect `/tmp/xynoos-017-*` after abnormal termination.

During the table run, decisions were Observe at low system pressure (e.g. CPU 1.01%, memory 0%, I/O 0.95%). An earlier real run under different host activity saw CPU PSI 11.62% twice and returned Recommend, explicitly describing sustained visible-system CPU pressure with target impact and benefit unproven. The release CLI independently returned Observe then Recommend for CPU PSI 11.36%. These avg10 readings are shared, overlapping windows, not proof the test workload caused pressure. Existing deterministic tests retain threshold, stale and missing-signal refusal coverage; no thresholds were tuned. Workload-specific recommendations and a real insufficient-signal experiment remain unverified.

## Isolated kernel test procedure

On a disposable isolated Linux VM or physical machine, obtain explicit authorization and arrange an exclusively controlled, non-root-owned delegated cgroup v2 parent, distinct from the mount root, with `memory` already enabled in `cgroup.subtree_control`. Coordinate other writers; do not use Codespaces host-owned cgroups. Confirm current telemetry is available and run:

```sh
XYNOOS_KERNEL_TEST=1 XYNOOS_CGROUP_PARENT=/absolute/path/to/delegated/parent \
  cargo test -p xynoos-linux-control --test kernel_transaction -- --ignored --nocapture
```

The test creates an empty disposable child, applies and reads back `memory.low`, restores and verifies the original, and removes the child. Capture the output separately from fixture results. On error inspect for `xynoos-test-*` children and investigate before manual cleanup. Concurrent writes, crashes and non-atomic checks remain hazards. It does not control running applications.

## Next steps

Add verified cgroup identity and scoped PSI without equating membership with exclusivity; connect scoped evidence to the decision contract with explicit refusal and independent source deltas. Measure repeated overhead with finer exit timing and CPU/resource accounting. Test real sustained workload-specific pressure on an isolated delegated machine, then perform the kernel procedure above. Only controlled interventions with repeated outcome measurements can establish an optimization benefit.
