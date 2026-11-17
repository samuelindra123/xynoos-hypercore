#!/bin/sh
# Xynoos HyperCore convenience bootstrap. Inspect before executing.
# Initial trust: GitHub HTTPS + repository control. Installer source is pinned;
# downloaded release executables additionally require the embedded Ed25519 key.
set -eu
main() {
    case "$(uname -s)/$(uname -m)" in
        Linux/x86_64) ;;
        *) printf '%s\n' 'Xynoos: supported target is Linux x86_64 only.' >&2; return 1 ;;
    esac
    for tool in curl python3 openssl sha256sum mktemp chmod; do
        command -v "$tool" >/dev/null 2>&1 || { printf 'Xynoos: required host tool missing: %s\n' "$tool" >&2; return 1; }
    done
    umask 077
    stage=$(mktemp -d "${TMPDIR:-/tmp}/xynoos-bootstrap.XXXXXXXX")
    trap 'rm -rf "$stage"' EXIT
    trap 'exit 129' HUP
    trap 'exit 130' INT
    trap 'exit 143' TERM
    # This is our fresh mktemp directory, never an existing user directory.
    chmod 700 -- "$stage"
    curl --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 120 --max-filesize 1048576 -fsS \
        'https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/v0.1.0-preview.3/distribution/manager.py' \
        -o "$stage/manager.py"
    printf '%s  %s\n' '700df539d58227b1e90dbc754585afea723de8b1ea171b159077fcc71f3ab849' "$stage/manager.py" | sha256sum -c - >/dev/null
    python3 -I "$stage/manager.py" install "$@"
}
main "$@"
