#!/usr/bin/env python3
"""Publish maintained Wiki pages using normal Git history; no force or branch deletion."""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
REMOTE = 'https://github.com/samuelindra123/xynoos-hypercore.wiki.git'


def run(*args, **kwargs):
    return subprocess.check_output(args, **kwargs).decode().strip()


def main():
    if run('git', 'status', '--porcelain', cwd=ROOT):
        raise SystemExit('Commit and push documentation before Wiki publication')
    source = run('git', 'rev-parse', 'HEAD', cwd=ROOT)
    files = sorted((ROOT / 'docs/wiki').glob('*.md'))
    if len(files) != 27:
        raise SystemExit('Expected 25 pages, sidebar and footer')
    with tempfile.TemporaryDirectory(prefix='xynoos-wiki-') as temporary:
        target = Path(temporary) / 'wiki'
        subprocess.run(['git', 'clone', REMOTE, str(target)], check=True)
        for path in files:
            shutil.copyfile(path, target / path.name)
        subprocess.run(['git', 'add', '--', *[p.name for p in files]], cwd=target, check=True)
        if subprocess.run(['git', 'diff', '--cached', '--quiet'], cwd=target).returncode:
            subprocess.run(['git', 'commit', '-m', 'Publish maintained Xynoos Wiki from ' + source], cwd=target, check=True)
            subprocess.run(['git', 'push', 'origin', 'HEAD'], cwd=target, check=True)
        head = run('git', 'rev-parse', 'HEAD', cwd=target)
        remote = run('git', 'ls-remote', 'origin', 'HEAD', cwd=target).split()[0]
        if head != remote:
            raise SystemExit('Wiki remote head differs; publication not verified')
        print('Verified Wiki commit: ' + head)
        print('https://github.com/samuelindra123/xynoos-hypercore/wiki')


if __name__ == '__main__':
    main()
