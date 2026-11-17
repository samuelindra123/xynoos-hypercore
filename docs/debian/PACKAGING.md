# Debian packaging and reproducible builds

This is an upstream developer package, not an official Debian archive package
or a signed release. See REPORT.md for measured status. Primary target: Debian 13
Trixie amd64; MX based on Trixie requires independent validation. Only amd64 is
declared. ARM, i386 and other architectures are not built or supported.

The actual Cargo package is `xynoos-cli`, version `0.1.0-preview.3`, with two binary
targets. Only `xynoos` is installed. Debian maps the prerelease to
`0.1.0~preview.3-1`, which sorts before `0.1.0`. The revision is packaging-specific;
the upstream version is unchanged. Future releases must update Cargo and changelog
together. Do not reuse a published version for different bytes.

The package uses source format 3.0 (quilt), debhelper-compat 13, standard
`dpkg-buildpackage`, `dh_shlibdeps`, `dh_gencontrol` and `dh_fixperms`. It installs
`/usr/bin/xynoos` (root:root 0755), documentation (0644) and a manual page.
No maintainer scripts, conffiles, triggers, daemon, desktop or shell files exist.
The ELF is dynamically linked; `${shlibs:Depends}` produces runtime constraints.
Do not substitute guessed dependency versions. No Python, OpenSSL, Rust compiler,
Cargo, GPU library or systemd is required at runtime. The optional build feature
`debian-package` disables the embedded installer and automatic diagnostic saving.

The source uses `Option::is_none_or` (Rust 1.82). Locked dependencies declare
requirements at or below that level. Rust 1.82 tests and release builds passed in
CI and the manifests declare it. Trixie's packaged 1.85.1 passed clean builds; Bookworm's 1.63
cannot compile this source or parse the v4 lockfile. Do not replace the system
compiler or install rustup on the target desktop. Build on Trixie. Bookworm runtime
tests passed and are separate from build support. CI also validates the existing 1.100.0
toolchain; the signed release workflow is untouched.

Sources: [Trixie rustc](https://packages.debian.org/trixie/rustc),
[Bookworm rustc](https://packages.debian.org/bookworm/rustc),
[Rust API](https://doc.rust-lang.org/core/option/enum.Option.html),
[Cargo lock formats](https://doc.rust-lang.org/cargo/CHANGELOG.html),
[Debian Policy 4.7.2](https://packages.debian.org/trixie/debian-policy),
[sbuild](https://manpages.debian.org/trixie/sbuild/sbuild.1.en.html),
[Cargo vendor](https://doc.rust-lang.org/cargo/commands/cargo-vendor.html).

## Source and license preparation

From a clean, committed checkout, with an exact toolchain and network available:

```sh
rustup run 1.100.0 python3 packaging/prepare.py /tmp/xynoos-source
```

Python 3.12+ and Cargo are required. The output directory must not exist and must
be outside the checkout. Preparation exports Git HEAD, runs `cargo vendor --locked
--versioned-dirs`, verifies the lock is unchanged, generates relative Cargo source
replacement configuration, dependency metadata and complete license notices, and
creates a deterministic `.orig.tar.xz`. Cargo checks registry checksums; validation
checks every vendor file again. No third-party source is committed to Git.
The source archive contains it all, including Cargo.lock and provenance.

The 15 registry crates have MIT alternatives, with Unicode-3.0 additionally
required for unicode-ident. The generator copies all LICENSE/COPYING/NOTICE/
COPYRIGHT files, records authors, versions, checksums and declared licenses, and
refuses unknown licenses or sources. It appends complete MIT license text per crate
to Debian copyright, adding Unicode terms where required. Those notices and the compiler's standard
library copyright are installed. Project code is GPL-3.0-only. `Built-Using`
records the Rust source package version. Retain the matching Debian compiler
source when redistributing; the snapshot identifies it. Vendoring is an upstream
choice for exact dependency preservation, not a claim of Debian archive compliance.
Debian archive inclusion would need a separate review and typically packaged Rust
dependencies through dh-cargo. No cargo-deb or third-party build plugin is needed.

## Isolated CI build

Heavy work runs only in `.github/workflows/debian-package.yml`. The build container
is pinned by OCI digest. Its privileged capability is confined to a disposable
GitHub runner so sbuild can create namespaces. No secrets, signing or publishing
permissions are granted. Source preparation happens before isolation; dependency
installation uses the fixed Debian and security snapshots `20261116T000000Z`,
including the updates suite. Archive signatures
remain mandatory. Historical expiry checks alone are disabled. This snapshot must
be deliberately refreshed for security updates; it is not a runtime repository.

`mmdebstrap` creates a clean build tarball. sbuild unshare extracts it independently
twice, builds with networking disabled, an empty package-local CARGO_HOME,
`--frozen`, generic x86-64 flags, remapped source paths and no incremental cache.
`dpkg-buildpackage` derives SOURCE_DATE_EPOCH from changelog. The Git timestamp is
recorded separately. No developer HOME, native CPU flags or host compiler is used.

The CI container runs these commands through the checked-in script:

```sh
sbuild --chroot-mode=unshare --chroot=/srv/trixie-amd64.tar \
  --dist=trixie --arch=amd64 --no-enable-network --no-run-lintian \
  --build-dir=/out/first --build-path=/build/first /out/source/*.dsc
lintian --fail-on error,warning --show-overrides /out/first/*.deb /out/source/*.dsc
```

An operator can rebuild the downloaded `.dsc` using the same snapshot and tools.
Inside a disposable Trixie build environment with declared dependencies installed,
`dpkg-source -x PACKAGE.dsc` then `dpkg-buildpackage -b -us -uc` also works offline.
Do not use `-d` for a binary build; source preparation alone uses it because no
compiler is needed for that step. `nocheck` is supported by convention but CI does
not set it. No network fetch occurs in debian/rules.

Artifacts include `.deb`, `.buildinfo`, `.changes`, `.dsc`, original and Debian
source tarballs, full build logs, tool package list, build-root digest,
`provenance.json` and `SHA256SUMS`. A comparison failure fails CI and retains both
packages. Only the `.deb` bytes are compared; differing `.buildinfo` paths/dates
are expected. This bounded experiment does not establish reproducibility across
different compilers, snapshots or architectures. SHA256 proves consistency, not
authenticity. Retention is 30 days: archive the source and evidence before expiry.

## Troubleshooting

Missing vendor/source.json: run preparation; a raw checkout is intentionally not
a self-contained Debian source. Lock or vendor mismatch: stop and restore reviewed
inputs; never regenerate the lock during packaging. Old compiler: use the Trixie
builder; do not mix suites on the desktop. Namespace failure: use a disposable
runner supporting user namespaces, never weaken desktop kernel settings. Snapshot
failure: retry or deliberately update the pinned snapshot and repeat validation;
never silently fall back to live repositories. Inspect full logs for lintian or
build failures. The sole scoped override is `initial-upload-closes-no-bugs`:
this upstream package is not a Debian archive upload, and no Debian ITP exists.
Inventing a bug number would be incorrect. CI displays the override; no errors
or applicable warnings are suppressed. Do not add overrides just to pass.
