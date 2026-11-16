<p align="center"><img src="assets/logo.svg" alt="Xynoos HyperCore" width="680"></p>

# Xynoos HyperCore

**Experimental Linux system intelligence, built around evidence and bounded action.**

Xynoos HyperCore is a Rust workspace exploring how telemetry, policy and cooperative memory management might help applications respond to resource pressure. It combines read-only Linux observation, deterministic policy simulation, safety-oriented control research and reproducible workload experiments.

> **Research software. Production readiness: NO. Physical kernel mutation: NOT VERIFIED for the integrated path and disabled in the MVP.** No desktop-wide speedup has been established. Use the documented observation and simulation paths for evaluation; read the relevant safety reports before experiments.

## The problem and the vision

Resource pressure is difficult to interpret: memory, CPU and I/O signals interact, telemetry can be missing or stale, and an intervention can cost more than it saves. HyperCore aims to make those tradeoffs explicit through measurable, reversible, narrowly scoped experiments. The long-term vision is understandable system intelligence that earns trust through evidence, rather than automatic tuning based on assumptions.

## Status at a glance

| Area | Status |
| --- | --- |
| Linux telemetry and observation | Implemented; availability depends on the host and permissions |
| Policy and transaction simulation | Implemented; deterministic decisions for a given trace |
| Cooperative owned-memory experiment | Experimental; limited to participating application-owned data |
| Kernel/cgroup control | Research and guarded infrastructure; integrated physical mutation is not verified and MVP admission refuses |
| Desktop performance improvement | Not demonstrated |
| Production readiness | No |

The repository contains fixtures and tests for control lifecycles and an earlier opt-in disposable cgroup transaction test. These do not establish that the integrated physical mutation path is safe or verified. Exclusive delegation assumptions, durable crash recovery, cleanup under failure and real-world workload behavior remain open questions.

## Architecture

```text
Linux telemetry → PressureIQ → engine / decision policy → observe or simulate
                                                  ↘ experimental owned-memory action
Linux control research → guarded sessions and lifecycle fixtures (physical admission locked)
```

- **PressureIQ** collects and qualifies Linux resource signals, including pressure information where available.
- **Engine and decision** evaluate state and policy; missing or unreliable inputs require conservative handling.
- **AURA and HyperMemory** explore transactional policy and bounded memory techniques, including cooperative owned-data experiments.
- **Linux control** contains safety-oriented cgroup inspection and experimental lifecycle infrastructure.
- **CLI and benchmarks** expose observation, simulation and measurement workflows.

See [MVP architecture](docs/MVP-ARCHITECTURE.md), [safety and recovery](docs/MVP-SAFETY-RECOVERY.md), and the [mutation sink audit](docs/reports/TASK-018E-MUTATION-SINK-AUDIT.md) for boundaries and evidence.

## What works today

- Read-only telemetry and human-readable or JSON observation.
- Trace-driven policy simulation and simulation-only journal inspection.
- Bounded, cooperative compression of data owned by an experimental process, with verification and reversal.
- Workload harnesses, recorded measurements and packaging scripts for controlled research.
- Fail-closed admission for the MVP's physical control interface.

Availability and results vary by environment. Experimental work does not transparently optimize unrelated applications.

## Measurement and limitations

The [owned-memory report](docs/reports/PHASE4-OWNED-PERFORMANCE.md) describes a Codespaces experiment where a compressible owned corpus retained less memory at a CPU and completion-time cost. This is a scoped tradeoff, not a general memory or desktop improvement. The [laptop study](docs/reports/PHASE4-LAPTOP-RESULTS.md) reports no demonstrated performance benefit and no physical N5000 evidence. The [workload report](docs/reports/PHASE4-PERFORMANCE.md) records baseline/observe comparisons and their limits. Results are workload- and environment-specific; timing proxies are not GUI latency.

Telemetry can be unavailable or stale. Recovery evidence for simulation is not durable physical crash recovery. Controller state may be retained only in memory; exclusive delegation and cleanup cannot be assumed across every failure. Host coverage, independent replication, real workloads and physical validation remain incomplete. Reports and raw artifacts should be read alongside any numbers.

## Build and try

A Rust toolchain and Cargo are required. Linux is needed for live Linux telemetry; Python 3.9 or newer is required for the relevant benchmark harnesses. From the repository root:

```sh
cargo build --workspace --release
cargo test --workspace
./target/release/xynoos-cli mvp observe --count 1 --human
./target/release/xynoos-cli mvp simulate --human
./target/release/xynoos-cli mvp recovery --human
```

Observation is read-only; simulation works on bounded private data. Recovery diagnostics do not replay physical writes. See the [quickstart](docs/MVP-QUICKSTART.md) for trace and journal examples, prerequisites, and the explicitly refused controlled interface. Do not treat experimental packages as a production installation.

## Repository map

| Path | Purpose |
| --- | --- |
| `crates/` | Rust workspace components and CLI |
| `docs/` | Architecture, safety, validation guides and reports |
| `artifacts/` | Recorded experimental evidence and package metadata |
| `benchmarks/`, `scripts/` | Measurement, inspection, packaging and quality tools |
| `fixtures/` | Reproducible simulation inputs |

## Safety philosophy

Prefer observation before intervention, explicit authorization and bounded ownership, conservative refusal when evidence is incomplete, and independently inspectable results. A test or fixture is evidence for its tested conditions, not a guarantee for arbitrary hosts. No root daemon, global swap tuning or kernel module is required for the MVP examples above.

## Roadmap and contributions

The [roadmap](docs/ROADMAP.md) separates completed foundations from experimental work, validation gates and future product ideas. The [proposed backlog and PR plan](docs/PROJECT-BACKLOG.md) records unresolved risks and review boundaries. Contributions that improve reproducibility, documentation, tests and careful review are welcome; open an issue or pull request with the environment, evidence and scope of any claim.

## License and authorship

The repository includes the [GNU General Public License v3.0](LICENSE); the Rust crates declare `GPL-3.0-only`. This copyleft license carries obligations when distributing covered modified versions; consult the license text. Project history identifies Samuel Indra Bastian and contributors; use the repository issue tracker for project discussion.
