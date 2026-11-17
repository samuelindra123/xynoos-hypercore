# OpenCode procedure: owned lossless corpus, no kernel mutation

This is a separate experimental worker. Phase 4 remains PARTIAL until this
protocol supplies repeated N5000 evidence. Previous baseline/observe laptop
results (seven/eight complete arms and one low-memory refusal, as reported by
the operator) establish no active benefit. This cycle cannot inspect or create
new physical evidence from Codespaces.

## Transfer, verify, run

Use a normal N5000 / 4 GB / MX Linux user session, on AC power. Do not use sudo,
systemd-run, the research CLI, controller writes, sysctl, swap or ZRAM changes.
Transfer the archive, matching `.sha256` and `PACKAGE.json` from
`artifacts/phase4-owned/`. Keep them in a new directory. Python 3.9+, x86_64
Linux and procfs/PSI are required; Rust and external Python packages are not.
Run these exact commands there:

```sh
archive=$(python3 -c 'import json,pathlib; print(pathlib.Path(json.load(open("PACKAGE.json"))["archive"]).name)')
sha256sum -c "$archive.sha256"
tar -xzf "$archive"
cd "${archive%.tar.gz}"
sha256sum -c SHA256SUMS
cat SOURCE_COMMIT
uname -a
cat /etc/os-release
getconf PAGESIZE
cat /proc/meminfo
cat /proc/pressure/memory
./xynoos-mvp observe --count 1 --human
./xynoos-mvp recovery --human
python3 benchmark_owned.py --worker ./xynoos-owned --environment physical --label 'N5000 4GB MX Linux AC session1; record actual desktop state in session-note.txt' --authorize-owned --corpus mixed --repeats 6 --requests 128 --interval-ms 10 --output n5000-owned-session1
python3 inspect_owned.py n5000-owned-session1/events.jsonl > n5000-owned-session1/inspection.json
sha256sum n5000-owned-session1/events.jsonl n5000-owned-session1/summary.json n5000-owned-session1/inspection.json > n5000-owned-session1/evidence.sha256
```

Record desktop applications, display/compositor settings, AC/battery state,
visible lag and MemAvailable in `session-note.txt`. Keep the same desktop state
across all arms. Prefer at least 384 MiB MemAvailable before starting to allow
for the Python harness (measured separately) and worker. The hard admission floor
is 256 MiB; hosts starting around 270–330 MiB may legitimately refuse after
allocation. Never weaken the threshold to obtain a result. If the command fails,
run the inspector anyway and preserve the raw evidence. Stop on visible lag.

In a separate comparable session, repeat the benchmark and inspection with
`--label 'N5000 4GB MX Linux AC session2; actual desktop state in session-note2.txt'`
and `--output n5000-owned-session2`. Do not overwrite earlier directories.
Then run the identical command with `--corpus entropy`, label it accurately and
use `--output n5000-owned-entropy`. Entropy is a negative control; no compression
saving is expected. Keep all failed and refused attempts. Archive both session
notes, raw events, summaries, inspections and hashes. Return these to the
repository for review; do not mark Phase 4 complete from a terminal PASS alone.

The previous copy/CRC A/B remains available through `benchmark_physical.py` and
`inspect_physical.py`; see `BASELINE-N5000.md`. It is a different workload and
must not be pooled with this three-arm corpus comparison.

## Work and interpretation

Each arm creates exactly the same 16 × 1 MiB corpus, with independently seeded
random bytes in 256 bytes of each 1024-byte record. The remaining 768 bytes repeat
a fixed application-record string. This deliberately defined compressible corpus
is not sampled desktop content. Entropy mode uses random bytes throughout.
Every request copies or losslessly decodes four cyclic blocks, validates their
original CRCs, then hashes the same 4 MiB result. Each block is visited repeatedly.
Matched output identities and per-request checksums are compared across all arms.
No required read or byte is dropped. The generator is only initialization and an
independent final equality oracle; it is not used to serve requests.

Baseline retains raw blocks. Observe runs live PressureIQ and policy on each
request and keeps raw blocks. Active runs that same policy and can replace raw
blocks with smaller boxed LZ4 blocks. Original bytes are freed after exact decode
equality. Decoding uses one reusable 1 MiB scratch buffer. Fresh host telemetry
admits execution; an application retention budget of 8 MiB, observed twice,
drives AURA's choice. Host PSI is never overwritten or manufactured. CPU/I/O
pressure causes backoff, missing/stale/reset telemetry refuses, and attempts have
a 10-second cooldown. No-benefit blocks remain raw. Policy can legitimately make
zero interventions, which does not constitute active benefit.

One ABC warmup is recorded and excluded. Six measured triples cover all six
permutations; multiples of six remain balanced. A single request is in flight.
`response_ns` includes pipes, scheduling, per-request PressureIQ/policy and any
intervention. `service_ns` covers copies/decodes and CRCs. Completion includes
10 ms think times. `arm_wall_ns` and wait4 child CPU include initialization,
compression, verification and full restoration. Nearest-rank p50/p95/p99 are
reported; inspect per-run paired ratios, not only pooled quantiles. Shared host
PSI, swap, CPU and ZRAM samples are context, not attributable effects.

Payload retention includes scratch but excludes metadata/allocator overhead;
steady RSS comes from `/proc/PID/status` on responses after the first eight.
Peak HWM retains initialization and restoration costs. wait4 peak RSS can inherit
the parent's fork high-water mark; it is raw evidence, not the primary memory
claim. Harness CPU/RSS are reported separately. This is a cooperative private
memory experiment, not desktop latency or Firefox/VS Code optimization.

## Limits, interruption and crash recovery

Only `active mixed|entropy --authorize-owned` enables the worker's action; the
harness independently requires `--authorize-owned`. All workers impose and read
back 128 MiB RLIMIT_AS, 8 CPU seconds RLIMIT_CPU, zero core dumps, a 25-second
independent watchdog and at most 256 requests. The harness arm timeout is 24 s.
Limits lower to an inherited smaller hard limit; they never raise one. This is
bounded experimental code, not a sandbox for arbitrary programs. There are no
commands, PIDs, paths, variable allocation sizes or subprocess tools in the worker.
The watchdog depends on OS scheduling and does not guarantee real-time response
to SIGSTOP, kernel failure or uninterruptible I/O.

Ctrl-C, SIGTERM or SIGHUP stops new requests. The harness asks its worker to
cancel, allows up to one second for reverse verification and reaps it. A stalled
or failed child is terminated; SIGKILL escalation and missing restoration stay
incomplete. The harness signals only its exact unreaped child PID. No process
groups or historical PIDs are signalled. Plain stdin EOF also restores and exits.
Parent SIGKILL may prevent logging, but pipe closure or the independent worker
watchdog bounds its lifetime. SIGKILL/power loss can skip restoration; all state
is disposable, process-private memory, so no persistent external state needs
rollback. OS reclamation is distinct from byte-verified graceful restoration.

For partial evidence:

```sh
python3 inspect_owned.py n5000-owned-session1/events.jsonl > interrupted-inspection.json
```

A missing terminal run is cleanup unknown; inspection never replays actions,
sends signals or upgrades partial evidence to success. Durable start/end/run
records bracket line-flushed samples. A truncated final row is reported; a
corrupt complete row is rejected. Preserve the files and choose a new directory.
No cgroup is created, delegated, migrated or written. Production mutation guards
remain disabled. Live systemd identity, exclusivity, TOCTOU, kernel write/read-back
ambiguity and physical crash recovery remain NO-GO for the separate kernel path;
this package does not exercise or certify them.
