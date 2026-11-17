"""The protected publisher accepts a delegated signer and rejects primary-key exposure."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from check_signing_key import inspect


class SigningKeyTests(unittest.TestCase):
    def keyring(self, home, *args, input=None):
        env = dict(os.environ, GNUPGHOME=str(home))
        return subprocess.run(["gpg", "--batch", *args], env=env, input=input,
                              text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              check=True).stdout

    def make_key(self, home):
        self.keyring(home, "--passphrase", "", "--quick-generate-key",
                     "Xynoos TEST ONLY <test-key@example.invalid>", "ed25519", "cert", "1y")
        listing = self.keyring(home, "--with-colons", "--list-keys")
        fingerprint = next(line.split(":")[9] for line in listing.splitlines()
                           if line.startswith("fpr:"))
        self.keyring(home, "--passphrase", "", "--quick-add-key", fingerprint,
                     "ed25519", "sign", "1y")
        return fingerprint

    def import_material(self, home, material):
        subprocess.run(["gpg", "--batch", "--import"],
                       env=dict(os.environ, GNUPGHOME=str(home)), input=material,
                       text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                       check=True)

    def test_delegated_expiring_subkey_is_accepted(self):
        with tempfile.TemporaryDirectory(prefix="xynoos-signing-test-") as temp:
            source, delegated = Path(temp) / "source", Path(temp) / "delegated"
            source.mkdir(mode=0o700)
            delegated.mkdir(mode=0o700)
            fingerprint = self.make_key(source)
            material = self.keyring(source, "--passphrase", "", "--armor",
                                    "--export-secret-subkeys", fingerprint)
            self.import_material(delegated, material)
            with mock.patch.dict(os.environ, {"GNUPGHOME": str(delegated)}):
                inspect(fingerprint)

    def test_primary_private_key_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="xynoos-signing-test-") as temp:
            source = Path(temp) / "source"
            source.mkdir(mode=0o700)
            fingerprint = self.make_key(source)
            with mock.patch.dict(os.environ, {"GNUPGHOME": str(source)}):
                with self.assertRaisesRegex(SystemExit, "Primary private key is present"):
                    inspect(fingerprint)

    def test_wrong_fingerprint_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="xynoos-signing-test-") as temp:
            source = Path(temp) / "source"
            source.mkdir(mode=0o700)
            fingerprint = self.make_key(source)
            material = self.keyring(source, "--passphrase", "", "--armor",
                                    "--export-secret-subkeys", fingerprint)
            with mock.patch.dict(os.environ, {"GNUPGHOME": str(source)}):
                with self.assertRaisesRegex(SystemExit, "primary fingerprint mismatch"):
                    inspect("A" * 40)


if __name__ == "__main__":
    unittest.main()
