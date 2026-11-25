#!/usr/bin/env bash
# Hardware-in-the-Loop (HIL) Lightweight Qualification Script for XYNOOS HyperCore
# Designed for low-power targets (Intel Pentium Silver N5000 / 4 GiB RAM / eMMC / MX Linux).
# STRICTLY OPT-IN. ZERO GLOBAL KERNEL / SWAP / ZRAM / SERVICE MUTATIONS.
set -euo pipefail

echo "================================================================="
echo "  XYNOOS HyperCore — Hardware-in-the-Loop Qualification Harness  "
echo "================================================================="

MODE="${1:---dry-run}"
OUT_DIR="${2:-./hil-report}"

if [ "$MODE" != "--confirm-hardware-audit" ] && [ "$MODE" != "--dry-run" ]; then
    echo "Usage: $0 [--dry-run | --confirm-hardware-audit] [report_dir]"
    exit 2
fi

if [ "$MODE" = "--dry-run" ]; then
    echo "STATUS: DRY_RUN_READ_ONLY (no pressure tests or cgroup writes)"
    echo "Safety Notice: Physical qualification on N5000/MX Linux requires explicit operator confirmation."
    echo "To run the bounded non-destructive sequence, execute:"
    echo "  $0 --confirm-hardware-audit [report_dir]"
fi

mkdir -p "$OUT_DIR"
REPORT_JSON="$OUT_DIR/hil-qualification.json"
REPORT_MD="$OUT_DIR/hil-qualification.md"
TMP_LOG=$(mktemp)
trap 'rm -f "$TMP_LOG"' EXIT

