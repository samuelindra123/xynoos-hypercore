#!/usr/bin/env python3
"""Read-only observer. No cgroup writes/deletions; see TASK-018E-CLEANUP.md."""
import ctypes
import errno
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import time
import uuid


class EvidenceError(Exception):
    def __init__(self, reason, status="UNVERIFIED"):
        self.reason, self.status = reason, status
        super().__init__(reason)


def require(ok, reason, status="UNVERIFIED"):
    if not ok:
        raise EvidenceError(reason, status)


PROPERTIES = ("Id", "LoadState", "ActiveState", "InvocationID", "MainPID",
              "ControlGroup", "Transient")


def parse_state(text):
    state = {}
    for line in text.splitlines():
        key, sep, value = line.partition("=")
        require(sep and key in PROPERTIES and key not in state, "malformed_unit_information")
        state[key] = value
    require(set(state) == set(PROPERTIES), "missing_unit_information")
    require(state["MainPID"].isdigit(), "malformed_main_pid")
    invocation = state["InvocationID"]
    require(not invocation or re.fullmatch(r"[0-9a-f]{32}", invocation), "malformed_invocation")
    return state


def show(unit):
    try:
        p = subprocess.run(["systemctl", "--user", "show", unit,
                            "--property=" + ",".join(PROPERTIES)],
                           capture_output=True, text=True, timeout=2)
    except (OSError, subprocess.TimeoutExpired) as e:
        raise EvidenceError("unit_information_unavailable") from e
    require(p.returncode == 0, "unit_information_unavailable")
    return parse_state(p.stdout)


def terminal(state, original):
    require(state["Id"] == original["Id"], "unit_name_reuse", "FAIL")
    invocation = state["InvocationID"]
    require(not invocation or invocation == original["InvocationID"], "invocation_mismatch", "FAIL")
    load, active = state["LoadState"], state["ActiveState"]
    if load == "not-found":
        require(active == "inactive" and state["MainPID"] == "0"
                and not invocation and not state["ControlGroup"], "contradictory_collected_state", "FAIL")
        return "collected"
    require(load == "loaded", "unsupported_load_state")
    require(invocation == original["InvocationID"] and state["Transient"] == "yes",
            "inactive_identity_unavailable")
    require(state["ControlGroup"] in ("", original["ControlGroup"]), "unit_cgroup_changed", "FAIL")
    require(active == "inactive" and state["MainPID"] == "0", "service_not_inactive", "FAIL")
    return "inactive_awaiting_collection"


def identity(fd):
    st = os.fstat(fd)
    with open(f"/proc/self/fdinfo/{fd}", encoding="ascii") as stream:
        mounts = [line.split()[1] for line in stream if line.startswith("mnt_id:")]
    require(len(mounts) == 1, "mount_identity_unavailable")
    return (st.st_dev, st.st_ino, mounts[0])


def cgroupfs(fd):
    # An oversized aligned buffer avoids depending on libc's statfs field layout.
    buffer = (ctypes.c_long * 64)()
    libc = ctypes.CDLL(None, use_errno=True)
    require(libc.fstatfs(fd, ctypes.byref(buffer)) == 0, "filesystem_identity_unavailable")
    require(buffer[0] == 0x63677270, "not_cgroup_v2")


