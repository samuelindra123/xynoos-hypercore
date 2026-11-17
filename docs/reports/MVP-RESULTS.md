# Integrated MVP results — 2026-11-16

Delivered scope: working read-only observation, deterministic pressure/AURA policy
decisions, a bounded private-buffer executor, structured/human CLI, conservative
simulation evidence and restart diagnostics, repeatable comparisons and a static
Linux x86_64 entry point. **Physical mutation NO-GO; production readiness NO.**

## Repository and reuse

Initial HEAD: `3c3402efad45d23eb9f047d085e21cf05d468a7e`, branch
`task-018-real-optimization`, GitHub PR #14 open/draft, same remote head. There
were no tracked edits or AGENTS.md instructions. Existing `SHA256SUMS`, `ss` and
six distribution/checksum archives were preserved. No reset, merge, live systemd
launch, stress tool or physical laptop experiment was performed.

The audit covered core types, HyperMemory allocation/accounting/CRC, AURA baseline
and research policies, PressureIQ source contracts, Linux mutation sinks, private
ControllerSession/Adapter/SessionIo, coordinator/worker transactions, launcher,
CLI, benchmark and tests. Working components were reused; the composition is
documented in [architecture](../MVP-ARCHITECTURE.md). Existing physical locks and
fault-injection implementations remain authoritative for their scope.

## Actual benchmark evidence

Final machine-readable evidence: [raw runs](../../artifacts/mvp/benchmark-final.json)
and [summary](../../artifacts/mvp/benchmark-final.summary.json). Environment:
Codespaces x86_64, AMD EPYC 7763, Linux 6.8.0-1070-azure, glibc 2.39. These are not
measurements of the N5000, MX Linux, four-GB pressure or eMMC behavior.
The separately named preliminary outputs preserve the earlier measurement pass;
they are excluded from this report and the package's selected benchmark summary.

Release binaries; 12 processes per arm; 16 responses per process; six balanced
order permutations twice; one discarded warmup per arm. All use matching trace
and workload checksums. Workload input is 64 × 4096 bytes with a fixed mixed seed.
Times below are actual measured work with **synthetic pressure replay**.

| Metric | A baseline | B observe replay | C policy simulation |
| --- | ---: | ---: | ---: |
| Response p50, µs | 134.000 | 145.922 | 168.314 |
| Response p95, µs | 228.926 | 254.504 | 299.679 |
| Response p99, µs | 267.588 | 320.878 | 484.773 |
| Decision p50, µs | 0.070 | 9.708 | 10.088 |
| Engine loop total p50, ms | 2.390 | 2.184 | 3.041 |
| Process CPU p50, ms | 4.150 | 4.004 | 4.862 |
| Process wall p50, ms | 4.595 | 4.600 | 5.673 |
| Reported process VmHWM p50, KiB | 2944 | 3096 | 2984 |
| Sampled loaded RSS p50, KiB | 2988 | 3076 | 3044 |
| Compression steps / 192 | 0 | 0 | 60 |
| Safety refusals / 192 | 0 | 24 | 24 |
| Verified private-buffer workloads | 192 | 192 | 192 |

Each compressed step stored 88,674 payload bytes for 262,144 original bytes;
uncompressed steps stored 262,144. This is representation size for one private
copy, with original corpus, metadata, temporary buffers and runtime overhead
additional. The full-process RSS data does not demonstrate a memory reduction.
Approximate kernel counters sometimes show a sampled RSS larger than final
VmHWM; both raw readings are retained. Inherited Python spawn high-water data is
retained separately and excluded from the process-RSS summary.

CPU and response times show the cost of compression. The lower observe loop/CPU
median in this small shared-host run is not an optimization result: independent
raw allocation/scheduler variance can exceed policy overhead. Small samples,
pooled correlated steps, polling granularity and warm-cache effects limit inference.
There is no statistical or desktop performance claim.

Twelve separate live-observe processes measured PressureIQ collection p50
631.747 µs, p95/p99 711.977 µs; live decision p50 33.652 µs, p95/p99 37.710 µs.
These include real visible-namespace telemetry but apply no action. Captured
host swap-in/out deltas were zero in this run; that is shared-host context, not
proof of swap avoidance by Xynoos. All PSI/meminfo and counters remain in raw JSON.
Method, units and attribution limits: [benchmark contract](../MVP-BENCHMARK.md).

## Recovery and artifacts

