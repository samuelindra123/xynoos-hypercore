#!/usr/bin/env python3
"""Recover a release→Pages interruption by promoting only a verified existing prerelease."""
import json
import os
import re
import sys
from archive import require, run

commit, version = sys.argv[1:]
repo = os.environ['GITHUB_REPOSITORY']
require(os.environ['GITHUB_REF'] == 'refs/heads/main', 'Promotion only on main')
require(re.fullmatch(r'[0-9a-f]{40}', commit), 'Exact source commit required')
require(re.fullmatch(r'\d+\.\d+\.\d+~preview\.\d+-\d+', version), 'Preview Debian version required')
comparison = json.loads(run('gh', 'api', f'repos/{repo}/compare/{commit}...main'))
require(comparison['merge_base_commit']['sha'] == commit, 'Source must be in main')
tag = 'debian/' + version.replace('~', '-')
ref = json.loads(run('gh', 'api', f'repos/{repo}/git/ref/tags/{tag}'))
require(ref['object']['type'] == 'commit' and ref['object']['sha'] == commit, 'Immutable tag/source mismatch')
release = json.loads(run('gh', 'api', f'repos/{repo}/releases/tags/{tag}'))
require(release['prerelease'] and not release['draft'], 'Existing public preview required')
run('gh', 'release', 'download', tag, '--repo', repo, '--dir', 'release')
print('Existing immutable prerelease selected; attestation verification required next')
