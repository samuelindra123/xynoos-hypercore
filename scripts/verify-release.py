#!/usr/bin/env python3
"""Verify package, signature, checksums and executable identity before publication."""
import argparse
import hashlib
import importlib.util
from pathlib import Path
import tarfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("manager", ROOT / "distribution/manager.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
parser = argparse.ArgumentParser()
parser.add_argument("directory", type=Path)
parser.add_argument("--unsigned", action="store_true", help="Build validation only; never authorizes publication")
args = parser.parse_args()
d = args.directory.resolve()
raw = (d / "manifest.json").read_bytes()
manifest = m.strict_json(raw)
m.validate_manifest(manifest, manifest["version"])
if not args.unsigned:
    m.verify_signature(raw, (d / "manifest.json.sig").read_bytes(), d)
binary = d / manifest["binary"]["name"]
m.verify_binary(binary.read_bytes(), manifest)
expected = set()
for line in (d / "SHA256SUMS").read_text().splitlines():
    digest, name = line.split("  ", 1)
    m.require(Path(name).name == name and name not in expected, "Unsafe/duplicate checksum path")
    m.require(hashlib.sha256((d / name).read_bytes()).hexdigest() == digest, "Package checksum disagreement: " + name)
    expected.add(name)
m.require({manifest["binary"]["name"], "manifest.json", "LICENSE", "INSTALL.md", "N5000-VALIDATION.md", "manager.py", "release-key.pem"}.issubset(expected), "Incomplete release package")
if not args.unsigned:
    m.require({"manifest.json.sig", "provenance.sigstore.json"}.issubset(expected), "Signed package lacks provenance/signature checksums")
archives = list(d.glob("*.tar.gz"))
m.require(len(archives) == 1 and archives[0].name in expected, "Missing release archive checksum")
with tarfile.open(archives[0]) as archive:
    names = set()
    for member in archive:
        m.require(member.isfile() and Path(member.name).name == member.name and member.name not in names, "Unsafe archive entry")
        m.require(member.name in expected and member.size <= m.MAX_BINARY, "Unexpected archive member")
        m.require(archive.extractfile(member).read() == (d / member.name).read_bytes(), "Archive/asset disagreement")
        names.add(member.name)
    m.require(manifest["binary"]["name"] in names and "manifest.json" in names, "Incomplete archive")
# Only after all required verification succeeds may the packaged binary run.
m.health_binary(binary, manifest)
print("Release package verified" + (" (UNSIGNED BUILD ONLY)" if args.unsigned else " (signature + SHA256 + identity + archive)"))
