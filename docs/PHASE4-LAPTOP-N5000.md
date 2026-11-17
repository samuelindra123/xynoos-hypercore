# Exact OpenCode N5000 validation instructions

Status: physical kernel mutation **NO-GO**. Real desktop improvement **not
established**. This protocol is for an N5000 / 4 GB Debian 13 or MX-derived
machine, running as its normal desktop user. Codespaces numbers do not certify
this laptop. Do not use sudo, systemd-run, stress-ng, drop_caches, swap changes,
controller writes, process migration or automatic application configuration.
Keep existing archives/evidence. Use a fresh output directory for every attempt.

## Verify the static package

Build on Codespaces only, from committed package sources:

```sh
python3 scripts/package-laptop.py
```

The command prints `PACKAGE.json` with the exact archive path and SHA256 and
creates a commit-specific directory under `artifacts/phase4-laptop/`. Transfer
that archive, matching `.sha256` and `PACKAGE.json` to a new N5000 directory.
Inside that transfer directory, run:

```sh
archive=$(python3 -c 'import json,pathlib; print(pathlib.Path(json.load(open("PACKAGE.json"))["archive"]).name)')
sha256sum -c "$archive.sha256"
tar -xzf "$archive"
cd "${archive%.tar.gz}"
sha256sum -c SHA256SUMS
cat SOURCE_COMMIT
uname -a
cat /etc/os-release
python3 --version
./xynoos-mvp observe --count 1 --human
./xynoos-mvp recovery --human
```

The binaries are static Linux x86_64 musl executables, with no dynamic interpreter
or NEEDED library. Python 3.9+ stdlib, Linux pidfd/memfd sealing, readable procfs,
PSI and resolvable cgroup v2 telemetry are required. Static Rust binaries do not
bundle Python. Check the normal user owns the extracted binaries and they are not
group/other writable; package modes are 0755. Do not weaken a refusal check.

## Diagnose ordinary lag first

Write `session-note.txt` manually: CPU/RAM/storage model, kernel/MX release,
AC/battery, Firefox tab count and workload, VS Code project/extensions activity,
desktop/compositor/display settings, background updates/builds/indexers, installed
OOM manager and ZRAM/swap setup if known, visible lag and wall time. Do not include
private browsing content. Keep these conditions fixed across arms; record changes.

```sh
python3 laptop_diagnostic.py --environment physical --label 'N5000 AC session1 ordinary desktop; see session-note.txt' --seconds 60 --output n5000-diagnostic1.jsonl
```

Use Firefox, VS Code and normal desktop tasks manually during the capture. Do
not manufacture pressure or exhaust RAM. Stop on visible worsening, abnormal
heat or repeated freezes. Inspect the last JSON row (`window`) and the samples:

- CPU PSI with high busy fraction suggests contention; cgroup cpu.stat throttling
  can show quota effects even when host-wide CPU use is low.
- Memory PSI some/full plus direct reclaim/allocstall, refaults, paging and lower
  MemAvailable support a reclaim hypothesis. Low available RAM alone does not.
- pswpin/out (pages), ZRAM io/mm/stat deltas and CPU PSI support investigation of
  swap/compression cost. They do not isolate compression CPU time.
- I/O PSI plus per-device disk read/write/io/weighted-io deltas support a storage
  bottleneck. `mmcblk` may be eMMC or an SD card: confirm the actual device.
- With little PSI, investigate application event loops, compositor/GPU, thermal,
  power, locks or other causes. This package cannot attribute all lag.

The diagnostic includes process names/UID/RSS, power/thermal context, effective
cgroup/ancestor limits, memory.events, CPU quotas and I/O statistics. Readings are
sequential and namespace-visible. Pgmajfault is a count, not stall duration;
memory PSI total is stall time in microseconds. Unavailable data stays unknown.

## A/B: baseline versus Xynoos observe

Use the same package and corpus in at least **three matched sessions**, preferably
on separate occasions under the recorded ordinary desktop workload. Before each,
check free storage for evidence, MemAvailable >=384 MiB and no visible instability.
The harness also requires >=256 MiB finite cgroup headroom and fresh PSI. These
floors are emergency rules, not permission to allocate up to the remaining RAM.

```sh
python3 benchmark_desktop.py --worker ./xynoos-desktop --observer ./xynoos-mvp --corpus source-corpus.txt --environment physical --label 'N5000 AC session1 AB; see session-note.txt' --output n5000-ab-session1
python3 inspect_desktop.py n5000-ab-session1/events.jsonl > n5000-ab-session1/inspection.json
```

Default is AB only, with one recorded/excluded warmup pair and six measured pairs
in balanced AB/BA order. Baseline includes the identical sampler/harness and source
workload; observe additionally runs the unmodified read-only PressureIQ/AURA MVP.
Repeat using `n5000-ab-session2` and `n5000-ab-session3`, with accurate labels.
Keep failed/refused attempts; do not rerun until a preferred number appears.

## C: independently admitted cooperative optimization only

Before running C, an independent reviewer/operator must verify source/binary
hashes, staged package smoke, safety/test results, exact owned-worker identity,
EOF/cancellation cleanup, telemetry availability and that indexing contention is a
plausible contributor. Record the review and authorization in the session note.
Without that explicit authorization, collect only A/B and the diagnostic.

C only defers **new chunks sent to this disposable source indexer**. It does not
change Firefox, VS Code, desktop settings, process priorities, cgroups or memory.low.
The following flag is explicit authorization for that userspace scope, and has no
physical controller authority:

