#!/usr/bin/env python3
"""Xynoos HyperCore rootless distribution manager. Python 3.9+, OpenSSL 3.

Trust anchor: Ed25519 release signing key below. No metadata becomes shell code.
Artifacts are raw ELF files: there is no archive extraction or executable metadata.
The same source is embedded in the Rust CLI for update/rollback/uninstall.
"""
import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import ssl
import stat
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

REPO = "samuelindra123/xynoos-hypercore"
TARGET = "x86_64-unknown-linux-musl"
PUBLIC_KEY = b"""-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAJfGz70mxvEUjhJupDAMfEtcGSbOAE1KEX3Lqe4yuwLA=
-----END PUBLIC KEY-----
"""
VERSION_RE = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-preview\.([1-9][0-9]*))?$")
MAX_BINARY = 32 * 1024 * 1024
MAX_METADATA = 1024 * 1024


class Refusal(RuntimeError):
    pass


def require(condition, message):
    if not condition:
        raise Refusal(message)


def version_key(version):
    require(isinstance(version, str), "Invalid version type")
    m = VERSION_RE.fullmatch(version)
    require(m is not None, "Unsupported release version: " + version)
    a, b, c, preview = m.groups()
    return (int(a), int(b), int(c), 1 if preview is None else 0, int(preview or 0))


def canonical_version(version):
    version = version if version.startswith("v") else "v" + version
    version_key(version)
    return version


def strict_json(data):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "Duplicate metadata key")
            result[key] = value
        return result
    require(len(data) <= MAX_METADATA, "Metadata exceeds limit")
    try:
        return json.loads(data, object_pairs_hook=pairs, parse_constant=lambda _: (_ for _ in ()).throw(Refusal("Non-finite JSON")))
    except (ValueError, UnicodeError) as e:
        raise Refusal("Invalid JSON metadata") from e


def check_platform():
    require(platform.system() == "Linux", "Supported target: Linux x86_64 only")
    require(platform.machine() == "x86_64", "No verified release for CPU architecture " + platform.machine())
    require(shutil.which("openssl") is not None, "OpenSSL 3 is required for Ed25519 verification")


class Redirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        url = urllib.parse.urlsplit(newurl)
        require(url.scheme == "https" and url.hostname in {"github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"}
                and url.username is None and url.password is None and url.port in (None, 443), "Untrusted artifact redirect")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def download(url, limit):
    parsed = urllib.parse.urlsplit(url)
    require(parsed.scheme == "https" and parsed.hostname in {"api.github.com", "github.com"}
            and parsed.username is None and parsed.password is None and parsed.port in (None, 443), "Untrusted download origin")
    opener = urllib.request.build_opener(Redirects(), urllib.request.HTTPSHandler(context=ssl.create_default_context()))
    request = urllib.request.Request(url, headers={"User-Agent": "Xynoos-HyperCore-installer/1", "Accept": "application/vnd.github+json" if parsed.hostname == "api.github.com" else "application/octet-stream"})
    try:
        with opener.open(request, timeout=30) as response:
            require(response.status == 200, "Download did not return HTTP 200")
            size = response.headers.get("Content-Length")
            if size is not None:
                require(size.isdigit() and int(size) <= limit, "Download exceeds allowed size")
            chunks = []
            total = 0
            deadline = time.monotonic() + 120
            while True:
                require(time.monotonic() <= deadline, "Download exceeded 120 second deadline")
                chunk = response.read(min(65536, limit + 1 - total))
                if not chunk:
                    break
                chunks.append(chunk)
                total += len(chunk)
                require(total <= limit, "Download exceeds allowed size")
            if size is not None:
                require(total == int(size), "Partial download refused")
            return b"".join(chunks)
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        raise Refusal("Download failed: " + str(e)) from e


