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
    exec python3 "$(dirname "$0")/xynoos-unit-preflight.py" "$binary"
    ;;
  *)
    echo 'Usage: xynoos-018d-launcher.sh preflight|unit-preflight' >&2
    echo 'unit-preflight starts a transient systemd user service; no controller activation is implemented.' >&2
    exit 2
    ;;
esac
