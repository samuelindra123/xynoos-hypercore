# XYNOOS HyperCore — Physical Hardware Qualification Runbook
## Target Hardware: Intel Pentium Silver N5000 / MX Linux (Operator-Controlled HIL Procedure)

- **Target Device**: Intel Pentium Silver N5000 (4C/4T @ 1.10 GHz – 2.70 GHz)
- **Target Memory**: 4 GB LPDDR4 / DDR4
- **Target Graphics**: Intel UHD Graphics 605
- **Operating System**: MX Linux 23 / Debian 12 (Bookworm) / Debian 13 (Trixie)
- **Init Systems Supported**: SysVinit (default on MX Linux) or systemd
- **Target Package**: `xynoos-hypercore` (`v0.1.0-rc.1` / `0.1.0-preview.3`)
- **Execution Policy**: Operator-controlled only. DO NOT run without explicit operator authorization.
- **Current Status**: **PREPARED / NOT RUN (Pending Operator Execution on Physical Laptop)**

> [!IMPORTANT]
> This runbook is strictly operator-guided. Never run intrusive benchmarks automatically.
> HyperCore defaults to Observe-Only mode. All intrusive cgroup modifications are restricted to disposable child cgroups and require explicit flag authorization.
> If any abort condition is met, execute **Stage 9 (Uninstallation and Rollback)** immediately.

---

## Stage 1: Read-Only Environment Preflight

Verify system specifications, active memory subsystems, and kernel interfaces without making any mutations.

### Commands:
```bash
# 1. Verify CPU model and architecture
lscpu | grep -E 'Model name|CPU\(s\)|Architecture|Thread'

# 2. Verify total RAM, swap devices, and ZRAM
free -h
cat /proc/swaps
ls -la /sys/block/zram* 2>/dev/null || echo "No zram block devices found"

# 3. Verify Linux kernel version and PSI controllers
uname -a
ls -la /proc/pressure

# 4. Verify cgroup v2 mount and delegation
stat -f -c %T /sys/fs/cgroup
cat /sys/fs/cgroup/cgroup.controllers 2>/dev/null || true
```

### Expected Output:
- Model name contains: `Intel(R) Pentium(R) Silver N5000 CPU`
- Architecture: `x86_64`
- Total memory: ~3.7 GiB – 3.8 GiB visible
- Filesystem type for `/sys/fs/cgroup`: `cgroup2fs`
- `/proc/pressure` contains `cpu`, `io`, and `memory` files.

### Abort Conditions:
- System is not x86_64 or memory is less than 1.5 GiB.
- Kernel lacks cgroup v2 or PSI support (`/proc/pressure` missing).

---

## Stage 2: Package Authenticity Verification

Verify the integrity and provenance of the Debian release candidate package prior to installation.

### Commands:
```bash
# 1. Download or locate release candidate artifact
PKG="xynoos-hypercore_0.1.0-preview.3_amd64.deb"
sha256sum "$PKG"

# 2. Inspect package metadata and control scripts
dpkg-deb -I "$PKG"
dpkg-deb -c "$PKG"
```

### Expected Output:
- Package name: `xynoos-hypercore`
- Architecture: `amd64`
- Conffiles: `/etc/xynoos/hypercore.json`
- Binaries: `/usr/bin/xynoos`
- Init scripts: `/etc/init.d/xynoos-hypercore`, `/lib/systemd/system/xynoos-hypercore.service`

### Abort Conditions:
- Package contains unexpected files outside `/usr/bin`, `/etc/xynoos`, `/etc/init.d`, `/lib/systemd`.
- Package attempts to mutate global sysctl or system files during pre-install.

---

## Stage 3: Installation Safety Verification

Install the package via standard Debian package management and verify that NO services or intrusive settings are activated as a side-effect.

### Commands:
```bash
# 1. Install package using dpkg or apt
sudo dpkg -i "$PKG"

# 2. Check service status immediately after installation (must be INACTIVE / STOPPED)
if [ -d /run/systemd/system ]; then
    systemctl is-active xynoos-hypercore || true
    systemctl is-enabled xynoos-hypercore || true
else
    /etc/init.d/xynoos-hypercore status || true
fi

# 3. Verify binary version and health
xynoos --version
xynoos doctor
```

### Expected Output:
- Service is NOT active immediately upon installation.
- `xynoos --version` prints `xynoos 0.1.0-preview.3` or release candidate tag.
- `xynoos doctor` runs in observe-only mode, reporting CPU, RAM, PSI, and storage availability.

### Abort Conditions:
- Package installation automatically enabled or started the background daemon.
- Package altered existing `/etc/fstab`, sysctl, or swap priorities.

---

## Stage 4: ObserveOnly Execution

