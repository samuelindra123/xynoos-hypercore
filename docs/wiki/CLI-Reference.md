# CLI Reference

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

The `xynoos` product binary exposes `help`, `doctor`, `status`, `capabilities`, `diagnose` and `benchmark`. Use `--json`, `--no-color`, NO_COLOR or TERM=dumb where needed. `diagnose` accepts count 1–60 and interval 100–10000 ms with a bounded total duration. `benchmark` measures observer overhead, not application speedups.

`memory` collects two observations (interval 100–60000 ms), optionally replays bounded JSON with `--replay FILE`, and supports `--human`. Replay is not authenticated and cannot authorize control. `resources --json` reports hardware capabilities with an advisory AURA plan. `pressure`, `system` and `shadow` retain their research interfaces in [source/reference](https://github.com/samuelindra123/xynoos-hypercore/blob/main/crates/xynoos-cli/src/main.rs).

Debian builds refuse `update`, `rollback` and `uninstall` and direct the user to APT. The rootless standalone build routes those operations to its signed release manager. The historical `mvp` workspace from PR14 is not merged and is not an installed CLI promise. See the [manual page](https://github.com/samuelindra123/xynoos-hypercore/blob/main/debian/xynoos.1).
