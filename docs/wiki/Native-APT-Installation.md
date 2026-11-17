# Native APT Installation

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

**Publication pending; no runnable bootstrap is advertised.** Target: `https://samuelindra123.github.io/xynoos-hypercore/apt/`. The generated, signature-checked metadata uses suite **preview**, component **main**, architecture **amd64**. This is not a stable channel.

After #36 records public HTTP/signature/install evidence, the maintainer can publish the exact bootstrap: create `/usr/share/keyrings` with root ownership/mode 0755; download the binary public key `apt/xynoos-archive-keyring.gpg` over HTTPS to a temporary file; confirm its full fingerprint against an independently trusted maintainer announcement; install it root-owned/readable at mode 0644. Configure `/etc/apt/sources.list.d/xynoos.list` with `arch=amd64` and `signed-by=/usr/share/keyrings/xynoos-archive-keyring.gpg`, then use normal `sudo apt update` and `sudo apt install xynoos-hypercore`.

The complete source stanza and exact commands must be enabled only after public verification, using [the installation/status guide](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/distribution/INSTALLATION.md). Never use apt-key, trusted=yes, allow-insecure-repositories or signature bypasses. HTTPS bootstrapping alone is not an independent trust anchor.