Run the daemon in safe observe-only mode for a bounded duration.

### Commands:
```bash
# Start daemon in ObserveOnly mode directly in terminal for 30 seconds
xynoos daemon --mode observe-only --interval-ms 1000 --status
```

### Expected Output:
- Output shows periodic telemetry snapshots (every 1000 ms).
- Process operates without root privileges.
- Zero kernel writes or cgroup modifications.

### Abort Conditions:
- Daemon crashes, panics, or attempts any write to `/sys/fs/cgroup`.

---

## Stage 5: Daemon Resource Overhead Measurement

Measure the exact memory (RSS, PSS) and CPU overhead of the running daemon on physical hardware.

### Commands:
```bash
# 1. Launch daemon in background with PID file
xynoos daemon --mode observe-only --interval-ms 1000 --pid-file /tmp/test-xynoos.pid > /tmp/xynoos-daemon.log 2>&1 &
DAEMON_PID=$!
sleep 5

# 2. Inspect memory and file descriptors
cat /proc/$DAEMON_PID/status | grep -E 'VmRSS|VmHWM|VmPeak|voluntary'
ls -l /proc/$DAEMON_PID/fd | wc -l

# 3. Terminate test daemon cleanly
kill -TERM $DAEMON_PID
wait $DAEMON_PID 2>/dev/null || true
rm -f /tmp/test-xynoos.pid /tmp/xynoos-daemon.log
```

### Expected Output:
- `VmRSS`: < 5.0 MiB (< 5120 KiB).
- File descriptor count: <= 5.
- Clean shutdown on `SIGTERM`.

### Abort Conditions:
- `VmRSS` exceeds 10 MiB.
- Daemon fails to terminate cleanly on SIGTERM within 3 seconds.

---

## Stage 6: Advisory Evaluation

Run the daemon in Advisory mode to verify recommendation generation without intrusive kernel actuation.

### Commands:
```bash
xynoos daemon --mode advisory --interval-ms 500 --status
```

### Expected Output:
- Daemon evaluates multi-resource pressure (CPU, memory, I/O PSI).
- Recommends cgroup or tiering actions when memory pressure increases.
- All kernel execution remains declined (`Outcome::Declined`).

---

## Stage 7: Separately Authorized Isolated Transactional Testing

Test transactional cgroup control **exclusively** inside an isolated, non-root delegated child cgroup.

> [!WARNING]
> Requires explicit operator flag `--confirm-hardware-audit`.
> NEVER target system slices or root cgroups.

### Commands:
```bash
# 1. Create isolated disposable child cgroup under delegated user slice
TARGET_CG="/sys/fs/cgroup/user.slice/user-$(id -u).slice/xynoos-test-$(date +%s)"
mkdir -p "$TARGET_CG"

# 2. Run diagnostic preflight verification
xynoos doctor --json > /tmp/doctor-hil.json

# 3. Clean up child cgroup
rmdir "$TARGET_CG" 2>/dev/null || true
```

### Expected Output:
- Diagnostic doctor validates cgroup v2 presence and ownership.
- Disposable child cgroup is removed cleanly without remnants.

---

## Stage 8: Cleanup Verification

Verify that no temporary files, locks, or orphaned cgroups exist.

### Commands:
```bash
ls -la /run/xynoos 2>/dev/null || echo "Clean /run/xynoos"
find /sys/fs/cgroup -maxdepth 4 -name "xynoos-*" 2>/dev/null || true
```

### Expected Output:
- No orphaned PID files or journals.
- No dangling `xynoos-*` test cgroups.

---

## Stage 9: Uninstallation and Rollback

Perform complete uninstallation of the package and verify clean host rollback.

### Commands:
```bash
# 1. Stop service if running
sudo /etc/init.d/xynoos-hypercore stop 2>/dev/null || true

# 2. Remove package
sudo dpkg -r xynoos-hypercore

# 3. Verify clean system state
which xynoos || echo "Binary cleanly removed"
dpkg -l | grep xynoos || echo "Package cleanly removed"
```

### Expected Output:
- Binary `/usr/bin/xynoos` removed.
- Service definitions removed.
- Zero leftover configuration in active init paths.

---

## Stage 10: Lightweight Evidence Report

Generate a summary report of physical qualification execution.

### Template:
```markdown
# Physical Hardware Qualification Evidence Report
- **Date**: YYYY-MM-DD
- **Operator**: [Name/Handle]
- **Hardware**: Intel Pentium Silver N5000 / 4 GB RAM / MX Linux 23
- **Kernel**: [uname -r]
- **Stages Completed**: 1 through 10
- **Daemon RSS**: [Measured KiB]
- **Verdict**: PASS / FAIL / BLOCKED
```