class Hierarchy:
    def __init__(self):
        self.fds = []
        self.chain = []

    def open(self, name, parent=None, directory=True):
        flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW
        if directory:
            flags |= os.O_DIRECTORY
        fd = os.open(name, flags, dir_fd=parent)
        self.fds.append(fd)
        return fd

    def capture(self, record, unit):
        b = record["binding"]
        path = record["parent_path"]
        require(isinstance(path, str) and path.startswith("/") and
                all(p not in ("", ".", "..") for p in path.split("/")[1:]) and
                Path(path).name == unit, "unexpected_parent_path")
        require(record["manager_path"] == path + "/manager", "unexpected_manager_path")
        fd = self.open("/")
        self.chain.append((None, "/", fd, identity(fd)))
        for component in path.split("/")[1:-1]:
            child = self.open(component, fd)
            self.chain.append((fd, component, child, identity(child)))
            fd = child
        cgroupfs(fd)
        self.ancestor = fd
        self.name = unit
        self.parent = self.open(unit, fd)
        self.manager = self.open("manager", self.parent)
        self.ids = {}
        self.events = {}
        for name, directory in (("parent", self.parent), ("manager", self.manager)):
            cgroupfs(directory)
            ident = identity(directory)
            require(list(ident[:2]) == [b[name + "_descriptor"][k] for k in ("device", "inode")],
                    name + "_identity_mismatch", "FAIL")
            self.ids[name] = ident
            events = self.open("cgroup.events", directory, False)
            require(b"populated " in os.pread(events, 4096, 0), "events_unreadable")
            self.events[name] = events
        require(self.ids["parent"] != self.ids["manager"], "duplicate_cgroup_identity", "FAIL")
        require(self.ids["parent"][2] == self.ids["manager"][2] == identity(self.ancestor)[2],
                "mount_identity_mismatch")
        self.check_chain()
        return {"ancestor": list(identity(self.ancestor)),
                "parent": list(self.ids["parent"]), "manager": list(self.ids["manager"])}

    def check_chain(self):
        for parent, name, fd, expected in self.chain:
            current = self.open(name, parent)
            try:
                require(identity(fd) == expected and identity(current) == expected,
                        "ancestor_replaced", "FAIL")
            finally:
                os.close(current)
                self.fds.remove(current)

    def removed(self):
        self.check_chain()
        present = False
        for name, parent, entry in (("parent", self.ancestor, self.name),
                                    ("manager", self.parent, "manager")):
            try:
                fd = self.open(entry, parent)
            except OSError as e:
                require(e.errno in (errno.ENOENT, errno.ENODEV), "cgroup_lookup_ambiguous")
            else:
                try:
                    require(identity(fd) == self.ids[name], name + "_replaced", "FAIL")
                finally:
                    os.close(fd)
                    self.fds.remove(fd)
                present = True
        if present:
            return False
        # ENODEV from a fresh offset-zero read of each original cgroup.events
        # is required. EOF, ENOENT, nlink and pathname absence are not substitutes.
        for name in ("parent", "manager"):
            require(identity(getattr(self, name)) == self.ids[name], "held_identity_changed", "FAIL")
            try:
                os.pread(self.events[name], 4096, 0)
            except OSError as e:
                require(e.errno == errno.ENODEV, "events_removal_ambiguous")
            else:
                return False
        self.check_chain()
        return True

    def close(self):
        for fd in reversed(self.fds):
            os.close(fd)
        self.fds.clear()


def capture_unit(record, unit, state):
    b = record["binding"]
    require(record["record"] == "manager_preflight" and record["unit"] == unit
            and record["binding_revalidation"] == "passed", "invalid_manager_record")
    require(b["unit_id"] == unit and state["Id"] == unit, "unit_name_reuse", "FAIL")
    invocation = b["invocation_id"]
    require(isinstance(invocation, str) and re.fullmatch(r"[0-9a-f]{32}", invocation)
            and invocation != "0" * 32, "missing_invocation_identity")
    require(state["InvocationID"] == invocation, "invocation_mismatch", "FAIL")
    require(state["LoadState"] == "loaded" and state["ActiveState"] == "active"
            and state["Transient"] == "yes", "snapshot_not_active", "FAIL")
    require(state["MainPID"] == str(record["manager_pid"]) == str(b["main_pid"])
            and int(state["MainPID"]) > 0, "main_pid_mismatch", "FAIL")
    require(state["ControlGroup"] == b["control_group"] and
            b["actual_membership"] == state["ControlGroup"] + "/manager", "membership_mismatch", "FAIL")


