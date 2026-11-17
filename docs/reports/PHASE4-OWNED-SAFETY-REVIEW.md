# Adversarial self-review: cooperative corpus experiment

Scope: `xynoos-aura::cooperative`, `xynoos-engine::owned`, `xynoos-owned`,
`benchmark_owned.py`, `inspect_owned.py`, package builder and operator procedure.
This is an engineering self-review with source/tests/Codespaces execution. It is
not an independent human review or physical kernel validation.

| Gate | Evidence and remaining limit |
| --- | --- |
| Exclusive ownership | Worker constructs private `Corpus`; no PID, cgroup path, arbitrary command or foreign buffer input. Executor accepts no caller-supplied executable plan. One synchronous Rust owner controls every block. |
| Live systemd identity | Not needed for private memory. No service or delegated hierarchy is created. Existing adapter production guard still always refuses permits. Actual systemd/kernel identity validation remains unverified for the separate disabled physical path. |
| Delegation / TOCTOU | No delegation or external shared control file. Compression compares decoded bytes to the still-owned original before an atomic Rust enum replacement. Existing external cgroup check/write races are not solved or enabled. |
| Write/read-back ambiguity | No kernel-control writes. Candidate decoding and exact equality precede replacement; partial actions leave each committed block independently readable. Errors never produce a successful final report. In-memory replacement is not a filesystem transaction. |
| Resource bounds | Fixed 16 MiB corpus, 1 MiB scratch, bounded candidate, fixed 256-request maximum. Worker installs/read-backs lower-only 128 MiB AS / 8 s CPU / zero core limits before allocation and starts an independent 25 s watchdog. Harness checks live MemAvailable and mounted-root context, bounds lines/output, and enforces <=24 s per arm. Watchdog is scheduler-dependent; kernel failure or externally stopped processes are outside real-time guarantees. Harness overhead is separately measured; worker limits are not claims that Python itself uses 128 MiB. |
| Termination | Graceful finish/cancel reverses and verifies, then exits zero; harness drains pipes, requires final report plus successful exit and reaps. Signals act only on exact unreaped owned child. Escalation/reap ambiguity prevents success. Blocking/dying/oversized-output fixtures exercise failure paths. Idle stdin watchdog exit 124 is directly tested; EOF reversal is tested. |
| Reverse recovery | Every block is decoded/CRC-checked and compared byte-for-byte to independently regenerated input. Packed blocks return to raw. Corruption after partial restoration remains failure; already restored blocks stay valid. Cancellation prohibits further work. Peak restoration memory is included via a final self-reported procfs HWM reading. |
| Crash safety | Corpus is disposable process memory; no persistent application data or control needs rollback after worker death. Parent EOF exits/restores when pipes allow; watchdog bounds an orphan that remains blocked. SIGKILL/power loss may skip reverse verification and logging. Missing terminal evidence means cleanup unknown, never recovery complete. No real power-loss/OOM/crash claim. |
| Missing / stale / partial failure | Reuse PressureIQ contract/state validation and swap monotonicity. Additional <=1 s freshness at choice and each block; missing/future/stale/reset readings refuse. CPU/I/O pressure backs off. Local two-observation retention state resets on invalid continuity. Every attempt has a 10 s cooldown, including no-benefit attempts. Refusals are retained. |
| Evidence integrity | Matched corpus identities and ordered request checksums across all three arms; full independent equality at exit. All six permutations, excluded recorded warmup, fixed requests/think time, raw events and environmental hashes. Inspector requires complete expected runs including warmup, verified restore/exit/reaping, and matching output; contradictory end markers cannot promote failures. CRC detects accidental corruption; it is not cryptographic authentication. |

Review corrections: preserve raw bytes until candidate equality; shrink compressed
allocations into boxed slices; leave expanding or <12.5%-saving blocks raw;
recheck freshness during a multi-block action and report a partial freshness stop;
retain cooldown after telemetry refusal; capture restoration HWM inside the
worker; preserve missing peak HWM as unknown rather than zero; require package inspector status (not just its exit code). No safety
checks, reads, CRCs or required workload work were removed for benchmark gains.

The package excludes the broad CLI, launchers and research executors. The new
worker only calls `Engine::evaluate` with `Mode::Observe`; this branch is read-only.
It never calls the controller/coordinator/transaction executors. Existing
`physical_admission`, adapter permit guard and `physical_mutation_disabled`
remain unconditional production refusals; their source is unchanged. Compile-fail
and kernel refusal regression tests remain required. No sudo, memory.low,
controller activation, swap/ZRAM or global configuration operation was performed.