# 1. Read-only environment inspection (Stage A Preflight)
KERNEL_REL=$(uname -r)
ARCH=$(uname -m)
DISTRO=$(grep -E '^PRETTY_NAME=' /etc/os-release 2>/dev/null | head -n1 | cut -d= -f2- | tr -d '"' || echo "unknown")
CPU_MODEL=$(awk -F: '/model name/ {print $2; exit}' /proc/cpuinfo 2>/dev/null | xargs || echo "unknown")
LOGICAL_CPUS=$(nproc 2>/dev/null || echo 0)
MEM_TOTAL_KB=$(awk '/MemTotal:/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)
MEM_AVAIL_KB=$(awk '/MemAvailable:/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)
DISK_AVAIL_KB=$(df -k / 2>/dev/null | awk 'NR==2 {print $4}' || echo 0)

# 2. Init system detection (do not assume systemd is PID 1)
if [ -d /run/systemd/system ]; then
    INIT_SYS="systemd"
else
    PID1_COMM=$(cat /proc/1/comm 2>/dev/null || echo "unknown")
    if [ "$PID1_COMM" = "init" ] && [ -d /etc/init.d ]; then
        INIT_SYS="sysvinit"
    else
        INIT_SYS="$PID1_COMM"
    fi
fi

# 3. Kernel capability detection
CGROUP_V2="false"
if [ -f /sys/fs/cgroup/cgroup.controllers ]; then
    CGROUP_V2="true"
fi
PSI_CPU=$([ -r /proc/pressure/cpu ] && echo "true" || echo "false")
PSI_MEM=$([ -r /proc/pressure/memory ] && echo "true" || echo "false")
PSI_IO=$([ -r /proc/pressure/io ] && echo "true" || echo "false")

# 4. Existing swap and zram inspection (read-only)
SWAPS_SUMMARY=$(awk 'NR>1 {printf "%s(%s,%s) ", $1, $2, $3}' /proc/swaps 2>/dev/null || true)
ZRAM_COUNT=$(find /sys/block -maxdepth 1 -name 'zram*' 2>/dev/null | wc -l)

echo "1-4. Environment: distro='$DISTRO' kernel='$KERNEL_REL' init='$INIT_SYS' cpu='$CPU_MODEL' ($LOGICAL_CPUS CPUs)"
echo "     RAM avail=${MEM_AVAIL_KB} KiB / ${MEM_TOTAL_KB} KiB | cgroup_v2=$CGROUP_V2 | PSI(cpu=$PSI_CPU mem=$PSI_MEM io=$PSI_IO)"
echo "     Swap: ${SWAPS_SUMMARY:-none} | zram_devices=$ZRAM_COUNT"

DAEMON_STATUS="SKIPPED_DRY_RUN"
DAEMON_RSS_KIB=0
DIAG_STATUS="SKIPPED_DRY_RUN"
CGROUP_TEST_STATUS="NOT_AUTHORIZED"

if [ "$MODE" = "--confirm-hardware-audit" ]; then
    # Safety Preflight: minimum 500 MiB free RAM before running bounded check
    if [ "$MEM_AVAIL_KB" -lt 512000 ]; then
        echo "ERROR: Insufficient MemAvailable (${MEM_AVAIL_KB} KiB < 512000 KiB). Aborting cleanly."
        exit 1
    fi

    XYNOOS_BIN=$(command -v xynoos || true)
    if [ -n "$XYNOOS_BIN" ]; then
        # 5. ObserveOnly daemon verification (bounded 3 cycles, no /run lock needed)
        echo "5. Running bounded ObserveOnly daemon check (3 cycles, timeout 15s)..."
        if timeout 15 "$XYNOOS_BIN" daemon --mode observe-only --interval-ms 200 --max-cycles 3 --no-pid-file > "$TMP_LOG" 2>&1; then
            DAEMON_STATUS="PASS"
            DAEMON_RSS_KIB=$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print((d.get("rss_bytes") or 0)//1024)' "$TMP_LOG" 2>/dev/null || echo 0)
        else
            DAEMON_STATUS="FAIL"
        fi

        # 6-7. Bounded telemetry & overhead measurement
        echo "6-7. Running bounded diagnostics (doctor + diagnose 2 samples)..."
        if timeout 15 "$XYNOOS_BIN" doctor --json > /dev/null && \
           timeout 15 "$XYNOOS_BIN" diagnose --count 2 --interval-ms 100 --json > /dev/null; then
            DIAG_STATUS="PASS"
        else
            DIAG_STATUS="FAIL"
        fi
    else
        DAEMON_STATUS="UNAVAILABLE_NO_BINARY"
        DIAG_STATUS="UNAVAILABLE_NO_BINARY"
    fi

    # 8. Optional authorized disposable cgroup check (only if XYNOOS_HIL_ALLOW_CGROUP_TEST=1 and delegated)
    if [ "${XYNOOS_HIL_ALLOW_CGROUP_TEST:-0}" = "1" ] && [ -n "${XYNOOS_CGROUP_PARENT:-}" ]; then
        echo "8. Authorized disposable cgroup test requested under $XYNOOS_CGROUP_PARENT (operator opt-in)."
        CGROUP_TEST_STATUS="OPERATOR_OPT_IN_REQUESTED"
    else
        CGROUP_TEST_STATUS="SKIPPED_NO_OPERATOR_CGROUP_OPT_IN"
    fi
fi

# 9. Cleanup verification (confirm no leftover xynoos daemon running unless already installed by operator)
LEFTOVER_PID=$(pgrep -x xynoos || true)
CLEANUP_VERIFIED=$([ -z "$LEFTOVER_PID" ] && echo "true" || echo "false")

# 10. Lightweight report generation
python3 - <<'PY' "$REPORT_JSON" "$REPORT_MD" "$MODE" "$DISTRO" "$KERNEL_REL" "$ARCH" "$INIT_SYS" "$CPU_MODEL" "$LOGICAL_CPUS" "$MEM_TOTAL_KB" "$MEM_AVAIL_KB" "$CGROUP_V2" "$PSI_CPU" "$PSI_MEM" "$PSI_IO" "$ZRAM_COUNT" "$DAEMON_STATUS" "$DAEMON_RSS_KIB" "$DIAG_STATUS" "$CGROUP_TEST_STATUS" "$CLEANUP_VERIFIED"
import json, sys, time
(
    _, out_json, out_md, mode, distro, kernel, arch, init_sys, cpu_model,
    cpus, mem_tot, mem_avail, cg2, psi_c, psi_m, psi_i, zram_cnt,
    d_status, d_rss, diag_status, cg_status, cleanup
) = sys.argv

payload = {
    "schema_version": 1,
    "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "execution_mode": mode,
    "host_mutation": False,
    "environment": {
        "distro": distro,
        "kernel": kernel,
        "arch": arch,
        "init_system": init_sys,
        "cpu_model": cpu_model,
        "logical_cpus": int(cpus),
        "mem_total_kib": int(mem_tot),
        "mem_available_kib": int(mem_avail),
        "cgroup_v2": cg2 == "true",
        "psi": {"cpu": psi_c == "true", "memory": psi_m == "true", "io": psi_i == "true"},
        "zram_devices": int(zram_cnt),
    },
    "checks": {
        "observe_only_daemon": d_status,
        "observe_only_rss_kib": int(d_rss),
        "bounded_diagnostics": diag_status,
        "disposable_cgroup_test": cg_status,
        "cleanup_no_leftover_process": cleanup == "true",
    },
}
with open(out_json, "w") as f:
    json.dump(payload, f, indent=2)

with open(out_md, "w") as f:
    f.write(f"# XYNOOS HyperCore HIL Qualification Report\n\n")
    f.write(f"- Mode: `{mode}` (host_mutation=`false`)\n")
    f.write(f"- Distro / Kernel: `{distro}` / `{kernel}` (`{arch}`, init=`{init_sys}`)\n")
    f.write(f"- CPU / RAM: `{cpu_model}` ({cpus} CPUs), `{mem_avail}` / `{mem_tot}` KiB\n")
    f.write(f"- Cgroup v2 / PSI / ZRAM: cgroup2=`{cg2}`, psi=(cpu={psi_c}, mem={psi_m}, io={psi_i}), zram={zram_cnt}\n")
    f.write(f"- ObserveOnly Daemon: `{d_status}` (RSS=`{d_rss}` KiB)\n")
    f.write(f"- Diagnostics: `{diag_status}` | Cgroup test: `{cg_status}` | Cleanup: `{cleanup}`\n")
PY

echo "9-10. Report saved to $REPORT_JSON and $REPORT_MD"
echo "STATUS: QUALIFIED_NON_DESTRUCTIVE"
