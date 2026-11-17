# Quick Start

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

For an installed verified `xynoos` binary:

```sh
xynoos --version --json
xynoos doctor --json
xynoos status --json
xynoos memory --interval-ms 1000
xynoos resources --json
xynoos diagnose --count 2 --interval-ms 100 --json
```

Commands read visible Linux telemetry and provide experimental diagnosis/advice. Missing sensors remain unavailable. Read-only observation does not prove a cause or performance benefit. The Debian build emits diagnostic evidence on stdout; redirect it to a user-owned file if needed. The standalone build uses its private evidence manager. Research commands `pressure`, `system` and `shadow` remain available.
