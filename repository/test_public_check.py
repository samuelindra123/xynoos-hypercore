"""Regression tests for actionable public Pages read-back diagnostics."""
from contextlib import redirect_stderr
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import urllib.error
import unittest
from unittest.mock import call, patch


sys.path.insert(0, str(Path(__file__).parent))
SPEC = importlib.util.spec_from_file_location('public_check', Path(__file__).with_name('public-check.py'))
public_check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(public_check)


class PublicFetchTests(unittest.TestCase):
    def test_http_failure_reports_expected_resource_url_status_and_retry_context(self):
        expected_url = public_check.BASE + 'apt/dists/preview/InRelease'

        def missing(*args, **kwargs):
            raise urllib.error.HTTPError(expected_url, 404, 'Not Found', {}, None)

        diagnostics = io.StringIO()
        with patch.object(public_check.urllib.request, 'urlopen', side_effect=missing) as urlopen, \
             patch.object(public_check.time, 'sleep') as sleep, \
             redirect_stderr(diagnostics):
            with self.assertRaisesRegex(RuntimeError, 'Public read-back failed') as raised:
                public_check.fetch('apt/dists/preview/InRelease', 'APT InRelease metadata')

        message = str(raised.exception)
        self.assertIn('expected resource: APT InRelease metadata', message)
        self.assertIn('exact URL: ' + expected_url, message)
        self.assertIn('HTTP 404: Not Found', message)
        self.assertEqual(urlopen.call_count, 6)
        self.assertEqual(sleep.call_args_list, [call(5)] * 5)
        self.assertIn('attempt 6/6', diagnostics.getvalue())
        self.assertIn(expected_url, diagnostics.getvalue())

    def test_transient_http_failure_retries_then_returns_bytes(self):
        class Response:
            status = 200
            headers = {}

            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

            def read(self, limit):
                return b'published bytes'

        transient_url = public_check.BASE + 'studio.js'
        transient = urllib.error.HTTPError(transient_url, 503, 'Service Unavailable', {}, None)
        diagnostics = io.StringIO()
        with patch.object(public_check.urllib.request, 'urlopen', side_effect=[transient, Response()]) as urlopen, \
             patch.object(public_check.time, 'sleep') as sleep, \
             redirect_stderr(diagnostics):
            contents = public_check.fetch('studio.js', 'Studio asset studio.js')

        self.assertEqual(contents, b'published bytes')
        self.assertEqual(urlopen.call_count, 2)
        sleep.assert_called_once_with(5)
        self.assertIn('HTTP 503: Service Unavailable', diagnostics.getvalue())
        self.assertIn(transient_url, diagnostics.getvalue())


class StudioReadBackTests(unittest.TestCase):
    def test_readback_uses_project_relative_routes_and_compares_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            built = Path(directory)
            files = {
                '.nojekyll': b'',
                '_astro/app.js': b'console.log("Studio");',
                'guide/index.html': b'<title>Studio guide</title>',
                'index.html': b'<title>Studio</title>',
            }
            for relative, data in files.items():
                path = built / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            published = {
                '_astro/app.js': files['_astro/app.js'],
                'guide/': files['guide/index.html'],
                '': files['index.html'],
            }
            with patch.object(public_check, 'fetch', side_effect=lambda route, _: published[route]) as fetch:
                public_check.verify_studio(built)
                self.assertEqual([args.args[0] for args in fetch.call_args_list], list(published))
                published['_astro/app.js'] = b'unexpected bytes'
                with self.assertRaisesRegex(ValueError, 'Public Studio asset differs: _astro/app.js'):
                    public_check.verify_studio(built)


if __name__ == '__main__':
    unittest.main()
