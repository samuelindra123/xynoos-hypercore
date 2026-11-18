#!/bin/bash
# Exercise the production public install only inside a disposable Debian runner.
set -euo pipefail
test -f /.dockerenv
test "$(dpkg --print-architecture)" = amd64
. /etc/os-release
test "$ID:$VERSION_ID" = debian:13
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl gnupg
base=https://samuelindra123.github.io/xynoos-hypercore/apt
key=/usr/share/keyrings/xynoos-archive-keyring.gpg
curl --proto '=https' -fsSL "$base/xynoos-archive-keyring.gpg" -o "$key"
chmod 0644 "$key"
fingerprint=$(gpg --batch --show-keys --with-colons "$key" | awk -F: '$1=="pub"{primary=1;next} primary && $1=="fpr"{print $10;primary=0}')
test "$fingerprint" = "${XYNOOS_APT_FINGERPRINT:?}"
printf 'deb [arch=amd64 signed-by=%s] %s preview main\n' "$key" "$base" >/etc/apt/sources.list.d/xynoos.list
apt-get -o APT::Update::Error-Mode=any update
apt-cache policy xynoos-hypercore
apt-get install -y xynoos-hypercore
test "$(dpkg-query -W -f='${Version} ${Architecture}' xynoos-hypercore)" = "${VERSION:?} amd64"
cli=$(/usr/bin/xynoos doctor --json)
printf '%s\n' "$cli"
grep -Fq '"intervention_active":false' <<<"$cli"
grep -Fq '"physical_kernel_mutation":false' <<<"$cli"
/usr/bin/xynoos --version --json
test -z "$(dpkg-query -L xynoos-hypercore | grep -E '/(systemd|init\.d|sysctl\.d|udev)/' || true)"
apt-get purge -y xynoos-hypercore
test ! -e /usr/bin/xynoos
printf 'PASS: public signed APT update, Debian 13 amd64 install, CLI diagnostics and purge\n'
