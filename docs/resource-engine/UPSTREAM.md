# Upstream selection and license audit

Research date: 2026-11-17. Sources below were inspected online; this is an integration assessment, not a guarantee of a future release or device support. No upstream project is copied or vendored. Daemon RSS/CPU was not measured on target hardware; unknown overhead is a reason to avoid making it mandatory.

| Candidate | Health and license | Compatibility, privilege and integration | Decision / overhead |
|---|---|---|---|
| Linux procfs, sysfs, PSI, cgroup v2 | Maintained kernel interfaces; Linux GPL-2.0-only with syscall exception for UAPI | Debian/MX kernel configuration and namespace determine availability; reads usually unprivileged; cgroup writes require delegation | Selected thin Rust reads; no extra daemon. Existing PressureIQ parsers retained. |
| power-profiles-daemon | Debian carries current packages; upstream page blocked by anti-bot protection during audit; verify exact package copyright before shipping | D-Bus service, Polkit authorization; supported profiles depend on driver; do not assume performance exists on N5000 | Preferred future profile owner; backend deferred until ownership and rollback can be proven. No mandatory dependency or measured daemon footprint. |
| tuned / tuned-ppd | Maintained repository and review activity; GPL-2.0 family (COPYING below) | Debian package alternative; D-Bus/CLI; privileged tuning daemon and Python runtime | Alternative owner only; broader policy and footprint than required here. Never activate alongside a competing profile manager. |
| thermald | Intel upstream repository maintained; GPL-2.0 family | Intel platform/firmware dependent; privileged thermal daemon; Debian package; D-Bus/CLI | Delegate thermal policy to distribution configuration. No fan or thermal writes from Xynoos. Actual N5000 applicability unverified. |
| lm-sensors / libsensors | Established maintained repository; tools GPL, library LGPL-2.1 family | hwmon driver dependent; ordinary readings often unprivileged; library and sensors CLI | Use documented hwmon directly for raw temperatures; no library dependency needed yet. Calibration/channel naming remains limited. Never run sensors-detect automatically. |
| intel-gpu-tools / intel_gpu_top | Debian package and maintained IGT ecosystem; per-file permissive/MIT-family licensing, inspect exact source before reuse | i915 PMU; permissions depend on perf security policy; JSON CLI possible | Manual validation tool; avoid continuous subprocess and privilege changes. PMU adapter deferred. Intel frequency does not establish engine utilization. |
| cpupower | Kernel tools maintained with Linux; GPL-2.0 family, per-file audit needed before copying | cpufreq driver dependent, reads mostly unprivileged; writes privileged; CLI/lib | Direct documented policy reads give needed telemetry without spawning CLI or linking a library. |
| AMD amdgpu sysfs | Kernel maintained | Driver/device exposes busy, VRAM and hwmon conditionally; no root normally required for reads | Selected vendor adapter; absent metrics retain explicit errors. |
| NVIDIA NVML | Vendor maintained documentation; proprietary runtime license, outside open-source-first baseline | NVIDIA driver/GPU dependent; library API, some operations privileged | PCI identification only in this release; NVML adapter deferred to an optional feature after license and hardware validation. No nvidia-smi polling. |
| systemd / oomd | Distribution maintained; component-specific LGPL/GPL | Not universal on MX; cgroup v2 and PSI dependent | Existing cgroup boundaries retained. Do not compete with distribution OOM policy or introduce process killing. |

Primary sources:

- [PSI](https://docs.kernel.org/accounting/psi.html), [cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html), [cpufreq](https://docs.kernel.org/admin-guide/pm/cpufreq.html), [hwmon ABI](https://docs.kernel.org/hwmon/sysfs-interface.html).
- [power-profiles-daemon Debian](https://packages.debian.org/en/trixie/power-profiles-daemon), [upstream](https://gitlab.freedesktop.org/upower/power-profiles-daemon).
- [tuned](https://github.com/redhat-performance/tuned), [COPYING](https://github.com/redhat-performance/tuned/blob/master/COPYING).
- [thermald](https://github.com/intel/thermal_daemon), [COPYING](https://github.com/intel/thermal_daemon/blob/master/COPYING).
- [lm-sensors](https://github.com/lm-sensors/lm-sensors), [library license](https://github.com/lm-sensors/lm-sensors/blob/master/COPYING.LGPL).
- [intel_gpu_top manual](https://manpages.debian.org/testing/intel-gpu-tools/intel_gpu_top.1.en.html), [i915 documentation](https://www.kernel.org/doc/html/latest/gpu/i915.html).
- [cpupower](https://github.com/torvalds/linux/tree/master/tools/power/cpupower), [AMD interfaces](https://docs.kernel.org/gpu/amdgpu/thermal.html), [NVML](https://docs.nvidia.com/deploy/nvml-api/latest/index.html).

## Rust dependency footprint

Workspace remains GPL-3.0-only. No new external crate was introduced: Phase 5 reuses serde and the existing telemetry crate. Cargo metadata from the resolved lockfile supplies the machine-readable inventory in dependency-licenses.json. MIT/Apache-2.0, BSD and Unicode licenses in this inventory are compatible with the workspace distribution license; preserve notices when packaging. This audit does not cover optional proprietary driver redistribution, which is excluded. Existing lz4_flex private-buffer storage is retained; it does not compress arbitrary application memory.
