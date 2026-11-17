# Xynoos HyperCore — Experimental Developer Preview

Preview.3 fixes the installer permission failures reported on Debian 13 / Intel
N5000. The physical preview.2 test confirmed the published binary, Ed25519/SHA256,
CLI, telemetry, isolated update and rollback, but normal HOME installation failed
on permissions. **Preview.3 installation on the physical N5000 is pending independent
OpenCode validation.** Codespaces regression tests do not certify that laptop.
See [compatibility](COMPATIBILITY.md) and [regression evidence](reports/PREVIEW-3-INSTALLER.md).

## Supported baseline

Linux x86_64, a functioning CA certificate store, curl, Python 3.9 or newer, and
OpenSSL 3 with Ed25519 support. No Git, Cargo, Rust, GitHub CLI, GitHub login, sudo
or archive extraction is needed by end users. The executable is static musl and
built for the baseline x86_64 CPU, without native CPU flags. Other architectures
are refused. Other distributions are compatibility targets, not tested promises.
Debian 13 / XFCE / Pentium Silver N5000 / 4 GB is the primary hardware target.
Prior physical results cover isolated execution; preview.3 normal HOME installation
requires a new physical check.

## Trust model

The convenience command executes a small shell bootstrap over GitHub HTTPS. You
initially trust GitHub, its TLS certificate authorities, repository control, and
the bootstrap you fetched. This is remote code execution by choice, not an
independently authenticated bootstrap. Read `install.sh` before using it.

The bootstrap pins the installer source by SHA256 and a versioned Git reference.
That installer embeds the release Ed25519 public key in
`distribution/release-key.pem`. It verifies the detached signature on the exact
manifest bytes, then binds the release version, platform, source commit, workflow
identity, binary size and SHA256. The executable is never run until those checks
succeed. SHA256 downloaded beside a binary is not the authentication anchor.

Signing-key SPKI (base64):
`MCowBQYDK2VwAyEAJfGz70mxvEUjhJupDAMfEtcGSbOAE1KEX3Lqe4yuwLA=`.
Treat a reviewed installer commit or this key obtained through a separately
trusted channel as the initial trust anchor. A key fetched alongside an artifact
without an existing pin is not independent authentication.

GitHub Actions also attests the binary and manifest using GitHub OIDC/Sigstore.
Release publication verifies those attestations against the repository, release
workflow and exact source commit. The end-user installer requires the pinned
Ed25519 signature and signed provenance identity; it does **not** independently
validate the Sigstore certificate chain. `provenance.sigstore.json` is supplied for
independent auditing. Maintainers/auditors can use `gh attestation verify` with
`--repo samuelindra123/xynoos-hypercore --signer-workflow
samuelindra123/xynoos-hypercore/.github/workflows/release.yml --source-digest COMMIT`.
GitHub CLI is not required to install or run Xynoos.

## Pinned, auditable alternative

The bootstrap in main is mutable; `--version` alone pins the binary selection,
not the bootstrap. The alternative below fetches the versioned preview.3 manager
and pins its exact bytes by SHA256. Pin the reviewed installer source, inspect it,
and verify the digest locally
**before** running Python. Confirm the commit/digest through your trusted channel;
copying both from a compromised page would not provide independent authentication.

```sh
curl --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 120 --max-filesize 1048576 -fsSLo xynoos-installer.py https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/v0.1.0-preview.3/distribution/manager.py
printf '%s  %s\n' 700df539d58227b1e90dbc754585afea723de8b1ea171b159077fcc71f3ab849 xynoos-installer.py | sha256sum -c - &&
python3 -I xynoos-installer.py install --version v0.1.0-preview.3
```

Inspect the downloaded script before running the verification/execution pair.
A failed digest check prevents Python execution.

There is one primary convenience command in the README/site. This multi-step
alternative intentionally makes initial trust and inspection explicit.

## Locations and first use

Executable: `~/.local/bin/xynoos`, an owned symlink through a single atomically
switched `current` symlink. Releases: `${XDG_DATA_HOME:-~/.local/share}/xynoos/versions/`.
State/evidence: `${XDG_STATE_HOME:-~/.local/state}/xynoos/`. No config or cache is
created unnecessarily. Private Xynoos directories require mode 0700; owned files
cannot be symlinks, hardlinks or writable by another user. Unsafe ancestor
permissions and symlinks fail closed. XDG roots must be absolute without `..`.
New directories are explicitly set to 0700 using a no-follow directory descriptor,
including when a default ACL would otherwise produce 0500 under umask 077. New
files receive explicit 0600 (metadata) or 0700 (binary). Existing directories are
validated without changing their permissions, owners or ACLs.

### Existing ~/.local/bin at mode 0775

Group write permits another group member to replace an executable link. Preview.3
refuses this directory before creating release/state directories or downloading a
release and prints the exact path and manual remedy. It does not assume a private
user group or accept an environment override. An existing user-owned 0755 or 0700
bin directory is accepted without chmod.

