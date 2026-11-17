# Documentation

Start with the [project overview and quick start](../README.md). The project is experimental and is not production-ready.

| I want to… | Read |
| --- | --- |
| Install, update or roll back the preview | [Installation and trust model](INSTALL.md) · [Public distribution proof](reports/PREVIEW-DISTRIBUTION.md) |
| Check preview compatibility and permission fixes | [Compatibility](COMPATIBILITY.md) · [Preview.3 regression evidence](reports/PREVIEW-3-INSTALLER.md) |
| Validate on the N5000 laptop | [Compact OpenCode protocol](N5000-PREVIEW-VALIDATION.md) |
| Understand the components | [Architecture](ARCHITECTURE.md) · [Unified telemetry](reports/TASK-014-UNIFIED-SYSTEM-INTELLIGENCE-REPORT.md) |
| Review safety and limitations | [Kernel transaction report](reports/TASK-015C-REAL-KERNEL-TRANSACTION-REPORT.md) · [Telemetry reliability](reports/TASK-015A-TELEMETRY-RELIABILITY-REPORT.md) |
| Examine measurements | [Memory correctness benchmark](reports/TASK-002-MEMORY-CORRECTNESS-BENCHMARK-REPORT.md) · [Adaptive storage](reports/TASK-003-ADAPTIVE-STORAGE-REPORT.md) · [Workload validation](reports/TASK-017-REAL-WORKLOAD-VALIDATION-REPORT.md) |
| Follow the research milestones and commands | [Research history](RESEARCH-HISTORY.md) |
| Contribute or find open work | [Contribution guidance](../README.md#direction-and-contributing) · [Open issues](https://github.com/samuelindra123/xynoos-hypercore/issues) |

Historical reports and raw measurements remain in [`reports/`](reports/) and [`benchmarks/`](benchmarks/). Read each result with its environment and methodology; simulation evidence does not establish physical safety or desktop-wide benefit.

## Debian packaging

See [packaging and reproducible builds](debian/PACKAGING.md),
[MX installation](debian/MX-LINUX.md), [lifecycle and recovery](debian/LIFECYCLE.md),
[security](debian/SECURITY.md), [hardware acceptance](debian/HARDWARE.md), and
[engineering evidence](debian/REPORT.md). Unsigned CI artifacts are not releases.

## Distribution ecosystem and developer Wiki

[Installation channels/status](distribution/INSTALLATION.md) · [Owner signing setup](distribution/OWNER-SETUP.md) · [Archive maintenance](distribution/MAINTENANCE.md) · [Validation report](distribution/REPORT.md) · [25 maintained Wiki pages](wiki/Home.md). Public APT and Wiki publication remain explicitly pending until verified.
