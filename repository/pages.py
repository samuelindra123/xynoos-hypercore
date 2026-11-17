#!/usr/bin/env python3
"""Assemble Studio and the signed archive into the ONLY Pages artifact."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import urllib.error
import urllib.request
from archive import require, verify, run

REPO = 'samuelindra123/xynoos-hypercore'
BASE = 'https://samuelindra123.github.io/xynoos-hypercore/'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    require(not args.output.exists(), 'New output directory required')
    with tempfile.TemporaryDirectory() as temporary:
        state = Path(temporary)
        remote = f'https://github.com/{REPO}.git'
        refs = run('git', 'ls-remote', '--heads', remote, 'refs/heads/distribution/apt-history').decode().strip()
        if refs:
            run('git', 'clone', '--depth=1', '--branch', 'distribution/apt-history', remote, str(state / 'history'))
            archive = state / 'history/apt'
            verify(archive, os.environ.get('XYNOOS_APT_FINGERPRINT', ''))
        else:
            # Missing remote history never authorizes removal of a public archive.
            try:
                with urllib.request.urlopen(BASE + 'apt/dists/preview/InRelease', timeout=30):
                    raise ValueError('Public archive exists but history branch is missing')
            except urllib.error.HTTPError as error:
                require(error.code == 404, 'Public archive absence not verified')
            archive = None
        shutil.copytree('website', args.output)
        if archive is not None:
            shutil.copytree(archive, args.output / 'apt')
        (args.output / '.nojekyll').touch()
        for original in Path('website').rglob('*'):
            if original.is_file():
                require((args.output / original.relative_to('website')).read_bytes() == original.read_bytes(), 'Studio preservation failed')
        print('Studio bytes preserved; archive ' + ('verified' if archive else 'not yet configured'))


if __name__ == '__main__':
    main()
