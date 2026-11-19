#!/usr/bin/env python3
"""Deterministic evidence fixtures; never invoke systemd or touch cgroupfs."""
import contextlib
import copy
import errno
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("cleanup", Path(__file__).parents[1] / "xynoos-unit-preflight.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
UNIT = "xynoos-018d-fixture.service"
INV = "12" * 16
STATE = dict(Id=UNIT, LoadState="loaded", ActiveState="active", InvocationID=INV,
             MainPID="123", ControlGroup="/user.slice/" + UNIT, Transient="yes")
RECORD = dict(record="manager_preflight", unit=UNIT, binding_revalidation="passed",
              manager_pid=123, binding=dict(unit_id=UNIT, invocation_id=INV, main_pid=123,
              control_group=STATE["ControlGroup"], actual_membership=STATE["ControlGroup"] + "/manager"))


class Fixtures(unittest.TestCase):
    def error(self, reason, fn, status="UNVERIFIED"):
        with self.assertRaises(m.EvidenceError) as caught:
            fn()
        self.assertEqual((caught.exception.status, caught.exception.reason), (status, reason))

    def test_normal_snapshot(self):
        self.assertIsNone(m.capture_unit(RECORD, UNIT, STATE))
        self.assertEqual(m.parse_state("\n".join(k + "=" + v for k, v in STATE.items())), STATE)

    def test_inactive_awaiting_collection(self):
        s = dict(STATE, ActiveState="inactive", MainPID="0")
        self.assertEqual(m.terminal(s, STATE), "inactive_awaiting_collection")

    def test_collected_preserved_identity(self):
        s = dict(STATE, LoadState="not-found", ActiveState="inactive", MainPID="0", InvocationID="", ControlGroup="")
        self.assertEqual(m.terminal(s, STATE), "collected")

    def test_active(self):
        self.error("service_not_inactive", lambda: m.terminal(STATE, STATE), "FAIL")

    def test_contradictory(self):
        self.error("contradictory_collected_state", lambda: m.terminal(dict(STATE, LoadState="not-found"), STATE), "FAIL")

    def test_missing_malformed_bus(self):
        for text in ("", "LoadState=loaded\nLoadState=loaded", "garbage", "Other=x"):
            self.error("missing_unit_information" if not text else "malformed_unit_information", lambda: m.parse_state(text))
        text = "\n".join(k + "=" + ("bad" if k == "InvocationID" else v) for k, v in STATE.items())
        self.error("malformed_invocation", lambda: m.parse_state(text))

    def test_bus_unavailable_and_command_timeout(self):
        for outcome in (subprocess.CompletedProcess([], 1, "", "no bus"),
                        subprocess.TimeoutExpired("systemctl", 2)):
            with patch.object(m.subprocess, "run", **({"side_effect": outcome} if isinstance(outcome, Exception) else {"return_value": outcome})):
                self.error("unit_information_unavailable", lambda: m.show(UNIT))

    def test_stale_record_and_missing_inactive_invocation(self):
        record = copy.deepcopy(RECORD)
        record["binding"]["invocation_id"] = "0" * 32
        self.error("missing_invocation_identity", lambda: m.capture_unit(record, UNIT, STATE))
        self.error("inactive_identity_unavailable", lambda: m.terminal(dict(STATE, ActiveState="inactive", MainPID="0", InvocationID=""), STATE))

    def test_invocation_and_name_reuse(self):
        self.error("invocation_mismatch", lambda: m.capture_unit(RECORD, UNIT, dict(STATE, InvocationID="34" * 16)), "FAIL")
        self.error("unit_name_reuse", lambda: m.terminal(dict(STATE, Id="other.service"), STATE), "FAIL")
        self.error("invocation_mismatch", lambda: m.terminal(dict(STATE, InvocationID="34" * 16), STATE), "FAIL")

    def hierarchy(self, root):
        parent = Path(root) / UNIT
        manager = parent / "manager"
        manager.mkdir(parents=True)
        for directory in (parent, manager):
            (directory / "cgroup.events").write_text("populated 0\nfrozen 0\n")
        record = copy.deepcopy(RECORD)
        record.update(parent_path=str(parent), manager_path=str(manager))
        for name, directory in (("parent", parent), ("manager", manager)):
            st = directory.stat()
            record["binding"][name + "_descriptor"] = dict(device=st.st_dev, inode=st.st_ino)
        h = m.Hierarchy()
        with patch.object(m, "cgroupfs"):
            evidence = h.capture(record, UNIT)
        self.addCleanup(h.close)
        return h, parent, manager, evidence

    def remove_fixture(self, parent, manager):
        (manager / "cgroup.events").unlink()
        manager.rmdir()
        (parent / "cgroup.events").unlink()
        parent.rmdir()

    def test_present_and_removed_exact_evidence(self):
        with tempfile.TemporaryDirectory() as root:
            h, parent, manager, evidence = self.hierarchy(root)
            self.assertEqual(set(evidence), {"ancestor", "parent", "manager"})
            self.assertFalse(h.removed())
            self.remove_fixture(parent, manager)
            # Ordinary filesystem EOF/data cannot stand in for kernfs deactivation.
            self.assertFalse(h.removed())
            with patch.object(m.os, "pread", side_effect=OSError(errno.ENODEV, "fixture deactivated")):
                self.assertTrue(h.removed())
            with patch.object(m.os, "pread", side_effect=OSError(errno.EIO, "fixture")):
                self.error("events_removal_ambiguous", h.removed)

    def test_replaced_parent_and_child(self):
        for target in ("parent", "manager"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as root:
                h, parent, manager, _ = self.hierarchy(root)
                path = parent if target == "parent" else manager
                path.rename(str(path) + ".old")
                path.mkdir()
                self.error(target + "_replaced", h.removed, "FAIL")

    def test_uncertain_or_replaced_ancestor(self):
        with tempfile.TemporaryDirectory() as root:
            base = Path(root) / "ancestor"
            base.mkdir()
            h, parent, manager, _ = self.hierarchy(base)
            base.rename(str(base) + ".old")
            base.mkdir()
            self.error("ancestor_replaced", h.removed, "FAIL")
            base.rmdir()
            with self.assertRaises(FileNotFoundError):
                h.removed()

    def run_supervisor(self, record=True, exit_status=0, removed=True, state=None, capture_error=None, raw_output=None):
        class Stdout(io.StringIO):
            def __init__(self):
                super().__init__()
                self.buffer = io.BytesIO()
        output = Stdout()
        with tempfile.TemporaryFile() as stream:
            if raw_output is not None:
                stream.write(raw_output)
            elif record:
                stream.write((json.dumps(RECORD) + "\n").encode())
            stream.seek(0)
            process = unittest.mock.Mock(stdout=stream, stdin=io.BytesIO(), returncode=exit_status)
            process.communicate.return_value = (b"", None)
            hierarchy = unittest.mock.Mock()
            hierarchy.capture.return_value = {"ancestor": [1, 2, "3"], "parent": [1, 4, "3"], "manager": [1, 5, "3"]}
            hierarchy.capture.side_effect = capture_error
            clock = [0]
            def removal():
                clock[0] = 6
                return removed
            hierarchy.removed.side_effect = removal
            end = state or dict(STATE, ActiveState="inactive", MainPID="0")
            with patch.object(m.selectors, "DefaultSelector", m.selectors.SelectSelector), patch.object(m.uuid, "uuid4", return_value=unittest.mock.Mock(hex="fixture")), patch.object(m.subprocess, "Popen", return_value=process), patch.object(m, "Hierarchy", return_value=hierarchy), patch.object(m, "show", side_effect=[STATE, STATE, end, end]), patch.object(m.time, "monotonic", side_effect=lambda: clock[0]), contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
                code = m.supervise("/fixture/binary")
            return code, json.loads(output.getvalue())

    def test_supervised_pass_inactive_and_collected(self):
        for state, termination in ((None, "inactive_awaiting_collection"), (dict(STATE, LoadState="not-found", ActiveState="inactive", MainPID="0", InvocationID="", ControlGroup=""), "collected")):
            code, r = self.run_supervisor(state=state)
            self.assertEqual((code, r["status"], r["reason"]), (0, "PASS", "cleanup_verified"))
            self.assertEqual(r["evidence"]["termination"], termination)
            self.assertEqual(r["evidence"]["removal"], "retained_events_ENODEV_and_bound_absence")
            self.assertEqual(r["evidence"]["invocation"], INV)

    def test_cleanup_timeout(self):
        code, r = self.run_supervisor(removed=False)
        self.assertEqual((code, r["status"], r["reason"]), (1, "UNVERIFIED", "cleanup_timeout"))
        self.assertNotIn("removal", r["evidence"])

    def test_missing_output_and_diagnostic_failure(self):
        for status in (0, 37):
            code, r = self.run_supervisor(record=False, exit_status=status)
            self.assertEqual((code, r["status"], r["reason"], r["diagnostic_exit"]), (status or 1, "UNVERIFIED", "missing_manager_record", status))
            self.assertEqual(r["evidence"], {})

    def test_malformed_manager_json(self):
        code, r = self.run_supervisor(raw_output=b"{bad json}\n")
        self.assertEqual((code, r["status"], r["reason"]), (1, "UNVERIFIED", "observer_error"))
        self.assertEqual(r["evidence"], {})

    def test_duplicate_manager_record(self):
        code, r = self.run_supervisor(raw_output=((json.dumps(RECORD) + "\n") * 2).encode())
        self.assertEqual((code, r["status"], r["reason"]), (1, "UNVERIFIED", "duplicate_manager_record"))
        self.assertEqual(r["evidence"], {})

    def test_partial_trailing_record(self):
        code, r = self.run_supervisor(raw_output=(json.dumps(RECORD) + "\n{partial").encode())
        self.assertEqual((code, r["status"], r["reason"]), (1, "UNVERIFIED", "unexpected_trailing_output"))
        self.assertEqual(r["evidence"], {})

    def test_diagnostic_failure_cannot_pass(self):
        code, r = self.run_supervisor(exit_status=37)
        self.assertEqual((code, r["status"], r["reason"]), (37, "FAIL", "diagnostic_failed"))

    def test_absence_without_identity_never_passes(self):
        code, r = self.run_supervisor(capture_error=m.EvidenceError("ancestor_uncertain"))
        self.assertEqual((code, r["status"], r["reason"]), (1, "UNVERIFIED", "ancestor_uncertain"))
        self.assertNotIn("removal", r["evidence"])


if __name__ == "__main__":
    unittest.main()
