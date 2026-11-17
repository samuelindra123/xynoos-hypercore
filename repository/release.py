#!/usr/bin/env python3
"""Verify every attested release subject before immutable GitHub prerelease publication."""
import argparse
import hashlib
import json
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    provenance = validate(args.directory, args.commit, args.version)
    tag = 'debian/' + args.version.replace('~', '-')
    if args.publish:
        refs = json.loads(run('gh', 'api', f'repos/{REPO}/git/matching-refs/tags/{tag}'))
        require(not any(r['ref'] == 'refs/tags/' + tag for r in refs), 'Immutable tag exists; use promotion recovery, never overwrite')
        notes = f'''Experimental Debian preview — {args.version}

amd64 package: xynoos-hypercore. Read-only on-demand diagnostics; no service activation or global kernel/swap/ZRAM/CPU/GPU tuning. No physical N5000 or desktop performance benefit is proven. Not production-ready.

Exact source: {args.commit}
Build and validation: {provenance['run_url']}

Includes .deb, .buildinfo, .changes, source archives, SHA256SUMS, source provenance, scoped Cargo CycloneDX SBOM and GitHub attestation bundle. Every payload subject was independently checked with gh attestation verify before publication. Debian .changes/.buildinfo are checksum/provenance subjects, not separately OpenPGP-signed upload material.

The dedicated APT OpenPGP key signs repository metadata, separately from GitHub's OIDC attestations and the standalone installer's Ed25519 trust anchor. APT availability requires a successful combined Studio/apt Pages deployment and public verification. See docs/distribution/INSTALLATION.md and the Wiki source in docs/wiki.
'''
        with tempfile.NamedTemporaryFile('w', suffix='.md') as stream:
            stream.write(notes)
            stream.flush()
            subprocess.run(['gh', 'release', 'create', tag, '--repo', REPO, '--target', args.commit,
                            '--prerelease', '--title', 'Xynoos Debian ' + args.version + ' — Experimental Preview',
                            '--notes-file', stream.name, *map(str, sorted(args.directory.iterdir()))], check=True)
        release = json.loads(run('gh', 'api', f'repos/{REPO}/releases/tags/{tag}'))
        require(release['prerelease'] and not release['draft'], 'Expected public prerelease')
        require({p.name for p in args.directory.iterdir()} == {a['name'] for a in release['assets']}, 'Release asset mismatch')
        # Read back immutable public release bytes, not just asset names.
        with tempfile.TemporaryDirectory() as temp:
            run('gh', 'release', 'download', tag, '--repo', REPO, '--dir', temp)
            for path in args.directory.iterdir():
                require((Path(temp) / path.name).read_bytes() == path.read_bytes(), 'Published asset mismatch')
        print(release['html_url'])


if __name__ == '__main__':
    main()
