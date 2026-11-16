# Phase 4 performance engineering result

Implemented and executed a real bounded userspace workload A/B harness. Baseline
and live observe arms are **GO for operator-run validation**. Active optimization
is **NO-GO / unavailable**; physical kernel mutation remains locked. The only
production runtime optimization is lower-allocation observation report generation.
No physical desktop improvement or production readiness is established.

All new measurements below ran in **GitHub Codespaces**, Linux x86_64, on
2026-11-16. CPU model, OS, kernel, raw counters, binary/harness SHA256 and input
configuration are in the raw manifest. Starting source commit:
`0a25347db420074f49e8017f34169dbeb50ddc6a`; working-source hashes are in
`artifacts/phase4/PROVENANCE.json`. The operator's previous N5000/4 GB/MX Linux
PASS results and medians are context supplied by the operator. Raw laptop evidence
was not inspected, and this cycle supplies no new N5000 measurements.

## Profile → implement → measure

`xynoos-perf profile` times live PressureIQ collection, fixture-driven state
machine+AURA, isolated AURA, real verified HyperMemory roundtrips, telemetry
contract construction and report serialization. It uses bounded iteration counts,
no perf privileges and no added dependencies. State-machine samples advance
synthetic monotonic timestamps to exercise the valid path and bounded history;
they are algorithm timings, not live policy outcomes. AURA-only timing is near
the clock/black-box floor. HyperMemory uses 64 identical compressible 4 KiB pages;
these figures do not characterize arbitrary entropy or memory pressure.

An initial six-process profile is in `artifacts/phase4/profile-before.json`.
PressureIQ collection dominated at roughly 0.5 ms. Report materialization was a
smaller measurable source of repeated allocation: `json!` cloned the contract
and plan into a JSON value tree before allocating the output string. The human
path also built that unused tree. Replaced that path with borrowed typed
serialization through a buffered writer. The schema, telemetry errors, refusal
reasons and write locks are preserved; no sampling, freshness, checksum or
safety work was removed. A schema-equivalence test covers every replay step
and both observe/refused-controlled plans.

The profiler's old stage creates the original JSON tree and serializes to a Vec;
the new stage serializes borrowed fields directly to a Vec. Vec output gives both
paths the same sink; stdout/syscall costs are outside this component comparison.
The initial profiler binary was preserved before the change. Six process pairs
alternate before/after and after/before; raw individual samples, order and binary
hashes are in `artifacts/phase4/profile-ab.json`.

| Component median, ns | Before | After |
| --- | ---: | ---: |
| PressureIQ live collection | 503,699 | 498,308.5 |
| State machine with AURA | 5,290 | 5,300 |
| AURA only | 30 | 30 |
| HyperMemory 64-page verified roundtrip | 75,581 | 85,028.5 |
| Contract build | 9,487 | 9,518 |
| Report build + serialization | 20,569 | 5,250 |

The measured report-stage median fell **74.5%** (15.319 µs). This is a component
result, not a 74.5% end-to-end improvement. Unchanged stages vary, including a
12.5% HyperMemory increase in these separate binaries/processes; that code was
not changed. This experiment does not distinguish noise, allocator effects or
compiler/code-layout effects for that difference. It is not suppressed. No
HyperMemory speedup is claimed. Collection remains dominant; no caching or
telemetry omissions were introduced without evidence of safe equivalence.

## Test → benchmark

The new physical harness starts a fixed owned worker subprocess, waits for
readiness, sends one copy/CRC request at a time, and measures actual response and
completion times. Observe runs exactly the same worker plus real live sampling,
decisions and JSON generation. A recorded warmup pair is excluded, followed by
eight AB/BA balanced pairs, 200 verified requests per arm per pair. CRC identities
match across all runs. All 16 measured arms completed with verified reaping and
no kill escalation. The raw JSONL also preserves both warmup arms.

Reproduction from a release build:

```sh
cargo build --release -p xynoos-engine --bin xynoos-perf -p xynoos-cli --bin xynoos-mvp
python3 scripts/benchmark_physical.py --environment Codespaces --label 'Codespaces Linux x86_64, real owned workload, no physical laptop' --repeats 8 --requests 200 --output /tmp/xynoos-phase4-new
python3 scripts/inspect_physical.py /tmp/xynoos-phase4-new/events.jsonl
```

Measured evidence: `artifacts/phase4/codespaces-ab/events.jsonl`, `summary.json`
and `inspection.json`. Latency quantiles are nearest-rank pooled requests;
completion quantiles summarize eight runs. See the exact methodology and
limitations in `docs/PHASE4-N5000.md`.

| Codespaces metric | Baseline | Observe |
| --- | ---: | ---: |
| Requests | 1,600 | 1,600 |
| Response p50, ms | 1.491179 | 1.471191 |
| Response p95, ms | 1.777152 | 1.763727 |
| Response p99, ms | 2.003488 | 1.937839 |
| Worker service p50, ms | 1.408896 | 1.391924 |
| Completion p50, s | 2.366302 | 2.371880 |
| Completion CV | 0.393% | 0.287% |
| Worker CPU p50 per arm, ms | 292.557 | 290.033 |
| Observer CPU p50 per arm, ms | unavailable: absent | 5.497 |
| Sampled worker RSS p50, KiB | 4,428 | 4,488 |
| Sampled observer RSS p50, KiB | unavailable: absent | 2,680 |
| Harness CPU p50 per arm, ms | 62.751 | 67.976 |
| Timer overshoot p99, ms | 3.319 | 3.706 |

