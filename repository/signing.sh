#!/bin/bash
# Source only on an ephemeral protected runner. Never upload GNUPGHOME.
set -euo pipefail
: "${RUNNER_TEMP:?}" "${XYNOOS_APT_PRIVATE_KEY:?Configure apt-preview signing secret}"
: "${XYNOOS_APT_PASSPHRASE:?Configure apt-preview passphrase secret}"
: "${XYNOOS_APT_FINGERPRINT:?Configure full approved fingerprint}"
export GNUPGHOME
GNUPGHOME=$(mktemp -d "$RUNNER_TEMP/xynoos-gnupg.XXXXXX")
chmod 700 "$GNUPGHOME"
trap 'gpgconf --kill gpg-agent || true; rm -rf -- "$GNUPGHOME"' EXIT
printf '%s\n' "$XYNOOS_APT_PRIVATE_KEY" | gpg --batch --quiet --import
unset XYNOOS_APT_PRIVATE_KEY
