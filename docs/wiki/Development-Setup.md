# Development Setup

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

Use a Linux checkout, Git, Python and the documented Rust toolchain. Declared MSRV is 1.82; CI also checks 1.100.0. Debian 13's packaged Rust 1.85.1 builds the vendored source; Debian 12's stock compiler is below MSRV even though its package runtime passed.

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 -m unittest discover -s repository -p 'test_*.py' -v
```

Archive tests require apt-utils, GnuPG and dpkg and use only disposable test keys. Run heavy release/sbuild builds in Actions. Destructive package lifecycle runs only in explicit disposable Debian containers, never the owner's physical laptop. [Packaging guide](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/debian/PACKAGING.md) describes exact pinned snapshot/toolchain sources; [owner setup](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/distribution/OWNER-SETUP.md) covers protected publication.
