# N5000 / 4 GB / MX Linux validation handoff

No laptop validation was run from Codespaces. Physical memory-control mutation
remains disabled and NOT VERIFIED. This is a later local procedure requiring
explicit operator authorization; none of its steps is an instruction to remotely
stress the laptop now.

## Safe software comparison on the target

1. Copy the committed static package to a normal user directory. Verify the
   archive checksum and extracted `SHA256SUMS`. Record `SOURCE_COMMIT`, MX/Debian
   version, kernel, CPU, RAM, free storage, power mode and background applications.
   Do not require root or change zram, swap, swappiness or memory controls.
2. Run `./xynoos-mvp observe --count 1 --human`. Unsupported PSI/cgroup fields must
   remain explicit and produce backoff, not fabricated zeroes. Then run
   `./xynoos-mvp observe --count 5 --interval-ms 1000` and preserve JSON.
3. Run `./xynoos-mvp simulate --human`. This works on a 256-KiB owned dataset and
   does not create pressure proportional to four GB. A failed checksum or typed
   error is a failure; do not retry by increasing memory limits.
4. For a full baseline/observe/simulation comparison, build the included source
   on a suitable build machine or target using `cargo build --workspace --release`.
   Run `python3 scripts/benchmark_mvp.py --repeats 12 --output /tmp/n5000-mvp-new.json`
   locally. Preserve all raw results. This compares actual userspace work under
   simulated telemetry, not the effect of a kernel policy on the desktop.
5. Keep the input seed, trace, binaries, environment and ordering fixed. Repeat
   in separate sessions on AC power with comparable desktop activity. Report
   p50/p95/p99, CPU, process RSS, missing telemetry and safety refusals. Treat host
   PSI/swap changes as shared context, not causally attributed improvements.
6. Stop on desktop lag, unexpectedly high RSS, low available memory, I/O errors,
   corrupt evidence or missing telemetry. Preserve logs. Do not invoke any
   system-wide cleanup command. These tools need no persistent daemon to stop.

## Future controlled physical A/B admission

This phase is blocked by software/operational safety requirements, not simply
by the laptop being unavailable. The delivered package cannot enable it.

Before any new executable can admit a physical experiment, independent review
must establish all of the following:

- An enforceable exclusive delegation/lifecycle owner, addressing same-UID and
  systemd competition. Delegate=yes and repeated snapshots are insufficient.
- Exact transient service InvocationID, manager PID/start ticks, parent/manager
  and child/control descriptor identities, stable hierarchy, no unrelated members,
  fresh safety telemetry and bounded action admission at every write boundary.
- A durable physical intent/result protocol that captures originals before each
  write, retains partial/unknown outcomes through death and storage failure, and
  performs no replay until fresh identity, expected state and ownership are proven.
- Worker gating before migration, bounded stop/reap certainty, cancellation and
  low → child → controllers restoration, verified independently after cleanup.
- Fault injection for every transition, replacement/competing values, full disk,
  crash/power-loss stages and reboot/stale identity. Any uncertainty must retain
  resources/evidence and refuse further writes.
- No required swap/global tuning and no effect on unrelated desktop processes.
  Linux still provides no atomic compare-and-swap for these controls; any remaining
  critical race must keep admission closed.

Only after that work and separate explicit authorization: isolate an explicitly
launched bounded workload in its exact disposable service. Pre-register A as
baseline and B as the admitted intervention; hold binary/input/resources constant,
alternate AB/BA pairs, preserve all results including refusals and failures, and
compare task completion and p50/p95/p99 response latency, CPU, RSS, workload-scoped
PSI, faults and swap alongside desktop responsiveness. Define stop thresholds
before running. Reap the owned workload before reverse restoration and independently
verify the bound hierarchy's cleanup; an inactive/collected unit alone is insufficient.

Do not describe memory.low, zram or swapping as a benefit by itself. A performance
claim requires reproducible target-device A/B improvements without increased
failures, harm to other processes or discarded safety/recovery outcomes. Current
status remains **NO-GO for physical mutation; experimental software MVP only**.
