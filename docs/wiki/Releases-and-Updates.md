# Releases and Updates

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

Use the [release history](https://github.com/samuelindra123/xynoos-hypercore/releases) and inspect prerelease labels, exact source SHA and workflow URL. Standalone v0.1.0-preview.1–3 are existing releases; a Debian-prefixed prerelease is a separate delivery with its actual Debian version. Never overwrite a tag or release asset.

After the signed repository is verified/configured, ordinary APT refresh and upgrade manage updates. A package does not silently rewrite user configuration or self-update through the standalone manager. Repository metadata expires after 30 days and is renewed weekly by a protected workflow; missed expiry causes refusal until signed renewal. Stable rollout and key rotation need separate review (#36 and maintenance follow-ups).
