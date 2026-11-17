#!/usr/bin/env python3
"""Verify every attested release subject before immutable GitHub prerelease publication."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from archive import require, run

REPO = 'samuelindra123/xynoos-hypercore'


def validate(directory, commit, version):
    provenance = json.loads((directory / 'source-provenance.json').read_text())
    require(provenance['source_commit'] == commit and provenance['debian_version'] == version,
            'Release source/version mismatch')
    sums = {}
    for line in (directory / 'SHA256SUMS').read_text().splitlines():
        sha, name = line.split('  ')
        require(Path(name).name == name and name not in sums, 'Invalid checksum subject')
        require(hashlib.sha256((directory / name).read_bytes()).hexdigest() == sha, 'Release checksum mismatch')
        sums[name] = sha
    subjects = set(sums) | {'SHA256SUMS'}
    require(subjects == {p.name for p in directory.iterdir() if p.is_file() and p.name != 'provenance.sigstore.json'},
            'Unverified release files')
    for name in sorted(subjects):
        run('gh', 'attestation', 'verify', str(directory / name), '--repo', REPO,
            '--signer-workflow', REPO + '/.github/workflows/apt-publish.yml',
            '--source-digest', commit, '--source-ref', 'refs/heads/main')
    return provenance


def download_and_verify(tag, directory, release):
    expected = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                for path in directory.iterdir() if path.is_file()}
    require({asset['name'] for asset in release['assets']} == set(expected),
            'Release asset names differ from verified payload')
    with tempfile.TemporaryDirectory() as temporary:
        run('gh', 'release', 'download', tag, '--repo', REPO, '--dir', temporary)
        downloaded = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                      for path in Path(temporary).iterdir() if path.is_file()}
    require(downloaded == expected, 'GitHub release asset bytes differ from verified payload')


def publish_release(directory, commit, version, provenance):
    tag = 'debian/' + version.replace('~', '-')
    require(os.environ.get('XYNOOS_APT_IMMUTABLE_RELEASES') == 'enabled',
            'Owner must verify GitHub immutable releases and set XYNOOS_APT_IMMUTABLE_RELEASES=enabled')
    refs = json.loads(run('gh', 'api', f'repos/{REPO}/git/matching-refs/tags/{tag}'))
    require(not any(r['ref'] == 'refs/tags/' + tag for r in refs),
            'Immutable tag exists; use promotion recovery, never overwrite')
    notes = f'''Experimental Debian preview — {version}

amd64 package: xynoos-hypercore. Read-only on-demand diagnostics; no service activation or global kernel/swap/ZRAM/CPU/GPU tuning. No physical N5000 or desktop performance benefit is proven. Not production-ready.

Exact source: {commit}
Build and validation: {provenance['run_url']}

Includes .deb, .buildinfo, .changes, source archives, SHA256SUMS, source provenance, scoped Cargo CycloneDX SBOM and GitHub attestation bundle. Every payload subject was independently checked with gh attestation verify before publication. Debian .changes/.buildinfo are checksum/provenance subjects, not separately OpenPGP-signed upload material.

The dedicated APT OpenPGP key signs repository metadata, separately from GitHub's OIDC attestations and the standalone installer's Ed25519 trust anchor. APT availability requires a successful combined Studio/apt Pages deployment and public verification. See docs/distribution/INSTALLATION.md and the Wiki source in docs/wiki.
'''
    with tempfile.NamedTemporaryFile('w', suffix='.md') as stream:
        stream.write(notes)
        stream.flush()
        subprocess.run(['gh', 'release', 'create', tag, '--repo', REPO, '--target', commit,
                        '--draft', '--prerelease', '--title', 'Xynoos Debian ' + version + ' — Experimental Preview',
                        '--notes-file', stream.name, *map(str, sorted(directory.iterdir()))], check=True)

    release = json.loads(run('gh', 'api', f'repos/{REPO}/releases/tags/{tag}'))
    require(release['draft'] and release['prerelease'] and release['tag_name'] == tag,
            'Expected the exact prerelease in draft state before immutable publication')
    ref = json.loads(run('gh', 'api', f'repos/{REPO}/git/ref/tags/{tag}'))
    require(ref['object']['type'] == 'commit' and ref['object']['sha'] == commit,
            'Release tag does not point to the exact source commit')
    download_and_verify(tag, directory, release)

    # Publishing is the irreversible boundary when immutable releases are enabled.
    published = json.loads(run('gh', 'api', '-X', 'PATCH',
                               f'repos/{REPO}/releases/{release["id"]}', '-F', 'draft=false'))
    require(not published['draft'] and published['prerelease'] and published.get('immutable') is True,
            'Published release must be an immutable prerelease')
    final = json.loads(run('gh', 'api', f'repos/{REPO}/releases/{release["id"]}'))
    require(not final['draft'] and final['prerelease'] and final.get('immutable') is True,
            'Public release read-back is not an immutable prerelease')
    download_and_verify(tag, directory, final)
    return final['html_url']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    provenance = validate(args.directory, args.commit, args.version)
    if args.publish:
        print(publish_release(args.directory, args.commit, args.version, provenance))


if __name__ == '__main__':
    main()
