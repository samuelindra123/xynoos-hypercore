#!/usr/bin/env python3
"""Compare packages and record unsigned artifact checksums and measurements."""
import hashlib
from email.parser import Parser
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    out = Path(sys.argv[1]).resolve()
    first = list((out / "first").glob("*.deb"))
    second = list((out / "second").glob("*.deb"))
    assert len(first) == len(second) == 1, "Expected exactly one package per build"
    for pattern in ("*.buildinfo", "*.changes", "*.build"):
        assert list((out / "first").glob(pattern)), f"Missing {pattern}"
    for path in list((out / "source").glob("*.dsc")) + list((out / "first").glob("*.changes")):
        control = Parser().parsestr(path.read_text())
        for line in control["Checksums-Sha256"].strip().splitlines():
            digest, size, name = line.split()
            assert Path(name).name == name
            artifact = path.parent / name
            assert artifact.stat().st_size == int(size)
            assert hashlib.sha256(artifact.read_bytes()).hexdigest() == digest
    with tempfile.TemporaryDirectory() as directory:
        subprocess.run(["dpkg-deb", "-x", str(first[0]), directory], check=True)
        binary = Path(directory) / "usr/bin/xynoos"
        header = binary.read_bytes()[:20]
        assert header[:6] == b"\x7fELF\x02\x01" and header[18:20] == b"\x3e\x00", "Not amd64 ELF"
        (out / "elf.txt").write_text(subprocess.check_output(
            ["readelf", "-h", "-l", "-d", "--version-info", str(binary)], text=True))
    same = first[0].read_bytes() == second[0].read_bytes()
    report = {"comparison": "PASS" if same else "FAIL", "scope": "two clean roots, different build paths; same snapshot and inputs",
              "signed": False, "deb_bytes": first[0].stat().st_size,
              "control": subprocess.check_output(["dpkg-deb", "-I", str(first[0])], text=True),
              "files": subprocess.check_output(["dpkg-deb", "-c", str(first[0])], text=True)}
    report["source"] = json.loads(next((out / "source").glob("xynoos-hypercore-*/packaging/source.json")).read_text())
    (out / "provenance.json").write_text(json.dumps(report, indent=2) + "\n")
    # Do not include the unpacked source tree or logs in the release-like manifest.
    paths = sorted(p for directory in ("first", "second", "source") for p in (out / directory).iterdir()
                   if p.is_file() and p.suffix in {".deb", ".buildinfo", ".changes", ".dsc", ".xz"})
    paths += [out / "provenance.json", out / "build-root.sha256", out / "bootstrap-packages.txt", out / "elf.txt"]
    (out / "SHA256SUMS").write_text("".join(
        hashlib.sha256(p.read_bytes()).hexdigest() + "  " + str(p.relative_to(out)) + "\n" for p in paths))
    subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=out, check=True)
    assert same, "Packages differ; see both build logs and artifacts"


if __name__ == "__main__":
    main()
