#!/usr/bin/env python3
"""Seal the signed package: include signature/provenance and refresh all checksums."""
import hashlib
from pathlib import Path
import sys
import tarfile

d = Path(sys.argv[1])
assert (d / "manifest.json.sig").stat().st_size == 64
assert (d / "provenance.sigstore.json").is_file()
archives = list(d.glob("*.tar.gz"))
assert len(archives) == 1
archive = archives[0]
with tarfile.open(archive, "w:gz") as stream:
    for p in sorted(d.iterdir()):
        if p == archive or p.name == "SHA256SUMS": continue
        assert p.is_file() and not p.is_symlink()
        stream.add(p, arcname=p.name, recursive=False)
(d / "SHA256SUMS").write_text("".join(hashlib.sha256(p.read_bytes()).hexdigest() + "  " + p.name + "\n" for p in sorted(d.iterdir()) if p.is_file() and p.name != "SHA256SUMS"))
