#!/usr/bin/env python3
"""Export committed source and locked vendors; never build or install a package."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib


def run(*args, **kwargs):
    return subprocess.check_output(args, **kwargs)


def debian_version(version):
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-preview\.\d+)?", version):
        raise ValueError(f"Unsupported upstream version: {version}")
    return version.replace("-preview.", "~preview.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new directory outside the checkout")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if run("git", "status", "--porcelain", "--untracked-files=normal", cwd=root):
        raise SystemExit("Commit changes before exporting provenance")
    out = args.output.resolve()
    if out == root or root in out.parents:
        raise SystemExit("Output must be outside the checkout")
    out.mkdir(parents=True, exist_ok=False)
    commit = run("git", "rev-parse", "HEAD", cwd=root, text=True).strip()
    epoch = int(run("git", "show", "-s", "--format=%ct", "HEAD", cwd=root))
    cargo = tomllib.loads((root / "crates/xynoos-cli/Cargo.toml").read_text())
    version = debian_version(cargo["package"]["version"])
    tree = out / f"xynoos-hypercore-{version}"
    tree.mkdir()
    with tarfile.open(fileobj=io.BytesIO(run("git", "archive", "HEAD", cwd=root))) as archive:
        archive.extractall(tree, filter="data")
    lock_before = (tree / "Cargo.lock").read_bytes()
    config = run("cargo", "vendor", "--locked", "--versioned-dirs", "vendor", cwd=tree, text=True)
    if (tree / "Cargo.lock").read_bytes() != lock_before:
        raise SystemExit("Cargo.lock changed")
    (tree / ".cargo").mkdir(exist_ok=True)
    # Cargo may emit an absolute path. Keep the source archive relocatable.
    config = re.sub(r'directory = .*', 'directory = "vendor"', config)
    (tree / ".cargo/config.toml").write_text(config + "\n[net]\noffline = true\n")
    deps, notices, copyright_stanzas = [], [], []
    allowed = {"MIT", "MIT OR Apache-2.0", "Unlicense OR MIT", "(MIT OR Apache-2.0) AND Unicode-3.0"}
    lock = tomllib.loads(lock_before.decode())
    for package in lock["package"]:
        if "source" not in package:
            continue
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
            raise SystemExit("Unreviewed dependency source")
        name = f'{package["name"]}-{package["version"]}'
        crate = tree / "vendor" / name
        metadata = tomllib.loads((crate / "Cargo.toml").read_text())["package"]
        if metadata.get("license") not in allowed:
            raise SystemExit(f"Review license for {name}: {metadata.get('license')}")
        files = sorted(p for p in crate.rglob("*") if p.is_file() and
                       (p.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "COPYRIGHT"))))
        if not files:
            raise SystemExit(f"Missing license texts for {name}")
        deps.append({**package, "license": metadata["license"], "authors": metadata.get("authors", []),
                     "rust_version": metadata.get("rust-version"),
                     "notices": [str(p.relative_to(crate)) for p in files]})
        notices.append(f"\n=== {name} ({metadata['license']}) ===\n")
        for path in files:
            notices.extend([f"\n--- {path.relative_to(crate)} ---\n", path.read_text()])
        # Select the MIT alternative, preserving its exact terms and Unicode terms.
        mit = crate / "LICENSE-MIT"
        if not mit.exists():
            mit = crate / "LICENSE"
        selected = mit.read_text()
        license_name = "MIT"
        if metadata["license"] == "(MIT OR Apache-2.0) AND Unicode-3.0":
            selected += "\n" + (crate / "LICENSE-UNICODE").read_text()
            license_name = "MIT and Unicode-3.0"
        holders = [line.strip() for line in selected.splitlines() if re.search(r"copyright.*\d", line, re.I)]
        if not holders:
            holders = ["Not stated in upstream license; see Cargo authors in packaging/dependencies.json"]
        terms = "\n".join(" " + (line if line.strip() else ".") for line in selected.splitlines())
        copyright_stanzas.append(f"\nFiles: vendor/{name}/*\nCopyright: " + "\n ".join(holders)
                                 + f"\nLicense: {license_name}\n{terms}\n")
    with (tree / "debian/copyright").open("a") as stream:
        stream.write("".join(copyright_stanzas))
    (tree / "packaging/dependencies.json").write_text(json.dumps(deps, sort_keys=True, indent=2) + "\n")
    (tree / "packaging/THIRD_PARTY_NOTICES").write_text("".join(notices))
    source = {"commit": commit, "source_date_epoch": epoch, "upstream_version": cargo["package"]["version"],
              "debian_upstream_version": version, "target": "x86_64-unknown-linux-gnu",
              "cargo_lock_sha256": hashlib.sha256(lock_before).hexdigest(),
              "repository": "https://github.com/samuelindra123/xynoos-hypercore"}
    (tree / "packaging/source.json").write_text(json.dumps(source, sort_keys=True, indent=2) + "\n")
    orig = out / f"xynoos-hypercore_{version}.orig.tar.xz"
    with tarfile.open(orig, "w:xz", format=tarfile.PAX_FORMAT) as archive:
        for path in sorted(tree.rglob("*")):
            if path.relative_to(tree).parts[0] == "debian":
                continue
            info = archive.gettarinfo(str(path), arcname=str(path.relative_to(out)))
            info.uid = info.gid = 0
            info.uname = info.gname = "root"
            info.mtime = epoch
            info.mode = 0o755 if info.isdir() or os.access(path, os.X_OK) else 0o644
            if path.is_file():
                with path.open("rb") as stream:
                    archive.addfile(info, stream)
            else:
                archive.addfile(info)
    print(tree)


if __name__ == "__main__":
    main()
