# XYNOOS HyperCore — Phase 0 & Phase 1 Architecture Effectiveness Audit

## 1. Executive Summary

XYNOOS HyperCore is an **application-owned userspace compressed memory engine (`xynoos-hypermemory`) combined with a read-only Linux telemetry observer (`xynoos-pressureiq`) and a strictly bounded, opt-in cgroup v2 policy controller (`xynoos-aura`, `xynoos-linux-control`)**.

HyperCore **does not** transparently intercept or compress arbitrary process memory, **does not** hook kernel page faults, and **does not** replace Linux `swap`, `zram`, or `zswap`.

---

## 2. Capability Matrix (Source-Verified)

| Capability Category | Status | Implementing Symbols / Crates | Scope & Honest Boundaries |
|---|---|---|---|
| **Internal Memory Storage** | **IMPLEMENTED & VERIFIED** | `PageStore`, `ClusterStore`, `CodecScratch`, `compress_lz4hc_into` (`xynoos-hypermemory`) | Bounded, CRC32-verified userspace storage with `Raw`, `Lz4Standard`, and `Lz4Hc` codecs; explicit `MemoryAccounting`. |
| **Simulated Memory Tiering** | **IMPLEMENTED (Advisory Replay)** | `ShadowPlayer`, `ShadowCalibrator`, `PolicyConfig` (`xynoos-aura/src/profiler.rs`) | Counterfactual offline trace replay (`advisory_only = true`); does not mutate external workload memory. |
| **Real Application Integration** | **IMPLEMENTED (Opt-In Library API)** | `HyperMemoryPool`, `BufferHandle` (`xynoos-hypermemory/src/pool.rs`) | Explicit application-owned buffer pool; requires callers to opt in via Rust API. No `LD_PRELOAD` or `ptrace`. |
| **Read-Only System Observation** | **IMPLEMENTED & DEFAULT** | `collect_system`, `collect_multi_resource`, `diagnose_multi_resource`, `xynoos doctor/status/diagnose` | Reads `/proc/pressure/{cpu,memory,io}`, `/proc/meminfo`, `/proc/vmstat`, `/proc/stat`, `/sys/fs/cgroup`, `/sys/block/zram*`. |
| **Advisory Recommendations** | **IMPLEMENTED** | `AuraKernelCoordinator::recommend`, `AuraAdaptiveCoordinator::step_adaptive`, `xynoos-decision::Engine` | Generates recommendations and logs telemetry without mutating kernel state. |
| **Authorized Linux Control** | **IMPLEMENTED (Bounded, Opt-In)** | `DisposableCgroup`, `WorkloadCgroup`, `transaction::run`, `SustainedLease` (`xynoos-linux-control`) | Confined to explicitly authorized, non-root, exclusively delegated cgroup v2 targets (`memory.low`, `memory.high`, `memory.max`) with identity checks and rollback. |
| **Whole-System / Desktop Acceleration** | **NOT IMPLEMENTED** | N/A | No global kernel swap/zram replacement or transparent RAM compression exists. |

---

## 3. Answers to Phase 1 Architecture Questions

1. **Which data is actually stored in HyperMemory?**
   Only byte buffers explicitly inserted by callers into `PageStore`, `ClusterStore`, or `HyperMemoryPool` (`crates/xynoos-hypermemory/src/{lib,cluster,pool}.rs`). No external process pages are captured.
2. **Which components invoke `PageStore` and `ClusterStore`?**
   `AuraDaemon` (holds `page_store` and `cluster_store` in `crates/xynoos-aura/src/daemon.rs`), `AuraTieringCoordinator` / `AuraAdaptiveCoordinator` (`crates/xynoos-aura/src/tiering.rs`), `ShadowPlayer` / `ShadowCalibrator` (`crates/xynoos-aura/src/profiler.rs`), `xynoos-bench`, and opt-in Rust library callers via `HyperMemoryPool`.