Observe completion paired ratios range 0.9927–1.0115; the mixed latency and
completion differences do not support a workload benefit. The independent
harness is deliberately measured: it consumes CPU and roughly 24–26 MiB sampled
RSS during this run. Its sampling/logging and polling contribute to latency.
A host with 4 GB should still measure that overhead rather than substitute the
old MVP's much smaller process RSS. Wait4 peak RSS can inherit fork high-water
memory, so sampled loaded-process RSS/HWM are retained separately.

Minimum visible MemAvailable was 14,127,852 KiB, not a 4 GB or stressed target.
CPU busy context ranged roughly 9.8–16.0%; background noise was present. Swap
page deltas were zero, no ZRAM devices were visible, and memory PSI total deltas
were zero in the sampled windows. CPU/I/O PSI and mounted cgroup root context
remain in raw data. These shared counters cannot be causally attributed to an
arm. Timer overshoot is a scheduling/measurement proxy, not a desktop latency
measurement. No GUI claim follows from it.

## Failure, lifecycle and review

Workspace/CLI tests retain cancellation, stale/missing/reset telemetry, pressure
backoff, authorization refusal, private-buffer checksums and recovery diagnostics.
New harness tests exercise real child reaping, worker/observer death, timeout,
oversized output, MemAvailable loss, injected low memory and mounted-root
headroom, SIGINT/SIGTERM/SIGHUP cancellation, SIGKILL escalation, output collision,
incorrect Codespaces labels, and partial/corrupt evidence inspection. Pressure
faults are injected fixtures; the host was not deliberately driven into OOM.
Crash-tail fixtures represent incomplete persistence; there is no power-loss or
physical OOM recovery claim.

Adversarial self-review fixed two lifecycle/evidence issues before delivery:
failed child setup now reaps its subprocess, and a terminal success marker cannot
promote failed/missing arm evidence. Cleanup failures remain explicit and other
owned children are still cleaned. Signals target only owned, unreaped child PIDs,
which cannot be reused before wait4. There are no descendant processes or process
group-wide kill operations. A missing terminal record means cleanup unknown;
read-only inspection never signals a historical PID or replays a mutation.

The worker and serializer forbid unsafe Rust. The worker admits only bounded
`work` input and fixed buffers; neither accepts PIDs, kernel paths, arbitrary
commands, variable stress sizes or control operations. The static package excludes
the broad research CLI and launchers. `xynoos-mvp` still admits only observe,
simulate and read-only recovery; controlled mode is rejected even with an
authorization flag. Existing physical execution admission remains unconditional
refusal. No mutation API was opened.

Quality gate output is in `docs/reports/PHASE4-QUALITY-GATES.txt`: formatting,
Clippy all targets with warnings denied, workspace all-target check and tests,
Python benchmark/cleanup tests, optimization-example tests, launcher tests,
release workspace build, rustdoc with warnings denied, musl builds, file/static
inspection and diff checks. Follow-up Python tests cover the tightened inspector
and explicit kill escalation; logs are preserved in `artifacts/phase4/benchmark-tests.txt`.
The previous MVP benchmark also passed a six-repeat integrity/trace regression;
its synthetic-pressure arms are separately labelled in `artifacts/phase4/mvp-regression.json`.
An intentionally cancelled real Codespaces harness run is retained in
`artifacts/phase4/codespaces-cancelled/`; it is incomplete and excluded from
performance summaries. No independent human or external reviewer is claimed.

## Experiment decisions and package

| Arm/action | Decision | Evidence / reason |
| --- | --- | --- |
| Baseline owned-workload measurement | GO | Bounded real work; successful Codespaces runs; operator-run N5000 protocol supplied |
| Live observe comparison | GO | Read-only overlapping observer; measured overhead and verified cleanup |
| Report serialization optimization | GO | Equivalent schema; measured component improvement; no safety changes |
| Live userspace compression intervention | NO-GO | No demonstrated workload benefit or representative memory saving; retaining validation/raw buffers can erase savings |
| Priority/affinity or cache-hint intervention | NO-GO | No demonstrated bottleneck/benefit; not added speculatively |
| Physical kernel/cgroup/sysctl/swap/ZRAM mutation | NO-GO | Disabled; no interfaces activated or live control writes |
| Desktop speedup claim | NO-GO | No reproducible physical desktop A/B evidence |

The package builder verifies a committed source revision, builds static musl
x86_64 executables, checks ELF headers for absence of an interpreter, smoke-tests
the actual safe entry points, then emits a source archive, source manifest,
internal SHA256SUMS, an archive checksum and `artifacts/phase4/PACKAGE.json`.
The final package manifest identifies the exact source commit and archive digest;
archive binaries are local build artifacts rather than invented laptop evidence.
Use `docs/PHASE4-N5000.md` for exact non-root N5000 commands and interruption
handling. Physical results remain pending local execution.
