# CPU and GPU Hardware Support

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

Distribution architecture is **amd64 only**. Baseline x86-64 code generation avoids native CPU flags. This does not certify every x86 CPU, kernel or driver.

PressureIQ can inspect CPU topology/cpufreq/governor identities, selected throttle counters, DRM/PCI identity, Intel frequency paths, AMD busy/VRAM paths, thermal/hwmon and power sensors when exposed. Intel GPU utilization is not invented from unsupported counters. NVIDIA NVML/i915 PMU collectors are not implemented; other vendors and shared-memory ownership are not universally supported.

**CI tested:** fixtures and missing/error cases. **Physical N5000/UHD 605:** pending independent acceptance. **All GPU vendors:** not supported as a claim. No clocks, fans or governors are written. See [hardware protocol](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/debian/HARDWARE.md).