3. **Does the continuous daemon use adaptive compression in meaningful runtime paths?**
   - *Before Milestone H audit*: `AuraDaemon::step()` only ran legacy `AuraTieringCoordinator::step_with_pressure()` on an empty `page_store` and `run_with_signal()` did not collect `MultiResourceDiagnosis` or pass `SystemSnapshot`/`Backend`.
   - *After Milestone H fixes*: `AuraDaemon` wires `AuraAdaptiveCoordinator` and `collect_multi_resource()` / `diagnose_multi_resource()` via `step_multi_resource()`, applies adaptive compression whenever `page_store` holds application-owned buffers, preserves error/backoff state on transactional failure, and avoids synthetic `available_pct` fallbacks.
4. **Are compression savings limited to HyperCore-owned buffers?**
   **Yes.** Compression savings in `PageStore`, `ClusterStore`, and `HyperMemoryPool` apply exclusively to buffers owned by HyperCore data structures.
5. **Which AURA decisions affect real workloads?**
   Only explicitly authorized cgroup v2 controls (`memory.low`, `memory.high`, `memory.max`) applied to delegated workload cgroups via `WorkloadCgroup` + `SustainedLease` (or `AuraDaemon` in `Transactional` mode with an explicit `Backend`), and opt-in `HyperMemoryPool` codec choices (`Raw` vs `Lz4Standard` vs `Lz4Hc`) for application-owned buffers.
6. **Which kernel actions are temporary tests versus sustained control policies?**
   `xynoos_linux_control::transaction::run()` applies a control and restores the original value in the same call (temporary probe/restore test). `SustainedLease::acquire()` + `SustainedLease::release()` on `WorkloadCgroup` holds a bounded control policy across a workload's execution window before restoring and verifying readback.
7. **Can HyperCore currently improve system-level responsiveness?**
   Only indirectly via bounded cgroup v2 memory protection/throttling (`memory.low` / `memory.high`) on explicitly delegated workload cgroups, or inside opt-in `HyperMemoryPool`-backed applications. See `docs/reports/MILESTONE-H-SYSTEM-PERFORMANCE-QUALIFICATION-REPORT.md` for measured CI evidence across Baselines A/B and Candidates C–F.
8. **What architectural limitations prevent broader memory optimization?**
   - Unprivileged userspace cannot transparently intercept arbitrary process anonymous pages without kernel cooperation (`zram`/`zswap`/kernel cgroups).
   - Host-wide `memory.low`/`memory.high` tuning requires explicit cgroup v2 delegation (`systemd` user slice or root-delegated parent) and careful workload attribution to avoid starving background progress.

---

## 4. Phase 0 Defects Identified and Fixed in Milestone H

1. **`AuraDaemon::step()` error/backoff clobbering (`crates/xynoos-aura/src/daemon.rs`)**: `Outcome::Conflict | VerificationFailed | RollbackFailed` called `self.record_failure(msg)`, which was immediately overwritten by `self.current_backoff_ms = 0; self.last_error = None;` at the end of `step()`. Fixed with `step_had_error`.
2. **Synthetic fallback metric in `PressureHistoryEntry` (`crates/xynoos-aura/src/daemon.rs`)**: `available_pct_permille` substituted `500` (`50.0%`) when `available_pct` was `None`. Changed to `Option<u16>`.
3. **Disconnected Milestone G components in `AuraDaemon`**: `AuraAdaptiveCoordinator` and `MultiResourceDiagnosis` were not connected to `AuraDaemon::run_with_signal()` / `step()`. Wired via `step_multi_resource()`.
4. **Fixed 128 KiB LZ4HC hash table on small blocks (`crates/xynoos-hypermemory/src/codec.rs`, `cost_model.rs`)**: `prepare_hc_tables` allocated `32768 * 4` bytes even for 4 KiB pages. Dynamically scaled to `4096 * 4` (12-bit) for `<= 4 KiB` and `8192 * 4` (13-bit) for `<= 16 KiB`, plus an explicit `i32::MAX` input length guard.
5. **No sustained cgroup lease primitive (`crates/xynoos-linux-control/src/transaction.rs`)**: `transaction::run()` only performed immediate apply-and-restore probes. Added `WorkloadCgroup` and `SustainedLease` with identical non-root delegation, symlink-free `openat`, and identity verification invariants.
