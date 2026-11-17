# MX Linux Compatibility

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

**NOT TESTED physically** for the new Debian/APT channel. Debian compatibility does not automatically certify an MX release, desktop, custom kernel or repositories.

Use [MX guidance](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/debian/MX-LINUX.md) and [hardware acceptance](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/debian/HARDWARE.md). Record `/etc/os-release`, architecture, kernel, libc and dependency resolution. Never force incompatible dependencies or disable repository signatures. Debian 12 and 13 disposable package lifecycle evidence is cloud CI, not MX certification. Init-system independence follows from the on-demand package having no daemon/service activation; it is not universal MX support.
