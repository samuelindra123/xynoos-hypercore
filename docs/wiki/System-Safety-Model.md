# System Safety Model

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

The distributed package runs on demand, installs no daemon/maintainer service scripts, and does not alter kernel configuration, swap, ZRAM, CPU governors, GPU clocks or fans. Debian uses dpkg/APT authority; the standalone manager uses its own protected user directories.

Current CLI control preparation refuses absent implementations. Historical PR15 adds stronger controller gates but remains unmerged; #17–#20 and #38 track exclusive delegation, races, lifecycle cleanup, durable recovery and host telemetry. Library traits are not sandboxes against hostile consumer code. Historical opt-in systemd launchers can request transient service/cgroup creation and are not read-only diagnostics.

Do not infer physical crash recovery from simulation journals or unit tests. Read [package security](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/debian/SECURITY.md) and [historical audit](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/maintenance/HISTORICAL-PR-RESOLUTION.md).
