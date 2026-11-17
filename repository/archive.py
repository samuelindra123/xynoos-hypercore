#!/usr/bin/env python3
"""Append-only preview archive using Debian apt-ftparchive; no unsigned output accepted."""
import argparse
from datetime import datetime, timedelta, timezone
from email.parser import Parser
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

PACKAGE = 'xynoos-hypercore'
SUITE = 'preview'


def run(*args, **kwargs):
    return subprocess.check_output(args, **kwargs)


def require(value, message):
    if not value:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fingerprint(key):
    listing = run('gpg', '--batch', '--with-colons', '--show-keys', str(key), stderr=subprocess.DEVNULL).decode()
    primaries = [line for line in listing.splitlines() if line.startswith('pub:')]
    require(len(primaries) == 1, 'Exactly one public primary key required')
    return next(line.split(':')[9] for line in listing.splitlines() if line.startswith('fpr:'))


def safe_file(root, relative):
    path = Path(relative)
    require(not path.is_absolute() and '..' not in path.parts, 'Unsafe archive path')
    result = root / path
    require(result.resolve().is_relative_to(root.resolve()), 'Archive path escaped root')
    require(result.is_file() and not result.is_symlink(), 'Missing/nonregular archive file: ' + relative)
    return result


def paragraphs(data):
    return [Parser().parsestr(p) for p in data.strip().split('\n\n') if p.strip()]


def verify(root, expected):
    root = Path(root).resolve()
    require(re.fullmatch(r'[A-F0-9]{40}', expected), 'Full uppercase OpenPGP v4 fingerprint required')
    key = root / 'xynoos-archive-keyring.gpg'
    require(fingerprint(key) == expected, 'Public key fingerprint mismatch')
    suite = root / 'dists' / SUITE
    payload = run('gpgv', '--keyring', str(key), '--output', '-', str(suite / 'InRelease'), stderr=subprocess.PIPE)
    require(payload == (suite / 'Release').read_bytes(), 'InRelease/Release mismatch')
    run('gpgv', '--keyring', str(key), str(suite / 'Release.gpg'), str(suite / 'Release'), stderr=subprocess.PIPE)
    release = Parser().parsestr(payload.decode())
    for field, value in {'Suite': SUITE, 'Codename': SUITE, 'Components': 'main', 'Architectures': 'amd64',
                         'Acquire-By-Hash': 'yes'}.items():
        require(release[field] == value, 'Invalid Release ' + field)
    from email.utils import parsedate_to_datetime
    require(parsedate_to_datetime(release['Valid-Until']) > datetime.now(timezone.utc), 'Expired archive')
    indexed = set()
    for line in release['SHA256'].splitlines():
        if not line.strip():
            continue
        sha, size, relative = line.split()
        path = safe_file(suite, relative)
        require(path.stat().st_size == int(size) and digest(path) == sha, 'Release checksum mismatch: ' + relative)
        indexed.add(relative)
    require({'main/binary-amd64/Packages', 'main/binary-amd64/Packages.gz', 'versions.json'} <= indexed,
            'Release must authenticate both indexes and version ledger')
    packages = suite / 'main/binary-amd64/Packages'
    require(gzip.decompress(packages.with_suffix('.gz').read_bytes()) == packages.read_bytes(), 'Gzip index mismatch')
    ledger = json.loads((suite / 'versions.json').read_text())
    require(ledger['suite'] == SUITE, 'Invalid ledger suite')
    records = paragraphs(packages.read_text())
    actual = {}
    for record in records:
        require(record['Package'] == PACKAGE and record['Architecture'] == 'amd64', 'Wrong package or architecture')
        require(record['Version'] not in actual, 'Duplicate index version')
        path = safe_file(root, record['Filename'])
        require(record['Filename'].startswith('pool/main/x/xynoos-hypercore/'), 'Unexpected package path')
        require(path.stat().st_size == int(record['Size']) and digest(path) == record['SHA256'], 'Package checksum mismatch')
        actual[record['Version']] = {'filename': record['Filename'], 'sha256': record['SHA256']}
    require(set(actual) == set(ledger['versions']), 'Version ledger/index mismatch')
    for version, record in actual.items():
        require(all(ledger['versions'][version][k] == v for k, v in record.items()), 'Ledger digest/path mismatch')
    pool = {str(p.relative_to(root)) for p in root.rglob('*.deb')}
    require(pool == {r['filename'] for r in actual.values()}, 'Unindexed packages in archive')
    for name in ['Packages', 'Packages.gz']:
        path = packages.parent / name
        require((path.parent / 'by-hash/SHA256' / digest(path)).read_bytes() == path.read_bytes(), 'Missing by-hash index')
    require(actual, 'Empty archive')
    return ledger


