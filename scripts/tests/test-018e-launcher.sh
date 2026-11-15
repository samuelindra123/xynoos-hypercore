#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cat > "$tmp/systemd-run" <<'MOCK'
#!/bin/sh
for arg do case "$arg" in --unit=*) unit=${arg#--unit=};; esac; done
if [ "${CASE:-}" = missing ]; then echo "original diagnostic error" >&2; exit 37; fi
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
  status=0
  "$root/scripts/xynoos-018d-launcher.sh" unit-preflight >"$tmp/out" 2>&1 || status=$?
  if [ "$status" -eq 0 ]; then echo "fixture unexpectedly passed: $CASE" >&2; exit 1; fi
  case "$CASE" in missing) [ "$status" -eq 37 ]; rg -q 'original diagnostic error' "$tmp/out"; rg -q 'transient unit:' "$tmp/out"; rg -q 'unit inactive or collected' "$tmp/out"; rg -q 'cleanup unverified: no unique manager diagnostic record' "$tmp/out"; ! rg -q 'cgroup removal verified|Traceback|unpack' "$tmp/out";; failed) rg -q 'unit failed' "$tmp/out";; esac
done
printf '%s\n' 'launcher fixtures passed'
