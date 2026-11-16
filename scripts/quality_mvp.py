#!/usr/bin/env python3
"""Run the MVP gates sequentially and preserve complete command output."""
import datetime
import argparse
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, default=root / "docs/reports/MVP-QUALITY-GATES.txt")
args = parser.parse_args()
log_path = args.output
commands = [
    ["cargo", "fmt", "--all", "--", "--check"],
    ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
    ["cargo", "check", "--workspace", "--all-targets"],
    ["cargo", "test", "--workspace"],
    ["cargo", "build", "--release", "-p", "xynoos-engine", "--bin", "xynoos-perf", "-p", "xynoos-cli", "--bin", "xynoos-mvp"],
    ["python3", "-m", "unittest", "discover", "-s", "scripts/tests", "-p", "test_*.py", "-v"],
    ["cargo", "test", "-p", "xynoos-decision", "--example", "optimization_experiment"],
    ["cargo", "build", "--workspace", "--release"],
    ["cargo", "doc", "--workspace", "--no-deps"],
    ["sh", "scripts/tests/test-018e-launcher.sh"],
    ["cargo", "build", "-p", "xynoos-cli", "--bin", "xynoos-mvp", "--release", "--target", "x86_64-unknown-linux-musl"],
    ["cargo", "build", "-p", "xynoos-engine", "--bin", "xynoos-perf", "--release", "--target", "x86_64-unknown-linux-musl"],
    ["file", "target/x86_64-unknown-linux-musl/release/xynoos-mvp", "target/x86_64-unknown-linux-musl/release/xynoos-perf"],
    ["git", "diff", "--check"],
]
with log_path.open("x") as log:
    log.write("MVP gates " + datetime.datetime.now(datetime.timezone.utc).isoformat() + "\n")
    for command in commands:
        env = dict(os.environ)
        if command[:2] == ["cargo", "doc"]:
            env["RUSTDOCFLAGS"] = "-D warnings"
        log.write("\n$ " + ("RUSTDOCFLAGS='-D warnings' " if command[:2] == ["cargo", "doc"] else "") + " ".join(command) + "\n")
        log.flush()
        result = subprocess.run(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
        log.write("exit=" + str(result.returncode) + "\n")
        log.flush()
        print("exit=" + str(result.returncode) + " " + " ".join(command), flush=True)
        if result.returncode:
            print("Failure preserved in " + str(log_path), file=sys.stderr)
            sys.exit(result.returncode)
