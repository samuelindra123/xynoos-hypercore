# Laptop intervention selection, 2026-11-16

The physical N5000 bottleneck is **unknown**. Codespaces is a build/test host,
not the target desktop. Earlier private-buffer compression and copy/CRC results
cannot identify its bottleneck or demonstrate GUI benefit. Collect
`laptop_diagnostic.py` during ordinary lag before choosing a system-wide policy.

| Candidate | Causal mechanism and limitations | Decision |
| --- | --- | --- |
| cgroup v2 `memory.low` | Best-effort reclaim protection for an application working set. Effective protection depends on ancestors and sibling demand; overprotection can push reclaim onto other applications. Requires exclusive, correct hierarchy and workload identity. | Plausible only if measured foreground reclaim is dominant; physical writer remains locked. |
| `memory.high` on background work | Throttles allocations and applies reclaim above a limit. Can contain a known batch workload but can increase its direct-reclaim latency. Lowering a populated group's limit may itself cause expensive reclaim. | No automatic high/low tuning without target evidence and a separately validated protocol. |
| PSI feedback | Detects stalled tasks rather than inferring pressure from used RAM. CPU, memory and I/O have distinct signals; short-window counter deltas complement lagging avg10. | Read-only polling, no PSI trigger writes, shared with admission safety. |
| Cooperative admission | A batch worker performs at most one bounded indexing chunk per request. Stop issuing new chunks when a foreground request is pending, an arrival is within 5 ms, or CPU/memory/I/O PSI exceeds the fixed threshold. Reduces competition without changing another process. | **Selected bounded experiment**, explicit userspace authorization only. |
| Cooperative memory release | An application can discard caches or trim allocations at a safe point. Cannot generally release another application's live data. | Index map released after each refresh and on exit. No claim that this reduces Firefox/VS Code RSS. |
| nice / CPUWeight / IOWeight / scheduler changes | May prioritize known background CPU/I/O work; cannot solve all reclaim, compression, GPU or blocking-I/O stalls. CPUWeight needs delegated hierarchy and nice can leave memory/IO unchanged. | Existing OS facilities should be compared if CPU/IO diagnosis warrants; no priority/affinity writes here. |
| ZRAM / zswap / MGLRU | Existing kernel mechanisms already compress swap or manage reclaim. More private compression can add CPU cost and duplicate existing mechanisms. | Inspect their current activity; no reconfiguration and no replacement memory manager. |
| systemd-oomd / oomd / earlyoom | Existing reliability mechanisms terminate selected workloads under sustained pressure/low memory. They can prevent prolonged OOM stalls at the cost of lost work. | Record installed/operator-known configuration; do not start, stop, configure or duplicate a desktop OOM killer. |

The hypothesis is specifically: under **measured contention from a cooperating
source indexer**, deferring new index chunks can lower foreground source-search
p95/p99, and perhaps visible browser proxy latency, while finishing the same
background work within a predeclared acceptable delay. This is a CPU/resource
admission experiment; it does not establish a remedy for memory-pressure stalls.
If the laptop primarily stalls in eMMC paging, foreground working-set protection
or reducing application demand may be more appropriate. If the indexer contributes
little contention, expect no benefit. This is a falsifiable test, not a production
optimization or an assertion that arbitrary desktop applications cooperate.

The corpus contains committed Rust/Python source, file boundaries and escaped
non-ASCII bytes. Foreground searches count matching source lines for four fixed
queries. Background work builds a case-insensitive word-frequency index in
bounded chunks, then discards it; 16 repeated index refreshes are a declared
batch workload, not 16 distinct user projects. No bytes, queries, refreshes or
verification steps are skipped in active mode. A separate whole-corpus Python
oracle verifies search results and the sorted word/count index digest. The data
is small and cache warm; it does **not** emulate a large IDE workspace, memory
exhaustion, cold eMMC reads or an existing language server. Report overlap counts
and total background completion, especially when the index finishes early.

Primary sources reviewed (online 2026-11-16):

- [Kernel cgroup v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html): hierarchical protection, memory.high throttling, delegation and single-writer ownership.
- [Kernel PSI documentation](https://docs.kernel.org/accounting/psi.html): some/full, microsecond totals and cooperative load shedding; system CPU full is undefined.
- [Kernel ZRAM documentation](https://docs.kernel.org/admin-guide/blockdev/zram.html): compressed swap and mm_stat/io_stat counters; savings alone say nothing about latency.
- [systemd resource control](https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html): MemoryLow/High, CPU/IO weights and controller enablement can affect ancestors/siblings.
- [systemd memory pressure API](https://www.freedesktop.org/software/systemd/man/latest/sd_event_add_memory_pressure.html): existing application-cooperative pressure handling and allocator cache trimming; check the laptop's installed version before relying on APIs.
- [systemd-oomd](https://www.freedesktop.org/software/systemd/man/257/systemd-oomd.html), [oomd](https://facebookincubator.github.io/oomd/), [earlyoom](https://github.com/rfjakob/earlyoom): existing intervention designs. Research does not imply they are enabled or suitable on this machine.
