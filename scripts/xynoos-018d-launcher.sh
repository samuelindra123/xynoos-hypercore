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
    if systemd-run --user --wait --pipe --collect --unit="$unit" \
      --property=Type=exec --property='Delegate=cpu memory pids' \
      --property=DelegateSubgroup=manager \
      "$binary" manager-preflight "$unit.service"; then
      status=0
    else
      status=$?
    fi
    # Verify the unit is inactive or collected; failures remain visible.
    if ! state=$(systemctl --user show "$unit.service" --property=LoadState --property=ActiveState); then
      echo "unit termination unverified: $unit.service" >&2
      exit 1
    fi
    case "$state" in
      *'LoadState=not-found'*|*'ActiveState=inactive'*)
        echo "unit inactive or collected: $unit.service; cgroup removal not verified" >&2 ;;
      *'ActiveState=failed'*) echo "unit failed: $unit.service; cgroup removal not verified" >&2; exit 1 ;;
      *) echo "unit termination unverified: $unit.service ($state)" >&2; exit 1 ;;
    esac
    exit "$status"
    ;;
  *)
    echo 'Usage: xynoos-018d-launcher.sh preflight|unit-preflight' >&2
    echo 'unit-preflight starts a transient systemd user service; no controller activation is implemented.' >&2
    exit 2
    ;;
esac
