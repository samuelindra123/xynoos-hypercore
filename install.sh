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
    for tool in curl python3 openssl sha256sum mktemp; do
        command -v "$tool" >/dev/null 2>&1 || { printf 'Xynoos: required host tool missing: %s\n' "$tool" >&2; return 1; }
    done
    umask 077
    stage=$(mktemp -d "${TMPDIR:-/tmp}/xynoos-bootstrap.XXXXXXXX")
    trap 'rm -rf "$stage"' EXIT HUP INT TERM
    curl --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 120 --max-filesize 1048576 -fsS \
        'https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/v0.1.0-preview.1/distribution/manager.py' \
        -o "$stage/manager.py"
    printf '%s  %s\n' '4b40a9f891c6f015244be211563ed0ae523ebaba32c51a06a7f70ac258b575c9' "$stage/manager.py" | sha256sum -c - >/dev/null
    python3 -I "$stage/manager.py" install "$@"
}
main "$@"
