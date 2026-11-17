#!/usr/bin/env python3
"""Package one committed revision with only the safe static MVP entry point."""
import hashlib
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

root = Path(__file__).resolve().parents[1]
subprocess.run(["git", "diff", "--exit-code", "HEAD"], cwd=root, check=True)
subprocess.run(["cargo", "build", "-p", "xynoos-cli", "--bin", "xynoos-mvp", "--release",
                "--target", "x86_64-unknown-linux-musl"], cwd=root, check=True)
commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
tracked = subprocess.check_output(["git", "ls-files"], cwd=root, text=True).splitlines()
required = ["crates/xynoos-engine/src/lib.rs", "crates/xynoos-cli/src/bin/xynoos-mvp.rs",
            "docs/MVP-SAFETY-RECOVERY.md", "docs/MVP-N5000-VALIDATION.md"]
if not all(path in tracked for path in required):
    raise SystemExit("MVP sources/docs must be committed before packaging")
name = "xynoos-mvp-" + commit[:12] + "-linux-x86_64"
archive = root / "artifacts/mvp" / (name + ".tar.gz")
if archive.exists():
    raise SystemExit("Refusing to replace package: " + str(archive))
with tempfile.TemporaryDirectory(prefix="xynoos-mvp-package-") as temp:
    package = Path(temp) / name
    package.mkdir()
    files = {
        "target/x86_64-unknown-linux-musl/release/xynoos-mvp": "xynoos-mvp",
        "docs/MVP-QUICKSTART.md": "README.md",
        "docs/MVP-ARCHITECTURE.md": "ARCHITECTURE.md",
        "docs/MVP-SAFETY-RECOVERY.md": "SAFETY-RECOVERY.md",
        "docs/MVP-BENCHMARK.md": "BENCHMARK.md",
        "docs/MVP-N5000-VALIDATION.md": "N5000-VALIDATION.md",
        "docs/reports/MVP-RESULTS.md": "RESULTS.md",
        "docs/reports/MVP-QUALITY-GATES.txt": "QUALITY-GATES.txt",
        "artifacts/mvp/benchmark-final.summary.json": "BENCHMARK-SUMMARY.json",
        "fixtures/mvp-trace.json": "TRACE.json",
        "LICENSE": "LICENSE",
    }
    for source, destination in files.items():
        shutil.copy2(root / source, package / destination)
    (package / "SOURCE_COMMIT").write_text(commit + "\n")
    subprocess.run(["git", "archive", "--format=tar.gz", "--output=" + str(package / "SOURCE.tar.gz"), commit], cwd=root, check=True)
    sums = [hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name + "\n" for path in sorted(package.iterdir())]
    (package / "SHA256SUMS").write_text("".join(sums))
    subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=package, check=True)
    with tarfile.open(archive, "x:gz") as tar:
        tar.add(package, arcname=name)
checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix(archive.suffix + ".sha256").write_text(checksum + "  " + archive.name + "\n")
print("SOURCE_COMMIT=" + commit)
print(checksum + "  " + str(archive.relative_to(root)))
