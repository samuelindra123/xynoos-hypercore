#!/bin/sh
# Read-only inspection only. A transient service creates its own systemd cgroup.
set -eu
binary=${XYNOOS_EXPERIMENT_BINARY:-"$(dirname "$0")/optimization_experiment"}
binary=$(realpath "$binary")
case "${1:-}" in
  preflight)
    exec "$binary" preflight
    ;;
  unit-preflight)
    # This starts a transient user service; it does not activate controllers itself.
    unit="xynoos-018d-$$-$(date +%s)"
    output=$(mktemp)
    trap 'rm -f "$output"' EXIT HUP INT TERM
    if systemd-run --user --wait --pipe --collect --unit="$unit" \
      --property=Type=exec --property='Delegate=cpu memory pids' \
      --property=DelegateSubgroup=manager \
      "$binary" manager-preflight "$unit.service" >"$output"; then
      status=0
    else
      status=$?
    fi
    cat "$output"
    # Verify the unit is inactive or collected; failures remain visible.
    if ! state=$(systemctl --user show "$unit.service" --property=LoadState --property=ActiveState); then
      echo "unit termination unverified: $unit.service" >&2
      exit 1
    fi
    case "$state" in
      *'LoadState=not-found'*|*'ActiveState=inactive'*)
        echo "unit inactive or collected: $unit.service" >&2 ;;
      *'ActiveState=failed'*) echo "unit failed: $unit.service; cgroup removal not verified" >&2; exit 1 ;;
      *) echo "unit termination unverified: $unit.service ($state)" >&2; exit 1 ;;
    esac
    if ! python3 - "$output" "$unit.service" <<'PYCODE'
import json, os, sys, time
try:
    with open(sys.argv[1], encoding="utf-8") as stream:
        records = [json.loads(line) for line in stream if line.strip().startswith("{")]
    record, = [r for r in records if r.get("record") == "manager_preflight"]
    path = record["parent_path"]
    if not os.path.isabs(path) or os.path.basename(path) != sys.argv[2]:
        raise ValueError("unexpected parent path")
    for _ in range(50):
        os.stat(os.path.dirname(path))
        try:
            os.lstat(path)
        except FileNotFoundError:
            print("cgroup removal verified: " + path, file=sys.stderr)
            break
        time.sleep(0.1)
    else:
        raise ValueError("cgroup still present: " + path)
except (OSError, ValueError, KeyError, TypeError) as error:
    print("cgroup cleanup unverified: " + str(error), file=sys.stderr)
    sys.exit(1)
PYCODE
    then
      exit 1
    fi
    exit "$status"
    ;;
  *)
    echo 'Usage: xynoos-018d-launcher.sh preflight|unit-preflight' >&2
    echo 'unit-preflight starts a transient systemd user service; no controller activation is implemented.' >&2
    exit 2
    ;;
esac
