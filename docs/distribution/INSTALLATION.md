# Installation channels and verified availability

The signed **Debian 13 (trixie) amd64 preview** is available through [native APT](https://samuelindra123.github.io/xynoos-hypercore/apt/) and as an [immutable GitHub prerelease](https://github.com/samuelindra123/xynoos-hypercore/releases/tag/debian/0.1.0-preview.3-1). The first published package is `0.1.0~preview.3-1`. [Publication and Pages deployment evidence](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44276675682).

The existing [signed standalone preview](../INSTALL.md) remains available. Its release history uses v0.1.0-preview.* and a pinned Ed25519 trust anchor. It is independent from Debian/APT.

## Method A: native APT

On Debian 13 amd64, install the public archive key after comparing its full primary fingerprint with the independently pinned value below. Run these commands in a POSIX shell:

```sh
sudo apt update
sudo apt install ca-certificates curl gnupg
(
  set -eu
  base=https://samuelindra123.github.io/xynoos-hypercore/apt
  key=$(mktemp)
  trap 'rm -f "$key"' EXIT
  curl --proto '=https' -fsSL "$base/xynoos-archive-keyring.gpg" -o "$key"
  fingerprints=$(gpg --batch --show-keys --with-colons "$key" |
    awk -F: '$1=="pub"{primary=1;next} primary && $1=="fpr"{print $10;primary=0}')
  test "$fingerprints" = 494518E0726F9C019CFE091ECC02D315D84DD76E
  sudo install -o root -g root -m 0644 "$key" /usr/share/keyrings/xynoos-archive-keyring.gpg
  printf 'deb [arch=amd64 signed-by=/usr/share/keyrings/xynoos-archive-keyring.gpg] %s preview main\n' "$base" |
    sudo tee /etc/apt/sources.list.d/xynoos.list >/dev/null
  sudo apt -o APT::Update::Error-Mode=any update
  sudo apt install xynoos-hypercore
)
/usr/bin/xynoos --version --json
```

The `preview` suite is experimental. Remove the package with `sudo apt remove xynoos-hypercore`.

## Method B: direct Debian download

Use the [actual release list](https://github.com/samuelindra123/xynoos-hypercore/releases). Existing standalone assets are not .deb packages. Debian releases use a distinct `debian/` tag derived from `debian/changelog`, remain prereleases and contain .deb, SHA256SUMS, .buildinfo, .changes, source archives, scoped Cargo CycloneDX SBOM, source-provenance.json and GitHub attestation bundle.

Download all files for the same immutable version. Check SHA256SUMS, then verify package and checksum-manifest GitHub attestations against repository samuelindra123/xynoos-hypercore, signer workflow .github/workflows/apt-publish.yml, source ref refs/heads/main and exact source digest from provenance. Compare that SHA through reviewed source/release information. A checksum fetched beside a package is not independent authentication. After successful verification, install the actual local .deb using APT local-file installation. The preview 3 asset is [xynoos-hypercore_0.1.0.preview.3-1_amd64.deb](https://github.com/samuelindra123/xynoos-hypercore/releases/download/debian/0.1.0-preview.3-1/xynoos-hypercore_0.1.0.preview.3-1_amd64.deb). GitHub displays a dot where the attested manifest uses a tilde. Preserve the bytes and restore the attested filename before checking the manifest.

## Manager ownership and coexistence

APT owns `/usr/bin/xynoos` and `/usr/share/doc/xynoos-hypercore/`. It is the only lifecycle authority for that binary: the Debian build refuses standalone update/rollback/uninstall and directs you to APT. The rootless standalone installer uses only its owned `~/.local/bin/xynoos` link and XDG data/state, with strict foreign-link/path refusal; it has no `/usr/bin` install destination. Neither manager is allowed to overwrite the other's files. Existing signed bootstrap/key behavior remains unchanged.

Both commands can appear in PATH. Run `type -a xynoos` and use `/usr/bin/xynoos` explicitly for the Debian installation. To migrate, explicitly uninstall the standalone installation through its own verified user-path manager, preserve evidence, then install through APT. Installing a .deb does not silently delete another user's software/configuration.

## Upgrade, rollback and pinning

`apt-cache policy xynoos-hypercore` and `apt-cache madison xynoos-hypercore` show actual retained versions. Select an existing exact version for explicit `apt install --allow-downgrades` rollback. An unapproved `-y` downgrade must fail. Use `sudo apt-mark hold xynoos-hypercore` / `sudo apt-mark unhold xynoos-hypercore` to hold/resume a reviewed version; keep holds under review because they block security updates too. A future final version sorts after the corresponding Debian `~preview` version.

No user configuration is rewritten; there are no maintainer scripts or background services. Remove with `sudo apt remove xynoos-hypercore`; purge removes package-owned configuration if any is introduced in a reviewed future version, not user evidence. Source-list/keyring removal is separate and explicit.

Current upgrade/downgrade CI uses an older control-version **fixture with identical payload**. It is not a real historical upgrade claim. Two published Debian versions and physical MX/N5000 acceptance remain follow-ups; see [report](REPORT.md).
