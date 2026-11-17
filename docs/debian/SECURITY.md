# Privilege and distribution trust

## Current distribution status

The native APT publication design and protected workflows are proposed in
[PR #40](https://github.com/samuelindra123/xynoos-hypercore/pull/40). The PR is
not yet merged and the public APT archive and Debian GitHub release do not yet
exist. Current package CI uses disposable test keys; these are not production
signing credentials. See the [distribution report](../distribution/REPORT.md)
and its [internal security assessment](../distribution/SECURITY-ASSESSMENT.md)
for current evidence and owner gates. The internal assessment is not an
independent external audit or approval.

The package has no maintainer scripts, capabilities, setuid files, daemon,
Polkit rules, kernel/module/sysctl settings, swap/ZRAM changes, CPU/GPU controls,
desktop startup or shell changes. APT/dpkg needs root to write system files;
the executable needs no privilege. Read-only operations report unavailable data
without weakening host restrictions. Existing experimental library control code
is unchanged and is not exposed by these package commands.

The Debian feature prevents embedded installer operations before spawning a
process. Health reports package management, not signature or integrity success.
Diagnostics stay on stdout, eliminating the Python/OpenSSL dependency and hidden
HOME changes. Existing user evidence can be read by status but is never modified.
Observations and saved reports can contain host details; operators control storage
and sharing. Root can change package files and the dpkg database; checks cannot
protect against malicious root or a compromised build environment.

Existing `install.sh`, manager.py, release key, signed manifest, release workflow,
SHA256 verification, update and rollback are unchanged. Default Cargo builds
retain that behavior. Debian builds are separate artifacts with different bytes
and dynamic linkage. The old manifest verifier requires the musl target; it must
not be pointed at a `.deb`. A checksum beside a file is not authentication.

The original Debian package implementation produced unsigned CI review outputs;
`.changes` and `.buildinfo` were also unsigned (`-us -uc`). Workflow identity,
source commit and hashes support inspection but do not by themselves make those
outputs authenticated releases. PR #40 adds a separate release-attestation and
APT-signing path; it is not active on `main` until that PR is normally merged.

The APT design in PR #40 signs `InRelease` and `Release.gpg`; the signed Release
binds package indexes and package SHA256 values to a dedicated, independently
pinned OpenPGP fingerprint. GitHub OIDC attestations authenticate build
provenance separately. The existing standalone Ed25519 manifest does not cover
`.deb` files and must not be reused for them or described as compatible. Keep
key rotation/revocation and archive recovery under the owner procedure. Verify
production signatures and public bytes before any root installation. Installing
a local file with APT does not authenticate its origin.
