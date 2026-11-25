# XYNOOS HyperCore — Milestone J.1 Critical Systems Handoff
## Implementation & Verification Brief (Superseded to Gemini 3.8 Flash High)

- **Repository**: `samuelindra123/xynoos-hypercore`
- **Current Branch**: `feat/hypercore-milestone-j-release-qualification`
- **Current Baseline Commit**: `fa776d8242959d92b359b043d8ebe3afdd6af814`
- **Target Release Candidate**: `v0.1.0-rc.1`
- **Authoring Agent**: Gemini 3.8 Flash High (Primary Implementation & Systems Engineer)
- **Assigned Critical Engineering Model**: Gemini 3.8 Flash High (Superseded from Gemini 4 Hydrogen per Owner Authorization)

---

> [!IMPORTANT]
> **Model Assignment Supersession Notice (Milestone J.2–J.3)**:
> By explicit authorization of the repository owner, the previous engineering handoff requiring Gemini 4 Hydrogen has been formally superseded.
> Gemini 3.8 Flash High is designated as the sole primary engineering model to implement, adversarially verify, and deliver all remaining Milestone J requirements (CRITICAL-01, CRITICAL-02, CRITICAL-03, and AURA performance remediation).
> All original technical findings, architectural invariants, regression testing protocols, and acceptance criteria are strictly preserved.

---

## 1. Executive Summary

Milestone J.1 completed an independent evidence audit, verified historical benchmark methodology, resolved ordinary engineering defects (telemetry bounds, signal idempotency, generational handle rollover, slice blocklists, and atomic journal naming), and isolated three release-blocking critical concurrency and transactional durability invariants.

In accordance with the Hybrid Model Engineering Policy, Gemini 4 Hydrogen is assigned exclusively to resolve:
1. **CRITICAL-01**: Absence of durable on-disk transaction journal integration within `xynoos-linux-control` (`SustainedLease`), causing unrecoverable kernel control persistence if processes terminate abruptly.
2. **CRITICAL-02**: Formal review and verification of TOCTOU concurrency boundaries in Linux cgroup v2 transactional control paths.
3. **CRITICAL-03**: Inode recycling mitigation and identity verification in transient/recreated cgroups.

---

## 2. Inventory of Critical Findings

### Finding CRITICAL-01: In-Memory Only Lifecycle of `SustainedLease` Lacking Durable Crash Journal Integration

- **Target Files**:
  - `crates/xynoos-linux-control/src/transaction.rs`
  - `crates/xynoos-aura/src/daemon.rs`
- **Affected Constructs**:
  - `xynoos_linux_control::transaction::SustainedLease`
  - `xynoos_linux_control::transaction::WorkloadCgroup`
  - `xynoos_aura::daemon::TransactionJournal`
- **Exact Line Ranges**:
  - `crates/xynoos-linux-control/src/transaction.rs:720-850`
  - `crates/xynoos-aura/src/daemon.rs:300-440, 710-805, 988-1045`
- **Root Cause**:
  In Milestone I, `TransactionJournal` was hardened inside `xynoos-aura::daemon` with `0o600` permissions, CRC32 verification, and atomic rename. However, `xynoos-linux-control` remains a lower-level crate with zero awareness of `TransactionJournal`. Consequently:
  - When `SustainedLease::acquire()` writes to `memory.low` or `memory.high` in `xynoos-linux-control`, it holds the original control value strictly in userspace process memory (`self.original: u64`).
  - Standalone binaries (e.g., `xynoos-bench workload-lease` or non-daemon control utilities) do not write a durable journal file to `/run/xynoos/`.
  - If a process holding an active `SustainedLease` receives `SIGKILL`, loses power, crashes, or panics, the kernel cgroup control files remain modified indefinitely.
  - While `AuraDaemon` in `xynoos-aura` manually writes a journal in its main loop, this is decoupled from `SustainedLease::acquire` / `release`, introducing a crash window between journal write and cgroup write, and between lease release and journal cleanup.
- **Minimal Reproduction**:
  1. Open a delegated cgroup via `WorkloadCgroup::open("/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/test.slice", true)`.
  2. Acquire a lease: `let lease = SustainedLease::acquire(&mut backend, &snap, Action::MemoryLow { bytes: 33554432 }, true).unwrap();`.
  3. Send `SIGKILL` to the process (`kill -9 $PID`).
  4. Inspect `/sys/fs/cgroup/.../test.slice/memory.low`: value remains `33554432`.
  5. Inspect `/run/xynoos/`: no journal file was persisted, preventing any subsequent daemon or init script startup from autonomously rolling back the orphaned limit.
- **Potential Impact**:
  Accumulation of orphaned cgroup throttling limits across crash cycles; degraded application performance or excessive swap pressure due to abandoned `memory.low` or `memory.high` boundaries.
- **Proposed Architecture / Expected Implementation**:
  Architect an enforceable link between `SustainedLease` and durable journal recording:
  - Option A: Introduce a fallible, decoupled `DurableJournal` trait into `xynoos-linux-control` that `SustainedLease::acquire_durable` or a higher-level coordinator can accept.
  - Option B: Unify transaction journaling inside `xynoos-linux-control` or provide an atomic `JournaledLease` wrapper that guarantees intent is committed to disk before kernel write, marked applied after verification, and marked rolled back or purged upon release.
  - Ensure recovery is idempotent and handles re-boots cleanly via `/proc/sys/kernel/random/boot_id`.

---

### Finding CRITICAL-02: Linux Cgroup v2 TOCTOU Concurrency Race Boundary

- **Target Files**:
  - `crates/xynoos-linux-control/src/transaction.rs`
