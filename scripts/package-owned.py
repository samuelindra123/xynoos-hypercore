#!/usr/bin/env python3
"""Package committed static safe entry points and reviewed cooperative experiment."""
import hashlib
import json
import os
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
        'target/x86_64-unknown-linux-musl/release/xynoos-owned': 'xynoos-owned',
        'scripts/benchmark_owned.py': 'benchmark_owned.py',
        'scripts/inspect_owned.py': 'inspect_owned.py',
        'scripts/inspect_physical.py': 'inspect_physical.py',
        'docs/PHASE4-N5000.md': 'BASELINE-N5000.md',
        'docs/PHASE4-OWNED-N5000.md': 'README.md',
        'docs/reports/PHASE4-OWNED-PERFORMANCE.md': 'PERFORMANCE.md',
        'docs/reports/PHASE4-OWNED-SAFETY-REVIEW.md': 'SAFETY-REVIEW.md',
        'docs/MVP-SAFETY-RECOVERY.md': 'SAFETY-RECOVERY.md',
        'docs/reports/PHASE4-OWNED-QUALITY-GATES.txt': 'QUALITY-GATES.txt',
        'LICENSE': 'LICENSE',
    }
    tracked = set(subprocess.check_output(['git', 'ls-files'], cwd=ROOT, text=True).splitlines())
    required = {p for p in files if not p.startswith('target/')} | {
        'crates/xynoos-engine/src/bin/xynoos-perf.rs', 'crates/xynoos-engine/src/observation.rs',
        'scripts/package-owned.py', 'crates/xynoos-engine/src/owned.rs',
        'crates/xynoos-engine/src/bin/xynoos-owned.rs', 'crates/xynoos-aura/src/cooperative.rs'}
    if not required <= tracked:
        raise SystemExit('commit all package sources and documents first')
    subprocess.run(['cargo', 'build', '--locked', '--release', '--target', 'x86_64-unknown-linux-musl',
                    '-p', 'xynoos-cli', '--bin', 'xynoos-mvp', '-p', 'xynoos-engine', '--bin', 'xynoos-perf', '--bin', 'xynoos-owned'], cwd=ROOT, check=True)
    for name in ('xynoos-mvp', 'xynoos-perf', 'xynoos-owned'):
        binary = ROOT/'target/x86_64-unknown-linux-musl/release'/name
        headers = subprocess.check_output(['readelf', '-l', str(binary)], text=True)
        description = subprocess.check_output(['file', str(binary)], text=True)
        if 'INTERP' in headers or not ('static' in description and 'x86-64' in description):
            raise SystemExit('expected static Linux x86_64 executable: '+description)
    name = 'xynoos-owned-'+commit[:12]+'-linux-x86_64'
    archive = ROOT/'artifacts/phase4-owned'/(name+'.tar.gz')
    archive.parent.mkdir(parents=True, exist_ok=True)
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
            'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
            'cargo': subprocess.check_output(['cargo', '-V'], text=True).strip(),
            'build_target': 'x86_64-unknown-linux-musl',
            'rustflags': os.environ.get('RUSTFLAGS'),
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
        subprocess.run(['python3', '-B', str(package/'benchmark_owned.py'), '--worker', str(package/'xynoos-owned'),
                        '--environment', 'Codespaces', '--label', 'staged static package smoke, Codespaces only',
                        '--authorize-owned', '--repeats', '6', '--requests', '16', '--interval-ms', '0',
                        '--output', str(Path(temp)/'smoke')], stdout=subprocess.DEVNULL, check=True, timeout=30)
        inspection = json.loads(subprocess.check_output(
            ['python3', '-B', str(package/'inspect_owned.py'), str(Path(temp)/'smoke/events.jsonl')], timeout=5))
        if inspection['status'] != 'complete':
            raise SystemExit('package smoke evidence incomplete')
        (Path(temp)/'smoke/inspection.json').write_text(json.dumps(inspection, indent=2)+'\n')
        shutil.copytree(Path(temp)/'smoke', ROOT/'artifacts/phase4-owned'/('package-smoke-'+commit[:12]))
        with tarfile.open(archive, 'x:gz') as tar:
            tar.add(package, arcname=name)
    digest = sha256(archive)
    archive.with_suffix(archive.suffix+'.sha256').write_text(digest+'  '+archive.name+'\n')
    metadata = {'source_commit': commit, 'archive': str(archive.relative_to(ROOT)), 'sha256': digest,
                'safe_entrypoints': ['xynoos-mvp observe|simulate|recovery', 'xynoos-perf profile|workload', 'xynoos-owned baseline|observe mixed|entropy', 'xynoos-owned active mixed|entropy --authorize-owned'],
                'physical_mutation_available': False, 'active_optimization_available': True, 'active_scope': 'owned disposable 16 MiB corpus only; no kernel-control path',
                'measurements': 'Codespaces only; N5000 results pending'}
    with (ROOT/'artifacts/phase4-owned/PACKAGE.json').open('x') as f:
        json.dump(metadata, f, indent=2)
        f.write('\n')
    print(json.dumps(metadata, indent=2))


if __name__ == '__main__':
    main()
