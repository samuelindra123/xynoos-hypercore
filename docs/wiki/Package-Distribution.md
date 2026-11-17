# Package Distribution

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

The binary package remains `xynoos-hypercore`, architecture `amd64`. Current Debian version is `0.1.0~preview.3-1`, derived from Cargo `0.1.0-preview.3`. The tilde sorts before the final version. Debian revisions identify packaging changes; published bytes for a version must never be replaced.

The preview archive uses apt-ftparchive, dists/preview/main/binary-amd64 and pool/main/x/xynoos-hypercore. InRelease and Release.gpg sign Release; its SHA256 hashes bind Packages, Packages.gz and a version ledger; package records bind package bytes. By-hash indexes and old pool files are retained. New publication must advance the preview version. Stable and other architectures are not enabled.

Clean offline sbuild roots, reproducibility, lifecycle, provenance, scoped Cargo SBOM and artifact attestations gate publication. GitHub Releases remain immutable direct downloads; GitHub Packages is not used for Debian/APT. Current publication is blocked on #36. See [archive maintenance](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/distribution/MAINTENANCE.md).
