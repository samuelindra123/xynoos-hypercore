#!/usr/bin/env python3
"""Test public installation in a disposable HOME; never use a local release binary."""
import argparse
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--version", required=True)
parser.add_argument("--previous", help="Published older preview for real upgrade + offline rollback")
args = parser.parse_args()
assert re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+-preview\.[1-9][0-9]*", args.version)
os.umask(0o077)
base_env = os.environ.copy()
for key in ("XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "GITHUB_TOKEN", "GH_TOKEN"):
    base_env.pop(key, None)
with tempfile.TemporaryDirectory(prefix="xynoos-public-home-") as directory:
    env = dict(base_env, HOME=directory, NO_COLOR="1")
    home = Path(directory)
    binary = home / ".local/bin/xynoos"
    # Literal primary command, with PATH addition in the same terminal.
    command = ("curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/"
               + args.version + "/install.sh | sh -s -- --version " + args.version
               + ' && export PATH="$HOME/.local/bin:$PATH"')
    subprocess.run(["bash", "-o", "pipefail", "-c", command + ' && xynoos --version'], env=env, check=True)
    identity = json.loads(subprocess.check_output([str(binary), "--version", "--json"], env=env))
    assert "v" + identity["version"] == args.version
    for cli_args in (["doctor"], ["status"], ["diagnose", "--count", "2", "--interval-ms", "100", "--json"], ["update"]):
        subprocess.run([str(binary), *cli_args], env=env, check=True)
    if args.previous:
        # A second disposable HOME tests older public install -> current public update.
        older_home = home / "older"; older_home.mkdir(mode=0o700); older_home.chmod(0o700)
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
    # The reported laptop path is unsafe until an operator explicitly removes
    # group write. Refuse without mutation, then prove the documented remedy.
    group_home = home / "group-home"
    group_home.mkdir(mode=0o700)
    group_home.chmod(0o700)
    group_bin = group_home / ".local/bin"
    group_bin.parent.mkdir(mode=0o700)
    group_bin.parent.chmod(0o700)
    group_bin.mkdir(mode=0o700)
    group_bin.chmod(0o775)
    group_env = dict(env, HOME=str(group_home))
    refused = subprocess.run(["bash", "-o", "pipefail", "-c", command], env=group_env, capture_output=True)
    assert refused.returncode != 0 and b"chmod go-w -- " in refused.stderr
    assert group_bin.stat().st_mode & 0o777 == 0o775
    print(refused.stderr.decode(), end="")
    group_bin.chmod(0o755)  # Explicit test operator action.
    subprocess.run(["bash", "-o", "pipefail", "-c", command], env=group_env, check=True)
    group_binary = group_bin / "xynoos"
    assert json.loads(subprocess.check_output([str(group_binary), "--version", "--json"], env=group_env))["version"] == args.version[1:]
    assert group_bin.stat().st_mode & 0o777 == 0o755
    for path in (home / ".local/share/xynoos", home / ".local/state/xynoos", home / ".local/share/xynoos/versions"):
        assert path.stat().st_mode & 0o777 == 0o700
    marker = home / ".local/state/xynoos/operator-evidence.txt"
    marker.write_text("preserve")
    subprocess.run([str(binary), "uninstall"], env=env, check=True)
    assert not binary.exists() and not binary.is_symlink()
    assert marker.read_text() == "preserve"
    subprocess.run(["bash", "-o", "pipefail", "-c", command], env=env, check=True)
    assert json.loads(subprocess.check_output([str(binary), "--version", "--json"], env=env))["version"] == args.version[1:]
    print("PUBLIC LIFECYCLE PASS: umask 077, bin 0775 refusal/manual remedy, permissions, install, identity, doctor, status, diagnose, update, uninstall/evidence, reinstall" + (", upgrade + offline rollback" if args.previous else "; rollback requires a prior release"))
