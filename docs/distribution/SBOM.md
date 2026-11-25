# Software Bill of Materials (SBOM) & Supply Chain Security Audit
## XYNOOS HyperCore — Milestone I

**Schema**: Lightweight SPDX / CycloneDX equivalent markdown document  
**Repository**: `samuelindra123/xynoos-hypercore`  
**Generated**: 2026-11-25  
**License**: GNU General Public License v3.0 (`GPL-3.0-only`)

---

## 1. Primary Workspace Components

| Component Crate | Version | License | Path | Responsibility |
|---|---|---|---|---|
| `xynoos-core` | `0.1.0` | GPL-3.0-only | `crates/xynoos-core` | Core domain types, memory accounting, codec contracts |
| `xynoos-hypermemory` | `0.1.0` | GPL-3.0-only | `crates/xynoos-hypermemory` | Adaptive PageStore, ClusterStore, HyperMemoryPool, LZ4HC pure-safe codec |
| `xynoos-pressureiq` | `0.1.0` | GPL-3.0-only | `crates/xynoos-pressureiq` | Multi-resource kernel telemetry (/proc/pressure, /proc/meminfo, /proc/vmstat) |
| `xynoos-aura` | `0.1.0` | GPL-3.0-only | `crates/xynoos-aura` | AURA continuous daemon, RuntimeSafetyGuard circuit breaker, adaptive tiering |
| `xynoos-decision` | `0.1.0` | GPL-3.0-only | `crates/xynoos-decision` | Deterministic policy decision engine |
| `xynoos-linux-control` | `0.1.0` | GPL-3.0-only | `crates/xynoos-linux-control` | Delegated cgroup v2 control, transaction rollback, TransactionJournal |
| `xynoos-cli` | `0.1.0-preview.3` | GPL-3.0-only | `crates/xynoos-cli` | Command-line interface (`xynoos doctor`, `daemon`, `config validate`, `status`) |
| `xynoos-bench` | `0.1.0` | GPL-3.0-only | `crates/xynoos-bench` | Benchmarking harness (Candidate F app-bench, workload leasing) |

---

## 2. Third-Party Runtime & Build Dependencies

All runtime dependencies are maintained in `Cargo.lock` with pinned cryptographic checksums.

| Crate | Resolved Version | License | Type | Role |
|---|---|---|---|---|
| `cfg-if` | `1.0.5` | MIT / Apache-2.0 | Runtime | Conditional compilation helper |
| `crc32fast` | `1.5.5` | MIT / Apache-2.0 | Runtime | Hardware-accelerated / fallback CRC32 integrity verification |
| `itoa` | `1.0.19` | MIT / Apache-2.0 | Runtime | Fast integer formatting |
| `libc` | `0.2.191` | MIT / Apache-2.0 | Runtime | Standard POSIX syscall bindings (`sigaction`, `statfs`, `openat`, etc.) |
| `lz4_flex` | `0.11.7` | MIT | Runtime | Safe reference LZ4 standard block decompression and compression |
| `memchr` | `2.8.3` | MIT / Unlicense | Runtime | Optimized byte searching |
| `proc-macro2` | `1.0.107` | MIT / Apache-2.0 | Build | Proc macro infrastructure |
| `quote` | `1.0.47` | MIT / Apache-2.0 | Build | Proc macro quote generation |
| `serde` | `1.0.229` | MIT / Apache-2.0 | Runtime | Data serialization framework |
| `serde_core` | `1.0.229` | MIT / Apache-2.0 | Runtime | Serde core traits |
| `serde_derive` | `1.0.229` | MIT / Apache-2.0 | Build | Derive macros for Serde |
| `serde_json` | `1.0.154` | MIT / Apache-2.0 | Runtime | JSON parsing for configs, diagnostics, and recovery journals |
| `syn` | `3.0.10` | MIT / Apache-2.0 | Build | Rust syntax parsing for macros |
| `unicode-ident`| `1.0.27` | MIT / Apache-2.0 / Unicode-DFS-2016 | Build | Unicode identifier verification |
| `zmij` | `1.0.23` | MIT / Apache-2.0 | Runtime | Fast floating-point formatting |

---

## 3. Dependency Security Assessment

- **Known Vulnerabilities**: 0 reported across runtime dependencies.
- **Native C Libraries**: 0 custom C libraries linked. Only standard libc interface via `libc` crate.
- **License Compatibility**: All third-party crates use permissive open source licenses (MIT, Apache-2.0, Unlicense) fully compatible with GPL-3.0-only distribution.
- **Supply Chain Minimization**: No network communication crates (e.g. `reqwest`, `hyper`, `tokio`) are pulled into runtime daemon dependencies.
- **Unsafe Code Boundaries**: `#![forbid(unsafe_code)]` enforced in `xynoos-core`, `xynoos-hypermemory`, `xynoos-pressureiq`, `xynoos-decision`, and `xynoos-aura`. Only `xynoos-linux-control` and `xynoos-cli/signals` interface with low-level POSIX primitives.
