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
│  Milestone L: Universal Memory, Zram, Zswap, Swap & OOM Optimization   │
│  - Dynamic ZRAM creation, algorithm selection (LZ4/ZSTD), swap priority│
│  - Zswap vs Zram coordination, swappiness/vfs_cache_pressure tuning    │
│  - Cooperative OOM management integration without process double-kills │
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
│  Milestone O: Automatic One-Time Setup, Ownership & Safe Rollback      │
│  - Authorizable `xynoos setup --apply` execution engine                │
│  - Strict ownership tracking (/etc/xynoos/owned-configs.json)          │
│  - Atomic, idempotent restoration and transactional rollback           │
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

### Milestone K — Universal Hardware Intelligence, Kernel Capability Discovery & Existing Configuration Intelligence (Current)
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

### Milestone L — Universal Memory, Zram, Zswap, Swap & OOM Optimization
- **Objective**: Implement safe, authorizable memory optimization mechanisms.
- **Scope**:
  - Managed ZRAM lifecycle: automatic device sizing, kernel algorithm selection (LZ4, ZSTD, LZO).
  - Swap tiering coordination: prioritizing ZRAM swap over disk swap via priority weighting.
  - Coordination with kernel ZSWAP to eliminate double compression overhead.
  - Swappiness and page-cluster adaptation based on storage backing.
- **Acceptance Criteria**:
  - Requires explicit operator authorization (`--authorized`).
  - No disruption of existing third-party ZRAM managers without user consent.
  - Automatic rollback on daemon exit or failure.

### Milestone M — CPU, Scheduling, Thermal & Power Intelligence
- **Objective**: Coordinate CPU frequency governors, idle states, and scheduling policies.
- **Scope**:
  - Driver-specific scaling coordination (`intel_pstate`, `amd-pstate`, `acpi-cpufreq`).
  - Energy Performance Preference (EPP) tuning based on AC/battery state.
  - Heterogeneous core placement policies for big.LITTLE architectures.
  - CFS autogroup coordination and thermal throttle mitigation.
- **Acceptance Criteria**:
  - Observe-only by default; detect and defer when external daemons (TLP, auto-cpufreq) are active.
  - Fail-safe fallback to distribution default governors.

### Milestone N — GPU-Aware & Storage-Aware Optimization
- **Objective**: Optimize storage scheduling, queues, and graphics power profiles safely.
- **Scope**:
  - Storage media classification: NVMe (`none`), eMMC/SATA SSD (`mq-deadline`/`kyber`), HDD (`bfq`).
  - Dynamic read-ahead scaling for bulk streaming versus random access.
  - Intel/AMD GPU render node awareness for background batch processing deferral.
- **Acceptance Criteria**:
  - Zero direct PCI register writes; interact only through supported kernel sysfs/DRM interfaces.
  - Preserves mount options and filesystem integrity.

### Milestone O — Automatic One-Time Setup, Ownership & Rollback
- **Objective**: Deliver the unified `xynoos setup` experience with safe ownership tracking.
- **Scope**:
  - Single interactive command `xynoos setup --apply`.
  - Generation of `/etc/xynoos/owned-configs.json` tracking all modified parameters and original values.
  - Atomic restoration via `xynoos rollback` or `xynoos uninstall`.
- **Acceptance Criteria**:
  - Zero modifications to un-owned or foreign configuration files.
  - Complete, verifiable restoration back to original state upon rollback.

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
