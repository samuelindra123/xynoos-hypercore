# Resource intelligence integration report

## Baseline and integration decision

Audit date: 2026-11-17. Fetched main is c83d06c519b2ecf659b6e804df00bc449812914a. Initial clean checkout was infrastructure/phase5-artifact-pipeline at fb9aa98, two additive commits above main. PR #33 contains that artifact workflow. PR #32 is draft at 18b35416724bd7787363078ab0d8e43f90842c60 and targets backup/codespaces-full-snapshot-2026-11-17, not main. Its full branch comparison changes 382 files including backup artifacts, experiments and distribution/website deletions. Importing that branch wholesale would replace working release infrastructure and require a different workspace.

Integration/resource-intelligence starts from PR #33 and ports the focused Phase 5 feature, documentation and reliability changes with cherry-pick provenance. Resolve CLI/Cargo conflicts against main: preserve both product binaries, preview version, signed installer and current system schema; add only memory/AURA dependencies already in the workspace. Do not import the backup's xynoos-engine or mvp command. The original PRs and branches remain intact. The integration PR targets main and includes PR #33; review/close superseded PRs separately after acceptance. No main merge, force push or branch deletion is performed.

The audit inspected all eight crate manifests, history, source layout, existing tests/examples, distribution manager/bootstrap tests, release pipeline, CLI validation and website workflow. Core defines storage contracts; HyperMemory owns private buffers; AURA already contains placement/simulation policy; PressureIQ owns Linux parsers and scoped observations; decision owns existing hysteresis/cgroup advisory policy; linux-control owns experimental bounded transactions; CLI and bench are consumers. Dependencies remain serde/serde_json, libc, lz4_flex, crc32fast and their locked transitive crates. No new external dependencies.

## Implemented architecture

Hardware observation remains in PressureIQ, with hardware.rs as a thin bounded sysfs adapter. HyperCPU, HyperGPU and HyperStability are responsibilities within that adapter rather than new crates with duplicated collectors. HyperMemory's owned PageStore and Phase 5 memory diagnosis retain their separate boundaries. The private store never owns arbitrary application pages.

The resources command exposes a versioned envelope: hardware capabilities, existing system contract and AURA plan. CPU ticks/utilization (two samples required), CPU/memory/I/O PSI, selected disk deltas and cgroup counters remain in the existing system command. Memory diagnosis uses the Phase 5 two-sample command, including reclaim, allocation stalls, swap and ZRAM; it is not automatically resampled by resources. Hardware topology, driver/governor discovery, frequencies, throttle counters, GPU IDs, AMD busy/VRAM, thermal/hwmon and power readings add hardware context.

Every hardware attribute carries source, unit, status and optional value. Parent hardware timestamp and collection duration apply to its sequential readings. No missing measurement is converted to zero. Available, unsupported, permission denied, temporary failure, malformed, stale, counter reset, hardware removed and partial collection states are represented. counter_delta uses checked subtraction and source/unit matching; it cannot prove same-path hardware identity across unplug/replug. Freshness is a caller-measured monotonic age, never wall-clock trust. Discovery is capped at 256 entries per directory and text at 4096 bytes per attribute; nested hwmon collection is bounded too. Truncation is partial evidence, not full discovery. A read cannot guarantee a wall-clock deadline if the kernel itself blocks; no claim of hard real-time collection is made.

AURA resource planning is deterministic over supplied evidence and age, reports confidence, evidence, costs and limitations, and prioritizes gathering evidence on missing sensors. A 10% PSI threshold is an initial advisory heuristic. Existing memory and decision policies remain callable for compatibility. These are not calibrated performance predictors or a claim that all policies have been unified into one replacement API. Hardware readings inform availability/confidence; this release does not infer thermal causality, optimize clocks or invent Intel utilization.

The layers are observation → diagnosis → deterministic recommendation → read-only plan → control preparation rejection. New linux-control planning allowlists only balanced and power-saver profile intents; it always returns an audit-friendly rejection and the required future execution lifecycle. Permission, opt-in and manager-conflict checks cannot bypass the absent backend. It neither discovers nor starts power services. Existing experimental cgroup transaction APIs retain their established gates; no CLI silently invokes them. Timeouts, snapshot validation and rollback for service writes are specified but not claimed implemented. See DISTRIBUTION.md and UPSTREAM.md for integration choices and unresolved work.

