# Phase 4 physical workload comparison: N5000 / 4 GB / MX Linux

Historical copy/CRC A/B protocol. The separate cooperative active experiment is
documented in [PHASE4-OWNED-N5000.md](PHASE4-OWNED-N5000.md). Its results must not
be pooled with this workload.

The earlier physical MVP PASS results are operator-reported. Their raw laptop
evidence was not inspected here. This package supplies a new runnable A/B
experiment; it supplies no new N5000 results. Do not use root.

## Package and exact commands

Build/package on Codespaces or another build machine (Rust musl target and
`readelf` required), from a committed clean source tree:

```sh
python3 scripts/package-phase4.py
cat artifacts/phase4/PACKAGE.json
```

Transfer the archive named in `PACKAGE.json` and its `.sha256` sidecar to the
N5000. In the directory containing those two files:

```sh
sha256sum -c xynoos-phase4-*-linux-x86_64.tar.gz.sha256
tar -xzf xynoos-phase4-*-linux-x86_64.tar.gz
cd xynoos-phase4-*-linux-x86_64
sha256sum -c SHA256SUMS
cat SOURCE_COMMIT
uname -a
cat /etc/os-release
./xynoos-mvp observe --count 1 --human
./xynoos-mvp simulate --human
./xynoos-mvp recovery --human
./xynoos-perf profile > n5000-component-profile.json
python3 benchmark_physical.py --worker ./xynoos-perf --observer ./xynoos-mvp --environment physical --label 'N5000 4GB MX Linux 25.3 AC session1 idle desktop' --repeats 8 --requests 200 --interval-ms 10 --output n5000-session1
python3 inspect_physical.py n5000-session1/events.jsonl > n5000-session1-inspection.json
sha256sum n5000-component-profile.json n5000-session1/events.jsonl n5000-session1/summary.json n5000-session1-inspection.json > n5000-evidence.sha256
```

Keep only one package archive in that working directory so the globs resolve to
one file. Python 3.8+ and a normal writable directory are the runtime requirements;
Rust, systemd, sudo, perf and external Python packages are unnecessary.

Record AC/battery state, desktop applications, compositor/display settings and
visible lag in a session note. Repeat the benchmark in a separate comparable
session using `--output n5000-session2` and an accurate `--label`. Keep commands,
binaries, request count, interval and workload constant. Avoid downloads, builds
and application launches while collecting. Do not tune the desktop for one arm.

## Method and what the numbers mean

- **Baseline GO:** a real deterministic 1 MiB copy+CRC workload, 16 verified passes
  per request, two primary buffers totalling 2 MiB. One request is in flight at a
  time. This is a cache/memory/CPU micro workload, not a desktop application or
  realistic memory exhaustion workload. The input is identical across arms;
  completion and response times are measured, not simulated.
- **Observe GO:** the identical worker plus `xynoos-mvp observe`, live PressureIQ
  collection, state machine, AURA and JSON reports at 500 ms intervals. Work
  starts after the first observation. A refused policy stays in raw evidence;
  it does not authorize writes. The benchmark measures observation overhead.
- **Active optimization NO-GO:** no such arm exists in this executable. Buffer
  compression under synthetic pressure remains simulation and is excluded from
  this comparison. No desktop performance improvement is established.

One complete arm pair is a recorded, excluded warmup. Eight measured pairs
alternate AB/BA with four of each order. No dropping outliers or failed runs.
`response_ns` measures monotonic parent send-to-parsed-response latency including
pipes, scheduling, CRC/copy work, and small response reporting overhead.
`service_ns` is the worker's timed copy/CRC loop. Quantiles use nearest rank;
pooled p99 is not a per-run p99 or independent confidence interval. Inspect per-run
quantiles and paired ratios as well as pooled numbers.

`workload_completion_ns` spans first send through final response and includes
configured think time; `arm_wall_ns` also includes readiness and cleanup.
Spawn-inclusive child CPU/fault/switch counts come from Linux wait4. The child
`wait4_peak_rss_kib` can inherit the Python parent's fork high-water RSS; use
sampled `/proc/PID/status` RSS/HWM for loaded-process memory. Harness CPU and
sampled RSS are reported separately. Observe CPU includes readiness/cleanup and
varies with the number of collected samples; do not compare it without duration.

Host context is sampled at approximately 100 ms in both arms: MemAvailable,
CPU ticks/load, memory/CPU/I/O PSI, swap page counters and up to 16 ZRAM devices'
`mm_stat`, `io_stat`, `stat`. PSI total deltas use microseconds; swap counters use
pages (record `getconf PAGESIZE` when converting). Missing files stay null, counter
resets are flagged, no ZRAM devices is an empty array. Device discovery is capped;
this is namespace-visible telemetry. Mounted cgroup root context is labelled
separately and is not assumed to be workload scope. Reads are sequential.

Timer overshoot is a harness scheduling proxy including its own measurement and
logging costs, **not GUI input, frame latency or desktop responsiveness evidence**.
The workload is closed-loop: stalls reduce request rate; these are not open-loop
arrival latency distributions. Background activity is neither frozen nor
controlled. Report completion CV, shared PSI/CPU/swap changes and per-pair ratios.
A small or inconsistent difference is inconclusive. There are no significance
claims. Repeat physical sessions before drawing conclusions.

## Failure, interruption and recovery

Press Ctrl-C to cancel. SIGTERM and SIGHUP also stop admission, terminate only the
exact unreaped children launched by the harness, reap them, and write incomplete
results. Workers spawn no descendants. No services or persistent state are created.
The arm deadline is 25 seconds; observation sampling is finite (60 × 500 ms).
Low MemAvailable (<256 MiB), unavailable MemAvailable, mounted cgroup root
headroom <32 MiB, malformed reports, integrity failures, child termination or
oversized output stop the experiment. SIGKILL escalation marks a run incomplete.
These limits reduce risk; Python scheduling and procfs reads cannot guarantee
instantaneous admission or recovery from system OOM or uninterruptible I/O.

Durable `run_start` and final run records surround line-flushed raw samples and
responses; these intermediate rows are not individually fsynced. SIGKILL, power
loss or disk failure can leave partial evidence. Inspect it without replay:

```sh
python3 inspect_physical.py n5000-session1/events.jsonl > interrupted-inspection.json
./xynoos-mvp recovery --human > recovery-diagnostics.json
```

A missing terminal run record means **cleanup unknown**. The inspector never
signals PIDs, deletes files, replays effects or upgrades a partial run to success.
Do not signal a stale recorded PID. Preserve the original logs; choose a new output
directory for another attempt. A truncated final JSON row is tolerated and marked;
corruption of a complete row is rejected. Completed runs inside an interrupted
session remain exploratory; incomplete runs are not included in success quantiles.

Stop if the operator notices desktop lag. This package has no global sysctl,
swap/ZRAM setting, controller activation, memory.low write, unrelated process
migration, production mutation API or active kernel optimization entry point.
