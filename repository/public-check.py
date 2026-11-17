#!/usr/bin/env python3
"""HTTP read-back, fingerprint, signatures, indexes and all historical package checksums."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile
import time
import urllib.request
from urllib.parse import quote
from archive import require, verify

BASE = 'https://samuelindra123.github.io/xynoos-hypercore/'


def fetch(relative):
    url = BASE + quote(relative, safe='/')
    last = None
    for attempt in range(6):
        try:
            with urllib.request.urlopen(url, timeout=30) as response:
                require(response.status == 200, 'HTTP 200 required: ' + url)
                return response.read(100 * 1024 * 1024 + 1)
        except Exception as error:
            last = error
            time.sleep(5)
    raise last


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-apt', action='store_true')
    args = parser.parse_args()
    for original in Path('website').rglob('*'):
        if original.is_file():
            relative = str(original.relative_to('website'))
            public = fetch('' if relative == 'index.html' else relative)
            require(public == original.read_bytes(), 'Public Studio asset differs: ' + relative)
    # Discovery of history must succeed; do not silently skip a previously published archive.
    import subprocess
    history = subprocess.check_output(['git', 'ls-remote', '--heads', 'https://github.com/samuelindra123/xynoos-hypercore.git',
                                       'refs/heads/distribution/apt-history']).strip()
    if not history:
        require(not args.require_apt, 'APT archive not published')
        print('PASS: public Studio/assets; APT NOT PUBLISHED')
        return
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        def save(relative):
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(fetch('apt/' + relative))
            return path
        save('xynoos-archive-keyring.gpg')
        for name in ['InRelease', 'Release', 'Release.gpg', 'versions.json']:
            save('dists/preview/' + name)
        for name in ['Packages', 'Packages.gz']:
            relative = 'dists/preview/main/binary-amd64/' + name
            path = save(relative)
            save('dists/preview/main/binary-amd64/by-hash/SHA256/' + hashlib.sha256(path.read_bytes()).hexdigest())
        # Verify signatures before trusting the ledger's filenames; constrain paths too.
        import subprocess
        subprocess.run(['gpgv', '--keyring', str(root / 'xynoos-archive-keyring.gpg'), str(root / 'dists/preview/InRelease')], check=True)
        ledger = json.loads((root / 'dists/preview/versions.json').read_text())
        for record in ledger['versions'].values():
            relative = record['filename']
            require(relative.startswith('pool/main/x/xynoos-hypercore/') and '..' not in Path(relative).parts, 'Unsafe ledger path')
            save(relative)
        verify(root, os.environ.get('XYNOOS_APT_FINGERPRINT', ''))
    print('PASS: public Studio/assets, key fingerprint, InRelease/Release.gpg, SHA256 indexes and all retained packages')


if __name__ == '__main__':
    main()
