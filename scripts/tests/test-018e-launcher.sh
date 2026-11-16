#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
python3 "$root/scripts/tests/test_unit_cleanup.py"
# The shell's read-only preflight routing remains independently executable.
XYNOOS_EXPERIMENT_BINARY=/bin/true "$root/scripts/xynoos-018d-launcher.sh" preflight
