# Debian / MX integration and physical acceptance

The existing signed preview installer, version selection, verification, upgrade and rollback remain authoritative (see INSTALL.md). This branch is an engineering preview, not a published signed release. Build with cargo build --release --locked -p xynoos-cli --bin xynoos. Run as an ordinary user. No service installation, sudo, kernel writes or background work is needed for resources, memory, pressure, system or diagnose.

For Debian packaging, build inside the oldest supported Debian suite using sbuild, not on a newer host and assume glibc compatibility. Package /usr/bin/xynoos and GPL license/third-party notices under /usr/share/doc/xynoos. Use debhelper 13 and dh_shlibdeps to generate exact runtime dependencies from the built ELF; retain Cargo.lock and build with an offline local registry. Build-Depends: debhelper-compat (= 13), cargo, rustc, pkg-config; configure the Rust version required by the locked dependencies. Do not add maintainer scripts that enable services. An actual .deb, lintian and clean-install matrix are not supplied or validated by this integration. Existing static musl release CI remains the portable delivery path.

A future monitor should run unprivileged with bounded sample count/history, a configurable interval (initial recommendation >= 1 second), no telemetry persisted by default, and rotation limits when logging is opted in. On MX installations using sysvinit, keep the CLI usable without systemd. Procfs/sysfs access restrictions and containers may produce partial evidence; do not change security policy to hide that condition. No root daemon is included here. Configuration currently consists of explicit CLI arguments; unknown resource options fail closed.

Future service writes must live in a separate authorization boundary, recognize the single actual power manager (including tuned-ppd implementing the profile API), avoid D-Bus auto-activation during discovery, and use Polkit rather than root CLI commands. planning::prepare always rejects today. A future backend needs permission and capability checks, before/after identity and profile snapshots, allowlisted operations, cancellation/timeout, readback validation, rollback and durable bounded audit records. A daemon timeout does not prove that a write did not happen; reconcile state before retrying. Existing experimental empty-cgroup transactions retain their separate opt-in API and ignored kernel test.

Upgrade: preserve previous signed binary and selected version state; compare schema_version before consuming JSON. The new commands are additive, system schema v1 and existing product commands remain intact. Roll back using the existing installer rollback command, or replace a manually installed preview with the preserved previous binary. The new read-only paths require no kernel-state rollback. Never overwrite the running distribution's profile manager configuration.

## Hardware matrix

| Environment | Supported observations | Explicit limitations / verification |
|---|---|---|
| CPU-only / VM / container | proc memory, PSI, CPU ticks, disks, cgroup where mounted | Host/namespace scopes differ; frequency, GPU, sensors often absent. Cloud exercised. |
| N5000 / UHD 605 | Discover CPU topology/cpufreq; Intel DRM PCI ID/driver; legacy i915 frequency if exported | Expected i915/Gemini Lake path is a hypothesis until machine capture. No utilization or dedicated VRAM claim. No physical validation. |
| AMD amdgpu | PCI identity, optional busy percent and VRAM sysfs; global hwmon temperatures | Synthetic fixture tested; integrated VRAM counters are not total shared system memory. Physical validation pending. |
| NVIDIA | DRM PCI identity and bound driver | NVML not implemented; busy/memory unsupported. Proprietary driver may lack DRM node. |
| Unknown/hotplug GPU | DRM discovery, source-specific failures | No invented metrics. Same-path replacement cannot be detected by counter_delta alone; callers must rediscover identity. |

## N5000 acceptance procedure

1. Record uname -a, /etc/os-release, lscpu, RAM size, firmware and graphics driver. Preserve package/binary SHA256 and source SHA.
2. Run xynoos resources --json, system snapshot --json, memory --interval-ms 1000, and diagnose with the documented bounded options. Archive raw outputs with wall time.
3. Compare cpufreq policy/related_cpus, governor, frequency units and available lists against the corresponding read-only sysfs files. scaling_cur_freq is driver reported; it may be requested rather than actual hardware frequency.
4. Confirm GPU vendor/device and i915 binding. Inspect available gt_*_freq_mhz files. Use a separately installed intel_gpu_top for a bounded operator-driven comparison if permitted. Its PMU permissions and support must be checked; do not weaken perf restrictions automatically. Xynoos should continue reporting utilization unsupported.
5. Compare thermal labels and millidegrees with sensors output; do not infer throttling from temperature. Only checked positive deltas of exposed throttle counters provide throttle-event evidence.
6. Collect idle, browser/video and bounded compile workload samples. Use scripts/resource-overhead.py and the hardware_overhead example under /usr/bin/time -v. Record VmRSS, VmHWM, CPU time, collection distribution and version. No OOM stress on a 4 GB desktop; stop when interactive responsiveness degrades.
7. Verify missing/restricted sensor states as an unprivileged user. Exercise upgrade and rollback in a disposable user prefix. Never run privileged integration tests on the working desktop.

Acceptance remains open until raw physical captures and measured overhead are reviewed. Threshold calibration, sustained pressure behavior, battery drain and suspend/hotplug recovery require physical evidence.
