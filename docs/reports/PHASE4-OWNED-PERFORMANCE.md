# Phase 4 finalization: real owned-memory optimization

**Status: PARTIAL.** A real, reversible cooperative memory action is implemented,
measured and suitable for the explicitly scoped operator experiment. It reduces
retained memory for the defined compressible corpus, while increasing CPU and
completion time. No desktop speedup, N5000 active result or production readiness
is established. Physical kernel/cgroup mutation remains **NO-GO**.

Actual starting HEAD was `a189950` on `task-018-real-optimization`, with `4b7b191`
as its parent. Previous physical results (seven baseline, eight observe and one
low-memory refusal) are operator-supplied context, not evidence of optimization.
Their raw laptop evidence was not supplied in this execution. All new numbers
below are **GitHub Codespaces**, Linux x86_64, static musl, 2026-11-16. No laptop,
GUI, swap-pressure or kernel optimization result is inferred.

## Mechanism selection

Estimates before implementation were qualitative because costs depend on data
and request rate. The private corpus and request work are fixed across arms.

| Candidate | Likely benefit | CPU cost | Additional memory | Cooperation / reversibility | Safety / unmodified desktop applicability |
| --- | --- | --- | --- | --- | --- |
| Allocation reuse | Avoid allocation churn; usually no saving in necessary retained corpus | Lower allocator cost, implementation-dependent | One bounded reusable scratch | Application supplies scratch; reversible | Private allocations; cannot transparently alter another process. Used equally in all three arms, not claimed as active gain. |
| Retention/cache shrinking | Up to the disposable cache size | Regeneration and misses may dominate | Small bookkeeping | Application must identify replaceable entries; reconstruct on demand | Private only; would change work if required data were simply discarded. Not selected. |
| Lossless owned compression (selected) | Zero for entropy; plausibly several MiB on a 16 MiB compressible corpus | Compress + verify once, decode on reads, restore at end | 1 MiB common scratch plus at most one bounded codec candidate | Reads must use the store; reverse to original raw representation | One owner, no foreign PIDs/paths/control writes; cannot transparently manage Firefox/VS Code. |
| Work admission/backpressure | Lower peak in-flight buffers | Queueing and possible throughput/latency loss | Bounded queue metadata | Application must route requests; drain/reopen on reversal | Safe private queues; simply doing less required work would not be a fair gain. Not selected for this closed-loop workload. |
| OS control of arbitrary applications | Benefit unproven here | Unknown | Unknown | Requires supported kernel control and/or cooperation | Existing physical admission stays disabled; not selected or executed. |

The selected path uses safe LZ4 already present in the project and a small
single-owner representation, separate from simulation and production mutation.
It does not use the simulation PageStore to hold an extra compressed copy beside
all original buffers. Each candidate is decoded into shared scratch and compared
exactly to its raw block before replacement. `Box<[u8]>` trims codec capacity;
raw allocations are freed. Expanding or <12.5%-saving candidates stay raw.
There is no discard of required content, regeneration during requests, external
cache hint, hidden system setting, or kernel memory controller operation.

## Working integration

`PressureIQ collect_system` → existing contract/pressure state machine via
`Engine::evaluate(Mode::Observe)` → two fresh private retention observations →
`AURA cooperative::choose` → private typed `Choice` → opt-in/freshness/availability
admission → owned block replacement → exact equality and read CRC verification →
typed report and final reverse verification. Reports include AURA's actual choice,
host pressure state/PSI, refusal reason, changed blocks, retained bytes and cost.
Observe evaluates the same policy but cannot execute its choice.

The local 8 MiB retention budget is an application budget, not fabricated host
pressure. Live CPU/I/O pressure backs off; absent/stale/future/reset telemetry
refuses. Memory availability floor is 256 MiB. Two fresh observations enter
retention pressure, low retention clears it, continuity gaps reset it. All
attempts (including entropy/no-benefit) have a 10-second cooldown. Each block
rechecks <=1-second telemetry freshness; a late batch reports a partial stop.
A cancelled Corpus cannot serve more work. No public executable plan or external
resource identifier is accepted. Limits and failure handling are detailed in
[the safety review](PHASE4-OWNED-SAFETY-REVIEW.md).

## Matched measured results

Each mixed session has a recorded excluded ABC warmup and six measured triples,
one of every permutation. Every arm performs 128 matched requests, reading and
hashing 4 MiB per request across the same 16 MiB corpus; every block is visited
32 times. All outputs and independently regenerated final bytes match. Both
sessions completed all 18 measured arms without a failed/refused policy sample
or kill escalation. There were 12 real active actions and 192 raw blocks replaced
across the two sessions. Baseline/observe replaced zero blocks. All 42 arms,
including warmups, restored and exited successfully.

