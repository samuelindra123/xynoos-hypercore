import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

path = Path(__file__).resolve().parents[1] / "scripts/release-source-gate.py"
spec = importlib.util.spec_from_file_location("release_gate", path)
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class ReleaseGateTests(unittest.TestCase):
    commit = "a" * 40

    def fixture(self, path):
        if path.startswith("compare/"): return {"status": "ahead", "merge_base_commit": {"sha": self.commit}}
        if path.endswith("/pulls"): return [{"merged_at": "now", "base": {"ref": "main"}, "merge_commit_sha": self.commit}]
        return {"workflow_runs": [
            {"id": 1, "path": ".github/workflows/cli-validation.yml", "head_branch": "main", "conclusion": "success"},
            {"id": 2, "path": ".github/workflows/release.yml", "head_branch": "v0.1.0-preview.1", "conclusion": "failure"}]}

    def test_publication_permission_failure_does_not_change_reviewed_source(self):
        with patch.object(gate, "api", side_effect=self.fixture), patch.object(gate.subprocess, "check_output", return_value='[package]\nversion="0.1.0-preview.1"'):
            gate.validate("v0.1.0-preview.1", self.commit)

    def test_unmerged_source_refused(self):
        with patch.object(gate, "api", return_value={"status": "behind", "merge_base_commit": {"sha": "b"*40}}), patch.object(gate.subprocess, "check_output", return_value='[package]\nversion="0.1.0-preview.1"'):
            with self.assertRaises(AssertionError): gate.validate("v0.1.0-preview.1", self.commit)

    def test_failed_or_pending_source_checks_refused(self):
        for conclusion in ("failure", None, "cancelled"):
            def changed(path):
                value = self.fixture(path)
                if path.startswith("actions/"): value["workflow_runs"][0]["conclusion"] = conclusion
                return value
            with patch.object(gate, "api", side_effect=changed), patch.object(gate.subprocess, "check_output", return_value='[package]\nversion="0.1.0-preview.1"'):
                with self.assertRaises(AssertionError): gate.validate("v0.1.0-preview.1", self.commit)

    def test_tag_source_version_disagreement_refused(self):
        with patch.object(gate.subprocess, "check_output", return_value='[package]\nversion="0.2.0-preview.1"'):
            with self.assertRaises(AssertionError): gate.validate("v0.1.0-preview.1", self.commit)