[Static observe](../../artifacts/mvp/observe-static.json),
[simulation](../../artifacts/mvp/simulate-static.json) and
[read-only recovery](../../artifacts/mvp/recovery-static.json) commands ran locally.
The actual synced [simulation journal](../../artifacts/mvp/simulation-evidence.jsonl)
contains 49 records for 16 steps plus the terminal record. Its
[restart diagnostic](../../artifacts/mvp/recovery-journal.json) reports
`simulation_restored`, with physical/automatic recovery disabled. This is no
claim of durable physical recovery. Crash prefixes, truncation and corruption
remain unresolved/corrupt and are never repaired or replayed.

`target/x86_64-unknown-linux-musl/release/xynoos-mvp` is a static PIE x86_64 ELF.
It was smoke-tested in Codespaces; MX Linux remains untested. The package script
`python3 scripts/package-mvp.py` packages a clean committed revision, source,
commands, method, report, trace and checksums into
`artifacts/mvp/xynoos-mvp-<source-commit-prefix>-linux-x86_64.tar.gz`.
Only the restricted safe entry point is included; physical admission is excluded.
The archive's `SOURCE_COMMIT` and sidecar SHA256 identify its exact source.

## Tests, gates and review

[Complete gate transcript](MVP-QUALITY-GATES.txt): 218 workspace tests including
eight external API compile-fail tests, five optimization-example lifecycle tests,
and 20 launcher Python fixtures passed, with no ignored tests. Formatting,
all-target Clippy with warnings denied, all-target check, release build, rustdoc
with warnings denied, static build, file inspection and final diff check passed.

The new tests exercise the actual shared engine/CLI/store and existing adapter:
threshold sweep, pressure transition/cooldown, missing/stale/wrong-source/future
telemetry, swap reset, low memory, authorization and physical gate bypass,
cancellation, exact dataset equality, store budgets, journal limits/order/checksum,
every interrupted durable record stage, missing completion, mismatched restart
identity, evidence I/O failure, no overwrites, symlinks/special files, distribution
mode restrictions and end-to-end journal inspection. Existing injected-I/O tests
cover exclusive admission refusal, changed unit/controller/child identities,
activation/partial/ambiguous writes, competing values, failed child cleanup,
worker reaping uncertainty and reverse restoration conflicts.

Adversarial fixes made during this execution:

- Exposed typed active-pressure state instead of parsing policy explanation text.
- Added immediate CPU/I/O backoff despite memory transition cooldown, swap-counter
  reset detection, missing-telemetry history reset and latched cancellation.
- Corrected simulated stale-sample clocks to keep evaluation time increasing;
  bounded the public single-step conversion independently to avoid integer overflow.
- Required complete declared steps and a terminal evidence record; bounded records,
  rejected malformed transitions and poisoned failed journal writers.
- Capped retained session-adapter evidence without discarding history or permitting
  cleanup through exhausted diagnostic state.
- Fixed the existing benchmark's invalid-argument indexing panic and preserved its
  codec mode. Set the CLI default binary so existing cargo-run commands keep working.
- Reviewed syscall and mutation sinks. The approved
  [observe trace](../../artifacts/mvp/observe.strace) shows stdout-only writes and no
  writable opens/control/lifecycle calls in the sampled set. This is a single-path
  audit, not proof about every possible syscall or a physical validation.

Encountered failures/remediation: an initial test used `slice::repeat` on a
non-Copy trace struct (replaced with a bounded cloning iterator); an evidence-cap
test referenced a private constant (kept privacy and asserted its contract);
initial benchmark peak RSS included inherited Python memory (added direct Rust
VmHWM and reran all arms); the gate's first diff check found a README EOF blank
(fixed and repeated successfully). The optional syscall audit first hit the
sandbox ptrace restriction, then passed after explicit escalation approval. No
Clippy/rustdoc warning was suppressed and no failing test was removed.

## Remaining blockers

Read-only observation and the bounded software comparison are usable now. Physical
exclusive delegation, precise live write/lifecycle behavior under competition,
durable physical crash recovery and independent N5000 evidence remain missing.
Some require further engineering as well as hardware validation; they cannot be
resolved by an authorization flag. The physical locks remain unconditional.
The [safety contract](../MVP-SAFETY-RECOVERY.md) and
[N5000 handoff](../MVP-N5000-VALIDATION.md) define those limits. PR #14 remains draft.
