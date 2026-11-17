#!/usr/bin/env python3
"""Bind workflow attestation, reviewed main and Debian package version to one SHA."""
import json
import os
import re
import subprocess
import sys
import tomllib
from archive import require, run

sha, version = sys.argv[1:]
require(re.fullmatch(r'[0-9a-f]{40}', sha), 'Full exact commit required')
require(os.environ['GITHUB_REF'] == 'refs/heads/main', 'Publication only from main')
require(os.environ.get('XYNOOS_APT_IMMUTABLE_RELEASES') == 'enabled',
        'Owner must verify GitHub immutable releases and set XYNOOS_APT_IMMUTABLE_RELEASES=enabled')
require(sha == os.environ['GITHUB_SHA'] == run('git', 'rev-parse', 'HEAD').decode().strip(),
        'Workflow attestation and source SHA must be identical; select current main')
require(re.fullmatch(r'\d+\.\d+\.\d+~preview\.\d+-\d+', version), 'Preview Debian version required')
require(run('dpkg-parsechangelog', '-SVersion').decode().strip() == version, 'Changelog/version mismatch')
cargo = tomllib.loads(open('crates/xynoos-cli/Cargo.toml').read())['package']['version']
require(version.rsplit('-', 1)[0] == cargo.replace('-preview.', '~preview.'), 'Cargo/Debian mismatch')
repo = os.environ['GITHUB_REPOSITORY']
prs = json.loads(run('gh', 'api', f'repos/{repo}/commits/{sha}/pulls'))
require(any(p['merged_at'] and p['base']['ref'] == 'main' and p['merge_commit_sha'] == sha for p in prs),
        'Source must enter main via normal reviewed PR merge')
runs = json.loads(run('gh', 'api', f'repos/{repo}/actions/runs?head_sha={sha}&event=push&per_page=100'))['workflow_runs']
for workflow in ['debian-package.yml', 'distribution-validation.yml', 'cli-validation.yml']:
    candidates = [r for r in runs if r['path'] == '.github/workflows/' + workflow and r['head_branch'] == 'main']
    require(candidates and max(candidates, key=lambda r: r['id'])['conclusion'] == 'success',
            'Main source CI must pass: ' + workflow)
tag = 'debian/' + version.replace('~', '-')
# A network/auth failure is not absence: listing endpoints must succeed.
refs = json.loads(run('gh', 'api', f'repos/{repo}/git/matching-refs/tags/{tag}'))
require(not any(r['ref'] == 'refs/tags/' + tag for r in refs), 'Immutable release tag already exists')
print('Exact reviewed source, version and CI verified: ' + sha)
