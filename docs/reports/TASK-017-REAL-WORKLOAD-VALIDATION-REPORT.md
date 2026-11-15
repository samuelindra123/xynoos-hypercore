# TASK 017 — Real workload validation

Baseline: main `adb37d2986054c03394599ea3d8453521d5cb0d6`.

- **REAL WORKLOAD TELEMETRY: PARTIAL.** Process counters and shared cgroup PSI were observed. Isolated workload PSI remains unverified.
- **DECISION VALIDATION: PARTIAL.** Real shared-cgroup and visible-system pressure were evaluated. Isolated-scope decisions have fixture evidence only, and recommendations do not establish causation or benefit.
- **REAL KERNEL WRITE: NOT VERIFIED.** No safely delegated parent was established and no cgroup write was attempted.
- **PERFORMANCE IMPROVEMENT: NOT ESTABLISHED.** These are observation-only experiments.
- **WORKSPACE QUALITY GATES: PASS.** Tests, formatting, Clippy, check and documentation passed. The pre-existing duplicate example-name warning remains. The kernel test is ignored by default.

## Environment and scopes

Linux Codespaces `6.8.0-1070-azure`, x86_64, UID 1000, cgroup v2, `/proc/self/cgroup` `0::/init`. The visible cgroup mount is `rw`, but that does not prove exclusive delegation. `/proc/pressure` and `/proc/meminfo` describe the visible system, which may not be the whole host; the container and current cgroup can contain other processes. Do not treat either as exclusively belonging to the child process.

The new process API requires PID and `/proc/<pid>/stat` start time, checks identity around each read, and reports CPU ticks, resident memory bytes, and kernel-accounted I/O bytes with individual monotonic source timestamps and errors. It reports process PSI unsupported; Linux does not provide per-process PSI. CPU and I/O deltas reject mismatched identity, non-increasing time and counter resets; a missing source causes the combined delta to fail explicitly. Counters exclude children. PID start ticks are not a race-free pidfd identity and access restrictions or process exit can prevent reads. The cgroup collector maps the explicit process identity to its visible cgroup; it is not a general workload discovery mechanism. No background polling is introduced.

## Measurements

`cargo run --release -p xynoos-decision --example workload_validation` runs three alternating-order A/B pairs per workload, each child for 1.2 s, sampling every 200 ms in B; A performs no telemetry sampling. Memory is 32 MiB; temporary I/O is capped at 8 MiB. Five-second timeout and process guard bound child lifetime. Wall times include launch and exit polling. The following raw wall times are milliseconds; collection totals are microseconds for all six process/system/decision samples in each B run:

| workload | A wall ms | B wall ms | B collection µs |
|---|---|---|---|
| idle | 1219, 1203, 1220 | 1216, 1214, 1215 | 6143, 5053, 6128 |
| CPU | 1210, 1210, 1210 | 1205, 1205, 1207 | 5638, 5397, 7278 |
| memory | 1210, 1207, 1210 | 1205, 1206, 1208 | 7069, 6854, 5392 |
| I/O | 1219, 1219, 1220 | 1215, 1215, 1205 | 4960, 5457, 5836 |

Representative B process deltas over approximately 1.01–1.02 s: idle 0 ticks and 2,453,504 RSS bytes; CPU 101 ticks and 2,318,336 RSS bytes; memory 101 ticks and 35,983,360 RSS bytes; I/O 0 ticks and 8,388,608 written bytes. Tick frequency is kernel-defined. These totals include process and system sampling plus policy evaluation: approximately 0.83–1.21 ms per sample. Scheduling and 10 ms exit polling dominate small wall-time differences, so the pairs cannot establish a reliable end-to-end overhead percentage or improvement. This run used a named temporary file removed at worker exit; afterward cleanup was hardened to unlink immediately after opening, and the final release I/O worker was separately run successfully under a five-second timeout with no named file left. A kill between creation and unlink can still leave a file; inspect `/tmp/xynoos-017-*` after abnormal termination.

During the table run, decisions were Observe at low system pressure (e.g. CPU 1.01%, memory 0%, I/O 0.95%). An earlier real run under different host activity saw CPU PSI 11.62% twice and returned Recommend, explicitly describing sustained visible-system CPU pressure with target impact and benefit unproven. The release CLI independently returned Observe then Recommend for CPU PSI 11.36%. These avg10 readings are shared, overlapping windows, not proof the test workload caused pressure. Existing deterministic tests retain threshold, stale and missing-signal refusal coverage; no thresholds were tuned. Isolated-workload recommendations in real conditions remain unverified.

## Scoped integration and current evidence

The read-only collector resolves the process cgroup in a single visible cgroup v2 mount and same cgroup namespace, refuses ambiguous mappings and traversal, rejects symlink components, and records directory device/inode identity. It checks PID start time, membership and cgroup identity around each PSI read. CPU, memory and I/O sources retain their own path, monotonic timestamp, parsed PSI (avg10 percent and total microseconds), and independent unavailable/permission/error result. Independent PSI total deltas reject changed identity, source, non-increasing time and counter resets. These sequential checks are not atomic or race-free; PID start-time and inode reuse and concurrent migrations remain limitations.

