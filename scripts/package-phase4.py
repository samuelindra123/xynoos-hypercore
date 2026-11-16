#!/usr/bin/env python3
"""Package committed safe static MVP + fixed owned workload + stdlib A/B tools."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    subprocess.run(['git', 'diff', '--exit-code', 'HEAD'], cwd=ROOT, check=True)
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    files = {
        'target/x86_64-unknown-linux-musl/release/xynoos-mvp': 'xynoos-mvp',
        'target/x86_64-unknown-linux-musl/release/xynoos-perf': 'xynoos-perf',
        'scripts/benchmark_physical.py': 'benchmark_physical.py',
        'scripts/inspect_physical.py': 'inspect_physical.py',
        'docs/PHASE4-N5000.md': 'README.md',
        'docs/reports/PHASE4-PERFORMANCE.md': 'PERFORMANCE.md',
        'docs/MVP-SAFETY-RECOVERY.md': 'SAFETY-RECOVERY.md',
        'docs/reports/PHASE4-QUALITY-GATES.txt': 'QUALITY-GATES.txt',
        'LICENSE': 'LICENSE',
    }
    tracked = set(subprocess.check_output(['git', 'ls-files'], cwd=ROOT, text=True).splitlines())
    required = {p for p in files if not p.startswith('target/')} | {
        'crates/xynoos-engine/src/bin/xynoos-perf.rs', 'crates/xynoos-engine/src/observation.rs',
        'scripts/package-phase4.py'}
    if not required <= tracked:
        raise SystemExit('commit all package sources and documents first')
    subprocess.run(['cargo', 'build', '--release', '--target', 'x86_64-unknown-linux-musl',
                    '-p', 'xynoos-cli', '--bin', 'xynoos-mvp', '-p', 'xynoos-engine', '--bin', 'xynoos-perf'], cwd=ROOT, check=True)
    for name in ('xynoos-mvp', 'xynoos-perf'):
        binary = ROOT/'target/x86_64-unknown-linux-musl/release'/name
        headers = subprocess.check_output(['readelf', '-l', str(binary)], text=True)
        description = subprocess.check_output(['file', str(binary)], text=True)
        if 'INTERP' in headers or not ('static' in description and 'x86-64' in description):
            raise SystemExit('expected static Linux x86_64 executable: '+description)
    name = 'xynoos-phase4-'+commit[:12]+'-linux-x86_64'
    archive = ROOT/'artifacts/phase4'/(name+'.tar.gz')
    if archive.exists():
        raise SystemExit('refusing to overwrite existing package')
    with tempfile.TemporaryDirectory(prefix='xynoos-phase4-package-') as temp:
        package = Path(temp)/name
        package.mkdir()
        for source, destination in files.items():
            shutil.copy2(ROOT/source, package/destination)
        (package/'SOURCE_COMMIT').write_text(commit+'\n')
        source_paths = sorted(p for p in tracked if p.startswith(('crates/', 'scripts/')) or p in ('Cargo.toml', 'Cargo.lock'))
        (package/'SOURCE_MANIFEST.json').write_text(json.dumps({'commit': commit,
            'source_sha256': {p: sha256(ROOT/p) for p in source_paths}}, indent=2)+'\n')
        subprocess.run(['git', 'archive', '--format=tar.gz', '--output='+str(package/'SOURCE.tar.gz'), commit], cwd=ROOT, check=True)
        (package/'SHA256SUMS').write_text(''.join(sha256(p)+'  '+p.name+'\n' for p in sorted(package.iterdir())))
        subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], cwd=package, check=True)
        # Validate the actual distribution before archiving it.
        for command in ([str(package/'xynoos-mvp'), 'observe'], [str(package/'xynoos-mvp'), 'recovery'],
                        [str(package/'xynoos-perf'), 'profile']):
            subprocess.run(command, stdout=subprocess.DEVNULL, check=True, timeout=15)
        refused = subprocess.run([str(package/'xynoos-mvp'), 'controlled', '--authorize'], capture_output=True)
        if refused.returncode == 0:
            raise SystemExit('distribution reached controlled mode')
        with tarfile.open(archive, 'x:gz') as tar:
            tar.add(package, arcname=name)
    digest = sha256(archive)
    archive.with_suffix(archive.suffix+'.sha256').write_text(digest+'  '+archive.name+'\n')
    metadata = {'source_commit': commit, 'archive': str(archive.relative_to(ROOT)), 'sha256': digest,
                'safe_entrypoints': ['xynoos-mvp observe|simulate|recovery', 'xynoos-perf profile|workload'],
                'physical_mutation_available': False, 'active_optimization_available': False,
                'measurements': 'Codespaces only; N5000 results pending'}
    with (ROOT/'artifacts/phase4/PACKAGE.json').open('x') as f:
        json.dump(metadata, f, indent=2)
        f.write('\n')
    print(json.dumps(metadata, indent=2))


if __name__ == '__main__':
    main()
