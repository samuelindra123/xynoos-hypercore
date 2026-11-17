# Privilege and distribution trust

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

Current CI artifacts are unsigned review outputs. `.changes` and `.buildinfo` are
also unsigned (`-us -uc`). Workflow identity, source commit and hashes support
inspection but do not make them a release. No release, tag, APT repository or
signature is created by this change.

Future distribution needs a separately reviewed trust design: either an APT
repository whose InRelease/Release signatures bind Packages indexes and `.deb`
hashes to a pinned repository key, or a versioned manifest explicitly covering
the `.deb` architecture, version, byte size, SHA256 and source, authenticated by
the existing Ed25519 key under a new reviewed schema. Pin keys independently,
document rotation/revocation, archive matching source, and verify before root
installation. Do not extend the current manifest silently or call it compatible.
Future publication must require reviewed source and passing gates. Installing a
local file with APT does not authenticate its origin.
