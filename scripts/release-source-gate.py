#!/usr/bin/env python3
"""Publication requires a merged PR at main HEAD and passing validation checks."""
import json
import re
import subprocess
import sys
import tomllib

version = sys.argv[1]
assert re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+-preview\.[1-9][0-9]*", version), "Only developer previews are authorized"
with open("crates/xynoos-cli/Cargo.toml", "rb") as stream:
    assert tomllib.load(stream)["package"]["version"] == version[1:], "Tag/Cargo version mismatch"
commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
def api(path):
    return json.loads(subprocess.check_output(["gh", "api", "repos/samuelindra123/xynoos-hypercore/" + path]))
assert api("commits/main")["sha"] == commit, "Release source must be current main"
prs = api("commits/" + commit + "/pulls")
assert any(p["merged_at"] and p["base"]["ref"] == "main" and p["merge_commit_sha"] == commit for p in prs), "Source must enter main through a merged PR"
runs = api("commits/" + commit + "/check-runs")["check_runs"]
workflow_runs = api("actions/runs?head_sha=" + commit + "&event=push")["workflow_runs"]
assert any(r["path"] == ".github/workflows/cli-validation.yml" and r["conclusion"] == "success" for r in workflow_runs), "Passing main CLI validation workflow required"
assert not any(r["conclusion"] in ("failure", "cancelled", "timed_out", "action_required") for r in runs), "Failed check blocks release"
print("Merged main source and validation checks verified: " + commit)
