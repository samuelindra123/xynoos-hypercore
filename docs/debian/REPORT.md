# Phase 6 engineering report

Audit and validation date: 2026-11-17. Delivered as open, unmerged
[PR #35](https://github.com/samuelindra123/xynoos-hypercore/pull/35), stacked on #34.
Results below apply to source **e662052b3aa722bd3ea23a4a8734a48b494ca87c**.
The later report/help/source-export correction is validated by subsequent PR checks;
each artifact embeds its own identity. Sizes and digests below are for the named
source, not a promise for every subsequent commit.

| Item | Classification and evidence |
|---|---|
| A. Repository baseline | PASS: clean integration/resource-intelligence at b8c78b72f32e844e64eade5c35147e973bb82ab1; all remote refs fetched. #34 was open/draft/unmerged with passing checks; main was c83d06c519b2ecf659b6e804df00bc449812914a. Rechecked #34 unchanged |
| B. Architecture | PASS: debhelper-compat 13, quilt source, one amd64 package, /usr/bin/xynoos, manual, licenses/notices, lock and provenance; no maintainer scripts or conffiles |
| C. Debian compatibility | PASS in disposable containers: Trixie build/lifecycle and Bookworm lifecycle with the Trixie artifact. Other distributions NOT TESTED |
| D. Rust toolchain | PASS: 1.82.0 workspace tests/release build and Debian-feature tests; verified minimum declared in all manifests. Stock Trixie 1.85.1 builds offline. Bookworm 1.63 cannot build this source |
| E. Dependencies | PASS: generated libc6 (>= 2.34), libgcc-s1 (>= 4.2); ELF agrees; APT resolves them on both suites. No runtime Python, OpenSSL, Cargo, GPU or init dependency |
| F. Size | PASS: .deb 492,520 bytes; Installed-Size 1,327 KiB. Regular files total 1,338,136 bytes; allocated regular-file space 1,372,160 bytes on CI filesystem, excluding directories/dpkg database |
| G. Reproducibility | PASS within scope: two independent sbuild unshare roots at /build/first and /build/second produce identical .deb bytes. Frozen vendors, empty CARGO_HOME, network disabled, fixed snapshot; checksums verified locally |
| H. Lifecycle | PASS on both suites: clean install, unprivileged CLI, ownership/modes, dependencies, tamper detection, reinstallation, removal/purge and data preservation; raw results in evidence/ |
| I. Upgrade/rollback | PASS for older same-payload packaging fixture, noninteractive downgrade refusal, explicit downgrade, upgrade and recovery. Historical upgrade and power loss NOT TESTED |
| J. CI | PASS: [all five packaging jobs](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44059808958); [Phase 5 validation](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44059809072). Formatting, strict Clippy with/without feature, locked tests, release, lintian and lifecycle gates passed |
| K. Security | PASS for inspected/tested boundaries: no hooks, services, tuning, startup or shell changes. CLI works as nobody with no commands in PATH and inaccessible HOME. Debian manager refuses. Signed files unchanged. Artifacts unsigned |
| L. MX Linux | NOT TESTED: init-independent design; no MX environment exercised |
| M. N5000 | NOT TESTED: no machine access; no physical certification or benefit claim. Exact protocol in HARDWARE.md |
| N. Blockers | BLOCKED for physical acceptance by lack of hardware/MX environment. No automated packaging blocker remains. Future signed distribution needs separate review; no release published |
| O. Installation | Exact commands below and in MX-LINUX.md; recovery/downgrade in LIFECYCLE.md |

The package at this source has SHA256:
`d380eab92ba1ed01fc3bc8057068b5475f8a8f0ee6f319a9219beaa90a841183`.
This is a consistency value, not an authentication anchor.

## Evidence and limits

Committed evidence includes [summary](evidence/summary.json),
[provenance and inventory](evidence/provenance.json),
[Trixie lifecycle](evidence/lifecycle-trixie.json),
[Bookworm lifecycle](evidence/lifecycle-bookworm.json), and both
[first](evidence/first.buildinfo) / [second](evidence/second.buildinfo) buildinfo.
[SHA256SUMS](evidence/SHA256SUMS) refers to paths in the downloaded CI artifact;
binaries/source tarballs are not committed here. Full logs, .deb, .changes,
.dsc and source tarballs are in `debian-amd64-unsigned` on the linked run, retained
for 30 days. Archive before expiry. Reproduction across compilers, snapshots
or architectures is NOT TESTED. Buildinfo differs in paths/dates by design.

Both package builds passed **106 tests, zero failed, one privileged test ignored**.
The Rust jobs passed; 41 existing distribution tests and two source-validation
tests passed. Local work was limited to formatting, shell/Python syntax, the two
lightweight tests, metadata/source export and artifact verification. No heavy
build or installation ran on the developer desktop.

Lifecycle fixtures preserve configuration, evidence and unrelated user files
byte-for-byte. Unpack/configure recovers cleanly; missing dependencies are rejected
and repaired. No historical .deb exists, so the upgrade fixture uses the current
payload under an older Debian version. Random kills, power loss, cross-version
data migrations and disk corruption are NOT TESTED.

Initial CI attempts were FAIL and corrected: bootstrap lacked the security
archive; dh_clean deleted Cargo.toml.orig; sbuild lacked iproute2; artifact upload
rejected timestamp colons; slim images excluded docs during footprint measurement.
These were setup/implementation failures, not pre-existing Rust failures.
No error was bypassed.

Lintian passes with one explicit override: `initial-upload-closes-no-bugs`.
An upstream CI package has no Debian ITP bug to close. The override and reason
are shown in CI; all other warnings/errors fail. This is not zero findings.
Existing example filename collision warnings remain. The libc loader diversion
warning was assessed against ELF requirements and both successful runtime suites;
no dependency was hardcoded to hide it. Missing /dev/console warnings are specific
to the build container and did not fail sbuild.

## Signed release and operational assessment

Installer, manager, key, release.yml and Cargo.lock are byte-identical to baseline.
Default builds preserve installer behavior. Debian builds use APT, have different
bytes and dynamic linkage, and never invoke the manager. Diagnostics emit stdout
without saving. Evidence remains untouched. The existing Ed25519 schema does not
cover .deb files. No release/tag/repository publication, main modification, force
push, branch deletion or merge is performed.

The inherited Phase 5 artifact workflow uses a legacy binary and unpinned actions;
it is outside scope and unchanged. No unrelated CI is modified.

## Exact operator commands

Authenticate repository, source and run separately; download and extract
`debian-amd64-unsigned` from the successful run. From that directory:

```sh
sha256sum -c SHA256SUMS
dpkg-deb --info first/xynoos-hypercore_0.1.0~preview.3-1_amd64.deb
dpkg-deb --contents first/xynoos-hypercore_0.1.0~preview.3-1_amd64.deb
sudo apt install ./first/xynoos-hypercore_0.1.0~preview.3-1_amd64.deb
dpkg-query -W -f='${Package} ${Version} ${Architecture} ${Depends}\n' xynoos-hypercore
dpkg --verify xynoos-hypercore
stat -c '%U:%G %a %n' /usr/bin/xynoos
/usr/bin/xynoos --version --json
/usr/bin/xynoos resources --json
/usr/bin/xynoos diagnose --count 2 --interval-ms 100 --json > xynoos-diagnostic.json
```

Use the absolute path if a user installation shadows it. Installation requires
root; execution does not. Review LIFECYCLE.md before removing or downgrading.
