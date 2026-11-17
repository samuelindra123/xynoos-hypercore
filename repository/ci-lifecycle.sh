#!/bin/bash
# ONLY disposable hosted Debian containers; never install packages on a laptop.
set -euo pipefail
test -f /.dockerenv
test "$(dpkg --print-architecture)" = amd64
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends python3 gnupg apt-utils ca-certificates curl util-linux
rm -f /etc/dpkg/dpkg.cfg.d/docker
work=$(mktemp -d)
chmod 755 "$work"
export GNUPGHOME="$work/gnupg"
mkdir -m700 "$GNUPGHOME"
trap 'test -z "${server_pid:-}" || kill "$server_pid"; gpgconf --kill gpg-agent || true; rm -rf "$work"' EXIT
if test "$1" = test; then
  # Test-only identity never enters the production archive, artifacts or release.
  gpg --batch --passphrase '' --quick-generate-key 'DISPOSABLE CI TEST <test@example.invalid>' ed25519 sign 1d
  XYNOOS_APT_FINGERPRINT=$(gpg --batch --with-colons --list-keys | awk -F: '$1=="fpr" {print $10; exit}')
  export XYNOOS_APT_FINGERPRINT
  deb=$(find /artifacts/first -name '*.deb')
  test -n "$deb"
  version=$(dpkg-deb -f "$deb" Version)
  dpkg-deb -R "$deb" "$work/older"
  # Labeled fixture, identical payload; not an actual historical release.
  older=0.0.0~preview.1-1
  sed -i "s/^Version:.*/Version: $older/" "$work/older/DEBIAN/control"
  dpkg-deb --build --root-owner-group "$work/older" "$work/older.deb"
  python3 /work/repository/archive.py publish "$work/apt" --deb "$work/older.deb" --commit "$(printf 'a%.0s' {1..40})"
  python3 /work/repository/archive.py publish "$work/apt" --deb "$deb" --commit "$(printf 'b%.0s' {1..40})"
elif test "$1" = published; then
  test -n "${XYNOOS_APT_FINGERPRINT:-}"
  cp -a /apt "$work/apt"
  python3 /work/repository/archive.py verify "$work/apt"
  version=$(python3 -c 'import json,subprocess,functools; x=json.load(open("'"$work"'/apt/dists/preview/versions.json"))["versions"]; print(sorted(x,key=functools.cmp_to_key(lambda a,b: -1 if subprocess.call(["dpkg","--compare-versions",a,"lt",b])==0 else 1))[-1])')
  older=''
else
  exit 2
fi
install -d -m755 /usr/share/keyrings
install -o root -g root -m644 "$work/apt/xynoos-archive-keyring.gpg" /usr/share/keyrings/xynoos-ci-keyring.gpg
python3 -m http.server 8765 --bind 127.0.0.1 --directory "$work/apt" > "$work/http.log" 2>&1 &
server_pid=$!
for attempt in {1..20}; do curl -fsS http://127.0.0.1:8765/dists/preview/InRelease >/dev/null && break || sleep .2; done
printf 'deb [arch=amd64 signed-by=/usr/share/keyrings/xynoos-ci-keyring.gpg] http://127.0.0.1:8765 preview main\n' >/etc/apt/sources.list.d/xynoos-ci.list
apt-get update
apt-cache policy xynoos-hypercore
if test -n "$older"; then
  apt-get install -y "xynoos-hypercore=$older"
  apt-get upgrade -y xynoos-hypercore
else
  apt-get install -y xynoos-hypercore
fi
test "$(dpkg-query -W -f='${Version}' xynoos-hypercore)" = "$version"
/usr/bin/xynoos --version --json
/usr/bin/xynoos doctor --json
# APT authority: package CLI cannot invoke the standalone update manager.
for operation in update rollback uninstall; do
  if /usr/bin/xynoos "$operation"; then echo 'Package manager authority failure'; exit 1; fi
done
setpriv --reuid=65534 --regid=65534 --clear-groups python3 /work/repository/coexistence.py
if test -n "$older"; then
  if apt-get install -y "xynoos-hypercore=$older"; then echo 'Unapproved downgrade accepted'; exit 1; fi
  apt-get install -y --allow-downgrades "xynoos-hypercore=$older"
  test "$(dpkg-query -W -f='${Version}' xynoos-hypercore)" = "$older"
  apt-get install -y xynoos-hypercore
fi
apt-get purge -y xynoos-hypercore
test ! -e /usr/bin/xynoos
# APT itself must reject changed package bytes authenticated by the signed index.
package=$(find "$work/apt/pool" -name "*_${version}_amd64.deb")
test -n "$package"
printf x >> "$package"
if (cd "$work" && apt-get download "xynoos-hypercore=$version"); then echo 'Modified package accepted'; exit 1; fi
# Signature chain must fail if authenticated metadata is modified.
sed -i 's/Origin: Xynoos/Origin: Tampered/' "$work/apt/dists/preview/InRelease"
rm -f /var/lib/apt/lists/127.0.0.1* /var/lib/apt/lists/partial/127.0.0.1*
if apt-get -o APT::Update::Error-Mode=any update; then echo 'Invalid signature accepted'; exit 1; fi
printf 'PASS: signed native APT install/upgrade/explicit rollback/purge and invalid-signature refusal\n'
