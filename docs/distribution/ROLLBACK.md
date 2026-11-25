# XYNOOS HyperCore — Deployment Rollback Procedures
## Deterministic, Safe, and Reversible Operational Recovery

This document provides explicit, evidence-based recovery and rollback procedures for administrators and operators running XYNOOS HyperCore on Linux systems (systemd and SysVinit / MX Linux).

---

## 1. Safety Principles

1. **Observe-Only Default**: When in doubt, switch the daemon back to ObserveOnly mode (`--mode observe-only`). In ObserveOnly mode, HyperCore performs zero kernel mutations and zero memory modifications.
2. **Never Blindly Reset Global Kernel Parameters**: Do not execute blind sysctl resets, swap reconfiguration, or global zram resizing. Rollback only touches owned, delegated resources.
3. **Preserve Diagnostic Evidence**: If an error, crash, or journal conflict occurs, preserve the diagnostic logs in `/run/xynoos/` or `/var/log/` before removing temporary files.

---

## 2. Failure Scenarios and Resolution

### Scenario A — Invalid Configuration
**Symptoms**:
- `xynoos daemon` refuses to start with `Invalid daemon configuration: ...`
- `xynoos config validate` returns `✗ Configuration invalid: ...`

**Procedure**:
1. Run preflight validation to locate the invalid parameter:
   ```bash
   xynoos config validate --file /etc/xynoos/hypercore.json
   ```
2. If invalid, restore the known-good default configuration:
   ```bash
   cp /etc/xynoos/hypercore.json.bak /etc/xynoos/hypercore.json 2>/dev/null || rm -f /etc/xynoos/hypercore.json
   ```
3. Verify that default configuration passes validation:
   ```bash
   xynoos config validate
   ```

---

### Scenario B — Failed Daemon Startup (PID Lock Conflict / Stale PID)
**Symptoms**:
- Daemon fails startup with `Daemon sudah berjalan dengan PID ... (lock: /run/xynoos/hypercore.pid)`

**Procedure**:
1. Check daemon status:
   ```bash
   xynoos doctor
   ```
2. If `daemon_state` reports `stale_pid_file` (e.g., after an unexpected reboot or hard crash):
   ```bash
   # HyperCore's PidLock automatically cleans up stale PIDs on next startup if boot_id or start_ticks differ.
   # For manual operator verification:
   sudo rm -f /run/xynoos/hypercore.pid
   ```
3. Restart the service:
   - Systemd: `sudo systemctl restart xynoos-hypercore`
   - SysVinit: `sudo /etc/init.d/xynoos-hypercore restart`

---

### Scenario C — Pending Transaction Recovery Journal / Conflict
**Symptoms**:
- Daemon startup logs: `Konflik eksternal terdeteksi pada jurnal Pending/Applied` or `Recovery conflict detected`
- Journal file `/run/xynoos/hypercore_journal.json` or `/run/xynoos/lease_journal.json` present.

**Procedure**:
1. Check journal status via doctor:
   ```bash
   xynoos doctor
   ```
2. HyperCore's `TransactionJournal` is strictly fail-closed: it restores original cgroup values if and only if HyperCore's requested value remains active. If an external administrator or tool modified the control value, or if the cgroup was replaced with a new inode or boot ID changed, it marks `Conflict` or `Invalidated` and refuses rollback to prevent clobbering external state.
3. If the cgroup workload has already terminated and the parent cgroup is clean:
   ```bash
   # Preserve forensic backup
   sudo cp /run/xynoos/hypercore_journal.json /run/xynoos/hypercore_journal.json.bak 2>/dev/null || true
   sudo cp /run/xynoos/lease_journal.json /run/xynoos/lease_journal.json.bak 2>/dev/null || true
   sudo rm -f /run/xynoos/hypercore_journal.json /run/xynoos/lease_journal.json
   ```
4. Start daemon in ObserveOnly mode to inspect system telemetry safely:
   ```bash
   xynoos daemon --mode observe-only --status
   ```

---

### Scenario D — Failed Package Upgrade
**Symptoms**:
- Upgraded package binary fails health checks or exhibits regressions.

**Procedure (Debian / MX Linux / APT)**:
1. Stop the active daemon:
   ```bash
   sudo systemctl stop xynoos-hypercore || sudo /etc/init.d/xynoos-hypercore stop
   ```
2. Downgrade to the previous verified preview package:
   ```bash
   sudo apt install --allow-downgrades xynoos-hypercore=<previous-version>
   ```
3. Verify binary version:
   ```bash
   xynoos --version
   ```
4. Verify daemon health:
   ```bash
   xynoos doctor
   ```

---

### Scenario E — Package Removal & Clean Purge
**Symptoms**:
- Decommissioning HyperCore or testing fresh deployment.

**Procedure**:
1. Remove package (preserves logs and configuration):
   ```bash
   sudo apt remove xynoos-hypercore
   ```
2. Purge package (removes all owned configuration in `/etc/xynoos/` and runtime files in `/run/xynoos/`):
   ```bash
   sudo apt purge xynoos-hypercore
   ```
3. Verify that zero processes remain running:
   ```bash
   pgrep -x xynoos || echo "Clean: no processes running"
   ```
4. Confirm host kernel and swap remain untouched:
   - No sysctl settings were modified by packaging.
   - Global swap, zram, and CPU governors remain exactly as configured by the host distribution.
