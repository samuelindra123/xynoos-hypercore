# Package lifecycle and recovery

Install or upgrade an inspected, authenticated local package using `sudo apt
install ./PACKAGE.deb`. Retain the exact prior `.deb`, source and SHA256 separately
before upgrading. APT does not provide the signed installer's retained versions
or atomic symlink rollback. Processes already running can retain the old inode.
Close and reopen the command to use the new package. No service needs restarting.

Downgrades are refused by noninteractive APT unless explicitly allowed. After
checking the prior package and compatibility, run:

```sh
sudo apt install --allow-downgrades ./PRIOR-PACKAGE.deb
dpkg --verify xynoos-hypercore
/usr/bin/xynoos --version --json
```

Interactive APT can prompt for confirmation; raw dpkg permits downgrades. This
package adds no custom enforcement. There is no configuration or evidence migration.
JSON consumers must check schema_version; future compatibility is not promised.
The existing signed manager cannot roll back a Debian package.

```sh
sudo apt remove xynoos-hypercore
sudo apt purge xynoos-hypercore
sudo apt install ./PACKAGE.deb
sudo apt install --reinstall ./PACKAGE.deb
```

Remove deletes package-owned files. Purge currently has the same result because
there are no conffiles or maintainer scripts. User evidence, configuration, signed
installations and unrelated files are never touched. The package neither owns nor
creates any HOME path. It does not prune data. Avoid automatic recursive deletion.

After interruption, inspect `dpkg --audit`, then use `sudo dpkg --configure -a`
and, if dependencies are broken, review `sudo apt --fix-broken install`. Read the
proposed transaction first; APT may remove an unconfigurable package. Reinstall
the inspected `.deb` if files are damaged. Do not delete dpkg locks while a package
manager is running. The package has no scripts to resume; dpkg owns recovery.
Power loss, disk corruption and arbitrary termination remain untested.

The automated matrix runs only in marked disposable Debian containers:

| Test | Method / scope |
|---|---|
| Clean install and dependencies | APT installs the real artifact |
| Execution and CLI | nobody runs bounded commands; JSON and identity checked |
| Ownership and permissions | All archive members; dpkg file ownership |
| Integrity | dpkg verification detects appended bytes; reinstall restores |
| Same-payload fixture upgrade | Older synthetic Debian revision with current payload, then current artifact; **fixture evidence only** |
| Same-payload fixture downgrade | Noninteractive refusal, then explicit allow-downgrades; **fixture evidence only** |
| Remove, purge, reinstall | Binary absent/present and dpkg audit clean |
| Preservation | Byte comparison of user config, evidence and unrelated data |
| Interrupted boundary | Unpack, check unpacked state, configure and audit |
| Dependency failure | Fixture with nonexistent dependency; APT and dpkg refusal |

Fixtures are explicitly synthetic and never published. The current CI result does
not test a real historical release upgrade or downgrade. This does not test
cross-version migrations, power loss or a kill in the middle of unpack. Tests
verify the final clean state after failure recovery.
Do not execute lifecycle.py on a desktop: it requires a container marker and
explicit environment guard, and performs destructive package operations.

CI calls ci-lifecycle.sh, which creates the guard in a disposable container.
To reproduce, use the exact pinned image from the workflow, mount the checkout
read-only at `/work`, extracted artifacts read-only at `/artifacts`, and an empty
results directory at `/results`; run `bash /work/packaging/ci-lifecycle.sh trixie`.
No privileged container is needed for lifecycle tests. The script's APT source
changes are confined to the disposable container. Never run that script on a host.
The test removes the official slim image's dpkg documentation exclusion so the
full archive, including provenance and manuals, is installed and measured.

## Reproducible real-release upgrade and rollback

Run only after two independently versioned Debian prereleases are published and
retained by the signed APT archive. This procedure does not use the same-payload
fixture. First verify the `.deb` and `SHA256SUMS` attestations from each immutable
GitHub release against this repository, the `apt-publish.yml` signer workflow,
`refs/heads/main`, and the exact source commit in that release's provenance. For
each release's downloaded `.deb` and `SHA256SUMS`, use its exact provenance
commit as `SOURCE_SHA`:

```sh
gh attestation verify xynoos-hypercore_VERSION_amd64.deb \
  --repo samuelindra123/xynoos-hypercore \
  --signer-workflow samuelindra123/xynoos-hypercore/.github/workflows/apt-publish.yml \
  --source-ref refs/heads/main --source-digest SOURCE_SHA
gh attestation verify SHA256SUMS \
  --repo samuelindra123/xynoos-hypercore \
  --signer-workflow samuelindra123/xynoos-hypercore/.github/workflows/apt-publish.yml \
  --source-ref refs/heads/main --source-digest SOURCE_SHA
```

Then,
on a review workstation, start a disposable Debian 13 container with a read-only
source mount and a private result directory:

```sh
mkdir -m 700 -p lifecycle-results
docker run --rm --platform linux/amd64 --cgroupns private \
  -v "$PWD:/work:ro" -v "$PWD/lifecycle-results:/results" \
  -e XYNOOS_DISPOSABLE_TEST=1 \
  -e XYNOOS_APT_FINGERPRINT=APPROVED_FULL_FINGERPRINT \
  -e XYNOOS_LIFECYCLE_REPORT=/results/real-two-version-lifecycle.log \
  -e OLD_VERSION=OLDER_PUBLISHED_DEBIAN_VERSION \
  -e NEW_VERSION=NEWER_PUBLISHED_DEBIAN_VERSION \
  debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 \
  bash -ec 'touch /run/xynoos-package-test; bash /work/repository/real-two-version-lifecycle.sh "$OLD_VERSION" "$NEW_VERSION"'
```

The script checks that the image is Debian 13 amd64 and carries the explicit
container marker before package operations. It downloads and pins the public key
by its full fingerprint, verifies both Release signature forms, runs `apt-get
update`, checks old/new package hashes against the signed index, installs the old
version by package name, upgrades with normal APT, verifies implicit downgrade
refusal, explicitly downgrades, re-upgrades, checks dependencies and dpkg
ownership/integrity, and confirms operator files survive purge. The report log is
created under a restrictive umask. This script has **NOT BEEN RUN** because two
Debian prereleases are not publicly available; [Issue #41](https://github.com/samuelindra123/xynoos-hypercore/issues/41)
tracks that release gate. Never execute it on a host.
