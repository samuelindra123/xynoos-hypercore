# TASK 018E-1A — MX Linux live binding handoff

**LIVE SYSTEMD BINDING: NOT VERIFIED.** The previous physical attempt failed in D-Bus decoding before binding could be verified. This package contains the parsing and cleanup-reporting fixes. Run only on the N5000 laptop from a normal graphical login with its user systemd and D-Bus session. This diagnostic starts one temporary user service. It does not authorize or perform controller activation, memory.low writes, process migration, workload stress, or controlled mode. Do not use sudo or modify cgroups manually.

Requirements: x86_64 Linux, unified cgroup v2 visible in the user's namespace, systemd 254 or later with DelegateSubgroup support, user D-Bus, `systemd-run`, `systemctl`, `busctl`, `python3`, `realpath`, and `sha256sum`. The static musl executable avoids a host glibc requirement. Unsupported configurations must fail; do not change machine configuration to force a pass.

From the directory containing the new archive, extract it, then run from the extracted directory in a shell without `set -e` (so the diagnostic status and logs are retained):

```sh
tar -xzf xynoos-018e-1a-dbus-fix-mx-linux-x86_64.tar.gz
cd xynoos-018e-1a-dbus-fix
```

Then run:

```sh
sha256sum -c SHA256SUMS
cat SOURCE_COMMIT
systemctl --user --version
busctl --user status >/dev/null
./xynoos-018d-launcher.sh preflight
./xynoos-018d-launcher.sh unit-preflight > live-binding.stdout 2> live-binding.stderr
result=$?
cat live-binding.stdout
cat live-binding.stderr
printf 'exit status: %s\n' "$result"
```

A pass requires exit status 0, a `manager_preflight` JSON record with `binding_revalidation` equal to `passed`, matching unit ID and object, nonzero invocation ID, active/loaded transient service, `delegate: true`, `delegate_subgroup: manager`, matching MainPID and manager membership, distinct parent/manager descriptor inode identities, and the messages `unit inactive or collected` and `cgroup removal verified`. Preserve both output files and the source commit for review. The launcher prints the transient unit name before launch and checks termination even if the diagnostic fails. It verifies cgroup absence after service completion, within five seconds, when a trustworthy diagnostic path is available; otherwise cleanup is UNVERIFIED. It preserves a nonzero diagnostic exit status. If it reports a failure or cleanup unverified, record the unit name and inspect `systemctl --user status UNIT` and `journalctl --user -u UNIT`; do not delete cgroups by hand. Missing user bus, unsupported properties, a service exit, changed identities and mismatches are failures, not passes.

Observations are sequential and can change between checks; a passing diagnostic is not authorization for kernel mutation. Actual physical results must be reviewed before updating the status.
