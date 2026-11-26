#!/usr/bin/env python3
"""Build standalone portable archive for XYNOOS HyperCore."""
import hashlib
import os
from pathlib import Path
import shutil
import sys
import tarfile

def main():
    root = Path(__file__).resolve().parent.parent.parent
    dist_dir = root / "target" / "standalone"
    dist_dir.mkdir(parents=True, exist_ok=True)

    release_bin = root / "target" / "release" / "xynoos"
    if not release_bin.exists():
        print("Error: target/release/xynoos does not exist. Run 'cargo build --release' first.", file=sys.stderr)
        sys.exit(1)

    stage_dir = dist_dir / "xynoos-hypercore-standalone"
    if stage_dir.exists():
        shutil.rmtree(stage_dir)
    stage_dir.mkdir(parents=True)

    bin_dir = stage_dir / "bin"
    bin_dir.mkdir(parents=True)
    shutil.copy2(release_bin, bin_dir / "xynoos")

    release_xynoosd = root / "target" / "release" / "xynoosd"
    if release_xynoosd.exists():
        shutil.copy2(release_xynoosd, bin_dir / "xynoosd")

    installer = root / "packaging" / "standalone" / "install.sh"
    shutil.copy2(installer, stage_dir / "install.sh")
    os.chmod(stage_dir / "install.sh", 0o755)

    readme = root / "packaging" / "standalone" / "README.md"
    shutil.copy2(readme, stage_dir / "README.md")

    tar_path = dist_dir / "xynoos-hypercore-standalone-x86_64.tar.gz"
    with tarfile.open(tar_path, "w:gz") as tar:
        tar.add(stage_dir, arcname="xynoos-hypercore-standalone")

    hasher = hashlib.sha256()
    with open(tar_path, "rb") as f:
        while chunk := f.read(65536):
            hasher.update(chunk)
    digest = hasher.hexdigest()

    checksum_file = dist_dir / "SHA256SUMS"
    with open(checksum_file, "w") as f:
        f.write(f"{digest}  {tar_path.name}\n")

    print(f"Created standalone archive: {tar_path}")
    print(f"SHA-256: {digest}")

if __name__ == "__main__":
    main()
