#!/usr/bin/env python3
"""HTTP read-back, fingerprint, signatures, indexes and all historical package checksums."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from urllib.parse import quote
from archive import require, verify, fingerprint
from pages import STUDIO_DIST

BASE = 'https://samuelindra123.github.io/xynoos-hypercore/'


def fetch(relative, expected):
    url = BASE + quote(relative, safe='/')
    last = None
    attempts = 6
    for attempt in range(1, attempts + 1):
        try:
            with urllib.request.urlopen(url, timeout=30) as response:
                if response.status != 200:
                    raise urllib.error.HTTPError(url, response.status, 'Unexpected HTTP status', response.headers, None)
                return response.read(100 * 1024 * 1024 + 1)
        except Exception as error:
            last = error
            status = getattr(error, 'code', None)
            reason = getattr(error, 'reason', None) or str(error)
            detail = f'HTTP {status}: {reason}' if status is not None else f'{type(error).__name__}: {reason}'
            print(f'Public read-back attempt {attempt}/{attempts} failed; expected {expected}; URL {url}; {detail}',
                  file=sys.stderr)
            if attempt < attempts:
                time.sleep(5)
    status = getattr(last, 'code', None)
    reason = getattr(last, 'reason', None) or str(last)
    detail = f'HTTP {status}: {reason}' if status is not None else f'{type(last).__name__}: {reason}'
    raise RuntimeError(
        f'Public read-back failed after {attempts} attempts; expected resource: {expected}; '
        f'exact URL: {url}; last response/error: {detail}'
    ) from last


def verify_studio(built):
    require((built / 'index.html').is_file(), 'Astro build output missing')
    for original in sorted(built.rglob('*')):
        if original.is_file():
            relative = original.relative_to(built).as_posix()
            # GitHub Pages consumes the hidden sentinel rather than serving it.
            if relative == '.nojekyll':
                require(original.read_bytes() == b'', 'Unexpected .nojekyll contents')
                continue
            homepage = relative == 'index.html'
            route = relative.removesuffix('index.html') if original.name == 'index.html' else relative
            public = fetch(route, 'Studio homepage' if homepage else f'Studio asset {relative}')
            require(public == original.read_bytes(), 'Public Studio asset differs: ' + relative)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-apt', action='store_true')
    parser.add_argument('--apt-only', action='store_true', help='Verify only the published archive')
    args = parser.parse_args()
    if not args.apt_only:
        verify_studio(STUDIO_DIST)
    # Discovery of history must succeed; do not silently skip a previously published archive.
    history = subprocess.check_output(['git', 'ls-remote', '--heads', 'https://github.com/samuelindra123/xynoos-hypercore.git',
                                       'refs/heads/distribution/apt-history']).strip()
    if not history:
        require(not (args.require_apt or args.apt_only), 'APT archive not published')
        print('PASS: public Studio/assets; APT NOT PUBLISHED')
        return
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        def save(relative):
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(fetch('apt/' + relative, f'APT archive resource {relative}'))
            return path
        key = save('xynoos-archive-keyring.gpg')
        require(fingerprint(key) == os.environ.get('XYNOOS_APT_FINGERPRINT', ''), 'Public key fingerprint mismatch')
        for name in ['InRelease', 'Release', 'Release.gpg', 'versions.json']:
            save('dists/preview/' + name)
        for name in ['Packages', 'Packages.gz']:
            relative = 'dists/preview/main/binary-amd64/' + name
            path = save(relative)
            save('dists/preview/main/binary-amd64/by-hash/SHA256/' + hashlib.sha256(path.read_bytes()).hexdigest())
        # Verify signatures before trusting the ledger's filenames; constrain paths too.
        subprocess.run(['gpgv', '--keyring', str(root / 'xynoos-archive-keyring.gpg'), str(root / 'dists/preview/InRelease')], check=True)
        ledger = json.loads((root / 'dists/preview/versions.json').read_text())
        for record in ledger['versions'].values():
            relative = record['filename']
            require(relative.startswith('pool/main/x/xynoos-hypercore/') and '..' not in Path(relative).parts, 'Unsafe ledger path')
            save(relative)
        verify(root, os.environ.get('XYNOOS_APT_FINGERPRINT', ''))
    scope = 'public archive' if args.apt_only else 'public Studio/assets and archive'
    print(f'PASS: {scope}, key fingerprint, InRelease/Release.gpg, SHA256 indexes and all retained packages')


if __name__ == '__main__':
    main()