The scoped engine refuses stale, missing, denied, mismatched and non-increasing inputs. Shared cgroup readings yield Observe, even above the unchanged 10% entry threshold. To use the isolated path, the caller must explicitly attest exclusive ownership of the cgroup and descendants for the entire observation period; point-in-time checks also require exactly the target PID in `cgroup.procs`, no child cgroups, and matching fresh process and cgroup identities. The attestation is an external trust boundary, not a kernel proof of continuous isolation. False attestations, races, hidden processes, namespace ambiguity or migrations undermine the claim. The result is labeled operator-attested rather than independently verified. The collector's point-in-time checks alone never establish exclusivity. Process counters are not process PSI. The legacy engine continues to report visible-system pressure and no longer returns RequestAction; neither engine performs or requests kernel writes.

Real read-only probe (`cargo run --release -p xynoos-decision --example cgroup_probe`): `/sys/fs/cgroup/init`, device 28, inode 8186; CPU/memory/I/O PSI all readable. First `some avg10` values were 3.21%, 0%, 0%; corresponding totals were 89,865,936, 834,973 and 15,707,784 µs. Over approximately 202.6 ms, some totals increased by 72, 0 and 0 µs. Isolation check returned `cgroup not exclusively populated by target process`; the decision was Observe with `shared_cgroup` scope. The shared cgroup contains many PIDs and no controlled exclusive delegated workload cgroup is available. **Isolated workload pressure: NOT VERIFIED.**

Fixture tests, separate from the live probe, exercise isolated high-pressure recommendations after two fresh matching samples, shared-scope refusal, stale and repeated sources, denied and absent PSI, failed isolation checks, path traversal, identity changes, and independent reset handling. The isolated fixture assumes the operator guarantee; it does not prove actual isolation. Live bounded workload samples also showed shared-cgroup Observe at CPU PSI 18.16% while visible-system CPU PSI was 20.08% and the system engine returned Recommend; no attribution to the child or optimization benefit follows.

The updated release harness (three alternating-order pairs per workload) produced these wall times in milliseconds and sampled collection totals in microseconds (six samples per run):

| workload | unsampled wall ms | sampled wall ms | sampled collection µs |
|---|---|---|---|
| idle | 1213, 1210, 1219 | 1211, 1209, 1214 | 18626, 20030, 14503 |
| CPU | 1210, 1211, 1211 | 1203, 1203, 1207 | 14045, 13310, 16693 |
| memory | 1209, 1211, 1210 | 1207, 1206, 1210 | 16173, 20456, 25890 |
| I/O | 1219, 1219, 1219 | 1218, 1213, 1214 | 17421, 13721, 15016 |

The sampled totals include process and system collection, cgroup PSI, isolation checks and both decisions: about 2.2–4.3 ms per sample. Workspace tests ran concurrently during part of this measurement, and scheduling and exit polling limit comparisons; wall times do not establish a meaningful end-to-end overhead percentage. No sampled cgroup collection returned an error. The separate two-sample self-probe took 2578 and 2465 µs including scoped decision. Full workspace tests, fmt, Clippy, check and rustdoc passed; the real kernel test remained ignored. The release CLI shadow probe returned Observe twice at system CPU 1.24%, memory 0%, I/O 2.69%.

## Isolated kernel test procedure

On a disposable isolated Linux VM or physical machine, obtain explicit authorization and arrange an exclusively controlled, non-root-owned delegated cgroup v2 parent, distinct from the mount root, with `memory` already enabled in `cgroup.subtree_control`. Coordinate other writers; do not use Codespaces host-owned cgroups. Confirm current telemetry is available and run:

```sh
XYNOOS_KERNEL_TEST=1 XYNOOS_CGROUP_PARENT=/absolute/path/to/delegated/parent \
  cargo test -p xynoos-linux-control --test kernel_transaction -- --ignored --nocapture
```

The test creates an empty disposable child, applies and reads back `memory.low`, restores and verifies the original, and removes the child. Capture the output separately from fixture results. On error inspect for `xynoos-test-*` children and investigate before manual cleanup. Concurrent writes, crashes and non-atomic checks remain hazards. It does not control running applications.

## Next steps

Strengthen cgroup identity and continuous isolation verification beyond the present externally attested, non-atomic checks. Measure repeated overhead without concurrent build/test activity and with finer exit timing and CPU/resource accounting. Test real sustained workload-specific pressure on an isolated delegated machine, then perform the kernel procedure above. Only controlled interventions with repeated outcome measurements can establish an optimization benefit.

The implementation is useful as a read-only limited foundation, but continuous isolation and a live isolated workload recommendation are not verified. PR #13 remains draft pending those safety and validation gaps.
