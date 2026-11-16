# Xynoos HyperCore distribution evidence

## Published baseline

- Release: [v0.1.0-preview.1](https://github.com/samuelindra123/xynoos-hypercore/releases/tag/v0.1.0-preview.1), published prerelease, not draft.
- Source: `6c249d12a43e251378eb3a075b1c9440b24e6125` (merged PR #27, with CLI/installer PR #26).
- Build: [43899858288](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/43899858288), successful build and attestation jobs; automated publication refused missing signing secret.
- Binary: static Linux x86_64 musl, 1,299,192 bytes, SHA256 `7f37a08dae63fc1095444c60e7a27814b8319f8b283f5df72dc46c88a0928757`.
- Manifest SHA256: `3fa94ca07a80386092c2442a1164db05e87aafcb4b9a77e8db1dce31eb655a23`.
- GitHub OIDC/Sigstore attestations for the binary and manifest verified against the repository, exact source digest, release workflow and GitHub-hosted runner.
- Ed25519 manifest signature, SHA256 sidecars, raw ELF, binary identity and final archive contents verified before publication. Signing used the privately held key after the attested build was downloaded, not a rebuilt binary.

## Public installation proof

[Full captured output](preview-public-lifecycle-1.txt) records the actual public
bootstrap with GitHub credentials removed and a disposable HOME. The package came
from public Release assets; no local binary/archive supplied installation input.

Passed: platform detection, pinned installer source digest, release signature,
binary SHA256 and identity, rootless install, `--version`, `doctor`, `status`,
bounded diagnostics, already-current update detection, uninstall with evidence
preserved, reinstall and version read-back. First-release rollback correctly needs
a second retained verified version; adversarial fixture tests cover offline
rollback, concurrent attempts, partial downloads/writes and recovery boundaries.

The validation runner initially reused a command variable and failed before its
reinstall subprocess. Correcting the runner and repeating the entire public
lifecycle produced the PASS record above. This was a test-runner failure; it was
not reported as a passing reinstall until the repeat completed.

## Preview 2 validation scope

The follow-up preview improves the default overview, permission-state reporting,
semantic terminal colors, evidence-save warnings and the validated lifecycle
runner. The installer trust anchor/protocol is unchanged. Public upgrade from
preview 1 and offline rollback require preview 2 publication and are a final
release gate, not a claim made by this source document in advance.

## Limits and owner configuration

Workspace tests and clippy pass; 30 Python security/release tests pass, including real SIGKILL recovery at three switch boundaries. The opt-in
physical kernel test remains ignored. The website retains the Studio design,
motion study, branding assets and animations. Clipboard success and unavailable
fallback are checked separately from existing site interactions.

Host tested: Ubuntu 24.04 Codespaces, x86_64, kernel `6.8.0-1070-azure`. The log
preserves observed pressure, including nonzero CPU/IO PSI. Debian 13, N5000, 4 GB,
XFCE, existing laptop swap/ZRAM and Firefox/VS Code workload overhead are **not
tested here**. No desktop performance benefit is proven. No kernel, swap, ZRAM,
governor, system service or unrelated application mutation occurred.

The Codespaces GitHub integration returned HTTP 403 for both workflow dispatch and
Actions secret configuration. Tag pushes start the reviewed release workflow.
Future automatic signing needs the owner to configure the existing private key
once, using an owner-authenticated session:

```sh
gh secret set XYNOOS_RELEASE_SIGNING_KEY --repo samuelindra123/xynoos-hypercore < /home/codespace/.config/xynoos-release/signing-key.pem
```

That private file has mode 0600 outside the repository. It must not be printed,
committed or uploaded as a release asset. Until owner configuration succeeds,
automatic publication fails closed; attested builds can use the verified offline
signing/publishing helper. This permission limit is not hidden by the successful
manual publication.
