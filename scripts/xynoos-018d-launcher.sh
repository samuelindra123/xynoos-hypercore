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
    exec systemd-run --user --wait --pipe --collect --unit="$unit" \
      --property=Type=exec --property='Delegate=cpu memory pids' \
      --property=DelegateSubgroup=manager \
      "$binary" manager-preflight "$unit.service"
    ;;
  *)
    echo 'Usage: xynoos-018d-launcher.sh preflight|unit-preflight' >&2
    echo 'unit-preflight starts a transient systemd user service; no controller activation is implemented.' >&2
    exit 2
    ;;
esac