def discover(channel, pinned=None):
    releases = []
    # Explicitly paginate; never use /releases/latest (it excludes previews).
    for page in range(1, 11):
        batch = strict_json(download(f"https://api.github.com/repos/{REPO}/releases?per_page=100&page={page}", MAX_METADATA))
        require(isinstance(batch, list), "Invalid release list")
        releases.extend(batch)
        if len(batch) < 100:
            break
    else:
        raise Refusal("Release discovery exceeded bound; use --version")
    eligible = []
    for r in releases:
        if not isinstance(r, dict) or r.get("draft") is not False:
            continue
        tag = r.get("tag_name", "")
        if not isinstance(tag, str) or not VERSION_RE.fullmatch(tag):
            continue
        preview = "-preview." in tag
        if bool(r.get("prerelease")) != preview or preview != (channel == "preview"):
            continue
        if pinned and tag != pinned:
            continue
        assets = r.get("assets", [])
        required = {"manifest.json", "manifest.json.sig", "xynoos-" + TARGET}
        names = [a.get("name") for a in assets if isinstance(a, dict)]
        if not required.issubset(names):
            continue
        require(all(names.count(n) == 1 for n in required), "Duplicate release assets")
        # URLs are constructed from a validated version, never trusted from API metadata.
        eligible.append(tag)
    require(eligible, f"No compatible published {channel} release" + (f" for {pinned}" if pinned else ""))
    return max(eligible, key=version_key)


def safe_directory(path, create=False, private=False):
    path = Path(path)
    require(path.is_absolute() and ".." not in path.parts, "Installation paths must be absolute without traversal")
    current = Path(path.anchor)
    for part in path.parts[1:]:
        current /= part
        try:
            st = current.lstat()
        except FileNotFoundError:
            require(create, "Missing directory: " + str(current))
            current.mkdir(mode=0o700)
            st = current.lstat()
        require(stat.S_ISDIR(st.st_mode), "Symlink or non-directory refused: " + str(current))
        require(st.st_uid in (0, os.getuid()), "Directory has unexpected owner: " + str(current))
        # Shared sticky ancestors such as /tmp are allowed; private final dirs are not.
        require(not st.st_mode & 0o022 or (st.st_uid == 0 and st.st_mode & stat.S_ISVTX), "Writable directory refused: " + str(current))
    if private:
        st = path.lstat()
        require(st.st_uid == os.getuid() and st.st_mode & 0o077 == 0, "Xynoos directory must be owned by you with mode 0700: " + str(path))
    return path


def regular(path, max_size=MAX_METADATA, executable=False):
    st = path.lstat()
    require(stat.S_ISREG(st.st_mode) and st.st_uid == os.getuid() and st.st_nlink == 1
            and not st.st_mode & 0o022 and st.st_size <= max_size, "Unsafe owned file: " + str(path))
    if executable:
        require(st.st_mode & 0o111 != 0, "Binary is not executable")
    return path.read_bytes()


