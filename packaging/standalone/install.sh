#!/bin/sh
# XYNOOS HyperCore Standalone Installer
# Non-destructive standalone installation script for supported Linux distributions.
set -eu

PREFIX="/usr/local"
BIN_DIR=""
STATE_DIR="/var/lib/xynoos"
DRY_RUN=0
UNINSTALL=0

usage() {
    cat <<EOF
Usage: $0 [options]

Options:
  --prefix DIR    Install prefix (default: /usr/local)
  --bin-dir DIR   Explicit binary directory (default: <prefix>/bin)
  --uninstall     Uninstall existing standalone installation
  --dry-run       Show actions without modifying the filesystem
  -h, --help      Show this help message

Security Notice:
  Never run unverified scripts piped directly from the internet (e.g. curl | sudo sh).
  Always download, inspect, and verify the cryptographic checksum before running.
EOF
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            shift 2
            ;;
        --bin-dir)
            BIN_DIR="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --uninstall)
            UNINSTALL=1
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage
            ;;
    esac
done

if [ -z "$BIN_DIR" ]; then
    BIN_DIR="${PREFIX}/bin"
fi

if [ "$UNINSTALL" -eq 1 ]; then
    echo "Uninstalling XYNOOS HyperCore standalone installation..."
    if [ "$DRY_RUN" -eq 1 ]; then
        echo "[dry-run] Would remove: ${BIN_DIR}/xynoos"
        echo "[dry-run] Would remove: ${BIN_DIR}/xynoosd"
        echo "[dry-run] State directory preserved for evidence: ${STATE_DIR}"
    else
        rm -f "${BIN_DIR}/xynoos" "${BIN_DIR}/xynoosd"
        echo "Binaries removed from ${BIN_DIR}."
        echo "State directory preserved at ${STATE_DIR} (delete manually if desired)."
    fi
    exit 0
fi

echo "XYNOOS HyperCore Standalone Installer"
echo "====================================="
echo "Target Prefix : $PREFIX"
echo "Target Bin Dir: $BIN_DIR"

# Platform and libc compatibility check
ARCH=$(uname -m)
case "$ARCH" in
    x86_64|amd64)
        echo "Architecture  : x86_64 (Supported)"
        ;;
    aarch64|arm64)
        echo "Architecture  : aarch64 (Supported)"
        ;;
    *)
        echo "Warning: Unsupported architecture: $ARCH" >&2
        ;;
esac

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)

if [ ! -f "${SCRIPT_DIR}/xynoos" ] && [ ! -f "${SCRIPT_DIR}/bin/xynoos" ]; then
    echo "Error: Cannot locate xynoos binary in archive directory." >&2
    exit 1
fi

SRC_XYNOOS="${SCRIPT_DIR}/xynoos"
[ -f "$SRC_XYNOOS" ] || SRC_XYNOOS="${SCRIPT_DIR}/bin/xynoos"

if [ "$DRY_RUN" -eq 1 ]; then
    echo "[dry-run] Would create directory: ${BIN_DIR}"
    echo "[dry-run] Would install: ${SRC_XYNOOS} -> ${BIN_DIR}/xynoos (mode 0755)"
    echo "[dry-run] Would create state directory: ${STATE_DIR} (mode 0755)"
    exit 0
fi

mkdir -p "$BIN_DIR"
install -m 0755 "$SRC_XYNOOS" "${BIN_DIR}/xynoos"

SRC_XYNOOSD="${SCRIPT_DIR}/xynoosd"
[ -f "$SRC_XYNOOSD" ] || SRC_XYNOOSD="${SCRIPT_DIR}/bin/xynoosd"
if [ -f "$SRC_XYNOOSD" ]; then
    install -m 0755 "$SRC_XYNOOSD" "${BIN_DIR}/xynoosd"
fi

if [ "$(id -u)" -eq 0 ]; then
    mkdir -p "$STATE_DIR"
    chmod 0755 "$STATE_DIR"
fi

echo "Installation complete!"
echo "Run '${BIN_DIR}/xynoos doctor' to check platform compatibility."
echo "Run '${BIN_DIR}/xynoos setup --plan' to view optimization plan without modifying system."
