# Installation channels and verified availability

**Native APT: BLOCKED / not yet published. Direct Debian release: BLOCKED / not yet published.** Owner signing configuration and first public acceptance are tracked in [#36](https://github.com/samuelindra123/xynoos-hypercore/issues/36). The actual public key fingerprint is not yet available. No runnable APT bootstrap or nonexistent .deb download URL is advertised here.

The existing [signed standalone preview](../INSTALL.md) remains available. Its release history uses v0.1.0-preview.* and a pinned Ed25519 trust anchor. It is independent from Debian/APT.

## Method A: native APT

The pipeline implements the target `https://samuelindra123.github.io/xynoos-hypercore/apt/`, **preview/main**, **amd64**. APT installs .deb packages internally; users should configure the signed repository once, then use ordinary package-name install/upgrade commands. They should not manually download a .deb for this method.

Publication acceptance requires a downloadable binary public key, independently approved full fingerprint, valid InRelease and Release.gpg, authenticated Packages/Packages.gz and checksum-valid .deb URLs, and a successful disposable Debian 13 package-name installation. The [owner procedure](OWNER-SETUP.md) gives exact keyring ownership/permissions, intended source stanza and public-check gates. Only after these pass will this guide publish the actual verified bootstrap.

For an already verified/configured installation:

```sh
sudo apt update
sudo apt install xynoos-hypercore
sudo apt upgrade
```

These are normal APT operations, **not proof the Xynoos source has already been published**. The package is experimental; the suite is preview and must not be labeled stable.

## Method B: direct Debian download

Use the [actual release list](https://github.com/samuelindra123/xynoos-hypercore/releases). Existing standalone assets are not .deb packages. Debian releases, once published, use a distinct `debian/` tag derived from `debian/changelog`, remain prereleases and contain .deb, SHA256SUMS, .buildinfo, .changes, source archives, scoped Cargo CycloneDX SBOM, source-provenance.json and GitHub attestation bundle.

Download all files for the same immutable version. Check SHA256SUMS, then verify package and checksum-manifest GitHub attestations against repository samuelindra123/xynoos-hypercore, signer workflow .github/workflows/apt-publish.yml, source ref refs/heads/main and exact source digest from provenance. Compare that SHA through reviewed source/release information. A checksum fetched beside a package is not independent authentication. After successful verification, install the actual local .deb using APT local-file installation. Exact filename/download commands will be supplied when verified assets exist; placeholders are deliberately not presented as runnable commands.

## Manager ownership and coexistence

APT owns `/usr/bin/xynoos` and `/usr/share/doc/xynoos-hypercore/`. It is the only lifecycle authority for that binary: the Debian build refuses standalone update/rollback/uninstall and directs you to APT. The rootless standalone installer uses only its owned `~/.local/bin/xynoos` link and XDG data/state, with strict foreign-link/path refusal; it has no `/usr/bin` install destination. Neither manager is allowed to overwrite the other's files. Existing signed bootstrap/key behavior remains unchanged.

Both commands can appear in PATH. Run `type -a xynoos` and use `/usr/bin/xynoos` explicitly for the Debian installation. To migrate, explicitly uninstall the standalone installation through its own verified user-path manager, preserve evidence, then install through APT. Installing a .deb does not silently delete another user's software/configuration.

## Upgrade, rollback and pinning

`apt-cache policy xynoos-hypercore` and `apt-cache madison xynoos-hypercore` show actual retained versions. Select an existing exact version for explicit `apt install --allow-downgrades` rollback. An unapproved `-y` downgrade must fail. Use `sudo apt-mark hold xynoos-hypercore` / `sudo apt-mark unhold xynoos-hypercore` to hold/resume a reviewed version; keep holds under review because they block security updates too. A future final version sorts after the corresponding Debian `~preview` version.

No user configuration is rewritten; there are no maintainer scripts or background services. Remove with `sudo apt remove xynoos-hypercore`; purge removes package-owned configuration if any is introduced in a reviewed future version, not user evidence. Source-list/keyring removal is separate and explicit.

Current upgrade/downgrade CI uses an older control-version **fixture with identical payload**. It is not a real historical upgrade claim. Two published Debian versions and physical MX/N5000 acceptance remain follow-ups; see [report](REPORT.md).
