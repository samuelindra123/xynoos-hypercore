"""Exercise the actual shell bootstrap without network or release execution."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class BootstrapTests(unittest.TestCase):
    def run_bootstrap(self, tamper=False):
        with tempfile.TemporaryDirectory(prefix="xynoos-bootstrap-test-") as d:
            home = Path(d)
            tools = home / "tools"
            tools.mkdir(mode=0o700)
            tools.chmod(0o700)
            source = home / "source.py"
            source.write_bytes(b"tampered" if tamper else (ROOT / "distribution/manager.py").read_bytes())
            curl = tools / "curl"
            curl.write_text('''#!/bin/sh
while [ "$#" -gt 0 ]; do
    case "$1" in
        https://*) printf '%s' "$1" > "$HOME/url" ;;
        -o) shift; cp "$HOME/source.py" "$1" ;;
    esac
    shift
done
''')
            python = tools / "python3"
            python.write_text('''#!/bin/sh
printf '%s\\n' "$@" > "$HOME/executed"
''')
            curl.chmod(0o700)
            python.chmod(0o700)
            env = dict(os.environ, HOME=str(home), TMPDIR=str(home), PATH=str(tools) + os.pathsep + os.environ["PATH"])
            result = subprocess.run(["sh", "-c", 'umask 077; sh "$1" --version v0.1.0-preview.3', "bootstrap-test", str(ROOT / "install.sh")], env=env, capture_output=True)
            executed = (home / "executed").read_text() if (home / "executed").exists() else None
            self.assertIn("/v0.1.0-preview.3/distribution/manager.py", (home / "url").read_text())
            self.assertEqual(list(home.glob("xynoos-bootstrap.*")), [])
            return result, executed

    def test_fixed_source_digest_and_arguments(self):
        digest = hashlib.sha256((ROOT / "distribution/manager.py").read_bytes()).hexdigest()
        self.assertIn(digest, (ROOT / "install.sh").read_text())
        result, executed = self.run_bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("-I\n", executed)
        self.assertIn("install\n--version\nv0.1.0-preview.3\n", executed)

    def test_tampered_source_never_executes_and_stage_removed(self):
        result, executed = self.run_bootstrap(tamper=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(executed)
