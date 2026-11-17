#!/usr/bin/env python3
"""Prepare immutable Debian release payload after the clean build and lifecycle gates."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib

from archive import require, run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('build', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--run-id', required=True)
    args = parser.parse_args()
    build, out = args.build, args.output
    require(run('git', 'rev-parse', 'HEAD').decode().strip() == args.commit, 'Checkout/source mismatch')
    report = json.loads((build / 'provenance.json').read_text())
    require(report['comparison'] == 'PASS' and report['source']['commit'] == args.commit, 'Build/source mismatch')
    debs = list((build / 'first').glob('*.deb'))
    require(len(debs) == 1, 'Expected one binary package')
    deb = debs[0]
    version = run('dpkg-deb', '-f', str(deb), 'Version').decode().strip()
    require(run('dpkg-deb', '-f', str(deb), 'Architecture').decode().strip() == 'amd64', 'Architecture mismatch')
    require(version == run('dpkg-parsechangelog', '-SVersion').decode().strip(), 'Package/changelog mismatch')
    data = run('dpkg-deb', '--fsys-tarfile', str(deb))
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        source = json.load(archive.extractfile('./usr/share/doc/xynoos-hypercore/source.json'))
    require(source['commit'] == args.commit, 'Embedded source mismatch')
    out.mkdir(parents=True, exist_ok=False)
    for folder, patterns in [('first', ['*.deb', '*.changes', '*.buildinfo']),
                             ('source', ['*.dsc', '*.xz', '*.changes', '*.buildinfo'])]:
        for pattern in patterns:
            for path in (build / folder).glob(pattern):
                require(not (out / path.name).exists(), 'Duplicate release asset')
                shutil.copyfile(path, out / path.name)
    for suffix in ['.deb', '.changes', '.buildinfo', '.dsc', '.xz']:
        require(any(p.name.endswith(suffix) for p in out.iterdir()), 'Missing release material: ' + suffix)
    orig = list((build / 'source').glob('*.orig.tar.xz'))
    require(len(orig) == 1, 'Expected exactly one original source archive')
    with tarfile.open(orig[0]) as archive:
        matches = [m for m in archive.getmembers() if m.name.endswith('/packaging/dependencies.json')]
        require(len(matches) == 1, 'Missing/ambiguous locked dependency inventory')
        deps = json.load(archive.extractfile(matches[0]))
    components = []
    for dep in deps:
        components.append({'type': 'library', 'name': dep['name'], 'version': dep['version'],
                           'purl': f"pkg:cargo/{dep['name']}@{dep['version']}",
                           'hashes': [{'alg': 'SHA-256', 'content': dep['checksum']}],
                           'licenses': [{'expression': dep['license']}]})
    sbom = {'bomFormat': 'CycloneDX', 'specVersion': '1.5', 'version': 1,
            'metadata': {'component': {'type': 'application', 'name': 'xynoos-hypercore', 'version': version},
                         'properties': [{'name': 'xynoos:scope', 'value': 'Cargo.lock registry dependencies; OS/toolchain packages are recorded in buildinfo, not fully inventoried here'}]},
            'components': components}
    (out / 'sbom.cdx.json').write_text(json.dumps(sbom, indent=2) + '\n')
    provenance = {'source_commit': args.commit, 'debian_version': version, 'architecture': 'amd64',
                  'repository': 'samuelindra123/xynoos-hypercore',
                  'workflow': '.github/workflows/apt-publish.yml', 'run_id': args.run_id,
                  'run_url': f'https://github.com/samuelindra123/xynoos-hypercore/actions/runs/{args.run_id}',
                  'build_report': report, 'source': source}
    (out / 'source-provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    sums = ''.join(hashlib.sha256(p.read_bytes()).hexdigest() + '  ' + p.name + '\n' for p in sorted(out.iterdir()))
    (out / 'SHA256SUMS').write_text(sums)
    subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], cwd=out, check=True)


if __name__ == '__main__':
    main()
