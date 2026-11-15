# TASK 015B — Linux control adapter (PARTIAL)

Baseline: `2fa7d3ddbf46fabc7023bc8d388c177560cdc510`. This is a read-only foundation, not a completed control adapter. **REAL WRITE: NOT VERIFIED.**

## Architecture and safety

`xynoos-linux-control` exposes cgroup v2 discovery, a typed `MemoryLow` request, structured serializable inspection results, and a fail-closed execution entry point. Preparation bounds the request to 64 MiB and 4096-byte alignment, rebuilds the PressureIQ v1 contract using the snapshot's monotonic time, and requires available, error-free, fresh visible-system memory PSI and meminfo. These sequential observations are not atomic and do not describe an arbitrary target cgroup. A direct child of the visible cgroup mount is inspected without following a final-component symlink, and its `memory.low`, device and inode are recorded. The permission probe uses effective credentials and is advisory, not proof of ownership or delegation. An inspected decision is not authorization to write.

Execution always declines, even with explicit authorization. Ownership and permissions alone do not prove isolation from other controllers, and cgroupfs offers no atomic compare-and-swap. There is no secure handle-based target resolution, exclusive-control protocol, apply/read-back transaction, rollback, crash recovery, or durable audit log. The path inspection itself is subject to races and is safe only because it performs no writes. No autonomous controller is connected. These gaps prevent meeting the acceptance criteria and must be resolved and tested before enabling execution.

## Real Linux evidence

In this Codespaces mount namespace, `statfs` identified cgroup v2 at `/sys/fs/cgroup`; mountinfo reported `rw`. Controllers and subtree control included memory, CPU, I/O and others. `/sys/fs/cgroup/init` was a visible root-owned child, with `memory.low` read as 0 and effective write access denied. This does not establish host-wide topology or a disposable delegated cgroup. The release probe collected actual PressureIQ data and produced an inspected dry-run result; execution declined. No kernel control file was written and no test cgroup was created.

## Validation and measurements

`cargo test --workspace`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`, and `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` passed. Three Linux integration tests cover stale/error telemetry, invalid paths, out-of-bounds values, unauthorized and authorized execution refusal, and discovery. There are no fixture transaction, competing-write, failure-injection, or rollback tests because no transaction exists. Denied permission was observed by the real probe; it is not a simulated write attempt.

A single release-mode probe of 1,000 iterations on this environment measured mean preparation/dry-run 170,977 ns per call, including contract construction, mount discovery and reads, and unauthorized refusal 20 ns per call. These are bounded local micro-measurements, not distributions, performance comparisons, or evidence of resource optimization. Telemetry collection itself was outside the loop.

## Remaining work

Design and validate an explicitly delegated disposable-cgroup lifecycle and a defensible isolation/coordination model; use descriptor-relative symlink-safe resolution and identity revalidation; implement bounded apply, read-back, failure handling and rollback without overwriting competing changes; add fixtures with injected failures and competing writes; then test a real isolated write and restore only when safely delegated. Verify recovery limits and audit storage. Until then, this is not ready for autonomous-controller development beyond interface research, and no performance improvement is claimed.
