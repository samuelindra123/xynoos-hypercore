"""Tests the immutable release publication boundary without GitHub writes."""
import json
import os
from pathlib import Path
import tempfile
import subprocess
import unittest
from unittest.mock import patch

import release


class ReleasePublicationTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp(prefix='xynoos-release-test-'))
        (self.directory / 'package.deb').write_bytes(b'test package bytes')
        (self.directory / 'SHA256SUMS').write_text('test checksums\n')
        self.commit = 'a' * 40
        self.version = '0.1.0~preview.4-1'
        self.tag = 'debian/0.1.0-preview.4-1'
        names = sorted(path.name for path in self.directory.iterdir())
        self.draft = {
            'id': 123,
            'tag_name': self.tag,
            'target_commitish': self.commit,
            'draft': True,
            'prerelease': True,
            'assets': [{'name': name} for name in names],
        }
        self.published = {**self.draft, 'draft': False, 'immutable': True,
                          'html_url': 'https://github.com/example/release'}

    def tearDown(self):
        for path in self.directory.iterdir():
            path.unlink()
        self.directory.rmdir()

    def test_requires_repository_immutability_before_creating_release(self):
        with patch.dict(os.environ, {}, clear=True), \
                patch.object(release, 'run') as run, \
                patch.object(release.subprocess, 'run') as create:
            with self.assertRaisesRegex(ValueError, 'must verify GitHub immutable releases'):
                release.publish_release(self.directory, self.commit, self.version,
                                        {'run_url': 'https://github.com/run'})
        run.assert_not_called()
        create.assert_not_called()

    def test_verifies_draft_assets_before_immutable_publication(self):
        calls = []

        def fake_run(*args, **kwargs):
            calls.append(args)
            if args[:2] == ('gh', 'api') and args[2].endswith('/immutable-releases'):
                return b'{"enabled":true}'
            if args[:2] == ('gh', 'api') and '/matching-refs/tags/' in args[2]:
                return b'[]'
            if args[:2] == ('gh', 'api') and '/releases?per_page=100' in args[2]:
                return json.dumps([self.draft]).encode()
            if args[:2] == ('gh', 'api') and '/git/ref/tags/' in args[2]:
                return json.dumps({'object': {'type': 'commit', 'sha': self.commit}}).encode()
            if args[:3] == ('gh', 'api', '-X') and args[3] == 'POST':
                return b'{"ref":"refs/tags/' + self.tag.encode() + b'"}'
            if args[:3] == ('gh', 'api', '-X'):
                return json.dumps(self.published).encode()
            if args[:2] == ('gh', 'api') and args[2].endswith('/releases/123'):
                return json.dumps(self.published).encode()
            if args[:3] == ('gh', 'release', 'download'):
                target = Path(args[args.index('--dir') + 1])
                for source in self.directory.iterdir():
                    (target / source.name).write_bytes(source.read_bytes())
                return b''
            raise AssertionError('Unexpected GitHub command: ' + repr(args))

        with patch.dict(os.environ, {'XYNOOS_APT_IMMUTABLE_RELEASES': 'enabled'}), \
                patch.object(release, 'run', side_effect=fake_run), \
                patch.object(release.subprocess, 'run') as create:
            result = release.publish_release(self.directory, self.commit, self.version,
                                             {'run_url': 'https://github.com/run'})

        self.assertEqual(result, self.published['html_url'])
        create.assert_not_called()
        self.assertEqual(sum(args[:3] == ('gh', 'release', 'download') for args in calls), 2)
        self.assertTrue(any(args[:3] == ('gh', 'api', '-X') and 'draft=false' in args
                            for args in calls))
        self.assertTrue(any(args[:3] == ('gh', 'api', '-X') and args[3] == 'POST'
                            and args[4].endswith('/git/refs') for args in calls))

    def test_slash_tag_is_escaped_and_existing_draft_is_reused(self):
        from urllib.parse import quote
        tag = 'debian/0.1.0-preview.4-1'
        self.assertEqual(quote(tag, safe=''), 'debian%2F0.1.0-preview.4-1')
        self.assertEqual(self.draft['tag_name'], tag)
        self.assertTrue(self.draft['draft'])


if __name__ == '__main__':
    unittest.main()
