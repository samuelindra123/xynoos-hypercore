#!/usr/bin/env python3
"""Recover a release→Pages interruption by promoting only a verified existing prerelease."""
import json
import os
import re
import sys
from pathlib import Path
from release import download_assets, find_release, finish_release, release_tag, restore_subject_names, validate
from archive import require, run

commit, version = sys.argv[1:]
repo = os.environ['GITHUB_REPOSITORY']
require(os.environ['GITHUB_REF'] == 'refs/heads/main', 'Promotion only on main')
require(re.fullmatch(r'[0-9a-f]{40}', commit), 'Exact source commit required')
require(re.fullmatch(r'\d+\.\d+\.\d+~preview\.\d+-\d+', version), 'Preview Debian version required')
comparison = json.loads(run('gh', 'api', f'repos/{repo}/compare/{commit}...main'))
require(comparison['merge_base_commit']['sha'] == commit, 'Source must be in main')
tag = release_tag(version)
release = find_release(tag)
require(release is not None and release['tag_name'] == tag and release['prerelease'] and
        release['target_commitish'] == commit, 'Existing exact Debian prerelease required')
directory = Path('release')
directory.mkdir()
download_assets(release, directory)
restore_subject_names(directory)
validate(directory, commit, version)
print(finish_release(directory, commit, tag, release))
