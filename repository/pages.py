#!/usr/bin/env python3
"""Assemble Studio and the signed archive into the ONLY Pages artifact."""
import argparse
import os
from pathlib import Path
import shutil
import tempfile
import urllib.error
import urllib.request
from archive import require, verify, run

REPO = 'samuelindra123/xynoos-hypercore'
BASE = 'https://samuelindra123.github.io/xynoos-hypercore/'
STUDIO_DIST = Path(__file__).resolve().parents[1] / 'website/dist'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    require(not args.output.exists(), 'New output directory required')
    built = STUDIO_DIST
    require((built / 'index.html').is_file(), 'Astro build missing; run npm run build --prefix website')
    require(not built.is_symlink() and not any(p.is_symlink() for p in built.rglob('*')),
            'Studio build must not contain symlinks')
    reserved = {'apt', 'legacy'}
    require(not any(p.name.casefold() in reserved for p in built.iterdir()),
            'Website output contains a reserved /apt or /legacy path')
    sentinel = built / '.nojekyll'
    require(not sentinel.exists() or (sentinel.is_file() and sentinel.read_bytes() == b''),
            'Invalid Studio .nojekyll')
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
        shutil.copytree(built, args.output)
        if archive is not None:
            shutil.copytree(archive, args.output / 'apt')
        (args.output / '.nojekyll').touch()
        for original in built.rglob('*'):
            if original.is_file():
                copied = args.output / original.relative_to(built)
                require(copied.read_bytes() == original.read_bytes(), 'Studio preservation failed')
        print('Astro build output assembled; archive ' + ('verified' if archive is not None else 'not yet configured'))


if __name__ == '__main__':
    main()
