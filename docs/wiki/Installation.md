# Installation

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

Two Debian channels are implemented in the publishing pipeline: a signed native APT repository for ordinary package-name installation, and immutable GitHub prereleases for direct .deb downloads. APT still downloads .deb files internally; manual .deb handling is unnecessary once the repository is configured.

Both Debian channels await first verified publication (#36). Do not copy a source entry or download a key that has not been published. The existing independent signed standalone channel remains available in [INSTALL.md](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/INSTALL.md).

Follow [[Native APT Installation|Native-APT-Installation]] or [[Direct Debian Installation|Direct-Debian-Installation]]. Avoid running the standalone installer with sudo. See [[Upgrade and Uninstall|Upgrade-and-Uninstall]] for manager-specific commands.
