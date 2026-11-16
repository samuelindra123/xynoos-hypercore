# Phase 4 laptop optimization cycle: no demonstrated benefit

The integrated engineering cycle is implemented, reviewed, tested and packaged.
**The performance success criterion is not met.** Physical N5000 evidence is
absent. No desktop, Firefox or VS Code latency improvement is established.
Physical kernel control remains NO-GO, with all prior locks unchanged.

## Diagnosis and selected intervention

The N5000's actual bottleneck remains unknown until the physical diagnostic is
run during its ordinary lag. The new diagnostic distinguishes evidence for CPU
contention, memory reclaim, swap/ZRAM activity and block I/O; it explicitly leaves
attribution unresolved when telemetry is absent or signals overlap. It records
host/process/ancestor-cgroup context, CPU quotas, thermal and power information.
It neither induces memory pressure nor assumes low RAM explains all lag.

After reviewing Linux PSI, cgroup memory protection/throttling, systemd pressure
cooperation and existing OOM projects, this cycle selects **cooperative source
index admission** instead of another compression or kernel-reclaim abstraction.
Foreground work searches real repository source. Background work builds complete
word-frequency indexes on that same corpus. Active mode admits no new chunk while
a search is pending, within 5 ms of the next arrival, or under measured pressure.
It never preempts an in-flight chunk or manipulates unrelated applications.
Independent whole-corpus oracles and cross-arm equality verify completed work.
See [research](../PHASE4-LAPTOP-RESEARCH.md) and [safety review](../PHASE4-LAPTOP-SAFETY.md).

## Codespaces measurements, not physical results

Raw evidence: `artifacts/phase4-laptop/session1/` and `session2/`, each with
`events.jsonl`, `summary.json` and independent read-only `inspection.json`.
Each contains an excluded ABC warmup and six measured triples, one of every
permutation. Every measured arm completed 120 fixed-arrival source searches
(20 ms spacing) and 16 index refreshes. No CPU affinity, memory pressure, swap,
ZRAM or desktop setting was changed. All 42 arms including warmups completed,
workers released state and were reaped; both inspections report complete.

The 918,398-byte source corpus (exact length and per-file SHA256 are in
`source-corpus.manifest.json`) is cached and intentionally modest. It is not an
IDE language server, large workspace, browser tab corpus or eMMC simulation.
The source/manifest are preserved so the exact measured input is available.
The final package regenerates its corpus from its committed source; do not pool
results from different corpus hashes. Provenance and safety hardening followed these first two sessions.
`final-source-session/` repeats the same input and full balanced workload with
the final harness, including immediate freshness/short-write checks and final
telemetry; the static package is additionally smoke tested. All timing evidence is preserved without rewriting the observations.
These are native x86_64 GNU release timings; the separately checked musl
distribution has functional smoke evidence, not equivalent performance claims.

| Codespaces session | Arm | Median run p50 / p95 / p99 (ms) | Median paired p95 ratio to baseline | Median paired index completion ratio |
| --- | --- | --- | --- | --- |
| 1 | baseline | 2.272 / 4.609 / 5.495 | 1 | 1 |
| 1 | observe | 2.238 / 4.824 / 5.730 | 1.029 | 0.955 |
| 1 | active | 2.311 / 4.932 / 6.344 | 1.058 | 1.953 |
| 2 | baseline | 2.280 / 4.598 / 5.324 | 1 | 1 |
| 2 | observe | 2.221 / 4.413 / 5.772 | 0.970 | 1.036 |
| 2 | active | 2.265 / 4.609 / 5.988 | 0.966 | 1.695 |
| final source | baseline | 2.252 / 4.710 / 5.848 | 1 | 1 |
| final source | observe | 2.280 / 4.750 / 5.754 | 1.067 | 0.975 |
| final source | active | 2.264 / 4.530 / 5.825 | 0.889 | 1.714 |

Quantiles use nearest rank. Median paired ratios are not ratios of the table's
median quantiles. Session 1 active p95 ratios ranged 0.724–1.171; session 2 ranged
0.800–1.416. These small samples support no significance claim. The active median
p95 worsened 5.8% in session 1 and improved 3.4% in session 2, while background
completion worsened 95.3% and 69.5%. The final-source session adds another 21 complete arms including warmup, with
verified final telemetry and exits. Its median paired active p95 improved 11.1%
(ratios 0.612–1.238), while index completion worsened 71.4%; this still fails
the proposed background budget and does not replicate across sessions.
Median index duration was 0.497/0.473/0.949 s
for A/B/C in session 1 and 0.468/0.483/0.803 s in session 2. Foreground completion
stayed approximately 2.38 s because the fixed arrival schedule dominates it.

