# Direct Debian Installation

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

The manual channel will use the [GitHub release list](https://github.com/samuelindra123/xynoos-hypercore/releases), with a distinct Debian tag derived from actual package metadata, e.g. `debian/0.1.0-preview.3-1` for package `0.1.0~preview.3-1`. Existing `v0.1.0-preview.*` standalone releases are preserved and must not be mistaken for .deb releases.

At audit time no Debian release is published. Unsigned Actions artifacts are review material, not authenticated releases. After publication, download the package, SHA256SUMS, source-provenance.json and attestation bundle from the same immutable prerelease. Verify SHA256, then use `gh attestation verify` pinned to this repository, `.github/workflows/apt-publish.yml`, `refs/heads/main` and the exact source digest recorded in provenance. A checksum alongside a binary does not independently prove origin.

Only install the verified local package using APT's local-file mode. Actual filename/version commands belong in [the verified publication guide](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/distribution/INSTALLATION.md) once the assets exist. The .buildinfo/.changes files are attested checksum subjects, not OpenPGP-signed Debian archive uploads.
