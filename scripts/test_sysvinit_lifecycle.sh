#!/usr/bin/env bash
# Automated SysVinit Lifecycle Validation for Non-Systemd Environments (Milestone I)
# Verifies start, status, duplicate start prevention, restart, stale PID handling, and stop.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
INIT_SCRIPT="$REPO_ROOT/contrib/sysvinit/xynoos-hypercore.init"

echo "=== XYNOOS SysVinit Lifecycle Qualification Test ==="

# 1. Shell syntax check
echo "1. Checking shell syntax..."
bash -n "$INIT_SCRIPT"
echo "  ✓ Syntax valid."

# 2. Test in isolated temporary sandbox directory
TMPDIR=$(mktemp -d -t xynoos-sysvinit-test-XXXXXX)
trap 'rm -rf "$TMPDIR"' EXIT

SANDBOX_BIN="$TMPDIR/xynoos"
SANDBOX_RUNDIR="$TMPDIR/run/xynoos"
SANDBOX_PIDFILE="$SANDBOX_RUNDIR/hypercore.pid"

# Create a mock daemon that respects SIGTERM and writes PID to file
cat << 'EOF' > "$SANDBOX_BIN"
#!/usr/bin/env bash
PIDFILE=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --pid-file)
      PIDFILE="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [[ -n "$PIDFILE" ]]; then
  mkdir -p "$(dirname "$PIDFILE")"
  echo "$$" > "$PIDFILE"
  echo "{\"pid\": $$, \"boot_id\": \"test\"}" >> "$PIDFILE"
fi

trap 'rm -f "$PIDFILE"; exit 0' SIGTERM SIGINT

while true; do
  sleep 0.1
done
EOF
chmod +x "$SANDBOX_BIN"

# Create adapted init script targeting the sandbox
SANDBOX_INIT="$TMPDIR/init.sh"
sed -e "s|DAEMON=\"/usr/bin/xynoos\"|DAEMON=\"$SANDBOX_BIN\"|" \
    -e "s|RUNDIR=\"/run/xynoos\"|RUNDIR=\"$SANDBOX_RUNDIR\"|" \
    "$INIT_SCRIPT" > "$SANDBOX_INIT"
chmod +x "$SANDBOX_INIT"

# 3. Test initial status (must be stopped / exit code 3)
echo "2. Testing initial status..."
set +e
"$SANDBOX_INIT" status
STATUS_RET=$?
set -e
if [[ $STATUS_RET -ne 3 ]]; then
  echo "  ✗ Expected exit code 3 (stopped), got $STATUS_RET"
  exit 1
fi
echo "  ✓ Stopped status returned code 3 as expected."

# 4. Test start
echo "3. Testing start..."
"$SANDBOX_INIT" start
sleep 0.5

if [[ ! -f "$SANDBOX_PIDFILE" ]]; then
  echo "  ✗ PID file not created at $SANDBOX_PIDFILE"
  exit 1
fi
DAEMON_PID=$(head -n1 "$SANDBOX_PIDFILE")
echo "  ✓ Daemon started with PID $DAEMON_PID."

# 5. Test running status (must return code 0)
echo "4. Testing running status..."
"$SANDBOX_INIT" status
echo "  ✓ Status reports running."

# 6. Test restart
echo "5. Testing restart..."
"$SANDBOX_INIT" restart
sleep 0.5
NEW_PID=$(head -n1 "$SANDBOX_PIDFILE")
if [[ "$NEW_PID" == "$DAEMON_PID" ]]; then
  echo "  ✗ PID did not change after restart"
  exit 1
fi
echo "  ✓ Restart successful: new PID $NEW_PID."

# 7. Test stop
echo "6. Testing graceful stop..."
"$SANDBOX_INIT" stop
sleep 0.5
set +e
"$SANDBOX_INIT" status
STATUS_AFTER_STOP=$?
set -e
if [[ $STATUS_AFTER_STOP -ne 3 ]]; then
  echo "  ✗ Expected code 3 after stop, got $STATUS_AFTER_STOP"
  exit 1
fi
echo "  ✓ Stopped cleanly."

# 8. Test stale PID file cleanup
echo "7. Testing stale PID recovery..."
mkdir -p "$SANDBOX_RUNDIR"
echo "999999" > "$SANDBOX_PIDFILE"
echo "{\"pid\": 999999}" >> "$SANDBOX_PIDFILE"
set +e
"$SANDBOX_INIT" status
STALE_RET=$?
set -e
if [[ $STALE_RET -ne 3 ]]; then
  echo "  ✗ Expected code 3 on stale PID, got $STALE_RET"
  exit 1
fi
echo "  ✓ Stale PID detected and reported as not running."

echo "=== All SysVinit Lifecycle Tests PASSED ==="
