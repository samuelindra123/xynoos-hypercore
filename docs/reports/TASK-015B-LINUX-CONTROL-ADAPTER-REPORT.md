# TASK 015B — Linux control adapter

Baseline: `2fa7d3ddbf46fabc7023bc8d388c177560cdc510`. The bounded transaction is implemented and fixture-tested. **REAL WRITE: NOT VERIFIED.**

## Architecture and safety

`xynoos-linux-control` provides visible-namespace capability discovery, the existing read-only inspection API, a typed `MemoryLow` action, and a temporary apply/restore transaction with serializable results and event messages. The transaction requires explicit authorization and fresh, error-free PressureIQ v1 memory PSI and meminfo, rebuilds the contract immediately before use and again before applying, and accepts only 4096-byte-aligned values up to 64 MiB. These sequential visible-system observations are not atomic and do not describe the target cgroup. The existing `execute(Audit, ...)` remains a refusal-only compatibility API; `transaction::run` is the separate execution API.

The Linux backend only creates a new randomly named empty child of a caller-designated parent. It requires non-root effective credentials, an owner-writable parent owned by that user, no group/other write permission, cgroup v2, and the memory controller enabled for children. It refuses the mount root and checks ownership and filesystem of the new target and control. It never migrates processes; this initial implementation therefore exercises empty test cgroups only, not a workload. Parent path components and child/control files are opened descriptor-relative with `O_NOFOLLOW`; device and inode identities, absence of processes and descendants, and the current value are rechecked. `cleanup` reports removal errors; drop attempts best-effort cleanup. A nonempty group cannot be removed by the kernel. The caller must designate a disposable delegated parent and coordinate other writers; permission checks alone cannot prove that privileged or same-user controllers will not interfere.

The transaction reads the original, revalidates, applies, verifies, and restores with read-back. If the write reports an error after changing the value, it attempts restoration when the requested state is observed. If a changed or ambiguous value is observed, it reports conflict and does not knowingly overwrite it. Rollback errors and verification failures are explicit. Cgroup files provide no compare-and-swap: another writer can race any check and write, including cleanup. Recovery is best effort, with no durable journal or guarantee after SIGKILL, power loss, process crash, or competing privileged changes. Structured reports are returned to the caller, not durably logged. Do not use this API for production workloads or autonomous control.

## Real Linux evidence

In this Codespaces mount namespace, `statfs` identified cgroup v2 at `/sys/fs/cgroup`; mountinfo reported `rw`. Controllers and subtree control included memory, CPU and I/O. `/sys/fs/cgroup/init` was a visible root-owned child, with `memory.low` read as 0 and effective write access denied. The backend refused this parent. Mount writability is not delegation and does not establish host-wide topology. A read-only probe collected actual PressureIQ telemetry and produced an inspection result. No verified disposable delegated parent was available; no cgroup was created or control file written. The real creation, write, read-back, restore, and cleanup paths remain unverified against the kernel.

## Tests and measurements

The temporary-directory fixture backend exercises eight transaction tests: successful restoration, authorization/freshness/bounds refusal, write failure before and after mutation, read-back mismatch, competing changes at three stages, rollback write failure, rollback verification failure, and real-backend rejection of unauthorized and unsuitable parents. These are fixtures, not kernel evidence. Three additional Linux tests exercise discovery, real telemetry consumption and invalid requests. `cargo test --workspace`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`, and `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` passed.

A release-mode in-memory backend measured 20,817 ns mean per complete apply/restore transaction over 1,000 iterations, including contract validation but excluding telemetry collection and kernel I/O. A prior release read-only probe measured 170,977 ns mean per preparation over 1,000 iterations on this environment. These are local bounded micro-measurements, not distributions or evidence of resource optimization.

## Remaining work

Validate the real backend and cleanup on a safely delegated disposable subtree, assess the kernel-specific behavior and races, strengthen coordination and audit persistence, and add workload-specific telemetry before considering autonomous-controller development. No performance improvement is claimed.
