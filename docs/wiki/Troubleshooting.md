# Troubleshooting

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

APT key/Release 404: publication may still be pending (#36); do not invent a source entry. NO_PUBKEY/BADSIG/expired metadata: stop, preserve the error and compare the independently approved fingerprint; never disable verification. Wrong architecture: only amd64 is supported. Dependency failure: retain `apt-cache policy` and OS release; do not force libc changes.

Unexpected command version: run `type -a xynoos` and inspect `/usr/bin/xynoos` versus `~/.local/bin/xynoos`. The two managers use different files; PATH can select the other installation. Use explicit paths while migrating. Foreign executable links are refused by the standalone manager rather than overwritten. Debian CLI lifecycle commands deliberately direct you to APT.

Missing GPU/sysfs/PSI fields mean unavailable evidence, not zero usage. See [compatibility](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/COMPATIBILITY.md), [permission fixes](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/INSTALL.md) and [[Reporting Issues|Reporting-Issues]].
