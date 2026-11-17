"""Exercise the real GPG/APT chain with a disposable TEST identity, never production keys."""
import getpass
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import functools
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import patch

import archive


class ArchiveTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='xynoos-archive-test-')
        cls.base = Path(cls.temp.name)
        cls.base.chmod(0o755)
        cls.gpg = cls.base / 'gnupg'
        cls.gpg.mkdir(mode=0o700)
        cls.environment = patch.dict(os.environ, GNUPGHOME=str(cls.gpg), XYNOOS_APT_PASSPHRASE='')
        cls.environment.start()
        archive.run('gpg', '--batch', '--passphrase', '', '--quick-generate-key',
                    'DISPOSABLE TEST ONLY <test@example.invalid>', 'ed25519', 'sign', '1d', stderr=subprocess.PIPE)
        listing = archive.run('gpg', '--batch', '--with-colons', '--list-keys').decode()
        cls.fpr = next(line.split(':')[9] for line in listing.splitlines() if line.startswith('fpr:'))
        cls.debs = {}
        for version, architecture in [('0.1.0~preview.1-1', 'amd64'), ('0.1.0~preview.2-1', 'amd64'),
                                      ('0.1.0~preview.3-1', 'arm64')]:
            tree = cls.base / (version + architecture)
            (tree / 'DEBIAN').mkdir(parents=True)
            (tree / 'DEBIAN/control').write_text(f'Package: xynoos-hypercore\nVersion: {version}\nArchitecture: {architecture}\nMaintainer: Disposable Test <test@example.invalid>\nDescription: archive test fixture only\n')
            (tree / 'usr/share/xynoos-test').mkdir(parents=True)
            (tree / 'usr/share/xynoos-test/fixture').write_text(version)
            deb = cls.base / (version + architecture + '.deb')
            archive.run('dpkg-deb', '--build', '--root-owner-group', str(tree), str(deb))
            cls.debs[architecture + version] = deb

    @classmethod
    def tearDownClass(cls):
        subprocess.run(['gpgconf', '--kill', 'gpg-agent'], check=True)
        cls.environment.stop()
        cls.temp.cleanup()

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(dir=self.base)
        self.root = Path(self.directory.name) / 'apt'
        self.add('0.1.0~preview.1-1')

    def tearDown(self):
        self.directory.cleanup()

    def add(self, version, arch='amd64'):
        archive.publish(self.root, self.debs[arch + version], 'a' * 40, self.fpr)

    def test_history_signatures_and_by_hash(self):
        before = {str(p.relative_to(self.root)): p.read_bytes() for p in self.root.rglob('*.deb')}
        self.add('0.1.0~preview.2-1')
        ledger = archive.verify(self.root, self.fpr)
        self.assertEqual(len(ledger['versions']), 2)
        for relative, value in before.items():
            self.assertEqual((self.root / relative).read_bytes(), value)
        archive.publish(self.root, None, None, self.fpr)
        self.assertEqual(archive.verify(self.root, self.fpr), ledger)

    def test_duplicate_version_and_wrong_architecture_refused(self):
        for version, arch in [('0.1.0~preview.1-1', 'amd64'), ('0.1.0~preview.3-1', 'arm64')]:
            with self.assertRaises(ValueError):
                self.add(version, arch)
        self.assertEqual(len(archive.verify(self.root, self.fpr)['versions']), 1)

    def test_modified_package_refused(self):
        deb = next(self.root.rglob('*.deb'))
        deb.write_bytes(deb.read_bytes() + b'tampered')
        with self.assertRaisesRegex(ValueError, 'Package checksum mismatch'):
            self.add('0.1.0~preview.2-1')

    def test_modified_metadata_refused(self):
        index = self.root / 'dists/preview/main/binary-amd64/Packages'
        index.write_text(index.read_text() + 'tampered')
        with self.assertRaisesRegex(ValueError, 'Release checksum mismatch'):
            archive.verify(self.root, self.fpr)

    def test_invalid_signature_and_wrong_trust_anchor_refused(self):
        with self.assertRaisesRegex(ValueError, 'fingerprint mismatch'):
            archive.verify(self.root, 'F' * 40)
        release = self.root / 'dists/preview/InRelease'
        release.write_bytes(release.read_bytes().replace(b'Experimental', b'Compromised'))
        with self.assertRaises(subprocess.CalledProcessError):
            archive.verify(self.root, self.fpr)

    def test_missing_key_fails_closed(self):
        empty = self.base / 'empty-gnupg'
        empty.mkdir(mode=0o700, exist_ok=True)
        with patch.dict(os.environ, GNUPGHOME=str(empty)):
            with self.assertRaises(ValueError):
                archive.publish(self.base / 'unsigned', self.debs['amd640.1.0~preview.1-1'], 'a' * 40, self.fpr)
        self.assertFalse((self.base / 'unsigned/dists/preview/InRelease').exists())

    def test_apt_retrieves_signed_index_and_both_versions(self):
        self.add('0.1.0~preview.2-1')
        handler = functools.partial(SimpleHTTPRequestHandler, directory=str(self.root.parent))
        server = ThreadingHTTPServer(('127.0.0.1', 0), handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            work = Path(self.directory.name) / 'client'
            work.mkdir()
            for sub in ['lists/partial', 'cache/archives/partial', 'empty']:
                (work / sub).mkdir(parents=True)
            source = work / 'sources.list'
            source.write_text(f'deb [arch=amd64 signed-by={self.root}/xynoos-archive-keyring.gpg] http://127.0.0.1:{server.server_port}/apt preview main\n')
            opts = ['-o', f'Dir::Etc::sourcelist={source}', '-o', f'Dir::Etc::sourceparts={work}/empty',
                    '-o', f'Dir::State::lists={work}/lists', '-o', f'Dir::Cache={work}/cache',
                    '-o', f'APT::Sandbox::User={getpass.getuser()}', '-o', 'APT::Get::List-Cleanup=0']
            archive.run('apt-get', *opts, 'update', stderr=subprocess.STDOUT)
            policy = archive.run('apt-cache', *opts, 'policy', 'xynoos-hypercore').decode()
            self.assertIn('0.1.0~preview.1-1', policy)
            self.assertIn('0.1.0~preview.2-1', policy)
            for version in ['0.1.0~preview.1-1', '0.1.0~preview.2-1']:
                archive.run('apt-get', *opts, 'download', f'xynoos-hypercore={version}', cwd=work, stderr=subprocess.STDOUT)
            self.assertEqual(len(list(work.glob('*.deb'))), 2)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == '__main__':
    unittest.main()
