# Preview.3 installer correction

Base: main `32a55d8309f33284fe9a7e8a44dafd4f4c9228ca` (merged PR #30).
Scope: rootless installer permissions, pinned bootstrap, regression evidence,
release notes and physical validation handoff. No engine or optimization changes.

## Physical findings supplied by the operator

On Debian 13 / XFCE / Intel Pentium Silver N5000 / 4 GB, preview.2's published
binary, Ed25519 signature, SHA256, CLI, telemetry, isolated update and rollback
worked. Normal HOME installation failed on existing ~/.local/bin mode 0775 and
new private directories observed at 0500 under umask 077. These are user-reported
physical results; this Codespaces session does not independently certify them.
Preview.3 physical installation remains **PENDING** until OpenCode reports back.

## Reproduction and fix

[Before output](preview3/before.txt) reproduces the unsafe bin refusal and real
0500 creation using a Linux inherited default ACL with owner r-x. Plain umask 077
alone yields 0700 here. The laptop's ACLs/root cause remain uninspected.

New directories are created with bounded umask, then secured to exactly 0700
through an O_DIRECTORY/O_NOFOLLOW descriptor after checking owner. Staging and
signature-verification directories receive the same treatment. Exclusively created
metadata, executable and lock files receive explicit modes too, so inherited ACLs
do not remove required owner rights. Existing paths are never automatically
chmodded. Failed mkdir races do not lead to chmod of the competing directory.

The executable directory is checked first. Existing user-owned 0755/0700 is
accepted unchanged. Group/other-writable paths fail closed with a shell-quoted
manual `chmod go-w -- PATH` remedy and replacement-risk explanation. No assumption
about private groups or insecure bypass is introduced. An existing private Xynoos
directory at 0500 is preserved and refused with an explicit manual 0700 remedy.
Final private/bin directories must be user-owned; root-owned sticky shared
ancestors are allowed only as ancestors. Symlink, traversal, foreign ownership,
hardlink, signature, immutable version and unexpected-file protections remain.

## Trust boundary review

The old bootstrap still pinned the preview.1 installer, including when selecting
preview.2. It now pins the corrected preview.3 manager by exact SHA256 and release
reference. Bootstrap scratch permissions are secured only on its own fresh mktemp
directory, and signal exits clean up that directory. No signing key changes.

The initial shell command still trusts GitHub/TLS/repository control. Version
selection alone does not authenticate the downloaded bootstrap. INSTALL.md provides
a separate exact-byte manager pin, with checksum success required before Python
execution; the pin must come from a separately trusted channel for independent
bootstrap authentication. The manager's embedded Ed25519 key authenticates the
manifest and binds binary SHA256/size, source, version and platform before execution.
Sigstore build attestation verification remains a publication gate, not an
additional end-user installer trust anchor. Same-UID/root adversaries and filesystem
corruption remain outside the existing guarantee; no new security claim is made.

## Validation before PR

- [Distribution/security/bootstrap tests](preview3/distribution-tests.txt): 41 PASS,
  none skipped in Codespaces. Includes clean HOME, umask 000/027/077/777, actual
  restrictive/permissive default ACLs, signed real executable health, 0775 refusal
  plus explicit operator retry, unchanged existing permissions, foreign/root-owned
  bin, symlink/traversal, corrupt signature/checksum/platform, interrupted download,
  SIGKILL journal recovery, update, offline rollback, preserving uninstall/reinstall.
- [Rust checks](preview3/rust-checks.txt): fmt, strict workspace/all-target Clippy,
  locked workspace tests (96 PASS, one opt-in physical kernel test ignored),
  release musl build PASS. Existing duplicate example-output-name Cargo warnings
  do not affect this build; no dependency changes beyond the CLI version bump.
- [Static ELF and traced commands](preview3/security-checks.txt): no PT_INTERP or
  DT_NEEDED; --version/doctor/status/diagnose PASS, no control syscalls or writable
  /proc, /sys, /etc or /dev opens other than /dev/null. Local precommit binary
  identity is labeled; the release pipeline rebuilds with the merged source SHA.
- Shell syntax, Python compilation and git diff whitespace checks PASS.

Manual diff/security review: no existing directory chmod path; creation is
exclusive before permission changes; no new writable-path exception; read-only
product behavior preserved. PR CI and applicable main source CI must pass before
release dispatch. The established pipeline refuses existing versions, validates
merged-PR source, signs the manifest with the existing Actions key, verifies
attestations and reads back a public installation. Preview.2 is never replaced.

Published artifact and lifecycle results are recorded after publication separately;
this pre-release report does not claim publication or physical preview.3 PASS.
