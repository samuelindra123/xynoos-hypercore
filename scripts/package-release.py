#!/usr/bin/env python3
"""Build deterministic release metadata from the executable identity and source tree."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("manager", ROOT / "distribution/manager.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

parser = argparse.ArgumentParser()
parser.add_argument("--binary", required=True, type=Path)
parser.add_argument("--output", default="dist-release", type=Path)
parser.add_argument("--version", required=True)
parser.add_argument("--run-id", required=True)
args = parser.parse_args()
tag = m.canonical_version(args.version)
commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
identity = json.loads(subprocess.check_output([str(args.binary.resolve()), "--version", "--json"]))
m.require(identity["version"] == tag[1:] and identity["source_commit"] == commit, "Binary/tag/source revision mismatch")
data = args.binary.read_bytes()
manifest = {"schema": 1, "repository": m.REPO, "version": tag, "channel": "preview" if "-preview." in tag else "stable",
            "source_commit": commit, "target": m.TARGET,
            "binary": {"name": "xynoos-" + m.TARGET, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()},
            "provenance": {"workflow": ".github/workflows/release.yml", "run_id": args.run_id,
                           "source": f"https://github.com/{m.REPO}/commit/{commit}"}}
m.validate_manifest(manifest, tag)
m.verify_binary(data, manifest)
# Require static ELF: no PT_INTERP or DT_NEEDED dynamic dependencies.
headers = subprocess.check_output(["readelf", "-l", str(args.binary)], text=True)
dynamic = subprocess.check_output(["readelf", "-d", str(args.binary)], text=True)
m.require("INTERP" not in headers and "(NEEDED)" not in dynamic, "Release binary must be static Linux ELF")
args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
(args.output / manifest["binary"]["name"]).write_bytes(data)
(args.output / manifest["binary"]["name"]).chmod(0o755)
(args.output / "manifest.json").write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n")
for source, dest in (("LICENSE", "LICENSE"), ("docs/INSTALL.md", "INSTALL.md"), ("docs/N5000-PREVIEW-VALIDATION.md", "N5000-VALIDATION.md"),
                     ("distribution/manager.py", "manager.py"), ("distribution/release-key.pem", "release-key.pem")):
    shutil.copyfile(ROOT / source, args.output / dest)
archive = args.output / f"xynoos-{tag}-{m.TARGET}.tar.gz"
# Archive is for manual inspection/distribution. The installer never extracts it.
with tarfile.open(archive, "w:gz") as stream:
    for p in sorted(args.output.iterdir()):
        if p == archive: continue
        stream.add(p, arcname=p.name, recursive=False)
checksums = "".join(hashlib.sha256(p.read_bytes()).hexdigest() + "  " + p.name + "\n" for p in sorted(args.output.iterdir()) if p.is_file())
(args.output / "SHA256SUMS").write_text(checksums)
print(json.dumps({"version": tag, "source_commit": commit, "manifest_sha256": hashlib.sha256((args.output / "manifest.json").read_bytes()).hexdigest()}))
