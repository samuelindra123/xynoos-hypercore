#!/usr/bin/env python3
"""Destructive dpkg tests: execute ONLY in an explicitly marked disposable container."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import io

PKG = "xynoos-hypercore"
RESULTS = {}


def command(*args, ok=True, env=None):
    result = subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            env=env, timeout=180)
    print("$", " ".join(map(str, args)), "\n", result.stdout, flush=True)
    if ok and result.returncode:
        raise AssertionError(f"Command failed ({result.returncode}): {args}")
    return result


def field(deb, key):
    return command("dpkg-deb", "-f", str(deb), key).stdout.strip()


def check(name, fn):
    try:
        fn()
    except Exception as exc:
        RESULTS[name] = {"status": "FAIL", "detail": str(exc)}
        raise
    RESULTS[name] = {"status": "PASS"}


def require(value, message):
    if not value:
        raise AssertionError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("deb", type=Path)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    require(os.geteuid() == 0 and Path("/.dockerenv").exists()
            and Path("/run/xynoos-package-test").is_file()
            and os.environ.get("XYNOOS_DISPOSABLE_TEST") == "1",
            "Refusing: use the documented disposable Docker command")
    require('ID=debian' in Path("/etc/os-release").read_text(), "Debian container required")
    deb = args.deb.resolve()
    args.report.parent.mkdir(parents=True, exist_ok=True)
    try:
        require(field(deb, "Package") == PKG, "Unexpected package")
        require(field(deb, "Architecture") == "amd64", "Unexpected architecture")
        version = field(deb, "Version")
        depends = field(deb, "Depends")
        require(depends and "${" not in depends, "Unexpanded/missing dependencies")
        for dep in depends.split(","):
            require(dep.strip().split()[0] in {"libc6", "libgcc-s1"}, "Unexpected runtime dependency")
        data = subprocess.check_output(["dpkg-deb", "--fsys-tarfile", str(deb)])
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            files = []
            for member in archive:
                require(member.uid == 0 and member.gid == 0, "Foreign ownership")
                require(not member.mode & 0o7022, "Unsafe permissions")
                if not member.isfile():
                    require(member.isdir(), "Unexpected link or special file")
                    continue
                path = member.name.removeprefix("./")
                require(path == "usr/bin/xynoos" or path.startswith("usr/share/doc/" + PKG + "/")
                        or path == "usr/share/man/man1/xynoos.1.gz"
                        or path == "usr/share/lintian/overrides/xynoos-hypercore", "Unexpected installed path: " + path)
                require(member.mode == (0o755 if path == "usr/bin/xynoos" else 0o644), "Wrong mode")
                files.append(path)
        with tempfile.TemporaryDirectory(prefix="xynoos-lifecycle-") as directory:
            root = Path(directory)
            control = root / "control"
            command("dpkg-deb", "-e", str(deb), str(control))
            require({p.name for p in control.iterdir()} <= {"control", "md5sums"},
                    "Unexpected maintainer script, trigger or conffiles")
            RESULTS["metadata_ownership_permissions"] = {"status": "PASS", "depends": depends, "files": files}
            home = root / "home"
            state = home / ".local/state/xynoos"
            config = home / ".config/xynoos"
            state.mkdir(parents=True)
            config.mkdir(parents=True)
            for path in (home / "unrelated.txt", state / "evidence.json", config / "operator.conf"):
                path.write_text("operator-owned preserve\n")
            preserved = {p: p.read_bytes() for p in home.rglob("*") if p.is_file()}
            env = dict(os.environ, HOME=str(home), XDG_STATE_HOME=str(home / ".local/state"),
                       XDG_CONFIG_HOME=str(home / ".config"), PATH="/no-external-commands")

            def unchanged():
                require({p: p.read_bytes() for p in home.rglob("*") if p.is_file()} == preserved,
                        "User data changed")

            def install(package=deb, downgrade=False):
                opts = ["--allow-downgrades"] if downgrade else []
                command("apt-get", "install", "-y", "--no-install-recommends", *opts, str(package))
                require(command("dpkg-query", "-W", "-f=${Status}", PKG).stdout == "install ok installed", "Not configured")

            check("clean_install_dependency_resolution", install)
            installed = [Path("/") / p for p in files]
            RESULTS["footprint"] = {"regular_file_bytes": sum(p.stat().st_size for p in installed),
                                    "regular_file_allocated_bytes": sum(p.stat().st_blocks * 512 for p in installed),
                                    "scope": "package regular files; excludes directories and dpkg database"}

            def execution():
                for argv in (["--version", "--json"], ["help", "--json"], ["doctor", "--json"],
                             ["status", "--json"], ["capabilities", "--json"], ["resources", "--json"], ["benchmark", "--json"],
                             ["system", "snapshot", "--json"], ["memory", "--interval-ms", "100"],
                             ["diagnose", "--count", "2", "--interval-ms", "100", "--json"]):
                    # Execute as nobody, with the test HOME inaccessible; stdout must still work.
                    v = json.loads(command("/usr/sbin/runuser", "--preserve-environment", "-u", "nobody", "--", "/usr/bin/xynoos", *argv, env=env).stdout)
                    require(v["schema_version"] == 1, "Wrong JSON schema")
                    if argv[0] == "--version":
                        installed = Path("/usr/share/doc/xynoos-hypercore/source.json")
                        source = json.loads(installed.read_text())
                        require(v["source_commit"] == source["commit"] and v["version"] == source["upstream_version"], "Identity mismatch")
                    if argv[0] == "diagnose":
                        require("evidence_save_error" not in v, "Diagnostic tried to save")
                    if argv[0] == "doctor":
                        require(v["installation"]["status"] == "DEBIAN PACKAGE", "Wrong health reporting")
                for action in ("update", "rollback", "uninstall"):
                    for extra in ([], ["--json"]):
                        result = command("/usr/bin/xynoos", action, *extra, ok=False, env=env)
                        require(result.returncode != 0 and "dpkg/APT" in result.stdout, "Manager not blocked")
                unchanged()
            check("unprivileged_execution_cli_and_manager_boundary", execution)
            check("package_file_ownership", lambda: require(
                command("dpkg-query", "-S", "/usr/bin/xynoos").stdout.strip() == PKG + ": /usr/bin/xynoos", "Wrong owner"))

            def integrity():
                require(not command("dpkg", "--verify", PKG).stdout.strip(), "Integrity failed")
                binary = Path("/usr/bin/xynoos")
                original = binary.read_bytes()
                binary.write_bytes(original + b"integrity-test")
                require(command("dpkg", "--verify", PKG).stdout.strip(), "Tampering not detected")
                command("apt-get", "install", "-y", "--reinstall", str(deb))
                require(binary.read_bytes() == original, "Reinstallation failed to restore")
            check("integrity_tamper_detection_and_reinstall", integrity)

            def fixture(name, new_version=None, new_depends=None):
                path = root / name
                command("dpkg-deb", "-R", str(deb), str(path))
                control_path = path / "DEBIAN/control"
                lines = control_path.read_text().splitlines()
                values = {"Version": new_version, "Depends": new_depends}
                control_path.write_text("\n".join(
                    f"{line.split(':')[0]}: {values[line.split(':')[0]]}"
                    if line.split(":")[0] in values and values[line.split(":")[0]] is not None else line
                    for line in lines) + "\n")
                output = root / (name + ".deb")
                command("dpkg-deb", "--root-owner-group", "-b", str(path), str(output))
                return output

            older = fixture("older-fixture", new_version=version + "~lifecycle1")

            def versions():
                # Same-payload older revision fixture, not a historical upstream release.
                require(command("apt-get", "install", "-y", str(older), ok=False).returncode != 0,
                        "Unapproved downgrade succeeded")
                require(command("dpkg-query", "-W", "-f=${Version}", PKG).stdout == version, "Downgrade changed package")
                install(older, downgrade=True)
                require(command("dpkg-query", "-W", "-f=${Version}", PKG).stdout == version + "~lifecycle1", "Wrong older version")
                install()
                require(command("dpkg-query", "-W", "-f=${Version}", PKG).stdout == version, "Upgrade failed")
                unchanged()
            check("fixture_downgrade_refusal_explicit_downgrade_upgrade", versions)

            def removal():
                command("apt-get", "remove", "-y", PKG)
                require(not Path("/usr/bin/xynoos").exists(), "Binary survived remove")
                unchanged()
                install()
                command("apt-get", "purge", "-y", PKG)
                require(not Path("/usr/bin/xynoos").exists(), "Binary survived purge")
                unchanged()
            check("remove_reinstall_purge_preserves_data", removal)

            def interruption():
                # Deterministic interruption boundary, no random kill and no host fault injection.
                command("dpkg", "--unpack", str(deb))
                require(command("dpkg-query", "-W", "-f=${Status}", PKG).stdout == "install ok unpacked", "Wrong intermediate state")
                command("dpkg", "--configure", "-a")
                require(not command("dpkg", "--audit").stdout.strip(), "dpkg remains interrupted")
                unchanged()
                command("apt-get", "purge", "-y", PKG)
            check("unpack_configure_recovery", interruption)

            broken = fixture("missing-dependency", new_depends="xynoos-nonexistent-test-dependency (= 1)")

            def dependency_failure():
                require(command("apt-get", "install", "-y", str(broken), ok=False).returncode != 0,
                        "APT accepted missing dependency")
                require(command("dpkg", "-i", str(broken), ok=False).returncode != 0,
                        "dpkg configured missing dependency")
                require(command("dpkg-query", "-W", "-f=${Status}", PKG).stdout == "install ok unpacked", "Wrong dependency-failure state")
                command("dpkg", "--remove", PKG)
                install()
                require(not command("dpkg", "--verify", PKG).stdout.strip(), "Restored integrity failed")
                unchanged()
                command("apt-get", "purge", "-y", PKG)
                require(not command("dpkg", "--audit").stdout.strip(), "Final dpkg audit failed")
            check("dependency_failure_and_recovery", dependency_failure)
        RESULTS["size"] = {"deb_bytes": deb.stat().st_size,
                           "installed_kib": int(field(deb, "Installed-Size")),
                           "sha256": hashlib.sha256(deb.read_bytes()).hexdigest()}
    finally:
        args.report.write_text(json.dumps(RESULTS, indent=2) + "\n")


if __name__ == "__main__":
    main()
