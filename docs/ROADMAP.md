# XYNOOS HyperCore — Official Product Engineering Roadmap

## 1. Product Mission & Vision

XYNOOS HyperCore is evolving from an experimental resource intelligence engine into a **Universal Adaptive Linux Optimization Engine**.

The ultimate product objective is to empower Linux users to:
1. Install any supported modern Linux distribution.
2. Install XYNOOS HyperCore.
3. Authorize a one-time automatic setup (`xynoos setup`).
4. Automatically discover host hardware, kernel capabilities, and existing configurations.
5. Preserve existing beneficial or administrator-declared tuning.
6. Safely configure supported optimizations without breaking the system.
7. Continuously and adaptively adjust to changing runtime workload pressure.
8. Avoid conflicts with third-party resource managers (e.g. TLP, tuned, systemd-oomd, earlyoom).
9. Guarantee complete, safe rollback of HyperCore-owned changes when requested.

Users should not need to repeatedly configure CPU governors, zram, swappiness, swap priorities, reclaim thresholds, or I/O schedulers after reinstalling Linux. Universal compatibility is achieved through **capability-based discovery and bounded adaptation**, not by forcing identical settings across divergent platforms.

XYNOOS HyperCore remains strictly independent from the separate XYNOOS Linux distribution.

---

## 2. Milestone Architecture Roadmap (A–R)

