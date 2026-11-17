# Known Limitations

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

- Native APT and direct Debian prerelease publication await protected signing setup (#36).
- Actual Wiki publication awaits initialization/access (#37); docs/wiki sources are maintained in Git.
- amd64 only; stable channel and other architectures are not enabled.
- Debian 12/13 container tests are not physical MX/N5000 certification.
- Real historical-version upgrade testing needs two published Debian versions; current downgrade/upgrade fixtures share payload.
- GPU vendor coverage is partial and sensor/driver dependent; NVML/i915 PMU and writable GPU control are absent.
- No proven desktop-wide performance gain, physical N5000 improvement or production readiness.
- Replay and CRC32 are not authenticated provenance. Sequential telemetry is not atomic/host-complete.
- Historical safety/MVP #14/#15 remain unmerged; delegation races, durable physical recovery and physical cleanup acceptance remain open (#17–#20, #38).

See [distribution report](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/distribution/https://github.com/samuelindra123/xynoos-hypercoreRT.md) for exact CI/publication classifications and URLs.

Follow-up acceptance: [real upgrades #41](https://github.com/samuelindra123/xynoos-hypercore/issues/41), [physical distribution #42](https://github.com/samuelindra123/xynoos-hypercore/issues/42), [archive operations #43](https://github.com/samuelindra123/xynoos-hypercore/issues/43), [future stable/architectures #44](https://github.com/samuelindra123/xynoos-hypercore/issues/44).
