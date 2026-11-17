#!/usr/bin/env python3
"""Maintainer publication helper. No clobber/overwrite; all assets are verified first."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

directory = Path(sys.argv[1]).resolve()
tag = sys.argv[2]
subprocess.run([sys.executable, "scripts/verify-release.py", str(directory)], check=True)
m = json.loads((directory / "manifest.json").read_bytes())
assert m["version"] == tag
subprocess.run([sys.executable, "scripts/release-source-gate.py", tag, "--source-commit", m["source_commit"]], check=True)
repo = m["repository"]
ref = subprocess.run(["gh", "api", f"repos/{repo}/git/ref/tags/{tag}"], capture_output=True)
if ref.returncode == 0:
    assert json.loads(ref.stdout)["object"]["sha"] == m["source_commit"], "Release tag/source mismatch"
binary = directory / m["binary"]["name"]
subprocess.run(["gh", "attestation", "verify", str(binary), "--repo", repo, "--signer-workflow", repo + "/.github/workflows/release.yml", "--source-digest", m["source_commit"]], check=True)
subprocess.run(["gh", "attestation", "verify", str(directory / "manifest.json"), "--repo", repo, "--signer-workflow", repo + "/.github/workflows/release.yml", "--source-digest", m["source_commit"]], check=True)
notes = f"""Experimental Developer Preview — {tag}

Xynoos HyperCore provides rootless, read-only Linux resource diagnostics. Physical kernel mutation is disabled. Desktop performance benefit is NOT PROVEN.

Target: Linux x86_64, static musl binary. Primary laptop target: Debian 13, Pentium Silver N5000, 4 GB RAM; preview.3 physical installation validation remains pending. Earlier physical testing confirmed isolated binary/signature/checksum/CLI/telemetry/update/rollback behavior and reported installer permission failures.

Installer fixes in preview.3: new private directories are explicitly secured as 0700 despite inherited default ACLs; existing group/other-writable ~/.local/bin is refused with an exact manual chmod remedy. Existing user directories are never chmodded automatically. Symlink, owner, traversal, signature, update/rollback and data-preservation checks remain enforced. The convenience bootstrap now pins the fixed installer by SHA256. See INSTALL.md for the pinned alternative and permission remedies.

Source: {m['source_commit']}
Build: https://github.com/{repo}/actions/runs/{m['provenance']['run_id']}

Authenticity: pinned Ed25519 release key signs manifest.json; the signed manifest binds source commit, version, platform and binary SHA256. GitHub Actions build attestations are published separately. See INSTALL.md for bootstrap trust, pinned verification, update, offline rollback and uninstall.

No background service, sudo, sysctl, swap, ZRAM, governor or application changes. Python 3.9+, curl and OpenSSL 3 are host prerequisites. ~/.local/bin must be on PATH for bare xynoos commands.
"""
with tempfile.NamedTemporaryFile("w", suffix=".md") as stream:
    stream.write(notes); stream.flush()
    subprocess.run(["gh", "release", "create", tag, "--repo", repo, "--target", m["source_commit"], "--prerelease", "--title", f"Xynoos HyperCore {tag} — Experimental Developer Preview", "--notes-file", stream.name,
                    *[str(p) for p in sorted(directory.iterdir()) if p.is_file()]], check=True)
actual = json.loads(subprocess.check_output(["gh", "api", f"repos/{repo}/releases/tags/{tag}"]))
assert actual["tag_name"] == tag and actual["prerelease"] and not actual["draft"]
assert {p.name for p in directory.iterdir() if p.is_file()}.issubset({a["name"] for a in actual["assets"]})
print(actual["html_url"])
