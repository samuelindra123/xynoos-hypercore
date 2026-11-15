#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cat > "$tmp/systemd-run" <<'MOCK'
#!/bin/sh
for arg do case "$arg" in --unit=*) unit=${arg#--unit=};; esac; done
if [ "${CASE:-}" = missing ]; then exit 1; fi
printf '{"record":"manager_preflight","parent_path":"%s/%s.service"}\n' "$FIXTURE_DIR" "$unit"
MOCK
cat > "$tmp/systemctl" <<'MOCK'
#!/bin/sh
if [ "${CASE:-}" = failed ]; then printf 'LoadState=loaded\nActiveState=failed\n'; else printf 'LoadState=not-found\nActiveState=inactive\n'; fi
MOCK
chmod +x "$tmp/systemd-run" "$tmp/systemctl"
export FIXTURE_DIR="$tmp" PATH="$tmp:$PATH" XYNOOS_EXPERIMENT_BINARY=/bin/true
"$root/scripts/xynoos-018d-launcher.sh" unit-preflight >"$tmp/out" 2>&1
rg -q 'cgroup removal verified' "$tmp/out"
for CASE in missing failed; do
  export CASE
  if "$root/scripts/xynoos-018d-launcher.sh" unit-preflight >"$tmp/out" 2>&1; then
    echo "fixture unexpectedly passed: $CASE" >&2; exit 1
  fi
  case "$CASE" in missing) rg -q 'cleanup unverified' "$tmp/out";; failed) rg -q 'unit failed' "$tmp/out";; esac
done
printf '%s\n' 'launcher fixtures passed'
