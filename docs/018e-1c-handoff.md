> **Superseded cleanup instructions:** `unit-preflight` now fails closed because independent hierarchy cleanup is unverified. See [mutation-sink audit](reports/TASK-018E-MUTATION-SINK-AUDIT.md). Do not treat the historical success criteria below as current.

# TASK 018E-1C read-only MX Linux handoff

This package inspects a transient user service and its delegated cgroup. It does not activate controllers, change memory.low, migrate processes, or run a workload. Starting and collecting the service necessarily causes systemd to create and remove its own transient cgroup; the diagnostic makes no cgroup control writes.

Prerequisites: x86_64 MX Linux, a running systemd user manager and user D-Bus, `systemd-run`, `systemctl`, `busctl`, `python3`, `realpath`, and systemd support for DelegateSubgroup. Run as the same ordinary logged-in user, without sudo. No installation or host configuration change is required.

1. Extract the archive into a new directory and enter it.
2. Run `sha256sum -c SHA256SUMS` and inspect `SOURCE_COMMIT`.
3. Run `./xynoos-018d-launcher.sh unit-preflight` and capture stdout, stderr and the exit status.
4. Confirm exit status 0, a single `manager_preflight` JSON record with `controller_session.ready` and `parent_empty` true, memory in `available_controllers`, and no memory in `enabled_controllers`. Confirm the binding identifies the requested transient unit, its invocation, current manager PID/start ticks, `/manager` membership, and distinct parent/manager descriptors. The launcher must report both unit inactive or collected and cgroup removal verified.
5. If any check fails, retain the complete output and status. A missing diagnostic record means the launcher cannot independently verify cgroup removal and reports that explicitly. Do not manually write to cgroups to fix it.

A passing result is point-in-time read-only evidence, not authorization for a kernel transaction. Live controller activation, transaction restoration, race handling, crash recovery and worker lifecycle evidence remain outstanding. Do not claim production readiness or optimization.
