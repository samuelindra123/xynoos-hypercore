# XYNOOS 018E cleanup diagnostic — Antigravity MX Linux handoff

Physical success: NOT VERIFIED. This package is for later validation on the MX
Linux x86_64 laptop from an ordinary logged-in user's systemd session.
No production-readiness or performance claim is made.

Prerequisites: unified cgroup v2, systemd supporting DelegateSubgroup (254+),
user D-Bus, systemd-run, systemctl, busctl, Python 3, procfs fdinfo, realpath,
sha256sum. The diagnostic executable is static x86_64 musl; the observer uses
host Python/libc. Unsupported interfaces or permissions must fail closed.

Do not use sudo, activate controllers, write memory.low, migrate workers, delete
cgroups manually, change machine configuration to force a pass, or run stress
or controlled modes. unit-preflight asks systemd to create and remove one
transient delegated user service. XYNOOS performs read-only hierarchy inspection.

From the extracted package directory, in a shell without `set -e`:

```sh
sha256sum -c SHA256SUMS
cat SOURCE_COMMIT
systemctl --user --version
busctl --user status >/dev/null
./xynoos-018d-launcher.sh preflight > preflight.stdout 2> preflight.stderr
preflight_status=$?
printf 'preflight exit: %s\n' "$preflight_status"
```

If preflight succeeds and prerequisites are supported, run once:

```sh
./xynoos-018d-launcher.sh unit-preflight > unit.stdout 2> unit.stderr
unit_status=$?
cat unit.stdout
cat unit.stderr
printf 'unit-preflight exit: %s\n' "$unit_status"
```

A candidate pass requires exit zero, exactly one manager_preflight record with
binding_revalidation=passed, controller_session.ready=true, matching unit,
invocation, PID and descriptor evidence, and a final unit_cleanup record with
status=PASS, reason=cleanup_verified, diagnostic_exit=0, original snapshot and
hierarchy evidence, termination either inactive_awaiting_collection or collected,
and removal=retained_events_ENODEV_and_bound_absence. The inactive case is not
unit collection; both cases require independent original hierarchy removal.
Confirm controller_activation=DISABLED and kernel_control_write=false.
The historical text messages from earlier launchers are no longer pass criteria.

Preserve SOURCE_COMMIT, both preflight logs, both unit logs, exit statuses,
systemd version and `uname -a`. On FAIL or UNVERIFIED, preserve the printed unit
name and inspect `systemctl --user status UNIT` and `journalctl --user -u UNIT`.
Do not retry in a loop, fix cleanup manually, use sudo or weaken verification.
The service has a runtime ceiling; that ceiling is not proof of cleanup. A
missing final JSON record is unverified even if the unit disappears afterward.

Return these artifacts for review before claiming physical success. The
protocol/report explains kernel-specific ENODEV semantics and snapshot limits.
Source is included as SOURCE.tar.gz; SOURCE_COMMIT identifies its revision.
All physical kernel transaction modes remain disabled.
