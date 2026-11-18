#!/usr/bin/env python3
"""Verify every attested release subject before immutable GitHub prerelease publication."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from urllib.parse import quote
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


def release_tag(version):
    return 'debian/' + version.replace('~', '-')


def find_release(tag):
    ids = run('gh', 'api', '--paginate', f'repos/{REPO}/releases?per_page=100',
              '--jq', '.[] | select(.tag_name == ' + json.dumps(tag) + ') | .id').split()
    require(len(ids) <= 1, 'Multiple releases match the exact Debian tag')
    if ids:
        return json.loads(run('gh', 'api', f'repos/{REPO}/releases/{int(ids[0])}'))


def download_assets(release, directory):
    directory.mkdir(exist_ok=True)
    for asset in release['assets']:
        name = asset['name']
        require(Path(name).name == name and name not in ('', '.', '..'), 'Invalid release asset name')
        with (directory / name).open('xb') as output:
            subprocess.run(['gh', 'api', '-H', 'Accept: application/octet-stream',
                            f'repos/{REPO}/releases/assets/{asset["id"]}'], stdout=output, check=True)
        data = (directory / name).read_bytes()
        require(len(data) == asset['size'] and
                'sha256:' + hashlib.sha256(data).hexdigest() == asset.get('digest'),
                'GitHub release asset size or digest mismatch: ' + name)


def restore_subject_names(directory):
    # GitHub replaces tildes in uploaded asset names with dots. The checksum
    # manifest supplies the original names, which validate() then authenticates.
    seen = set()
    for line in (directory / 'SHA256SUMS').read_text().splitlines():
        _, name = line.split('  ')
        require(Path(name).name == name and name not in ('', '.', '..'), 'Invalid checksum subject')
        normalized = name.replace('~', '.')
        require(normalized not in seen, 'Ambiguous normalized release asset name')
        seen.add(normalized)
        original = directory / name
        if original != directory / normalized:
            require(not (original.exists() and (directory / normalized).exists()),
                    'Duplicate release asset under normalized name')
            if not original.exists():
                (directory / normalized).rename(original)


def download_and_verify(directory, release):
    expected = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                for p in directory.iterdir() if p.is_file()}
    with tempfile.TemporaryDirectory() as temporary:
        target = Path(temporary)
        download_assets(release, target)
        restore_subject_names(target)
        downloaded = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in target.iterdir()}
        require(downloaded == expected, 'GitHub release asset bytes differ from verified payload')


def tag_ref(tag, commit):
    encoded = quote(tag, safe='')
    refs = json.loads(run('gh', 'api', f'repos/{REPO}/git/matching-refs/tags/{encoded}'))
    matches = [ref for ref in refs if ref['ref'] == 'refs/tags/' + tag]
    if matches:
        require(len(matches) == 1 and matches[0]['object']['type'] == 'commit' and
                matches[0]['object']['sha'] == commit,
            'Debian tag points to a different source; never overwrite')
    return bool(matches)


def require_immutable_configuration():
    require(os.environ.get('XYNOOS_APT_IMMUTABLE_RELEASES') == 'enabled',
            'Owner must verify GitHub immutable releases and set XYNOOS_APT_IMMUTABLE_RELEASES=enabled')


def finish_release(directory, commit, tag, release):
    require(release['tag_name'] == tag and release['prerelease'],
            'Expected exact Debian prerelease')
    require(release['target_commitish'] == commit, 'Release targets a different source commit')
    download_and_verify(directory, release)
    if release['draft']:
        require_immutable_configuration()
        if not tag_ref(tag, commit):
            run('gh', 'api', '-X', 'POST', f'repos/{REPO}/git/refs',
                '-f', 'ref=refs/tags/' + tag, '-f', 'sha=' + commit)
        require(tag_ref(tag, commit), 'Created Debian tag could not be read back')
        run('gh', 'api', '-X', 'PATCH', f'repos/{REPO}/releases/{release["id"]}', '-F', 'draft=false')

    final = json.loads(run('gh', 'api', f'repos/{REPO}/releases/{release["id"]}'))
    require(final['tag_name'] == tag and final['target_commitish'] == commit and
            not final['draft'] and final['prerelease'] and final.get('immutable') is True,
            'Public release read-back is not the exact immutable prerelease')
    require(tag_ref(tag, commit), 'Missing immutable release tag')
    download_and_verify(directory, final)
    return final['html_url']


def publish_release(directory, commit, version, provenance):
    tag = release_tag(version)
    require_immutable_configuration()
    release = find_release(tag)
    if release:
        return finish_release(directory, commit, tag, release)
    require(not tag_ref(tag, commit), 'Existing Debian tag has no release; refusing to recreate')
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
    release = find_release(tag)
    require(release is not None, 'Created draft could not be read back by exact tag')
    return finish_release(directory, commit, tag, release)


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
