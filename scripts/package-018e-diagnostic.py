#!/usr/bin/env python3
"""Build an auditable package from a clean tracked revision; no live service."""
import hashlib
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

root = Path(__file__).resolve().parents[1]
subprocess.run(["git", "diff", "--exit-code", "HEAD"], cwd=root, check=True)
subprocess.run(["sh", "scripts/tests/test-018e-launcher.sh"], cwd=root, check=True)
subprocess.run(["cargo", "build", "--release", "--target", "x86_64-unknown-linux-musl",
                "-p", "xynoos-decision", "--example", "optimization_experiment"], cwd=root, check=True)
commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
name = "xynoos-018e-cleanup-mx-linux-x86_64"
archive = root / (name + ".tar.gz")
if archive.exists():
    raise SystemExit("Refusing to replace existing archive: " + str(archive))
with tempfile.TemporaryDirectory(prefix="xynoos-018e-package-") as temp:
    package = Path(temp) / name
    package.mkdir()
    files = {
        "target/x86_64-unknown-linux-musl/release/examples/optimization_experiment": "optimization_experiment",
        "scripts/xynoos-018d-launcher.sh": "xynoos-018d-launcher.sh",
        "scripts/xynoos-unit-preflight.py": "xynoos-unit-preflight.py",
        "docs/reports/TASK-018E-CLEANUP-MX-README.md": "README",
        "docs/reports/TASK-018E-CLEANUP.md": "PROTOCOL.md",
        "docs/reports/TASK-018E-CLEANUP-GATES.txt": "QUALITY-GATES.txt",
        "LICENSE": "LICENSE",
    }
    for source, dest in files.items():
        shutil.copy2(root / source, package / dest)
    (package / "SOURCE_COMMIT").write_text(commit + "\n")
    subprocess.run(["git", "archive", "--format=tar.gz", "--output=" + str(package / "SOURCE.tar.gz"), commit], cwd=root, check=True)
    sums = []
    for path in sorted(package.iterdir()):
        sums.append(hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name + "\n")
    (package / "SHA256SUMS").write_text("".join(sums))
    subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=package, check=True)
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(package, arcname=name)
checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
(root / (name + ".tar.gz.sha256")).write_text(checksum + "  " + archive.name + "\n")
print("SOURCE_COMMIT=" + commit)
print(checksum + "  " + archive.name)
