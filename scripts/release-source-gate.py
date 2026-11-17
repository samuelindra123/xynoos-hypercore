#!/usr/bin/env python3
"""Require merged main ancestry and passing source CI, independent of publish state."""
import argparse
import json
import re
import subprocess
import sys
import tomllib

REPO = "samuelindra123/xynoos-hypercore"
SOURCE_WORKFLOWS = {".github/workflows/cli-validation.yml", ".github/workflows/studio-pages.yml"}


def api(path):
    return json.loads(subprocess.check_output(["gh", "api", f"repos/{REPO}/" + path]))


def validate(version, commit):
    assert re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+-preview\.[1-9][0-9]*", version), "Only developer previews are authorized"
    assert re.fullmatch(r"[0-9a-f]{40}", commit), "Invalid source commit"
    cargo = subprocess.check_output(["git", "show", commit + ":crates/xynoos-cli/Cargo.toml"], text=True)
    assert tomllib.loads(cargo)["package"]["version"] == version[1:], "Tag/Cargo version mismatch"
    comparison = api("compare/" + commit + "...main")
    assert comparison["status"] in ("identical", "ahead") and comparison["merge_base_commit"]["sha"] == commit, "Release source must be merged into main"
    prs = api("commits/" + commit + "/pulls")
    assert any(p["merged_at"] and p["base"]["ref"] == "main" and p["merge_commit_sha"] == commit for p in prs), "Source must enter main through a merged PR"
    runs = api("actions/runs?head_sha=" + commit + "&event=push")["workflow_runs"]
    latest = {}
    for run in sorted(runs, key=lambda r: r["id"]):
        if run["path"] in SOURCE_WORKFLOWS and run["head_branch"] == "main":
            latest[run["path"]] = run
    assert ".github/workflows/cli-validation.yml" in latest, "CLI validation workflow required"
    assert all(r["conclusion"] == "success" for r in latest.values()), "All applicable main source validation workflows must pass"
    print("Merged main source and validation checks verified: " + commit)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("version")
    parser.add_argument("--source-commit", help="Attested package source; defaults to checkout HEAD")
    args = parser.parse_args()
    commit = args.source_commit or subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    validate(args.version, commit)