def publish(root, deb, commit, expected):
    root = Path(root).resolve()
    root.mkdir(parents=True, exist_ok=True)
    require(re.fullmatch(r'[A-F0-9]{40}', expected), 'Full uppercase fingerprint required')
    # Refuse partial, unsigned, tampered or expired historical state. Recovery needs owner review.
    if any(root.iterdir()):
        ledger = verify(root, expected)
    else:
        require(deb is not None, 'Cannot refresh an absent archive')
        ledger = {'suite': SUITE, 'versions': {}}
    if deb is not None:
        deb = Path(deb).resolve()
        require(re.fullmatch(r'[0-9a-f]{40}', commit or ''), 'Exact source commit required')
        control = Parser().parsestr(run('dpkg-deb', '-f', str(deb)).decode())
        require(control['Package'] == PACKAGE and control['Architecture'] == 'amd64', 'Wrong package or architecture')
        version = control['Version']
        require(re.fullmatch(r'\d+\.\d+\.\d+~preview\.\d+-\d+', version), 'Only Debian preview versions accepted')
        require(version not in ledger['versions'], 'Immutable duplicate version')
        for old in ledger['versions']:
            require(subprocess.run(['dpkg', '--compare-versions', version, 'gt', old]).returncode == 0,
                    'New publication must advance preview; rollback is a client operation')
        relative = f'pool/main/x/{PACKAGE}/{PACKAGE}_{version}_amd64.deb'
        target = root / relative
        require(not target.exists(), 'Immutable pool filename')
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(deb, target)
        ledger['versions'][version] = {'filename': relative, 'sha256': digest(target), 'source_commit': commit}
    key = root / 'xynoos-archive-keyring.gpg'
    public = run('gpg', '--batch', '--export-options', 'export-minimal', '--export', expected)
    require(public, 'Signing public key unavailable')
    key.write_bytes(public)
    require(fingerprint(key) == expected, 'Signing identity mismatch')
    suite = root / 'dists' / SUITE
    binary = suite / 'main/binary-amd64'
    binary.mkdir(parents=True, exist_ok=True)
    (suite / 'versions.json').write_text(json.dumps(ledger, sort_keys=True, indent=2) + '\n')
    index = run('apt-ftparchive', '--arch', 'amd64', 'packages', 'pool', cwd=root)
    (binary / 'Packages').write_bytes(index)
    (binary / 'Packages.gz').write_bytes(gzip.compress(index, mtime=0))
    for name in ['Packages', 'Packages.gz']:
        path = binary / name
        for algorithm in ['sha256', 'sha512']:
            target = binary / 'by-hash' / algorithm.upper() / hashlib.new(algorithm, path.read_bytes()).hexdigest()
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists():
                require(target.read_bytes() == path.read_bytes(), 'By-hash collision')
            else:
                shutil.copyfile(path, target)
    from email.utils import format_datetime
    metadata = {'Origin': 'Xynoos', 'Label': 'Xynoos Experimental Preview', 'Suite': SUITE, 'Codename': SUITE,
                'Architectures': 'amd64', 'Components': 'main', 'Acquire-By-Hash': 'yes',
                'Description': 'Experimental Xynoos HyperCore; not a stable channel',
                'Valid-Until': format_datetime(datetime.now(timezone.utc) + timedelta(days=30), usegmt=True)}
    args = ['apt-ftparchive']
    for keyname, value in metadata.items():
        args += ['-o', f'APT::FTPArchive::Release::{keyname}={value}']
    args += ['-o', 'APT::FTPArchive::Release::Patterns::=versions.json', 'release', f'dists/{SUITE}']
    # Do not let stale Release files enter the new digest set.
    for name in ['Release', 'InRelease', 'Release.gpg']:
        (suite / name).unlink(missing_ok=True)
    release = run(*args, cwd=root)
    # Older apt-ftparchive versions omit Valid-Until despite accepting the option.
    if b'Valid-Until:' not in release:
        release = ('Valid-Until: ' + metadata['Valid-Until'] + '\n').encode() + release
    (suite / 'Release').write_bytes(release)
    for name, mode in [('InRelease', '--clearsign'), ('Release.gpg', '--detach-sign')]:
        run('gpg', '--batch', '--yes', '--pinentry-mode', 'loopback', '--passphrase-fd', '0',
            '--local-user', expected, '--digest-algo', 'SHA256', '--output', str(suite / name), mode,
            str(suite / 'Release'), input=os.environ.get('XYNOOS_APT_PASSPHRASE', '').encode(), stderr=subprocess.PIPE)
    verify(root, expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['publish', 'verify'])
    parser.add_argument('root', type=Path)
    parser.add_argument('--deb', type=Path)
    parser.add_argument('--commit')
    parser.add_argument('--fingerprint', default=os.environ.get('XYNOOS_APT_FINGERPRINT', ''))
    args = parser.parse_args()
    if args.operation == 'verify':
        print(json.dumps(verify(args.root, args.fingerprint), indent=2))
    else:
        publish(args.root, args.deb, args.commit, args.fingerprint)
        print('Signed preview archive verified')


if __name__ == '__main__':
    main()
