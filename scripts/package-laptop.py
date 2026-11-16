#!/usr/bin/env python3
"""Package committed static safe binaries, diagnostic and source admission experiment."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

from benchmark_desktop import make_corpus

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    subprocess.run(['git', 'diff', '--exit-code', 'HEAD'], cwd=ROOT, check=True)
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    tracked = subprocess.check_output(['git', 'ls-files'], cwd=ROOT, text=True).splitlines()
    sources = {
        'scripts/benchmark_desktop.py': 'benchmark_desktop.py',
        'scripts/benchmark_physical.py': 'benchmark_physical.py',
        'scripts/laptop_diagnostic.py': 'laptop_diagnostic.py',
        'scripts/inspect_desktop.py': 'inspect_desktop.py',
        'benchmarks/desktop-probe.html': 'desktop-probe.html',
        'docs/PHASE4-LAPTOP-N5000.md': 'README.md',
        'docs/PHASE4-LAPTOP-SAFETY.md': 'SAFETY.md',
        'docs/PHASE4-LAPTOP-RESEARCH.md': 'RESEARCH.md',
        'docs/reports/PHASE4-LAPTOP-RESULTS.md': 'RESULTS.md',
        'artifacts/phase4-laptop/quality-gates.txt': 'QUALITY-GATES.txt',
        'LICENSE': 'LICENSE',
    }
    required = set(sources) | {'scripts/package-laptop.py', 'crates/xynoos-engine/src/bin/xynoos-desktop.rs',
                               'scripts/tests/test_desktop.py'}
    if not required <= set(tracked):
        raise SystemExit('commit package source, documentation and quality evidence first')
    subprocess.run(['cargo', 'build', '--release', '--target', 'x86_64-unknown-linux-musl',
                    '-p', 'xynoos-engine', '--bin', 'xynoos-desktop', '-p', 'xynoos-cli', '--bin', 'xynoos-mvp'],
                   cwd=ROOT, check=True)
    names = ('xynoos-mvp', 'xynoos-desktop')
    for name in names:
        binary = ROOT / 'target/x86_64-unknown-linux-musl/release' / name
        program = subprocess.check_output(['readelf', '-l', str(binary)], text=True)
        dynamic = subprocess.check_output(['readelf', '-d', str(binary)], text=True)
        description = subprocess.check_output(['file', str(binary)], text=True)
        if 'INTERP' in program or 'NEEDED' in dynamic or not ('static' in description and 'x86-64' in description):
            raise SystemExit('expected static Linux x86_64 executable: ' + description)
        sources[str(binary.relative_to(ROOT))] = name
    destination = ROOT / 'artifacts/phase4-laptop' / commit[:12]
    destination.mkdir(exist_ok=False)
    name = 'xynoos-laptop-' + commit[:12] + '-linux-x86_64'
    archive = destination / (name + '.tar.gz')
    with tempfile.TemporaryDirectory(prefix='xynoos-laptop-package-') as temp:
        stage = Path(temp) / name
        stage.mkdir(mode=0o700)
        for source, target in sources.items():
            shutil.copy2(ROOT / source, stage / target)
            (stage / target).chmod(0o755 if target in names else 0o644)
            if target == 'RESULTS.md':
                text = (stage / target).read_text().replace('../PHASE4-LAPTOP-RESEARCH.md', 'RESEARCH.md').replace('../PHASE4-LAPTOP-SAFETY.md', 'SAFETY.md')
                (stage / target).write_text(text)
        make_corpus(ROOT, stage / 'source-corpus.txt')
        (stage / 'SOURCE_COMMIT').write_text(commit + '\n')
        manifest = {'commit': commit, 'target': 'x86_64-unknown-linux-musl',
                    'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
                    'cargo': subprocess.check_output(['cargo', '-V'], text=True).strip(),
                    'rustflags': os.environ.get('RUSTFLAGS'),
                    'source_sha256': {p: sha(ROOT / p) for p in tracked if
                        p.startswith(('crates/', 'scripts/', 'benchmarks/')) or p in ('Cargo.toml', 'Cargo.lock')},
                    'build_commands': ['cargo build --release --target x86_64-unknown-linux-musl -p xynoos-engine --bin xynoos-desktop -p xynoos-cli --bin xynoos-mvp']}
        (stage / 'SOURCE_MANIFEST.json').write_text(json.dumps(manifest, indent=2) + '\n')
        subprocess.run(['git', 'archive', '--format=tar.gz', '--output=' + str(stage / 'SOURCE.tar.gz'), commit], cwd=ROOT, check=True)
        (stage / 'SHA256SUMS').write_text(''.join(sha(p) + '  ' + p.name + '\n' for p in sorted(stage.iterdir())))
        subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], cwd=stage, check=True)
        for mode in ('observe', 'recovery'):
            subprocess.run([str(stage / 'xynoos-mvp'), mode], stdout=subprocess.DEVNULL, check=True, timeout=10)
        for argv in (['controlled'], ['controlled', '--authorize']):
            if subprocess.run([str(stage / 'xynoos-mvp'), *argv], capture_output=True, timeout=5).returncode == 0:
                raise SystemExit('unsafe MVP entrypoint admitted')
        subprocess.run(['python3', '-B', str(stage / 'laptop_diagnostic.py'), '--environment', 'Codespaces',
                        '--label', 'staged package diagnostic smoke; Codespaces only', '--seconds', '1',
                        '--output', str(destination / 'diagnostic-smoke.jsonl')], check=True, timeout=10)
        subprocess.run(['python3', '-B', str(stage / 'benchmark_desktop.py'), '--worker', str(stage / 'xynoos-desktop'),
                        '--observer', str(stage / 'xynoos-mvp'), '--corpus', str(stage / 'source-corpus.txt'),
                        '--environment', 'Codespaces', '--label', 'staged static package smoke; Codespaces only',
                        '--arms', 'all', '--authorize-disposable-index-admission', '--requests', '16',
                        '--interval-ms', '5', '--index-passes', '1', '--output', str(destination / 'package-smoke')],
                       stdout=subprocess.DEVNULL, check=True, timeout=45)
        inspected = subprocess.check_output(['python3', '-B', str(stage / 'inspect_desktop.py'),
                                            str(destination / 'package-smoke/events.jsonl')], timeout=5)
        report = json.loads(inspected)
        if report['status'] != 'complete':
            raise SystemExit('staged package smoke incomplete')
        (destination / 'package-inspection.json').write_bytes(inspected)
        with tarfile.open(archive, 'x:gz') as tar:
            tar.add(stage, arcname=name)
        shutil.copy2(stage / 'SOURCE_MANIFEST.json', destination / 'PACKAGE-SOURCE-MANIFEST.json')
    digest = sha(archive)
    archive.with_suffix('.gz.sha256').write_text(digest + '  ' + archive.name + '\n')
    metadata = {'source_commit': commit, 'archive': str(archive.relative_to(ROOT)), 'sha256': digest,
                'physical_kernel_mutation_available': False,
                'active_scope': 'explicitly authorized admission of new chunks to owned disposable source indexer only',
                'requirements': 'Linux x86_64, Python 3.9+, procfs/PSI/cgroup v2, pidfd, memfd seals; no Rust or sudo',
                'measurements': 'Codespaces only; no N5000 desktop benefit established'}
    (destination / 'PACKAGE.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(json.dumps(metadata, indent=2))


if __name__ == '__main__':
    main()
