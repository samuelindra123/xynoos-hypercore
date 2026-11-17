# Phase 4 laptop experiment safety and recovery

Physical kernel control remains **NO-GO**. No existing controller module, memory.low
writer, controller enablement or admission lock is changed. The static package
contains the safe MVP and a new disposable worker, not the research control CLI.
Self-imposed process resource limits are the only kernel resource settings made
by this worker; no global, scheduling, cgroup, swap or ZRAM settings are changed.

## Implemented cooperative boundary

- Default benchmark is A/B (baseline/observe). `--arms all` or `--only-arm active`
  requires `--authorize-disposable-index-admission`. This authorizes only admission
  of disposable indexing chunks. It never authorizes physical controller changes.
- Workers receive no commands to execute, target PIDs, cgroup paths, application
  handles or writable source paths. Corpus input is a sealed, immutable memfd,
  copied from a bounded regular ASCII source file. Changes to the original corpus
  cannot affect a running experiment. All workload state is process-private.
- The harness opens and pins each executable inode, checks caller ownership,
  rejects setuid/setgid and group/other writable binaries, then executes the open
  descriptor. A pidfd pins each live child for signalling. Before every work or
  finish request, check UID, PPID, start ticks, executable device/inode, exact
  cgroup membership and absence of process descendants. Only the unreaped direct
  child PID is used as a startup fallback if pidfd setup itself fails.
- Parent and workers stay in the existing cgroup. No exclusive delegation is
  required for **cooperation**, because nothing acts on a cgroup or unrelated
  tasks. Effective membership, device/inode and all visible ancestor memory
  limits are sampled; unresolved scope refuses workload admission. Host and
  cgroup telemetry have different scopes and are labelled. No systemd unit is
  started, and no unit ownership/InvocationID is used as authority. The caller's
  environment InvocationID is recorded as context only.
- MemAvailable must remain at least 384 MiB; every visible finite ancestor limit
  needs 256 MiB headroom. Missing memory/PSI/swap/OOM counters, counter resets,
  changed cgroup identity, and samples older than 300 ms refuse and stop work.
  Pressure only defers *new* active index chunks: CPU some >=15%, memory some
  >=2%, I/O some >=5%, using avg10 or interval total deltas. These thresholds are
  experimental constants, not calibrated universal lag boundaries. Baseline and
  observe retain the same diagnostic sampler and emergency stop rules.
- One search and one index request can be outstanding. Active does not preempt
  an already-running chunk. A chunk is 32 KiB plus at most 4096 token bytes.
  Corpus <=4 MiB; token length <=4096; queries <=1024; refreshes <=16; commands
  <=16384. Workers lower/read back RLIMIT_AS <=192 MiB, CPU <=20 s and core=0;
  an independent 60 s watchdog exits even with stdin held open. The harness's
  arm timeout is <=45 s. These reduce risk, not guarantees against system OOM,
  kernel failure, scheduling starvation or uninterruptible I/O.
- Every result acknowledges sequence, role and exact completed work. Whole-corpus
  independent search/index oracles and cross-arm equality detect omitted work.
  Successful worker finals attest private state release; successful wait4 confirms
  exit and provides CPU/fault/switch counters. RSS/HWM are sampled separately;
  wait4 peak RSS may inherit Python's fork watermark and is not a saving claim.
- Ctrl-C/TERM/HUP stops admission, sends cancel to exact owned workers, closes
  stdin, then uses bounded pidfd TERM/KILL and wait4. Escalation, uncertain exit,
  missing final acknowledgement or protocol ambiguity cannot become success.
  Workers spawn no child processes; their only extra thread is the watchdog.
  On parent death, EOF/broken pipes normally end them; watchdog bounds an idle
  worker's lifetime. Cancellation does not replay an interrupted chunk.

## Failure semantics and residual risks

There are **no external control writes to roll back**. Pipe short writes, broken
pipes, duplicate/unexpected reports, lost telemetry and log failures stop admission
and clean up owned workers; effects remain disposable computation. A partial pipe
command may or may not have run, so it is never retried. Corpus, output hashes,
completed counts, final release and exit are read-back evidence. A competing
process may change ambient pressure or cgroup membership between reads. Identity
checks are not atomic with pipe delivery; a hostile same-UID actor is outside
this experiment's trust boundary. Even a migration between checks cannot cause a
cgroup write because that capability is absent. This does not certify the kernel
adapter's TOCTOU or partial-write semantics.

Durable manifest/run_start/child_started/run/end records bracket flushed task and
telemetry rows. Task rows are not individually fsynced. Power loss, SIGKILL or
full storage can leave a partial final row or unlogged result. The read-only
`inspect_desktop.py` reports incomplete sessions and unknown cleanup for unmatched
run starts. It never signals recorded PIDs, replays commands or deletes files.
Worker finals cannot prove parent cleanup after parent SIGKILL; preserve partial
evidence even when workers have independently exited. Use a new output directory.
No automatic recovery of stale PIDs or systemd units is attempted.

The diagnostic reads process names, UID/RSS, namespace membership, system pressure,
block stats, temperatures and power state. Logs can reveal host context; review
before sharing. It does not read process cmdlines or application content. Disk,
OOM and PSI signals are not uniquely attributable to the test. System OOM events
must be correlated with operator observations; visible freezes or app OOMs are
recorded manually, without privileged kernel-log access.

## Exact admission checklist for any future physical cgroup writer

Every item is mandatory and remains unvalidated in this cycle. A single unresolved
critical item blocks mutation; an authorization flag cannot override a blocker.
Do not enable writers to test this package.

1. Obtain a **separate explicit authorization** naming host/boot, disposable unit,
   controller/property, old value, proposed bounded value, expiry and stop rules.
2. Diagnose foreground reclaim on this N5000; record why userspace cooperation or
   existing Linux/systemd mechanisms are insufficient. Predeclare latency and cost
   budgets. No Firefox/VS Code migration or production-session experiments.
3. Prove cgroup v2 delegation of a dedicated disposable subtree with exclusive
   single-writer ownership, correct ancestor/sibling effects, controller support,
   no unrelated members and no conflicting manager. A writable path is insufficient.
4. Validate systemd D-Bus unit object, manager, UID, ControlGroup, InvocationID,
   MainPID/start ticks/pidfd, boot ID, cgroup mount/device/inode and exclusive
   membership **live**, before and after admission. Environment variables alone
   do not prove invocation identity. Handle unit reuse and manager reconnects.
5. Resolve TOCTOU limits: pin directory/file descriptors and task identities;
   prevent migration/descendant creation or prove a protocol whose unavoidable
   races cannot affect unrelated tasks. Rechecking a path alone is insufficient.
6. Require fresh complete host, subtree and ancestor PSI/memory/events/controller
   readings, enough reserved headroom, no OOM, and no competing property changes.
7. Use only bounded changes. Treat short/partial writes, EINTR after possible effect,
   read-back mismatch and unexpected competing values as ambiguous, stop further
   mutation, and never blindly retry or overwrite another actor's value.
8. Prevalidate expected-old-value restoration semantics, exact ownership at rollback,
   durable write-ahead journal, recovery after each crash point, power-loss behavior
   and independent watchdog/supervisor. Do not overwrite a competing value.
9. Prove parent/child termination and empty disposable subtree, including parent
   SIGKILL, stopped workers, descendants, PID reuse and uninterruptible exit. A
   successful kill syscall is not verified termination.
10. Have an independent physical review of all preceding evidence and a new protocol
    before removing any lock. Existing compile-fail/guard tests remain required.