```sh
python3 benchmark_desktop.py --worker ./xynoos-desktop --observer ./xynoos-mvp --corpus source-corpus.txt --environment physical --label 'N5000 AC session1 ABC; bounded cooperative authorization in session-note.txt' --arms all --authorize-disposable-index-admission --output n5000-abc-session1
python3 inspect_desktop.py n5000-abc-session1/events.jsonl > n5000-abc-session1/inspection.json
```

One ABC warmup is excluded. Six measured triples cover ABC, ACB, BAC, BCA, CAB,
CBA. Repeat complete balanced sessions at least three times. Compare per-block and
per-session p50/p95/p99, completion, PSI, paging, RSS and CPU; retain all raw tasks.
A global pooled p99 is not an independent sample. The same searches and all 16
index refreshes must complete and match independent/cross-arm output checks.
A controller refusal, lost telemetry, missing final/exit, OOM or failure makes the
session incomplete. Do not weaken thresholds, increase loads or discard outliers.

Searches have fixed scheduled arrival times; each response is timed from its due
time, so delayed arrivals remain in latency. Only one search is outstanding, so
this is a bounded queue/backlog model, not unlimited open-loop application input.
The first due time starts at readiness. Index chunks are nonpreemptive; overlap
counts expose already-running background work at foreground arrival. Background
completion and arm wall time expose the cost of deferral. wait4 CPU includes
spawn/cleanup; sampled worker/observer/harness RSS is distinct from inherited
wait4 peak RSS. Each raw run includes p50/p95/p99, >1 s latency count, PSI/VM/
disk/cgroup deltas, engine/harness RSS and CPU, verified child exits and releases.

## Optional visible browser and application evidence

Open `desktop-probe.html` manually in a new browser tab, keep it visible, and
export every trial, including failed/hidden ones. It performs a small fixed filter
and measures event-to-handler, handler-to-two-animation-frames and timer gaps.
These are browser event-loop/rendering proxies; they do not measure actual display
presentation, hardware input-to-photon, VS Code keystrokes or whole-desktop latency.
Record manual task timing for a fixed Firefox local-page reload, VS Code search
on a disposable copied project, and switching windows using the same operator and
method. Keep those results separate, with timing resolution and operator error.
No applications are automatically launched, reconfigured or instrumented.

For manually synchronized 30 s browser trials, use `--only-arm baseline`, then
`observe`, then separately authorized `active` in **balanced whole-session order**
ABC/ACB/BAC/BCA/CAB/CBA. Example command for one arm:

```sh
python3 benchmark_desktop.py --worker ./xynoos-desktop --observer ./xynoos-mvp --corpus source-corpus.txt --environment physical --label 'N5000 browser block1 baseline; see session-note.txt' --only-arm baseline --requests 600 --interval-ms 50 --output n5000-browser-block1-baseline
```

The command records one warmup and six 30 s workload runs. Start one browser
trial at the beginning of each measured run, ignoring warmup, using console run
completion messages and local time in the session note to align. Foreground
browser input must stay in the probe; another operator may start the terminal
command. A single operator can start with a separate terminal before switching
into the probe; record alignment uncertainty. The index refreshes may finish
well before 30 s; report exposure/overlap, do not claim an effect outside it.
No browser results should be attributed to a specific arm without matching times.
For active single-arm commands add the explicit authorization flag after review.
This optional manual path supplies exploratory proxy evidence, not tightly
controlled automated GUI latency. If stronger GUI evidence is needed, design a
separate disposable application integration before declaring success.

## Predeclared decision and recovery

Before collecting, choose one primary metric (source-search per-run p95 is the
default) and acceptable background/CPU/memory budgets. Suggested conservative
admission criterion for a follow-up: >=10% improvement in median paired p95 in
**each of three sessions**, no p99 or observed GUI regression >5%, no new >1 s
freeze/OOM, all work/cleanup verified, background completion increase <=20%,
total CPU increase <=10%, and no increased memory peak. These are proposed
engineering budgets, not statistical significance or verified safe universal
limits. Also compare observe vs baseline to expose measurement overhead. If the
normal desktop provides insufficient comparable pressure or the primary metric
is faster than measurement noise, label the result inconclusive. Do not add
synthetic memory pressure. A genuine GUI/task result is required to claim a
Firefox/VS Code/desktop benefit; source search alone only supports that task.

Ctrl-C/TERM/HUP cancels. After any interruption, inspect the original log:

```sh
python3 inspect_desktop.py n5000-abc-session1/events.jsonl > n5000-abc-session1/inspection.json
./xynoos-mvp recovery --human > n5000-recovery.txt
sha256sum session-note.txt n5000-diagnostic1.jsonl n5000-abc-session1/events.jsonl n5000-abc-session1/summary.json n5000-abc-session1/inspection.json n5000-recovery.txt > n5000-evidence.sha256
```

Omit only nonexistent summary files after a hard crash, record that absence, and
keep partial rows. Never signal historical PIDs. After parent SIGKILL, wait at
least 60 s for the worker watchdog; the crashed log still has unknown cleanup.
Use system tools read-only to inspect any live process identity before escalating
to an operator-managed recovery. Do not replay writes or auto-clean stale units.
The exact physical-controller admission checklist is in `SAFETY.md`; all unresolved
critical blockers continue to prevent kernel mutation.
