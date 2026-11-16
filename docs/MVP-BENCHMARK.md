# MVP comparison methodology

Build release binaries, then use a fresh output path:

```sh
cargo build --workspace --release
python3 scripts/benchmark_mvp.py --repeats 12 --output /tmp/xynoos-mvp-new.json
```

The script reuses `xynoos-bench`, adding `--mvp baseline|observe|simulate` without
removing its existing codec benchmark. Each arm gets a new process and the same
fixed seed, mixed corpus (zero/pattern/structured/entropy), 64 4-KiB pages and
16-step pressure trace. There is one discarded warmup per arm. Twelve measured
repetitions per arm cover all six order permutations twice. Repeats must be a
multiple of six within 6–60. No systemd service, stress tool or physical laptop
is accessed. Each child has a 15-second harness deadline; timeout kills and reaps
only that owned subprocess.

| Arm | Policy | Workload |
| --- | --- | --- |
| A baseline | bypasses pressure/AURA evaluation | raw copy and full byte verification |
| B observe replay | real policy evaluation, reports actions only | identical raw workload |
| C policy simulation | same evaluation, explicitly authorized private compression | adaptive HyperMemory store when selected, full byte verification |

All arms pass through the same trace parsing and reporting shell, dataset creation,
response loop, RSS sampling and verification. A is a baseline for incremental
engine work inside this harness, not a completely uninstrumented application.
Trace conversion uses real PressureIQ parsers and supplies synthetic observations;
it performs no live reads. Dataset generation/checksum precede the engine interval.
`response_ns` includes trace conversion, decision, workload, byte checking, RSS
sample and release. `workload_ns` includes its byte checking, RSS read and release.
Optional evidence callbacks run in all arms but are no-ops for timing comparisons.
Durable fsync overhead is therefore not included. `total_ns` measures the engine
loop and report construction, excluding input generation and final JSON printing.

Per-step decision/workload/response times use Rust `Instant`. Per-process user and
system CPU use `wait4`. End-to-end process wall time includes exec, JSON serialization
and a 1-ms harness polling granularity. The script retains `wait4`'s spawn-inclusive
peak RSS as context; this can include inherited Python pre-exec memory. The summary's
process high-water RSS comes from Rust's own `/proc/self/status` `VmHWM` after the
loop. Current RSS is sampled while each raw/store allocation is still retained.
These observations include allocator/runtime/input overhead and may be quantized.
The kernel's approximate RSS/high-water counters can disagree (including a sampled
RSS exceeding a later VmHWM reading); the artifacts retain both without correction.
Neither is treated as precise live-heap accounting or evidence of memory savings.

Summary quantiles are nearest-rank p50/p95/p99. Step quantiles pool 192 responses
per arm and mix policy states; samples within a process are correlated. Process
quantiles have only twelve samples, so p95 and p99 select the same maximum. This
is descriptive microbenchmark evidence, not a confidence interval or significance
test. Short workloads, shared CPUs and scheduler noise can dominate differences.

Actual host memory PSI/meminfo and swap counters are captured before/after each
process. Missing readings remain null. They describe the visible namespace and
concurrent activity; they cannot attribute pressure or swap changes to Xynoos.
Trace swap counters/PSI are synthetic and never reported as measured activity.
The replay contains stale and missing telemetry, so each policy arm intentionally
reports two safety refusals per run. Every completed workload verifies all pages.
The companion simulation journal diagnostic exercises the evidence protocol;
neither verification count nor journal success measures physical recovery.

Twelve additional **separate live observe** processes measure real PressureIQ
collection and decision overhead with no workload action. They are not an extra
arm of the identical-trace comparison and do not establish low-memory behavior.

Raw JSON preserves every row, process timing/resource reading, host context, binary
hashes, source code hashes and trace checksum. Decisions/recovered bytes are
deterministic; timing and RSS are not. Synthetic page representation savings do
not prove lower total RSS, faster applications or a desktop optimization. A retained
original corpus and temporary compression allocations can increase memory use.

See [results](reports/MVP-RESULTS.md) for the exact measurements and
[N5000 procedure](MVP-N5000-VALIDATION.md) for the physical comparison still required.