Medians are per-run where applicable; response quantiles pool requests within
one session. Steady retention/RSS excludes the first eight responses. CPU and
wall include initialization, policy, candidates, verification and restoration.
Completion includes 10 ms think time. Peak HWM includes restoration and is not
inferred from fork-inherited wait4 RSS. Raw process/host samples remain available.

### Codespaces mixed session 1

| Metric | Baseline | Observe | Active |
| --- | ---: | ---: | ---: |
| Response p50, ms | 0.711706 | 1.564816 | 1.513480 |
| Response p95, ms | 0.842870 | 1.918435 | 1.975652 |
| Response p99, ms | 1.184467 | 2.270081 | 2.764144 |
| Completion p50, s | 1.424700 | 1.538384 | 1.556017 |
| Whole-arm wall p50, s | 1.525409 | 1.640440 | 1.668046 |
| Worker CPU p50, ms | 182.432000 | 293.579000 | 323.159000 |
| Steady payload bytes | 17,825,792 | 17,825,792 | 5,390,498 |
| Steady RSS p50, KiB | 18,208 | 18,344 | 6,204 |
| Peak HWM p50, KiB | 19,200 | 19,236 | 19,508 |
| Harness CPU p50, ms | 59.681845 | 61.613193 | 62.949601 |
| Harness RSS p50, KiB | 26,364 | 26,092 | 26,836 |
| Restoration p50, ms | 54.472667 | 54.782394 | 65.322467 |

### Codespaces mixed session 2

| Metric | Baseline | Observe | Active |
| --- | ---: | ---: | ---: |
| Response p50, ms | 0.711596 | 1.560168 | 1.511417 |
| Response p95, ms | 0.856106 | 1.915750 | 1.949523 |
| Response p99, ms | 1.168939 | 2.254458 | 3.122049 |
| Completion p50, s | 1.426164 | 1.537054 | 1.559058 |
| Whole-arm wall p50, s | 1.526824 | 1.638236 | 1.669411 |
| Worker CPU p50, ms | 182.623000 | 293.308000 | 323.158000 |
| Steady payload bytes | 17,825,792 | 17,825,792 | 5,390,498 |
| Steady RSS p50, KiB | 18,212 | 18,352 | 6,188 |
| Peak HWM p50, KiB | 19,200 | 19,208 | 19,516 |
| Harness CPU p50, ms | 60.733497 | 60.430582 | 63.287287 |
| Harness RSS p50, KiB | 26,340 | 26,064 | 26,812 |
| Restoration p50, ms | 54.327367 | 54.312419 | 64.559957 |

Payload retention fell **69.76%**, from 17,825,792 to 5,390,498 bytes including
scratch. Sampled steady RSS fell **65.93% / 66.02%** versus baseline, roughly
11.7 MiB per worker. Peak HWM did not improve: active median was 19,508/19,516 KiB
versus baseline 19,200/19,200 KiB. The original raw corpus and full restoration
still require memory. This mechanism cannot rescue unsafe initial admission.

The costs are material. Active worker CPU rose **77.14% / 76.95%** versus baseline
and **10.08% / 10.18%** versus observe. Paired active completion ratios ranged
**1.0885–1.1039** and **1.0877–1.1005** versus baseline: slower in every measured
triple. Observe itself adds roughly 8% completion time because this experiment
collects/decides on every request. Active whole-arm wall is also slower than
observe (median 1.6680 versus 1.6404 s in session 1). A smaller active p50 response
than observe does not establish faster completion or an isolated codec speedup.

Active compression action medians were 24.77 / 25.28 ms. Maximum active responses
were **34.25 / 29.78 ms**, compared with baseline **1.87 / 2.12 ms**. One action
per 128 requests can fall outside pooled p99: do not hide that initial stall.
Restoration costs around 65 ms active versus 54 ms raw on this host. This policy
is unsuitable as a general latency optimization. Retention-constrained,
cooperative, compressible storage is the demonstrated benefit; avoiding actual
swap or improving a desktop under pressure remains unproven.

Minimum sampled MemAvailable across mixed sessions was 14,066,984 KiB; this was
not a 4 GB host under memory pressure. Shared CPU busy fractions ranged roughly
9.8–18.7%. Swap deltas and memory-PSI deltas were zero, with no visible ZRAM devices.
CPU/I/O PSI, cgroup-root context and environmental metadata are in raw evidence.
There are no significance or causal desktop claims. The deterministic memory
saving repeated across both sessions, while timing also reflects host noise.

### Entropy negative control

A separately balanced six-triple control used **64 requests in every arm**.
Do not compare its completion/CPU directly with the 128-request mixed sessions.
All 18 measured arms completed with matching output/restoration, zero failures
and no policy telemetry refusal. Active attempted compression once in each run,
reported `no_benefit`, replaced zero blocks and retained exactly 17,825,792 bytes.
Steady RSS medians were 18,248 / 18,340 / 18,360 KiB for baseline/observe/active.
Worker CPU medians were 142.685 / 196.783 / 206.508 ms; completion medians were
0.707438 / 0.763828 / 0.773545 s. Active p99 response was 13.742 ms and the
no-benefit attempt median was 12.044 ms. Entropy earns no memory benefit and
pays overhead; cooldown prevents repeating the failed attempt during these runs.
The result is preserved, not filtered or turned into a performance PASS.

