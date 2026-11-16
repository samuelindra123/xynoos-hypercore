"""Adversarial distribution tests. Test signing key is ephemeral, never production."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("manager", Path(__file__).with_name("manager.py"))
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


class DistributionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.keys = tempfile.TemporaryDirectory()
        cls.private = Path(cls.keys.name) / "private"
        subprocess.run(["openssl", "genpkey", "-algorithm", "ED25519", "-out", str(cls.private)], check=True, capture_output=True)
        cls.public = subprocess.check_output(["openssl", "pkey", "-in", str(cls.private), "-pubout"])
        binary = Path(os.environ.get("XYNOOS_TEST_BINARY", "target/debug/xynoos"))
        cls.binary = binary.read_bytes()
        cls.identity = json.loads(subprocess.check_output([str(binary.resolve()), "--version", "--json"]))

    @classmethod
    def tearDownClass(cls):
        cls.keys.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)
        self.env = patch.dict(os.environ, {"HOME": str(self.home), "XDG_DATA_HOME": str(self.home / "data"), "XDG_STATE_HOME": str(self.home / "state")})
        self.env.start()
        self.key = patch.object(m, "PUBLIC_KEY", self.public)
        self.key.start()
        self.inst = m.Installation()
        m.JSON_OUTPUT = True

    def tearDown(self):
        self.key.stop()
        self.env.stop()
        self.temp.cleanup()

    def package(self, tag="v0.1.0-preview.1", data=None):
        data = self.binary if data is None else data
        manifest = {"schema": 1, "repository": m.REPO, "version": tag, "channel": "preview", "source_commit": self.identity["source_commit"],
                    "target": m.TARGET, "binary": {"name": "xynoos-" + m.TARGET, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()},
                    "provenance": {"workflow": ".github/workflows/release.yml", "run_id": "1", "source": "https://github.com/" + m.REPO + "/commit/" + self.identity["source_commit"]}}
        raw = json.dumps(manifest).encode()
        source = self.home / "sign-input"
        source.write_bytes(raw)
        sig = subprocess.check_output(["openssl", "pkeyutl", "-sign", "-rawin", "-inkey", str(self.private), "-in", str(source)])
        return raw, sig, data

    def install(self, tag="v0.1.0-preview.1", package=None, real_health=False):
        raw, sig, data = package or self.package(tag)
        def fetch(url, limit):
            if url.endswith("manifest.json.sig"): return sig
            if url.endswith("manifest.json"): return raw
            return data
        with patch.object(m, "discover", return_value=tag), patch.object(m, "download", side_effect=fetch), \
                (contextlib.nullcontext() if real_health else patch.object(m, "health_binary")):
            with self.inst.locked():
                return self.inst.install("preview", tag)

    def test_real_signed_install_health_and_entry(self):
        self.install(real_health=True)
        self.assertEqual(self.inst.current(), "v0.1.0-preview.1")
        output = json.loads(subprocess.check_output([str(self.inst.entry), "--version", "--json"]))
        self.assertEqual(output["product"], "Xynoos HyperCore")
        with self.inst.locked(): self.inst.verify_local(self.inst.current())

    def test_upgrade_offline_rollback_and_preserving_uninstall(self):
        self.install()
        self.install("v0.1.0-preview.2")
        with patch.object(m, "download", side_effect=AssertionError("network used")), patch.object(m, "health_binary"):
            with self.inst.locked(): self.inst.rollback()
        self.assertEqual(self.inst.current(), "v0.1.0-preview.1")
        evidence = self.inst.state / "evidence.json"
        evidence.write_text("operator evidence")
        with self.inst.locked(): self.inst.uninstall()
        self.assertFalse(self.inst.entry.exists())
        self.assertEqual(evidence.read_text(), "operator evidence")
        self.assertTrue((self.inst.root / "manager.lock").exists())
        self.install()

    def test_corrupted_signature_never_executes(self):
        raw, sig, data = self.package()
        with patch.object(m, "health_binary") as run:
            with self.assertRaises(m.Refusal): self.install(package=(raw, bytes(64), data), real_health=True)
            run.assert_not_called()
        self.assertIsNone(self.inst.current())

    def test_corrupted_binary_never_executes(self):
        raw, sig, data = self.package()
        with patch.object(m, "health_binary") as run:
            with self.assertRaises(m.Refusal): self.install(package=(raw, sig, data[:-1]), real_health=True)
            run.assert_not_called()

    def test_wrong_architecture_never_executes(self):
        data = bytearray(self.binary); data[18:20] = b"\xb7\x00"
        with self.assertRaises(m.Refusal): self.install(package=self.package(data=bytes(data)))

    def test_signed_wrong_metadata_refused(self):
        raw, sig, data = self.package("v0.1.0-preview.2")
        with self.assertRaises(m.Refusal): self.install(package=(raw, sig, data))

    def test_update_failure_preserves_working_version(self):
        self.install()
        raw, sig, data = self.package("v0.1.0-preview.2")
        with self.assertRaises(m.Refusal): self.install("v0.1.0-preview.2", (raw, sig, data[:100]))
        self.assertEqual(self.inst.current(), "v0.1.0-preview.1")
        self.assertIsNone(self.inst.history()["previous"])

    def test_interrupted_download_preserves_working_version(self):
        self.install()
        with patch.object(m, "discover", return_value="v0.1.0-preview.2"), patch.object(m, "download", side_effect=OSError("connection interrupted")):
            with self.inst.locked():
                with self.assertRaises(OSError): self.inst.install("preview")
        self.assertEqual(self.inst.current(), "v0.1.0-preview.1")

    def test_downgrade_refused(self):
        self.install("v0.1.0-preview.2")
        with self.assertRaises(m.Refusal): self.install()
        self.assertEqual(self.inst.current(), "v0.1.0-preview.2")

    def test_current_corruption_refuses_update_and_rollback(self):
        self.install()
        (self.inst.root / "versions/v0.1.0-preview.1/xynoos").write_bytes(b"corrupt")
        with self.assertRaises(m.Refusal): self.install("v0.1.0-preview.2")
        with self.inst.locked():
            with self.assertRaises(m.Refusal): self.inst.rollback()

    def test_no_previous_refuses_rollback(self):
        self.install()
        with self.inst.locked():
            with self.assertRaises(m.Refusal): self.inst.rollback()

    def test_lock_refuses_concurrent_subprocess(self):
        with self.inst.locked():
            result = subprocess.run(["python3", "-I", str(Path(m.__file__).resolve()), "health", "--json"], capture_output=True, env=os.environ)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"Another Xynoos", result.stderr)

    def test_symlink_parent_refused(self):
        (self.home / "elsewhere").mkdir()
        (self.home / "data").symlink_to(self.home / "elsewhere", target_is_directory=True)
        with self.assertRaises(m.Refusal):
            with self.inst.locked(): pass

    def test_writable_parent_refused(self):
        (self.home / "data").mkdir(mode=0o777)
        (self.home / "data").chmod(0o777)
        with self.assertRaises(m.Refusal):
            with self.inst.locked(): pass

    def test_entry_collision_preserved(self):
        self.inst.bin_dir.mkdir(parents=True)
        self.inst.entry.write_text("unrelated executable")
        with self.assertRaises(m.Refusal): self.install()
        self.assertEqual(self.inst.entry.read_text(), "unrelated executable")

    def test_entry_symlink_collision_preserved(self):
        self.inst.bin_dir.mkdir(parents=True)
        self.inst.entry.symlink_to("/bin/true")
        with self.assertRaises(m.Refusal): self.install()
        self.assertEqual(os.readlink(self.inst.entry), "/bin/true")

    def test_manifest_symlink_and_hardlink_refused(self):
        self.install()
        p = self.inst.root / "versions/v0.1.0-preview.1/manifest.json"
        original = p.read_bytes(); p.unlink()
        outside = self.home / "outside"; outside.write_bytes(original)
        p.symlink_to(outside)
        with self.inst.locked():
            with self.assertRaises(m.Refusal): self.inst.verify_local("v0.1.0-preview.1")
        p.unlink(); os.link(outside, p)
        with self.inst.locked():
            with self.assertRaises(m.Refusal): self.inst.verify_local("v0.1.0-preview.1")

    def test_uninstall_unknown_file_preserved(self):
        self.install()
        p = self.inst.root / "operator.txt"; p.write_text("preserve")
        with self.inst.locked():
            with self.assertRaises(m.Refusal): self.inst.uninstall()
        self.assertTrue(self.inst.entry.exists())
        self.assertEqual(p.read_text(), "preserve")

    def test_health_failure_restores_previous(self):
        self.install()
        with patch.object(m, "health_binary", side_effect=m.Refusal("injected health failure")):
            with self.inst.locked():
                with self.assertRaises(m.Refusal): self.inst.activate("v0.1.0-preview.1", "v0.1.0-preview.1")
        self.assertEqual(self.inst.current(), "v0.1.0-preview.1")
        self.assertFalse((self.inst.root / "transaction.json").exists())

    def test_crash_boundaries_recover_prior_selection(self):
        self.install(); self.install("v0.1.0-preview.2")
        for point in ("journal", "switch", "history"):
            with self.subTest(point=point):
                with self.inst.locked():
                    self.inst.switch("v0.1.0-preview.1")
                    m.atomic_json(self.inst.root / "history.json", {"previous": None})
                    m.atomic_json(self.inst.root / "transaction.json", {"before": "v0.1.0-preview.1", "previous": None})
                    if point in ("switch", "history"): self.inst.switch("v0.1.0-preview.2")
                    if point == "history": m.atomic_json(self.inst.root / "history.json", {"previous": "v0.1.0-preview.1"})
                # Next process takes lock and rolls back any uncommitted transaction.
                with self.inst.locked(): pass
                self.assertEqual(self.inst.current(), "v0.1.0-preview.1")
                self.assertIsNone(self.inst.history()["previous"])

    def test_partial_atomic_write_leaves_old_state(self):
        with self.inst.locked():
            p = self.inst.root / "history.json"
            m.atomic_json(p, {"previous": None})
            with patch.object(m.os, "replace", side_effect=OSError("power failure before rename")):
                with self.assertRaises(OSError): m.atomic_json(p, {"previous": "v0.1.0-preview.1"})
            self.assertEqual(m.strict_json(p.read_bytes()), {"previous": None})

    def test_crashed_staging_is_cleaned_under_lock(self):
        self.install()
        p = self.inst.root / "versions/.stage-abc12345"
        p.mkdir(mode=0o700)
        (p / "xynoos").write_bytes(b"partial write")
        q = self.inst.root / ".write-abc12345"
        q.write_bytes(b'{"partial":')
        with self.inst.locked(): pass
        self.assertFalse(p.exists())
        self.assertFalse(q.exists())
        self.assertEqual(self.inst.current(), "v0.1.0-preview.1")

    def test_discovery_semver_prerelease_and_platform(self):
        def release(tag, **kwargs):
            return dict(tag_name=tag, prerelease="preview" in tag, draft=False,
                        assets=[{"name": n} for n in ("manifest.json", "manifest.json.sig", "xynoos-" + m.TARGET)], **kwargs)
        entries = [release("v0.1.0-preview.9"), release("v0.1.0-preview.10"), release("v0.1.0"),
                   dict(release("v0.2.0-preview.1"), draft=True), dict(release("v0.3.0-preview.1"), assets=[])]
        with patch.object(m, "download", return_value=json.dumps(entries).encode()):
            self.assertEqual(m.discover("preview"), "v0.1.0-preview.10")
            self.assertEqual(m.discover("stable"), "v0.1.0")
            self.assertEqual(m.discover("preview", "v0.1.0-preview.9"), "v0.1.0-preview.9")
            with self.assertRaises(m.Refusal): m.discover("preview", "v9.0.0-preview.1")
        with patch.object(m, "download", return_value=b"[]"):
            with self.assertRaises(m.Refusal): m.discover("preview")
            with self.assertRaises(m.Refusal): m.discover("stable")

    def test_metadata_and_redirect_rejections(self):
        for version in ("../../bin/sh", "v01.1.0", "v1.0.0-preview.0", "v1.2.3;id", "v1.0.0-rc.1"):
            with self.assertRaises(m.Refusal): m.version_key(version)
        for value in (b'{"a":1,"a":2}', b'{"x":NaN}', b'[]garbage'):
            with self.assertRaises(m.Refusal): m.strict_json(value)
        handler = m.Redirects()
        req = m.urllib.request.Request("https://github.com/")
        for url in ("http://github.com/x", "https://evil.example/x", "https://github.com.evil.example/x", "https://user@github.com/x", "https://github.com:444/x"):
            with self.assertRaises(m.Refusal): handler.redirect_request(req, None, 302, "", {}, url)

    def test_network_size_and_partial_response(self):
        class Response(io.BytesIO):
            status = 200
            def __init__(self, data, size): super().__init__(data); self.headers = {"Content-Length": size}
        for response, limit in ((Response(b"abc", "4"), 10), (Response(b"abc", "100"), 10), (Response(b"a" * 11, "11"), 10)):
            opener = unittest.mock.Mock(); opener.open.return_value = response
            with patch.object(m.urllib.request, "build_opener", return_value=opener):
                with self.assertRaises(m.Refusal): m.download("https://github.com/x", limit)


if __name__ == "__main__":
    unittest.main()
