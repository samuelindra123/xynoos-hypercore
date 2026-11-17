#!/usr/bin/env python3
"""Fail closed on missing/mismatched generated source provenance."""
import hashlib
import json
from pathlib import Path
import re
import tomllib


def main():
    source = json.loads(Path("packaging/source.json").read_text())
    assert re.fullmatch(r"[0-9a-f]{40}", source["commit"]), "Invalid source commit"
    assert source["target"] == "x86_64-unknown-linux-gnu"
    lock = Path("Cargo.lock").read_bytes()
    assert hashlib.sha256(lock).hexdigest() == source["cargo_lock_sha256"], "Lock mismatch"
    cargo = tomllib.loads(Path("crates/xynoos-cli/Cargo.toml").read_text())
    assert source["upstream_version"] == cargo["package"]["version"]
    assert source["debian_upstream_version"] == source["upstream_version"].replace("-preview.", "~preview.")
    first = Path("debian/changelog").read_text().splitlines()[0]
    assert first.startswith(f'xynoos-hypercore ({source["debian_upstream_version"]}-')
    expected = {(p["name"], p["version"], p["checksum"]) for p in tomllib.loads(lock.decode())["package"] if "source" in p}
    deps = json.loads(Path("packaging/dependencies.json").read_text())
    assert {(p["name"], p["version"], p["checksum"]) for p in deps} == expected
    for p in deps:
        base = Path("vendor") / f'{p["name"]}-{p["version"]}'
        checks = json.loads((base / ".cargo-checksum.json").read_text())
        assert checks["package"] == p["checksum"]
        for relative, digest in checks["files"].items():
            assert hashlib.sha256((base / relative).read_bytes()).hexdigest() == digest, relative
    assert Path("packaging/THIRD_PARTY_NOTICES").stat().st_size > 0
    print("Source identity, lock and all vendor file checksums verified")


if __name__ == "__main__":
    main()
