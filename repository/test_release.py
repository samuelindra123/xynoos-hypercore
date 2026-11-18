"""Exercise publication and retry without GitHub writes."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import release


class ReleasePublicationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.commit = 'a' * 40
        self.version = '0.1.0~preview.4-1'
        self.tag = 'debian/0.1.0-preview.4-1'
        self.record = dict(id=123, tag_name=self.tag, target_commitish=self.commit,
                           draft=True, prerelease=True, immutable=False, html_url='https://example.org')
        self.ref = None
        self.writes = []

    def api(self, *args):
        if args[2] == '--paginate':
            return b'123\n'
        if args[2].endswith('/releases/123'):
            return json.dumps(self.record)
        if args[2].endswith('/matching-refs/tags/debian%2F0.1.0-preview.4-1'):
            return json.dumps([self.ref] if self.ref else [])
        if args[2:4] == ('-X', 'POST'):
            self.writes.append('tag')
            self.assertEqual(args[-3:], ('ref=refs/tags/' + self.tag, '-f', 'sha=' + self.commit))
            self.ref = dict(ref='refs/tags/' + self.tag, object=dict(type='commit', sha=self.commit))
            return '{}'
        if args[2:4] == ('-X', 'PATCH'):
            self.writes.append('publish')
            self.record.update(draft=False, immutable=True)
            return '{}'
        raise AssertionError(args)

    def recover(self):
        with patch.dict(os.environ, {'XYNOOS_APT_IMMUTABLE_RELEASES': 'enabled'}), \
                patch.object(release, 'run', side_effect=self.api), \
                patch.object(release, 'download_and_verify') as verify, \
                patch.object(release.subprocess, 'run') as create:
            result = release.publish_release(self.directory, self.commit, self.version, {})
        create.assert_not_called()
        self.assertEqual(verify.call_count, 2)
        return result

    def test_slash_tag_draft_recovery_and_retry(self):
        self.assertEqual(self.recover(), self.record['html_url'])
        self.assertEqual(self.recover(), self.record['html_url'])
        self.assertEqual(self.writes, ['tag', 'publish'])

    def test_wrong_existing_tag_blocks_publication(self):
        self.ref = dict(ref='refs/tags/' + self.tag, object=dict(type='commit', sha='b' * 40))
        with self.assertRaisesRegex(ValueError, 'different source'):
            self.recover()
        self.assertEqual(self.writes, [])

    def test_bad_assets_prevent_tag_creation(self):
        with patch.object(release, 'download_and_verify', side_effect=ValueError('bad assets')), \
                patch.object(release, 'run', side_effect=self.api):
            with self.assertRaisesRegex(ValueError, 'bad assets'):
                release.finish_release(self.directory, self.commit, self.tag, self.record)
        self.assertEqual(self.writes, [])

    def test_requires_immutability_configuration(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(release, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'must verify GitHub immutable releases'):
                release.publish_release(self.directory, self.commit, self.version, {})
        run.assert_not_called()

    def test_restores_original_attested_debian_asset_names(self):
        (self.directory / 'SHA256SUMS').write_text('hash  xynoos_0.1.0~preview.deb\n')
        (self.directory / 'xynoos_0.1.0.preview.deb').write_bytes(b'package')
        release.restore_subject_names(self.directory)
        self.assertEqual((self.directory / 'xynoos_0.1.0~preview.deb').read_bytes(), b'package')

    def test_rejects_ambiguous_normalized_names(self):
        (self.directory / 'SHA256SUMS').write_text('hash  a.b\nhash  a~b\n')
        (self.directory / 'a.b').write_bytes(b'package')
        with self.assertRaisesRegex(ValueError, 'Ambiguous'):
            release.restore_subject_names(self.directory)
