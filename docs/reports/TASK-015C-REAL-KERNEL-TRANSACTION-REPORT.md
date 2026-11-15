# TASK 015C — Real kernel transaction validation

Baseline: `b3eba920ed7251467d0f9cb5196736154b4e5147`.

## Results

- **PASS — read-only environment assessment:** effective UID 1000; cgroup v2 mounted at `/sys/fs/cgroup` with `rw` and memory enabled in `cgroup.subtree_control`. The mount root is excluded by the backend; its only visible directory, `init`, is root-owned. No suitable delegated parent was identified. Mount writability is not proof of delegation or exclusive control.
- **NOT VERIFIED — real kernel child creation, `memory.low` apply/read-back, restoration, and cleanup:** no safe parent was available; no creation or cgroup write was attempted. The separate kernel test is ignored by default. Fixture results do not establish kernel behavior.
- **PASS — fixture and safety regression:** nine transaction tests cover successful restoration, authorization and telemetry refusal, apply errors before/after mutation, mismatches, competing values, identity failures at transaction stages, rollback failures and unsuitable parents. Three Linux tests cover discovery and read-only decisions. These do not prove race freedom.
- **PASS — workspace gates:** `cargo test --workspace`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`, and `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`. The focused tests and clippy were repeated after the identity regression was added.
- **FAIL:** none observed. An unavailable live test is not a pass.

## Reproduce the kernel test

On an isolated Linux VM or physical test host, arrange an already delegated, exclusively controlled cgroup v2 parent owned by the non-root test user, with `memory` already enabled in its `cgroup.subtree_control`. It must not be the visible mount root and must not be group/other writable. The test never enables controllers or migrates processes. Coordinate all other writers. Run with fresh usable visible-system memory PSI and meminfo:

```sh
XYNOOS_KERNEL_TEST=1 XYNOOS_CGROUP_PARENT=/absolute/path/to/delegated/parent \
  cargo test -p xynoos-linux-control --test kernel_transaction -- --ignored --nocapture
```

The test creates a random empty child, records kernel read-back immediately after the bounded apply and restore, checks the final original value, and explicitly removes the child. Setup, telemetry, read-back or cleanup errors fail the test. Inspect the parent for a remaining `xynoos-test-*` child if cleanup fails; investigate identity and ownership before any manual action. The test is opt-in and requires both the ignored-test flag and environment confirmation.

## Hardening and limits

Explicit cleanup no longer silently retries through `Drop` after an error. Creation errors include cleanup failures, and setup cleanup checks the opened child's identity before removal; when identity is unavailable, cleanup is skipped and reported. The check and unlink remain non-atomic, including the initial mkdir/open interval. The transaction refuses to overwrite known changed or ambiguous state, but cgroupfs has no compare-and-swap: concurrent writers can race checks and writes. A crash, SIGKILL, or failed restoration may leave a changed value or child. No durable recovery journal exists. This remains an empty-child test facility, not production workload control.

## Measurement

One release-mode in-memory benchmark measured **21,798 ns mean per complete transaction**, 1,000 iterations, including contract validation but excluding telemetry collection and kernel I/O. This is a local bounded microbenchmark, not a latency distribution or live-kernel overhead measurement.
