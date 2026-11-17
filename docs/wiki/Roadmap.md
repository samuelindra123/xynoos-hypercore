# Roadmap

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

Current: read-only resource intelligence, preview standalone releases, Debian package CI and the proposed signed preview archive/Wiki publishing ecosystem. Verify live distribution status in #36/#37 before installing.

Next gates: publish and publicly verify preview packages; test upgrades between real retained package versions; physical Debian/MX N5000/UHD 605 acceptance; archive monitoring/retention/key-rotation drills; dependency-aware reconciliation of historical #14/#15 under #38. Stable channel, additional architectures and broader GPU telemetry need separate evidence and releases.

Writable control, automatic optimization, durable physical recovery and a production daemon are not implemented/promised. Existing #17–#23 describe prerequisite research and product decisions. No delivery date or performance guarantee is implied.
