"""Exercise provenance verification failures without downloads or compilation."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


class SourceTests(unittest.TestCase):
    def test_version_order(self):
        from prepare import debian_version
        self.assertEqual(debian_version("0.1.0-preview.3"), "0.1.0~preview.3")
        with self.assertRaises(ValueError):
            debian_version("v0.1.0")
        subprocess.run(["dpkg", "--compare-versions", "0.1.0~preview.3-1", "lt", "0.1.0-1"], check=True)

    def test_corruption_fails_closed(self):
        validator = Path(__file__).with_name("validate_source.py").resolve()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("packaging", "vendor/example-1.0.0", "crates/xynoos-cli", "debian"):
                (root / name).mkdir(parents=True)
            lock = b'[[package]]\nname="example"\nversion="1.0.0"\nsource="registry+example"\nchecksum="abc"\n'
            (root / "Cargo.lock").write_bytes(lock)
            (root / "crates/xynoos-cli/Cargo.toml").write_text('[package]\nversion="0.1.0-preview.3"\n')
            (root / "debian/changelog").write_text('xynoos-hypercore (0.1.0~preview.3-1) unstable; urgency=medium\n')
            source = {"commit": "a" * 40, "target": "x86_64-unknown-linux-gnu", "upstream_version": "0.1.0-preview.3",
                      "debian_upstream_version": "0.1.0~preview.3", "cargo_lock_sha256": hashlib.sha256(lock).hexdigest()}
            (root / "packaging/source.json").write_text(json.dumps(source))
            (root / "packaging/dependencies.json").write_text(json.dumps([{"name": "example", "version": "1.0.0", "checksum": "abc"}]))
            (root / "packaging/THIRD_PARTY_NOTICES").write_text("example license\n")
            vendor = root / "vendor/example-1.0.0"
            (vendor / "lib.rs").write_bytes(b"original")
            (vendor / ".cargo-checksum.json").write_text(json.dumps({"package": "abc", "files": {"lib.rs": hashlib.sha256(b"original").hexdigest()}}))

            def validate():
                return subprocess.run(["python3", str(validator)], cwd=root, capture_output=True).returncode

            self.assertEqual(validate(), 0)
            (vendor / "lib.rs").write_bytes(b"modified")
            self.assertNotEqual(validate(), 0)
            (vendor / "lib.rs").write_bytes(b"original")
            (root / "Cargo.lock").write_bytes(lock + b"\n")
            self.assertNotEqual(validate(), 0)
            (root / "Cargo.lock").write_bytes(lock)
            source["commit"] = "unknown"
            (root / "packaging/source.json").write_text(json.dumps(source))
            self.assertNotEqual(validate(), 0)


if __name__ == "__main__":
    unittest.main()