```
┌────────────────────────────────────────────────────────────────────────┐
│  Milestones A–J: Foundation & Qualification (Completed)               │
│  - Core engine, adaptive tiering, fallback safety, transactions        │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│  Milestone K: Universal Hardware & Existing Configuration Intelligence │
│  - Platform, CPU, Memory, GPU, Storage, Power/Thermal Discovery        │
│  - Existing Configuration Inventory & Non-Mutating Conflict Analysis  │
│  - Versioned SystemInventory Schema & Read-Only Optimization Planning  │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│  Milestone L: Reversible Configuration Engine & Universal Memory Opt   │
│  - Reversible execution engine, action model, journal & ownership ledger│
│  - Dynamic ZRAM creation, algorithm selection (LZ4/ZSTD), swap priority│
│  - Zswap vs Zram coordination, swappiness/page-cluster memory sysctls  │
│  - Cooperative OOM coexistence and authorized CLI setup lifecycle      │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│  Milestone M: CPU, Scheduling, Thermal & Power Intelligence            │
│  - CPU governor & energy performance preference (EPP) adaptation       │
│  - Heterogeneous topology (big.LITTLE / P+E cores) task placement      │
│  - Thermal-aware throttling avoidance and CFS autogroup coordination   │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│  Milestone N: GPU-Aware & Storage-Aware Optimization                   │
│  - NVMe / eMMC / SATA SSD / HDD adaptive I/O scheduler tuning          │
│  - GPU power profile coordination and hardware video accel awareness   │
│  - Read-ahead and discard/TRIM optimization per media type             │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│  Milestone O: Full Cross-Subsystem Automatic Setup & System Coexistence│
│  - End-to-end multi-subsystem `xynoos setup --auto` coordination       │
│  - Global conflict reconciliation across CPU, Memory, GPU & Storage    │
│  - Whole-system transactional verification and unified rollback        │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│  Milestone P: Cross-Distribution Packaging & Init Compatibility        │
│  - Debian (.deb), RPM (.rpm), Arch PKGBUILD, Void XBPS, Alpine APK     │
│  - Systemd, SysVinit, OpenRC, and Runit service orchestration          │
│  - Automated multi-distribution container test matrices                │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│  Milestone Q: Whole-System Adaptive Performance Qualification          │
│  - Sustained real-workload benchmarking across physical hardware tiers │
│  - N5000 low-resource laptop physical benchmark validation             │
│  - Evidence-based performance gain verification                        │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│  Milestone R: Stable Universal Product Release (v1.0.0)                │
│  - Hardened, security-audited, signed universal production release     │
│  - Comprehensive documentation, GUI/CLI telemetry integration          │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Milestone Definition & Acceptance Criteria

### Milestone K — Universal Hardware Intelligence, Kernel Capability Discovery & Existing Configuration Intelligence (Completed)
- **Objective**: Establish read-only discovery, inventory schema, conflict analysis, and optimization planning.
- **Scope**:
  - Real platform capability discovery (distro family, init system, cgroups, PSI).
  - Subsystem intelligence (CPU, Memory/Compression, GPU, Storage, Power/Thermal).
  - Existing configuration inventory & conservative ownership classification.
  - Non-mutating conflict analysis engine.
  - Versioned `SystemInventory` and `OptimizationPlan` schemas.
  - Read-only CLI inspection (`xynoos inspect`, `tuning audit`, `setup --plan`).
- **Acceptance Criteria**:
  - PASS: All discovery operations strictly read-only; zero host kernel or config mutation.
  - PASS: Synthetic cross-distribution fixture test suite passes in CI.
  - PASS: Physical read-only discovery executed and documented on N5000 reference hardware.
  - PASS: Setup planning strictly refuses `--apply` during Milestone K.

### Milestone L — Reversible Configuration Engine, Universal Memory Optimization & Authorized Automatic Setup Foundation (Completed)
- **Objective**: Implement working, authorizable, reversible configuration execution system and universal memory optimization foundation.
- **Scope**:
  - Deterministic versioned `ConfigurationAction` schema with typed allowlisted mutations.
  - Configuration ownership ledger (`/etc/xynoos/owned-configs.json`) with checksums and tamper detection.
  - Durable configuration journal (`config-journal.json`) with crash recovery semantics.
  - Managed ZRAM lifecycle: conservative allocation heuristics, algorithm intelligence (LZ4 for low-power N5000, ZSTD for desktops).
  - Swap tiering coordination: prioritizing ZRAM swap over disk swap via priority weighting.
  - Context-aware ZSWAP coordination (tiered coexistence with disk swap; eliminating double compression when ZRAM is sole swap).
  - Memory sysctl allowlist (`vm.swappiness`, `vm.page-cluster`, `vm.vfs_cache_pressure`) with managed drop-ins (`/etc/sysctl.d/99-xynoos-memory.conf`).
  - Transparent Huge Pages (THP) workload-aware policy.
  - Cooperative userspace OOM coexistence (EarlyOOM, systemd-oomd, nohang).
  - Complete CLI setup lifecycle (`setup --plan`, `setup --apply --authorized`, `setup --status`, `setup --rollback`, `doctor`).
- **Acceptance Criteria**:
  - PASS: Planning strictly read-only; zero mutation without explicit operator authorization.
  - PASS: Requires explicit operator authorization (`--authorized`); fails fail-closed without it.
  - PASS: Ownership ledger tracks all applied values, original values, and drop-in checksums.
  - PASS: Complete, verified restoration and drop-in removal upon rollback.
  - PASS: Refuses blind rollback if configuration files were tampered with or modified externally.
  - PASS: Real remote mutation qualification in disposable runners passes completely.

### Milestone M — Universal Whole-System Optimization, CPU/GPU/Storage/Power Intelligence & Continuous Adaptive Runtime (Completed)
- **Objective**: Expand HyperCore into a working whole-system adaptive optimization engine coordinating CPU, GPU-awareness, Storage/Block I/O, Power/Thermal, and continuous adaptive runtime.
- **Scope**:
  - Expanded `TypedMutation` for CPU governor, EPP, frequency bounds, block I/O schedulers, read-ahead, and power profiles.
  - Universal CPU policy planner (`CpuPlanner`) supporting Intel, AMD, ARM64, and virtualized systems with thermal safety.
  - GPU-aware discovery and coordination (`GpuPlanner`) with shared system memory awareness and safe observe-only defaults.
  - Storage block I/O optimization (`StoragePlanner`) aligning schedulers (`none`, `mq-deadline`, `bfq`) to media characteristics.
  - Platform power and thermal planner (`PowerPlanner`) integrating ACPI platform profiles and coexisting with TLP/thermald.
  - Unified multi-resource arbitration engine (`WholeSystemPlanner`) deferring aggressive tuning under thermal stress.
  - Cryptographic SHA-256 ownership ledger integrity and hardened multi-scope authorization model.
  - Guided setup CLI workflows (`setup --auto`, `setup --apply --authorized`, `setup --rollback`).
- **Acceptance Criteria**:
  - PASS: All whole-system mutations strictly require explicit typed authorization and valid SHA-256 plan fingerprint.
  - PASS: Observe-only defaults for non-mutating discovery; respectful coexistence with third-party tools.
  - PASS: Complete verified atomic rollback for all subsystems with zero residual drop-in files.
  - PASS: Real disposable remote qualification passes across all 10 qualification scenarios.
  - PASS: Physical read-only qualification on Intel N5000 / UHD 605 confirms zero drift and preserves existing configuration.

### Milestone N — Continuous Self-Tuning Daemon & Workload-Aware Background Controller (Next)
- **Objective**: Implement production-grade autonomous background daemon (`xynoosd`) with systemd service integration and real-time PSI-driven continuous self-tuning.
- **Scope**:
  - Production systemd unit integration (`xynoosd.service`) with watchdog and cgroup management.
  - Real-time PSI telemetry loop with hysteresis and intervention budget bounding.
  - Workload-aware continuous dynamic memory reclaim and CPU weight adjustment.
  - Battery/AC dynamic policy transitions and thermal throttling mitigation.
- **Acceptance Criteria**:
  - Steady-state daemon memory footprint strictly under 10 MiB RSS.
  - Zero performance degradation or CPU spikes during background operation.
  - Resilient fail-safe recovery on unhandled kernel pressure events.

### Milestone O — Full Cross-Subsystem Automatic Setup & System Coexistence
- **Objective**: Deliver end-to-end multi-subsystem `xynoos setup --auto` coordination.
- **Scope**:
  - Unified one-time setup orchestrating CPU, Memory, GPU, Storage, and Power policies.
  - Global cross-subsystem conflict resolution.
  - Whole-system transactional verification and unified multi-file rollback.
- **Acceptance Criteria**:
  - Zero uncoordinated writes across disparate hardware subsystems.
  - Verified atomic restoration back to baseline on any failure or user rollback request.

### Milestone P — Cross-Distribution Packaging & Init Compatibility
- **Objective**: Broaden package availability across major Linux distributions.
- **Scope**:
  - Native packages for Debian/Ubuntu (.deb), Fedora/RHEL (.rpm), Arch (PKGBUILD), Alpine (APK), Void (XBPS).
  - Service unit support for Systemd, SysVinit, OpenRC, and Runit.
- **Acceptance Criteria**:
  - Clean lintian and distribution packaging compliance.
  - Disposable container lifecycle verification across all target distros.

### Milestone Q — Whole-System Adaptive Performance Qualification
- **Objective**: Rigorous empirical performance qualification across physical hardware tiers.
- **Scope**:
  - Long-duration soak testing under memory pressure, compilation, gaming, and multitasking.
  - Physical reference qualification on Intel Pentium Silver N5000.
  - Independent statistical audit of performance gains versus un-tuned baseline.
- **Acceptance Criteria**:
  - Statistically significant reduction in latency / PSI stalls demonstrated.
  - Steady-state daemon memory footprint remains strictly under 10 MiB.

### Milestone R — Stable Universal Product Release (v1.0.0)
- **Objective**: First production-ready general release.
- **Scope**:
  - Independent security audit and vulnerability assessment.
  - Production cryptographic signing and immutable release distribution.
  - Stable CLI/API contracts.
- **Acceptance Criteria**:
  - All outstanding security and infrastructure blockers resolved.
  - Formal maintainer sign-off.