Inspect the path first. If it is your directory and it should not be writable by
other users, explicitly remove group/other write and retry:

```sh
stat -c '%U %G %a %n' "$HOME" "$HOME/.local" "$HOME/.local/bin"
# Inspect ACLs too, when getfacl is available:
getfacl -p "$HOME/.local/bin"
# Operator action, never done automatically by Xynoos:
chmod go-w -- "$HOME/.local/bin"
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/v0.1.0-preview.3/install.sh | sh -s -- --version v0.1.0-preview.3
export PATH="$HOME/.local/bin:$PATH"
```

If the shared permissions are intentional, stop and consult the directory owner;
do not weaken verification or use sudo to install. Group/other-writable ancestors
also require an explicit operator decision. Foreign owners and symlink ancestors
are refused without permission changes.

A private Xynoos directory left at 0500 by an earlier attempt is also preserved
and refused with an exact manual `chmod 0700 -- PATH` remedy. Inspect that path
and its ACL/ownership before deciding to run it; do not recursively chmod HOME.
No automatic repair of existing directories occurs. Record inherited default ACLs
with `getfacl` to help explain any recurrence on the physical machine.

The primary command adds `~/.local/bin` to the current terminal PATH. Debian's
default `~/.profile` normally adds it on subsequent logins once it exists. Custom
shell profiles may need the equivalent PATH entry; the installer never edits them.

```sh
xynoos --version
xynoos doctor
xynoos status
xynoos diagnose --count 10 --interval-ms 1000 --json
```

No daemon starts. Observe-only telemetry does not impose an artificial available
memory floor. Missing interfaces are reported as unavailable; memory pressure is
reported as observed. `--json`, `--no-color`, `NO_COLOR` and `TERM=dumb` are supported.
Research commands `pressure`, `system`, and `shadow` remain compatible.

## Updates, rollback and removal

`xynoos update` explicitly selects the highest eligible published SemVer preview
from paginated release lists, excluding drafts and releases without compatible
assets. It never uses `/releases/latest`. Use `--version v0.1.0-preview.N` to pin,
or `--channel stable` only after stable releases exist. Missing releases and
downgrades are errors. The preview version grammar is deliberately restricted to
`vMAJOR.MINOR.PATCH-preview.N`; unknown prerelease identifiers are ignored.

The manager holds a nonblocking advisory `flock` across download, verification,
staging, activation and recovery. Downloads have byte, socket-timeout and elapsed
time bounds. Only HTTPS and explicit GitHub/CDN redirect hosts are accepted.
Artifacts are raw ELF binaries; archive paths are never extracted. A new verified
version is fsynced and staged on the same filesystem before activation. Signature,
ELF architecture, executable identity and health failures stop activation.

`xynoos rollback` verifies the retained previous version's signature and digest
offline, health-checks it and switches atomically. The two most recent selections
can be toggled. Before a second verified version has been installed, rollback
correctly reports that no previous version exists. All downloaded versions are
retained; there is no automatic pruning in this preview (budget disk accordingly).

A durable transaction journal precedes the current-link switch. The next manager
operation restores the prior verified selection after an interrupted switch. A
process already executing the old inode continues safely on Linux. A crash may
temporarily leave the newly verified executable selected until the next manager
operation recovers the journal; there is no always-running supervisor. Kernel or
filesystem corruption and a malicious process with the same UID/root privileges
are outside the guarantee. Locks coordinate Xynoos writers, not arbitrary same-UID
writers. Do not edit the installation while an update is running.

`xynoos uninstall` preflights and removes only known owned versions and its exact
executable link. Unknown installation contents cause refusal and preservation.
Diagnostic evidence remains. Empty private installation directories and the lock
inode remain to preserve concurrency safety; no recursive HOME removal occurs.
Reinstallation reuses that lock. Evidence deletion is a separate operator action.

## Release maintenance

PR validation is unprivileged and never sees signing material. Tag/dispatch builds
require a source commit integrated into main through a merged PR and successful
applicable source validation. A later documentation merge does not invalidate an
already attested, reviewed source commit. Publication failures are not source-CI
failures; missing signing credentials still block automated publication.
Release publication verifies package/manifest/archive agreement, source/tag identity,
Ed25519 signature and GitHub build attestations. Actions are pinned by commit.

Configure the private key matching `distribution/release-key.pem` as the Actions
secret `XYNOOS_RELEASE_SIGNING_KEY`. The private key must never enter source, logs
or release assets. If secret configuration is permission-blocked, the publication
job fails closed after preserving the attested build for 30 days. An authorized
maintainer may download that exact attested artifact, sign its manifest offline,
and run `python3 scripts/publish-preview.py DIRECTORY VERSION`; this helper enforces
the same verification gates and never replaces published assets. Future automated
publication still requires that one-time owner secret configuration.

The initial key was generated during release preparation. Loss or compromise of
that key requires an explicit reviewed trust-anchor rotation; no metadata-provided
key is accepted automatically.