## Failure, adversarial review and gates

A real harness SIGTERM was delivered immediately after the active warmup
replacement. The worker reversed all compressed blocks, independently verified
them, reported cancellation, exited zero and was reaped without escalation.
The session correctly remains **incomplete**, excluded from performance. A
separate explicitly labelled low-memory admission fixture preserves refusal with
no child launch; no host OOM or synthetic pressure is mislabelled live evidence.

Tests cover exact raw/compressed output equivalence, actual allocation savings,
restoration, entropy/no-benefit, missing/reset/stale telemetry, low memory,
pressure transitions, cooldown/backoff, cancellation latch, corrupt raw/packed
blocks and partial reverse failure. Subprocess tests cover no authorization,
resource-limit read-back, EOF reverse, independent idle watchdog exit 124,
request/input/output bounds, child death/stall, signal cancellation, and immutable
partial-log inspection. Contradictory terminal markers, failed reaping and output
mismatches cannot promote evidence to complete. The separate physical baseline
lifecycle tests still exercise real OS signal delivery and kill escalation.

`PHASE4-OWNED-QUALITY-GATES.txt` records passing formatting, warnings-denied
Clippy, all-target workspace check/tests, compile-fail doctests, 48 initial Python tests,
optimization-example tests, release workspace build, warnings-denied rustdoc,
20 launcher tests, static musl builds and diff checks. Adversarial self-review and
corrections are recorded in `PHASE4-OWNED-SAFETY-REVIEW.md`. No independent
reviewer or physical kernel validation is claimed. Production mutation-lock
sources were unchanged and their regression tests passed.

## Reproduction, provenance and exit decision

```sh
cargo build --locked --release --target x86_64-unknown-linux-musl -p xynoos-engine --bin xynoos-owned
python3 scripts/benchmark_owned.py --worker target/x86_64-unknown-linux-musl/release/xynoos-owned --environment Codespaces --label 'actual Codespaces session' --authorize-owned --corpus mixed --repeats 6 --requests 128 --interval-ms 10 --output /tmp/owned-new
python3 scripts/inspect_owned.py /tmp/owned-new/events.jsonl
python3 scripts/package-owned.py
```

Evidence lives under `artifacts/phase4-owned/`: `codespaces-mixed-1`,
`codespaces-mixed-2`, `codespaces-entropy`, `codespaces-cancelled` and
`refusal-fixture`. Raw manifests contain worker/harness SHA256, base HEAD,
working-source hashes, matched input configuration, UTC, CPU/OS/kernel and host
context. Recorded source hashes match commit `d951c5b`. A subsequent reporting-only fix
preserves missing peak-HWM readings as unknown instead of zero, with a new
regression test; recomputing all three successful session summaries gives exactly
the same values. Final Python validation is in `final-python-tests.txt`. The measured
static worker SHA256 is
`4ced2f4498a0df87cf66a76d18b30fae37d6c81261a41adc1e7828e6a6fe8ff6`.
Package `SOURCE_COMMIT`, source archive/manifest, compiler metadata, internal
SHA256SUMS and external archive checksum identify the exact distributable source.
`artifacts/phase4-owned/PACKAGE.json` records the final package revision/digest.

| Exit criterion | Result |
| --- | --- |
| Real action changes owned workload resources | YES: raw blocks replaced, originals freed |
| Directly measured effect and output equivalence | YES: payload/RSS, matching reads, full equality on reversal |
| Safety, cancellation, bounded execution and cleanup | YES within the tested private Codespaces scope; crash/power-loss limits explicit |
| Repeated meaningful benefit for a defined workload | YES for steady memory of mixed corpus; NO speedup, NO entropy benefit |
| Limitations explicit | YES: cooperation required; peak/CPU/stall costs retained |
| Required N5000 active physical evidence | **MISSING** |

**GO:** reviewed, explicitly authorized N5000 evaluation of this disposable
cooperative corpus only, using [the exact OpenCode procedure](../PHASE4-OWNED-N5000.md).
**NO-GO:** enabling physical cgroup mutation, claiming transparent desktop
optimization or calling Phase 4 complete. Next experiment: two separate matched
six-triple N5000 mixed sessions (128 requests per arm), followed by the matched
entropy control, preserving low-memory refusals and cancellation evidence.
If laptop admission refuses or CPU/latency costs outweigh the memory benefit,
retain that result and keep Phase 4 PARTIAL/NO-GO for broader deployment.