def sync_dir(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def write_new(path, data, mode=0o600):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
    with os.fdopen(fd, "wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def atomic_json(path, obj):
    data = (json.dumps(obj, sort_keys=True) + "\n").encode()
    fd, name = tempfile.mkstemp(prefix=".write-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        if path.exists() or path.is_symlink():
            regular(path)
        os.replace(name, path)
        sync_dir(path.parent)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def verify_signature(manifest, signature, directory):
    require(len(signature) == 64, "Invalid Ed25519 signature size")
    with tempfile.TemporaryDirectory(prefix=".verify-", dir=directory) as temp:
        temp = Path(temp)
        for name, data in (("key", PUBLIC_KEY), ("manifest", manifest), ("signature", signature)):
            write_new(temp / name, data)
        result = subprocess.run(["openssl", "pkeyutl", "-verify", "-pubin", "-inkey", str(temp / "key"),
                                 "-rawin", "-in", str(temp / "manifest"), "-sigfile", str(temp / "signature")],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        require(result.returncode == 0, "Release signature verification FAILED; no binary executed")


def validate_manifest(m, tag):
    require(isinstance(m, dict) and set(m) == {"schema", "repository", "version", "channel", "source_commit", "target", "binary", "provenance"}, "Invalid manifest schema")
    require(m["schema"] == 1 and m["repository"] == REPO and m["version"] == tag and m["target"] == TARGET, "Release identity mismatch")
    require(m["channel"] == ("preview" if "-preview." in tag else "stable"), "Release channel mismatch")
    require(isinstance(m["source_commit"], str) and re.fullmatch("[0-9a-f]{40}", m["source_commit"]), "Invalid source identity")
    b = m["binary"]
    require(isinstance(b, dict) and set(b) == {"name", "size", "sha256"} and b["name"] == "xynoos-" + TARGET, "Invalid binary name")
    require(type(b["size"]) is int and 64 <= b["size"] <= MAX_BINARY and isinstance(b["sha256"], str) and re.fullmatch("[0-9a-f]{64}", b["sha256"]), "Invalid binary digest or size")
    p = m["provenance"]
    require(isinstance(p, dict) and set(p) == {"workflow", "run_id", "source"}, "Missing build provenance")
    require(p["workflow"] == ".github/workflows/release.yml" and isinstance(p["run_id"], str) and p["run_id"].isdigit()
            and p["source"] == f"https://github.com/{REPO}/commit/{m['source_commit']}", "Invalid build provenance identity")
    return m


def verify_binary(data, m):
    require(len(data) == m["binary"]["size"] and hashlib.sha256(data).hexdigest() == m["binary"]["sha256"], "Binary checksum/size mismatch")
    require(data[:6] == b"\x7fELF\x02\x01" and data[18:20] == b"\x3e\x00", "Wrong ELF platform; Linux x86_64 required")


def health_binary(path, manifest):
    result = subprocess.run([str(path), "--version", "--json"], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
    require(result.returncode == 0 and len(result.stdout) < 4096, "Verified executable failed health check")
    v = strict_json(result.stdout)
    require(v.get("version") == manifest["version"][1:] and v.get("source_commit") == manifest["source_commit"]
            and v.get("product") == "Xynoos HyperCore" and v.get("physical_kernel_mutation") is False, "Executable identity mismatch")


class Installation:
    def __init__(self):
        home = Path(os.environ.get("HOME", ""))
        require(home.is_absolute(), "HOME must be absolute")
        self.root = Path(os.environ.get("XDG_DATA_HOME", str(home / ".local/share"))) / "xynoos"
        self.state = Path(os.environ.get("XDG_STATE_HOME", str(home / ".local/state"))) / "xynoos"
        self.bin_dir = home / ".local/bin"
        require(self.root != self.state and self.bin_dir not in (self.root, self.state), "XDG locations must be distinct")
        self.entry = self.bin_dir / "xynoos"
        self.lock_file = None

    @contextlib.contextmanager
    def locked(self):
        safe_directory(self.root, create=True, private=True)
        safe_directory(self.state, create=True, private=True)
        safe_directory(self.bin_dir, create=True)
        safe_directory(self.root / "versions", create=True, private=True)
        path = self.root / "manager.lock"
        fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            st = os.fstat(fd)
            require(stat.S_ISREG(st.st_mode) and st.st_uid == os.getuid() and st.st_nlink == 1 and st.st_mode & 0o077 == 0, "Unsafe lock file")
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as e:
                raise Refusal("Another Xynoos install/update is running; retry after it finishes") from e
            self.clean_interrupted_staging()
            self.recover()
            yield
        finally:
            os.close(fd)

    def clean_interrupted_staging(self):
        # Only reserved random temporary names, under the held lock. Never follow links.
        for parent, prefix, names in ((self.root, ".verify-", {"key", "manifest", "signature"}),
                                      (self.root / "versions", ".stage-", {"xynoos", "manifest.json", "manifest.json.sig"})):
            for p in parent.iterdir():
                if re.fullmatch(re.escape(prefix) + r"[a-z0-9_]{8}", p.name):
                    safe_directory(p, private=True)
                    children = list(p.iterdir())
                    require(all(c.name in names for c in children), "Unexpected staging contents; preserved")
                    for c in children:
                        regular(c, MAX_BINARY)
                    for c in children:
                        c.unlink()
                    p.rmdir()
                    sync_dir(parent)
        for parent in (self.root, self.state):
            for p in parent.iterdir():
                if re.fullmatch(r"\.write-[a-z0-9_]{8}", p.name):
                    regular(p)
                    p.unlink()
        for p in self.root.iterdir():
            if re.fullmatch(r"\.link-[a-f0-9]{16}", p.name):
                require(p.is_symlink(), "Unsafe interrupted link")
                target = os.readlink(p)
                require(target.startswith("versions/") and target.count("/") == 1, "Unsafe interrupted link target")
                version_key(target[9:])
                p.unlink()

    def current(self):
        link = self.root / "current"
        if not link.is_symlink():
            require(not link.exists(), "Unexpected current path")
            return None
        value = os.readlink(link)
        require(value.startswith("versions/") and value.count("/") == 1, "Unsafe current link")
        version_key(value[9:])
        return value[9:]

    def switch(self, tag):
        current = self.current()
        if tag is None:
            if current:
                (self.root / "current").unlink()
        else:
            version_key(tag)
            name = self.root / (".link-" + os.urandom(8).hex())
            os.symlink("versions/" + tag, name)
            os.replace(name, self.root / "current")
        sync_dir(self.root)

    def history(self):
        p = self.root / "history.json"
        if not p.exists() and not p.is_symlink():
            return {"previous": None}
        h = strict_json(regular(p))
        require(isinstance(h, dict) and set(h) == {"previous"}, "Invalid local history")
        if h["previous"] is not None:
            version_key(h["previous"])
        return h

    def recover(self):
        p = self.root / "transaction.json"
        if not p.exists() and not p.is_symlink():
            return
        j = strict_json(regular(p))
        require(isinstance(j, dict) and set(j) == {"before", "previous"}, "Invalid recovery journal")
        for tag in j.values():
            if tag is not None:
                version_key(tag)
        if j["before"]:
            self.verify_local(j["before"])
        self.switch(j["before"])
        atomic_json(self.root / "history.json", {"previous": j["previous"]})
        p.unlink()
        sync_dir(self.root)
        progress("Recovered interrupted version switch", "PASS")

    def verify_local(self, tag):
        version_key(tag)
        directory = safe_directory(self.root / "versions" / tag, private=True)
        manifest = regular(directory / "manifest.json")
        verify_signature(manifest, regular(directory / "manifest.json.sig"), self.root)
        m = validate_manifest(strict_json(manifest), tag)
        verify_binary(regular(directory / "xynoos", MAX_BINARY, executable=True), m)
        return m

    def check_entry(self):
        if self.entry.is_symlink():
            require(os.readlink(self.entry) == str(self.root / "current/xynoos"), "Existing xynoos symlink belongs to another installation")
        else:
            require(not self.entry.exists(), "Existing xynoos executable collision; refusing replacement")

    def activate(self, tag, before):
        self.check_entry()
        previous = self.history()["previous"]
        atomic_json(self.root / "transaction.json", {"before": before, "previous": previous})
        try:
            self.switch(tag)
            health_binary(self.root / "current/xynoos", self.verify_local(tag))
            if not self.entry.is_symlink():
                os.symlink(str(self.root / "current/xynoos"), self.entry)
                sync_dir(self.bin_dir)
            atomic_json(self.root / "history.json", {"previous": before})
            (self.root / "transaction.json").unlink()
            sync_dir(self.root)
        except BaseException:
            self.recover()
            raise

    def install(self, channel, pinned=None):
        self.check_entry()
        before = self.current()
        if before:
            self.verify_local(before)
        tag = discover(channel, pinned)
        progress("Resolved release", tag)
        if before:
            require(version_key(tag) >= version_key(before), "Downgrade refused; use offline rollback for the previous verified version")
            if tag == before:
                return {"result": "Already up to date; installed signature and checksum verified", "version": tag, "path": str(self.entry)}
        base = f"https://github.com/{REPO}/releases/download/{tag}/"
        manifest = download(base + "manifest.json", MAX_METADATA)
        signature = download(base + "manifest.json.sig", 64)
        verify_signature(manifest, signature, self.root)
        m = validate_manifest(strict_json(manifest), tag)
        progress("Verified release signature", "PASS")
        data = download(base + m["binary"]["name"], MAX_BINARY)
        verify_binary(data, m)
        progress("Verified binary integrity", "PASS")
        final = self.root / "versions" / tag
        if final.exists() or final.is_symlink():
            existing = self.verify_local(tag)
            require(existing == m, "Immutable version collision")
        else:
            with tempfile.TemporaryDirectory(prefix=".stage-", dir=self.root / "versions") as temp:
                temp = Path(temp)
                write_new(temp / "manifest.json", manifest)
                write_new(temp / "manifest.json.sig", signature)
                write_new(temp / "xynoos", data, 0o700)
                health_binary(temp / "xynoos", m)
                sync_dir(temp)
                os.rename(temp, final)
                sync_dir(final.parent)
        self.activate(tag, before)
        progress("Installed verified binary", "PASS")
        return {"result": "Xynoos is ready", "version": tag, "source_commit": m["source_commit"], "path": str(self.entry), "channel": channel}

    def rollback(self):
        before = self.current()
        require(before is not None, "No installed version")
        self.verify_local(before)
        tag = self.history()["previous"]
        require(tag is not None and tag != before, "No previous verified version available offline")
        m = self.verify_local(tag)
        health_binary(self.root / "versions" / tag / "xynoos", m)
        self.activate(tag, before)
        return {"result": "Restored previous verified version offline", "version": tag, "path": str(self.entry)}

    def uninstall(self):
        self.check_entry()
        # Preflight every owned directory/file before deleting anything. Unknown files
        # are preserved and cause refusal. Never recursively follow links or delete HOME.
        paths = []
        versions = self.root / "versions"
        for p in versions.iterdir():
            version_key(p.name)
            safe_directory(p, private=True)
            require({c.name for c in p.iterdir()} == {"xynoos", "manifest.json", "manifest.json.sig"}, "Unexpected version contents; preserved")
            for c in p.iterdir():
                regular(c, MAX_BINARY)
                paths.append(c)
        allowed = {"versions", "current", "manager.lock", "history.json"}
        require(all(p.name in allowed for p in self.root.iterdir()), "Unexpected installation contents; preserved")
        self.current()
        self.history()
        if self.entry.is_symlink():
            self.entry.unlink()
            sync_dir(self.bin_dir)
        self.switch(None)
        for p in paths:
            p.unlink()
        for p in versions.iterdir():
            p.rmdir()
        if (self.root / "history.json").exists():
            (self.root / "history.json").unlink()
        sync_dir(versions)
        sync_dir(self.root)
        # Keep the lock inode and empty manager directories to avoid lock races.
        return {"result": "Uninstalled owned releases and executable; diagnostic evidence preserved", "evidence": str(self.state), "lock_retained": True}


JSON_OUTPUT = False


def progress(label, result):
    if not JSON_OUTPUT:
        print(f"  {label:<32} {result}", file=sys.stderr, flush=True)


def main(argv=None):
    global JSON_OUTPUT
    parser = argparse.ArgumentParser(description="Xynoos HyperCore verified rootless installer")
    parser.add_argument("command", choices=["install", "update", "rollback", "uninstall", "health", "record-diagnostic"])
    parser.add_argument("--channel", choices=["preview", "stable"], default="preview")
    parser.add_argument("--version")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--no-color", action="store_true")
    args = parser.parse_args(argv)
    JSON_OUTPUT = args.json
    if args.command not in ("install", "update"):
        require(args.version is None and args.channel == "preview", "Release options apply only to install/update")
    pinned = canonical_version(args.version) if args.version else None
    if pinned:
        require(("-preview." in pinned) == (args.channel == "preview"), "Pinned version does not match requested channel")
    check_platform()
    inst = Installation()
    with inst.locked():
        if args.command in ("install", "update"):
            if not args.json:
                print("\n /X/  XYNOOS HYPERCORE\n      Linux Resource Intelligence\n      Experimental Developer Preview\n", file=sys.stderr)
            progress("Checked Linux x86_64", "PASS")
            result = inst.install(args.channel, pinned)
        elif args.command == "rollback":
            result = inst.rollback()
        elif args.command == "uninstall":
            result = inst.uninstall()
        elif args.command == "record-diagnostic":
            value = strict_json(sys.stdin.buffer.read(MAX_METADATA + 1))
            atomic_json(inst.state / "last-diagnostic.json", value)
            result = {"result": "Evidence saved"}
        else:
            tag = inst.current()
            require(tag is not None, "Unmanaged binary; no verified installed release")
            inst.check_entry()
            m = inst.verify_local(tag)
            result = {"status": "AVAILABLE", "integrity": "Ed25519 signature + SHA256 VERIFIED", "version": tag,
                      "source_commit": m["source_commit"], "channel": m["channel"], "path": str(inst.entry)}
    if args.json or args.command == "health":
        print(json.dumps(result, sort_keys=True))
    else:
        print("\n" + result["result"] + ".")
        if "version" in result:
            print(result["version"] + " · " + result["path"])
        if args.command == "install":
            print("\nNext: xynoos doctor\n      xynoos status\n      xynoos update")
            if str(inst.bin_dir) not in os.environ.get("PATH", "").split(os.pathsep):
                print("\nAdd ~/.local/bin to PATH or run: " + str(inst.entry))
            print("\nTrust: pinned Ed25519 release key + SHA256. No kernel settings changed.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (Refusal, OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as error:
        print("Xynoos: " + str(error), file=sys.stderr)
        sys.exit(1)