def supervise(binary):
    unit = "xynoos-018d-" + uuid.uuid4().hex + ".service"
    print("transient unit: " + unit, file=sys.stderr, flush=True)
    result = {"record": "unit_cleanup", "unit": unit, "status": "UNVERIFIED", "evidence": {}}
    hierarchy, process = Hierarchy(), None
    status = None
    try:
        process = subprocess.Popen([
            "systemd-run", "--user", "--wait", "--pipe", "--collect", "--unit=" + unit,
            "--property=Type=exec", "--property=Restart=no", "--property=RuntimeMaxSec=20s",
            "--property=Delegate=cpu memory pids", "--property=DelegateSubgroup=manager",
            "--setenv=XYNOOS_CLEANUP_HANDSHAKE=1", binary, "manager-preflight", unit],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        record, data = None, b""
        deadline = time.monotonic() + 10
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while time.monotonic() < deadline and record is None:
                if not selector.select(max(0, deadline - time.monotonic())):
                    break
                chunk = os.read(process.stdout.fileno(), 65536)
                if not chunk:
                    break
                sys.stdout.buffer.write(chunk)
                sys.stdout.buffer.flush()
                data += chunk
                require(len(data) <= 1024 * 1024, "diagnostic_output_excessive")
                while b"\n" in data:
                    line, data = data.split(b"\n", 1)
                    if line.startswith(b"{"):
                        parsed = json.loads(line)
                        if isinstance(parsed, dict) and parsed.get("record") == "manager_preflight":
                            require(record is None, "duplicate_manager_record")
                            record = parsed
        require(record is not None, "missing_manager_record")
        require(not data.strip(), "unexpected_trailing_output")
        original = show(unit)
        capture_unit(record, unit, original)
        result["evidence"]["invocation"] = original["InvocationID"]
        result["evidence"]["hierarchy"] = hierarchy.capture(record, unit)
        require(show(unit) == original, "snapshot_changed", "FAIL")
        result["evidence"]["snapshot"] = original
        process.stdin.write(b"1")
        process.stdin.flush()
        process.stdin.close()
        process.stdin = None
        tail, _ = process.communicate(timeout=25)
        sys.stdout.buffer.write(tail)
        sys.stdout.buffer.flush()
        # Any additional JSON manager record makes the diagnostic ambiguous.
        for line in tail.splitlines():
            if line.startswith(b"{"):
                parsed = json.loads(line)
                require(not isinstance(parsed, dict) or parsed.get("record") != "manager_preflight",
                        "duplicate_manager_record")
        status = process.returncode
        deadline = time.monotonic() + 5
        while True:
            end = show(unit)
            result["evidence"]["termination"] = terminal(end, original)
            if hierarchy.removed():
                require(show(unit) == end, "terminal_snapshot_changed", "FAIL")
                result["evidence"]["removal"] = "retained_events_ENODEV_and_bound_absence"
                result["status"] = "PASS" if status == 0 else "FAIL"
                result["reason"] = "cleanup_verified" if status == 0 else "diagnostic_failed"
                break
            require(time.monotonic() < deadline, "cleanup_timeout")
            time.sleep(0.1)
    except EvidenceError as e:
        result.update(status=e.status, reason=e.reason)
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired) as e:
        result.update(status="UNVERIFIED", reason="observer_error", detail=str(e))
    finally:
        if process is not None and status is None:
            # EOF rejects handshake; never stop/kill the service or delete cgroups.
            if process.stdin is not None:
                try:
                    process.stdin.close()
                except OSError:
                    pass
                process.stdin = None
            try:
                tail, _ = process.communicate(timeout=25)
                sys.stdout.buffer.write(tail)
                status = process.returncode
            except (OSError, subprocess.TimeoutExpired):
                result["evidence"]["runner"] = "wait_failed_service_state_unverified"
        hierarchy.close()
    result["diagnostic_exit"] = status
    print(json.dumps(result), flush=True)
    return status if status is not None and status > 0 else (0 if result["status"] == "PASS" else 1)


if __name__ == "__main__":
    sys.exit(supervise(sys.argv[1]))