- **Affected Functions**:
  - `transaction::run<B>` (lines 80–186)
  - `transaction::check<B>` (lines 187–203)
  - `SustainedLease::acquire<B>` (lines 720–795)
  - `SustainedLease::release<B>` (lines 813–849)
- **Root Cause**:
  The Linux kernel's `cgroupfs` does not expose an atomic Compare-And-Swap (CAS) syscall or file locking (`flock`/`fcntl` are rejected on kernfs synthetic control files).
  HyperCore employs a check-before and check-after strategy:
  1. `check()` original value.
  2. `write_control(action, bytes)`.
  3. `check()` applied value.
  4. Prior to restore: `check()` applied value.
  5. `write_control(action, original)`.
  6. `check()` restored value.
  While this detects concurrent modifications post-facto and transitions to `Outcome::Conflict` rather than performing a blind overwrite, a microscopic race window exists between `check()` and `write_control()`. If an external system orchestrator (e.g., systemd, Kubernetes, or human administrator) writes a value in that exact instant, HyperCore's write succeeds, momentarily clobbering external state before `check()` flags a conflict.
- **Expected Implementation & Review**:
  - Formally review the conflict-handling state machine in `transaction.rs`.
  - Validate whether the current behavior (declining rollback on ambiguity and flagging `Outcome::Conflict`) satisfies all safety invariants.
  - Determine if file-descriptor-level state pinning (e.g. keeping open `File` handles to control files) provides any additional kernel guarantees, or if `Outcome::Conflict` is provably the optimal fail-closed response.

---

### Finding CRITICAL-03: Transient Cgroup Inode Identity Recycling

- **Target Files**:
  - `crates/xynoos-linux-control/src/transaction.rs`
- **Affected Types & Functions**:
  - `Identity` (lines 17–22)
  - `DisposableCgroup::identity` (lines 366–391)
  - `WorkloadCgroup::identity` (lines 612–630)
- **Root Cause**:
  `Identity` records:
  ```rust
  pub struct Identity {
      pub device: u64,
      pub inode: u64,
      pub control_inode: u64,
  }
  ```
  On virtual filesystems (`cgroup2fs` / `kernfs`), inode numbers are allocated dynamically by the kernel. If a delegated cgroup is removed and a new cgroup is immediately recreated at the exact same path by a separate process, there is a theoretical possibility that the kernel reuses the same inode numbers (`inode` and `control_inode`) if memory allocations align.
- **Expected Implementation**:
  - Audit whether inode reuse is plausible under Linux cgroup v2 kernfs.
  - Determine if an additional epoch, creation timestamp, or unique xattr/file sentinel inside the cgroup is required to eliminate identity ambiguity completely.

---

## 3. Failed or Inadequate Previous Fixes

1. In Milestone H/I, `SustainedLease` was introduced without a `Drop` implementation or disk persistence, relying solely on caller discipline in `xynoos-bench` and `daemon.rs`.
2. A lightweight warning in `Drop` was added in Milestone J.1 (`2d0a448`), but this merely warns on `stderr` and cannot perform the necessary kernel restoration or durable cleanup because `Drop` does not own the `Backend`.

---

## 4. Architecture & Safety Invariants

Hydrogen must preserve all existing repository governance and safety invariants:
1. **Rule 9 (Safe Operational Defaults)**: HyperCore must prioritize fail-safe, fail-closed behavior.
2. **`#![forbid(unsafe_code)]`**:
   - `xynoos-core`, `xynoos-hypermemory`, `xynoos-pressureiq`, `xynoos-decision`, and `xynoos-aura` must strictly retain `#![forbid(unsafe_code)]`.
   - Any unsafe operations in `xynoos-linux-control` must be explicitly documented with safety proofs.
3. **No Root Execution**: Non-root UID execution must be strictly maintained; root execution must continue to be refused (`geteuid() == 0`).
4. **Symlink Prevention**: `O_NOFOLLOW | O_CLOEXEC` on all path walks and control file access.
5. **No Blind Rollbacks**: Never restore or overwrite a control file if the current content differs from expected intent.

---

## 5. Required Regression Tests

Hydrogen must supply automated regression tests for each remediated finding:
1. **Durable Lease Test**: Simulate a crash/unwinding event during `SustainedLease` and verify that a recovery routine (or next startup) reads the durable intent and restores the cgroup control value.
2. **Conflict Abort Test**: Simulate concurrent modification between journal intent recording and cgroup write, verifying fail-closed `Outcome::Conflict`.
3. **Idempotency Test**: Verify that re-running recovery multiple times against an already restored or clean cgroup succeeds without side effects.

---

## 6. Acceptance Criteria for Hydrogen (Milestone J.2)

- [ ] CRITICAL-01 resolved: Durable journal recording or crash recovery is integrated with `SustainedLease` or unified transaction lifecycle.
- [ ] CRITICAL-02 resolved: TOCTOU race behavior is formally audited, documented, and proven fail-closed.
- [ ] CRITICAL-03 resolved: Cgroup identity verification is proven immune to inode recycling vulnerabilities.
- [ ] All new code compiles cleanly under Rust 1.82.0 (MSRV).
- [ ] `#![forbid(unsafe_code)]` preserved where required.
- [ ] Workspace tests pass locally and in GitHub Actions CI.
- [ ] Zero unverified performance or hardware claims.

---

## 7. Explicit Non-Goals for Hydrogen

- **Do NOT** manage GitHub issues, milestones, or pull request metadata.
- **Do NOT** perform Debian packaging, APT repository updates, or documentation restructuring.
- **Do NOT** run heavy physical benchmarks or modify developer laptop host settings.
- **Focus strictly** on source code implementation and security validation within `crates/xynoos-linux-control` and `crates/xynoos-aura`.
