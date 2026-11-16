#!/usr/bin/env python3
"""Test public installation in a disposable HOME; never use a local release binary."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--version", required=True)
parser.add_argument("--previous", help="Published older preview for real upgrade + offline rollback")
args = parser.parse_args()
base_env = os.environ.copy()
for key in ("XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "GITHUB_TOKEN", "GH_TOKEN"):
    base_env.pop(key, None)
with tempfile.TemporaryDirectory(prefix="xynoos-public-home-") as directory:
    env = dict(base_env, HOME=directory, NO_COLOR="1")
    home = Path(directory)
    binary = home / ".local/bin/xynoos"
    # Literal primary command, with PATH addition in the same terminal.
    command = 'curl -fsSL https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/main/install.sh | sh && export PATH="$HOME/.local/bin:$PATH"'
    subprocess.run(["bash", "-o", "pipefail", "-c", command + '; xynoos --version'], env=env, check=True)
    identity = json.loads(subprocess.check_output([str(binary), "--version", "--json"], env=env))
    assert "v" + identity["version"] == args.version
    for command in (["doctor"], ["status"], ["diagnose", "--count", "2", "--interval-ms", "100", "--json"], ["update"]):
        subprocess.run([str(binary), *command], env=env, check=True)
    if args.previous:
        # A second disposable HOME tests older public install -> current public update.
        older_home = home / "older"; older_home.mkdir(mode=0o700)
        older_env = dict(env, HOME=str(older_home))
        manager = home / "installer.py"
        subprocess.run(["curl", "--proto", "=https", "-fsSLo", str(manager), f"https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/{args.version}/distribution/manager.py"], check=True)
        # Verify source against the copy embedded in the checked-out reviewed source.
        import hashlib
        expected = (Path(__file__).resolve().parents[1] / "distribution/manager.py").read_bytes()
        assert hashlib.sha256(manager.read_bytes()).digest() == hashlib.sha256(expected).digest()
        subprocess.run(["python3", "-I", str(manager), "install", "--version", args.previous], env=older_env, check=True)
        older_bin = older_home / ".local/bin/xynoos"
        subprocess.run([str(older_bin), "update"], env=older_env, check=True)
        assert json.loads(subprocess.check_output([str(older_bin), "--version", "--json"], env=older_env))["version"] == args.version[1:]
        offline_env = dict(older_env, HTTPS_PROXY="http://127.0.0.1:9", HTTP_PROXY="http://127.0.0.1:9", ALL_PROXY="http://127.0.0.1:9", NO_PROXY="")
        subprocess.run([str(older_bin), "rollback"], env=offline_env, check=True)
        assert json.loads(subprocess.check_output([str(older_bin), "--version", "--json"], env=older_env))["version"] == args.previous[1:]
    marker = home / ".local/state/xynoos/operator-evidence.txt"
    marker.write_text("preserve")
    subprocess.run([str(binary), "uninstall"], env=env, check=True)
    assert not binary.exists() and not binary.is_symlink()
    assert marker.read_text() == "preserve"
    subprocess.run(["bash", "-o", "pipefail", "-c", command], env=env, check=True)
    assert json.loads(subprocess.check_output([str(binary), "--version", "--json"], env=env))["version"] == args.version[1:]
    print("PUBLIC LIFECYCLE PASS: install, identity, doctor, status, diagnose, update, uninstall/evidence, reinstall" + (", upgrade + offline rollback" if args.previous else "; rollback requires a prior release"))