## Validation and evidence

Cloud environment:

- cargo fmt --all -- --check, strict workspace/all-target Clippy, locked workspace tests and release workspace build pass.
- Workspace results: 106 tests passed, one privileged kernel transaction test intentionally ignored. Fixture coverage includes missing CPU/GPU paths, malformed AMD utilization, Intel unsupported utilization despite a fake busy file, permission error classification, counter resets/source changes, stale evidence, deterministic policy, discovery bounds and all control authorization/manager combinations. Existing memory/replay and product regressions pass.
- All 41 Python distribution tests pass outside the sandbox. Initial sandbox run failed because /tmp ownership was remapped; the installer safety checks were preserved.
- New CLI contract test verifies read-only plans, source/unit/error values and refusal of an apply option. No system service is installed/enabled by checks.
- Build logs/check summaries and benchmark data are retained under this directory. Existing Cargo example basename collisions produce warnings; they predate this work and do not fail checks.

CI: the integrated PR runs existing CLI/distribution/musl checks plus Phase 5 formatting, workspace tests, strict Clippy, release build and resources smoke. At source 76d8c820819ab64fc2284b052ea4c6973ad5a7d4, both PR workflows passed: [Phase 5 validation](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44034423440) and [CLI/distribution/musl validation](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44034423432). The latter includes Python distribution tests, static musl build and bounded product diagnostics. Release signing/publication and website deployment are not triggered intentionally.

Physical hardware: none tested during this mission. N5000/UHD 605 driver exposure, battery behavior, thermal counters, i915 PMU access and Debian/MX physical compatibility remain unverified. Use DISTRIBUTION.md for acceptance steps.

## Performance scope

Measured resources median: 12.91 ms wall, 11.68 ms CPU, 5.60 ms hardware collection; wall range 11.39–32.83 ms, standard deviation 5.74 ms. Memory median: 104.97 ms wall including sleep, 4.18 ms CPU. Per-child RSS high-water was 13,260 KiB for resources and 13,412 KiB for memory, subject to the inherited-page caveat below.

overhead.json contains 20 samples per release CLI command with per-child wait4 CPU time, wall time, peak RSS, and min/median/max/sample standard deviation. Run python3 scripts/resource-overhead.py after the release build. Hardware collection is measured inside the command. The memory command includes a 100 ms sleep and must not be compared directly to collection time. Per-child peak RSS may include inherited pre-exec pages; it is not steady-state RSS.

sampling.json contains 20 hardware collections separated by 100 ms sleeps and VmRSS after dropping each final snapshot. Reproduce with cargo run --release --locked -p xynoos-pressureiq --example hardware_overhead. This is an explicit bounded benchmark, not a monitor daemon. The first measured between-sample RSS was 2240 KiB; an independent wait4 run (sampling-process.txt) recorded 2288 KiB after collection, 88.92 ms CPU over 2.114 seconds including two seconds of intentional sleep (4.21% lifetime CPU on one CPU), with 11120 KiB process RSS high-water subject to the inherited-page caveat; no long-lived idle daemon exists. No performance improvement, power saving or system optimization is claimed. Sustained resource pressure and 4 GB system overhead remain physical acceptance work; do not deliberately OOM this workstation.

## Remaining implementation and verification

No writable power D-Bus backend, service discovery, optional monitor daemon, calibrated thermal diagnosis, NVML/i915 PMU collector, automatic optimization, finished .deb or production service lifecycle is delivered. Packaging is a documented distribution design, not a tested installable package. AMD/Intel sysfs paths are fixture-tested and opportunistically probed on the available environment, not certified across kernel/driver versions. Shared GPU memory and hwmon channel ownership are not guessed. Legacy Phase 5 optional values retain less detailed failure provenance than the new hardware contract; migration to richer statuses must be additive. Replay is not authenticated. Automatic reclaim/swap/ZRAM/sysctl/fan/GPU-clock writes are intentionally absent.

This is a tested read-only integration foundation ready for review, with explicit physical and service-backend acceptance gates. It is not a production-readiness declaration.
