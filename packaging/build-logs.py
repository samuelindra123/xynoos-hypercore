#!/usr/bin/env python3
"""Preserve sbuild logs under portable artifact filenames, including on failure."""
from pathlib import Path
import sys

for directory in ("first", "second"):
    root = Path(sys.argv[1]) / directory
    # sbuild also creates a latest-log symlink; preserve the concrete log only.
    for path in root.glob("*.build"):
        if path.is_symlink():
            path.unlink()
    for path in root.glob("*.build"):
        print(f"\nLast 100 lines: {path.name}", flush=True)
        print("\n".join(path.read_text(errors="replace").splitlines()[-100:]), flush=True)
        if ":" in path.name:
            path.rename(path.with_name(path.name.replace(":", "-")))