Only 2/1 baseline foreground requests (of 720 per session) arrived with an index
chunk in flight; active had zero. Thus the host offered little useful contention
for this intervention. Memory PSI some/full deltas were zero, swap activity was
zero, and no ZRAM device was visible. CPU and I/O PSI were nonzero shared-host
context. No measured source-search latency exceeded 1 s; no global OOM counter
increment was observed in sampled windows. These statements do not establish
absence of desktop freezes, app OOMs or recovery problems on a physical laptop.
No browser/GUI measurement was collected in Codespaces.

Median aggregate child CPU per run was 0.463–0.477 s across arms; median harness
CPU was 0.451–0.483 s. This short diagnostic harness uses roughly 19–20% of one
CPU core during a 2.38 s workload, including oracle, sampling, identity validation
and logging overhead; it is **not** suitable as a permanently running desktop
optimizer. The standalone 5.06 s diagnostic window used 0.072 s CPU (about 1.4%
of one core), with 66,219 us CPU some PSI, zero memory PSI and 2,069 us I/O some
PSI. Namespace-visible counters are context, not effects attributable to Xynoos.

Maximum sampled RSS across these measured runs: search 3,436 KiB, index 3,668 KiB,
observer 2,856 KiB and harness 41,560 KiB. Sampling misses short peaks; VmHWM and
wait4 values remain in raw evidence. The final-source session reached 42,052 KiB sampled harness RSS, 3,428 KiB
search, 3,668 KiB index and 2,832 KiB observer; it again observed zero memory
PSI/OOM deltas and no >1 s search. This is not a memory-saving result.

## Adversarial review and validation

Substantiated issues fixed during review:

- Pin executable inode and signal pidfds; reject unexpected UID/PPID/start ticks,
  executable/cgroup identity, process descendants and writable/set-ID binaries.
- Seal the corpus against concurrent changes and check whole-corpus search/index
  oracles, sequences, complete refresh counts and final release acknowledgements.
- Start arrivals at readiness, include backlog in response latency, expose overlap
  and deferral, keep baseline/observe sampling symmetric and report background cost.
- Reject missing/stale/reset PSI/VM/cgroup telemetry and insufficient host/ancestor
  headroom; bound token extension, memory, CPU, requests, wall time and output.
- Default to A/B; require a separate explicit flag for C. No controller lock changes.
- Treat cancellation, unexpected reports, ambiguous cleanup and partial logs as
  incomplete. Parent SIGKILL test verifies worker exit but leaves cleanup unknown
  in the crashed log; no stale PID recovery or action replay is attempted.
- Add harness/diagnostic hashes, monotonic/wall anchors and final post-exit telemetry
  read-back to the final package; a final OOM/reset/refusal cannot promote success.

Workspace Rust/all-target and doc tests (including nine API compile-fail tests),
Python tests, shell launcher tests, formatting, Clippy with warnings denied,
release builds, rustdoc with warnings denied and static musl build all passed.
`artifacts/phase4-laptop/quality-gates.txt` preserves the full run; new policy and
lifecycle cases are also in `desktop-tests.txt`. Browser JS passes Node syntax
checking; physical browser behavior is unvalidated. Python source compilation
and staged/extracted distribution checks are recorded in package validation.
The full Python rerun passed 67 tests before the final partial-write case was
added; the final targeted suite passed all 19 cases, including that addition.
Static package provenance includes source commit, source SHA256 manifest, compiler,
target and complete source archive. Package metadata and the archive checksum are
in the commit-specific delivery directory.

## Progress and recommendation

Delivered: physical diagnostic; measurable source-workload A/B/C harness; optional
manual browser proxy; bounded cooperative experiment; deterministic policy and
adversarial lifecycle tests; safety/recovery assessment; static x86_64 package;
SHA256/source provenance; exact physical/OpenCode protocol.

Remaining: diagnose the actual N5000 lag, independently admit any userspace C run,
collect at least three matched physical sessions, validate GUI/application proxies
and evaluate declared latency/CPU/background/recovery budgets. Kernel mutation
requires all ten independent checklist items in the safety assessment and a new
explicit authorization; it stays disabled regardless of this experiment's flag.

**Change the technical approach:** do not advance private compression or automatic
kernel tuning as a desktop optimizer on this evidence. Cooperative admission did
not show a reproducible benefit here and imposed a large background delay. Use the
new diagnostic to identify an actual laptop bottleneck, then integrate bounded
cooperation with an actual restartable application task or evaluate the existing
Linux/systemd facility that addresses that measured bottleneck. If physical data
also shows no relevant benefit, retire this admission policy rather than adding
more pressure abstractions. Phase 4 remains **PARTIAL / performance unproven**.
