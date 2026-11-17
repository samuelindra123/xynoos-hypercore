# Performance Validation

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

No physical N5000 gain, general desktop speedup or production readiness is proven. Synthetic models, Codespaces tests and operator-supplied observations are distinct evidence classes. Read every result with its source SHA, environment, workload, control, sample count and measurement limits.

Owned compression experiments showed memory/CPU/time tradeoffs. The historical laptop study did not establish a repeatable acceptable benefit. Observer-overhead measurements measure collection cost, not application improvement. Replay is unauthenticated and cannot authorize physical operations.

Use [hardware acceptance](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/debian/HARDWARE.md), [resource-engine report](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/resource-engine/https://github.com/samuelindra123/xynoos-hypercoreRT.md) and existing #21 for representative physical replication. Preserve raw counters, resets/errors, baseline/observe comparisons and regressions. Do not deliberately OOM the physical laptop.
