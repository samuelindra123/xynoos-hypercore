# TASK 016 — Workload-aware shadow decisions

Baseline: verified `origin/main` at `789e760b2ab91d425b0f0c2b3a8ab8ce37c83cc8`, merge of PR #11. Linux: Codespaces, `6.8.0-1070-azure`. Results are specific to this visible namespace and run.

- **REAL TELEMETRY: PASS** — PressureIQ v1 supplies real, sequential observations to the engine.
- **SHADOW DECISIONS: PASS** — real workload observations produced explained observe decisions; injected refusal and adapter-routing cases were tested. The available real workloads did not trigger a recommendation.
- **ACTUAL KERNEL OPTIMIZATION: NOT VERIFIED.** No kernel controls were changed.
- **PERFORMANCE IMPROVEMENT: NOT ESTABLISHED.** No A/B test was conducted.

## Policy and scope

`xynoos-decision` rebuilds the typed v1 contract at evaluation time, validates schema, age (maximum 5 s), status, error, value, source, scope and unit for CPU, memory and I/O PSI and meminfo. Missing, failed, stale, non-increasing or ambiguous required inputs decline and clear evidence. It requires two observations no more than 5 s apart; it keeps at most eight. Two consecutive PSI `some avg10` values at or above 10% enter a pressure state; two at or below 5% exit, with a 10 s cooldown after a transition. These are initial explicit heuristic thresholds, not calibrated performance claims. CPU, memory and I/O states are independent; decisions identify the active types. The output is typed observe, recommend, decline or request action, with an explanation. RequestAction requires an explicit matching current-cgroup target and adapter eligibility; the current adapter never reports kernel-write availability, so this state is presently unreachable. A sustained memory recommendation with a valid explicit target invokes read-only adapter preparation, preserving its audit and refusal. No transaction or execute API is called.

The visible-system PSI and meminfo are kernel views exposed inside the VM/container; they do not establish target-cgroup pressure or host-wide state. The current-cgroup memory gauge and CPU and I/O counter deltas are contextual, encompass other tasks in that cgroup, and do not attribute activity to the CLI or a particular application. First samples, counter resets, source changes and missing counters yield no delta. There is no process-local attribution, foreground detection, GPU utilization, video decoding or application identification. Future Firefox, YouTube, VS Code and PlatformIO profiles need explicit identities and reliable per-workload signals before affecting policy. AURA's policy boundary remains advisory: its synthetic page placement/storage-tier decisions do not represent Linux kernel pages, and no synthetic pages are passed to the adapter.

Run `cargo run --release -p xynoos-cli -- shadow 4 500` for JSON lines. Sample count and interval use PressureIQ bounds. No target argument or write option is exposed. `cargo run --release -p xynoos-decision --example validation` exercises stale, injected failures, adapter inspection and a complete-path micro-measurement.

## Real scenario evidence

Four samples per scenario, 500 ms apart, on the real engine. CPU was a single Python busy loop for 3 s; memory touched 32 MiB and held it for 3 s; I/O repeatedly wrote and synced a 1 MiB temporary file for 3 s, automatically removed afterward. These are bounded loads, not isolated benchmarks. Other cgroup activity was present. Values below are observations, not synthesized pressure:

| Scenario | CPU / memory / I/O PSI some avg10 (%) | Context in current cgroup | Decision |
|---|---|---|---|
| Idle/no intervention | 0.73–0.96 / 0 / 0.20–0.24 | CPU deltas 47,804–87,285 µs per interval | observe; no sustained pressure |
| CPU | 0.78–0.96 / 0 / 0.16–0.20 | CPU deltas 528,113–598,044 µs per interval | observe; no sustained pressure |
| Memory | 0.78–1.00 / 0 / 0.13–0.16 | memory.current about 7.06 GB; shared cgroup | observe; no sustained pressure |
| Temporary-file I/O | 0.82–3.57 / 0 / 1.74–3.23 | write deltas 189,865,984–241,455,104 bytes per interval | observe; no sustained pressure |

First observations explained that two samples were needed. PSI is a smoothed ten-second measure, so short workloads need not raise it substantially. Separate validation collected a real snapshot then evaluated it six seconds later: decline, `stale or non-increasing observation`. Injected I/O permission failure and missing meminfo each declined with `unusable or ambiguous` identifying the source. These are fault injections, not claims that the real sources failed. A real adapter probe using fresh telemetry and `/sys/fs/cgroup/init` returned `Inspected`, original memory.low 0, `control_writable=false`, `kernel_write_available=false`. This was a direct read-only adapter probe; the real low-pressure scenarios did not route an action. A synthetic sustained-memory test confirms engine-to-adapter routing and refusal on an unavailable target.

## Tests and overhead

Focused tests cover deterministic decisions, exact enter/exit boundaries, cooldown, stale/missing/failed and invalid-scope refusal, adapter unavailable path, and 1,000 observations with history capped at eight. `cargo test --workspace`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`, and `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` passed. The opt-in real kernel transaction test remains ignored and was not run.

Release measurement: 1,000 consecutive complete `collect_system` + contract + decision iterations averaged **515,612 ns** (about 0.52 ms). It includes filesystem telemetry collection and cgroup context but, because observed memory PSI was low, does not include adapter preparation. It is a local mean without a distribution, workload isolation, or performance comparison; it does not establish runtime impact under deployment conditions.

## Before kernel activation

Establish safe, exclusively delegated target ownership and coordination, reliable target-scoped pressure and workload identity, bounded action benefit and eligibility criteria, authorization, durable auditing and recovery, and independent live-kernel apply/read-back/rollback/cleanup validation. The existing transaction has non-atomic race and crash limitations documented in TASK 015B/C. Validate on an isolated delegated system and perform controlled A/B experiments before claiming improvement or considering autonomous activation.
