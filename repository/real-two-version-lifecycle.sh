#!/usr/bin/env bash
# Real historical-version acceptance. Run only inside a disposable Debian 13 container.
set -euo pipefail
umask 077

old_version=${1:?usage: real-two-version-lifecycle.sh OLD_VERSION NEW_VERSION}
new_version=${2:?usage: real-two-version-lifecycle.sh OLD_VERSION NEW_VERSION}
fingerprint=${XYNOOS_APT_FINGERPRINT:-}
base='https://samuelindra123.github.io/xynoos-hypercore/apt'
package=xynoos-hypercore
report=${XYNOOS_LIFECYCLE_REPORT:-/results/real-two-version-lifecycle.log}

test "$(id -u)" = 0
test -f /.dockerenv
test -f /run/xynoos-package-test
test "${XYNOOS_DISPOSABLE_TEST:-}" = 1
grep -q '^ID=debian$' /etc/os-release
grep -q '^VERSION_ID="13"$' /etc/os-release
test "$(dpkg --print-architecture)" = amd64
[[ $fingerprint =~ ^[A-F0-9]{40}$ ]]
[[ $old_version =~ ^[0-9]+\.[0-9]+\.[0-9]+~preview\.[0-9]+-[0-9]+$ ]]
[[ $new_version =~ ^[0-9]+\.[0-9]+\.[0-9]+~preview\.[0-9]+-[0-9]+$ ]]
dpkg --compare-versions "$old_version" lt "$new_version"
mkdir -p "$(dirname "$report")"
exec > >(tee "$report") 2>&1
printf 'REAL RELEASE LIFECYCLE: %s -> %s\n' "$old_version" "$new_version"

work=$(mktemp -d /tmp/xynoos-real-lifecycle.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl gnupg gpgv
curl --proto '=https' --tlsv1.2 -fsS "$base/xynoos-archive-keyring.gpg" -o "$work/keyring.gpg"
actual_fingerprint=$(gpg --batch --with-colons --show-keys "$work/keyring.gpg" |
  awk -F: '$1 == "fpr" { print $10; exit }')
test "$actual_fingerprint" = "$fingerprint"
curl --proto '=https' --tlsv1.2 -fsS "$base/dists/preview/InRelease" -o "$work/InRelease"
curl --proto '=https' --tlsv1.2 -fsS "$base/dists/preview/Release" -o "$work/Release"
curl --proto '=https' --tlsv1.2 -fsS "$base/dists/preview/Release.gpg" -o "$work/Release.gpg"
gpgv --keyring "$work/keyring.gpg" "$work/InRelease"
gpgv --keyring "$work/keyring.gpg" "$work/Release.gpg" "$work/Release"
curl --proto '=https' --tlsv1.2 -fsS "$base/dists/preview/versions.json" -o "$work/versions.json"
ledger_sha=$(awk '/^SHA256:/{in_sha=1; next} /^[^ ]/{in_sha=0} in_sha && $3 == "versions.json" {print $1}' "$work/Release")
test "$(sha256sum "$work/versions.json" | awk '{print $1}')" = "$ledger_sha"
install -o root -g root -m 0644 "$work/keyring.gpg" /usr/share/keyrings/xynoos-archive-keyring.gpg
printf 'deb [arch=amd64 signed-by=/usr/share/keyrings/xynoos-archive-keyring.gpg] %s preview main\n' "$base" \
  >/etc/apt/sources.list.d/xynoos.list
chmod 0644 /etc/apt/sources.list.d/xynoos.list
apt-get -o APT::Update::Error-Mode=any update

for version in "$old_version" "$new_version"; do
  apt-cache show "$package=$version" >/dev/null
  expected=$(apt-cache show "$package=$version" | awk '/^SHA256:/ { print $2; exit }')
  test "${#expected}" = 64
  (cd "$work" && apt-get download "$package=$version")
  deb=$(find "$work" -maxdepth 1 -name "${package}_${version}_amd64.deb" -print -quit)
  test -n "$deb"
  test "$(sha256sum "$deb" | awk '{print $1}')" = "$expected"
  test "$(dpkg-deb -f "$deb" Package)" = "$package"
  test "$(dpkg-deb -f "$deb" Version)" = "$version"
  test "$(dpkg-deb -f "$deb" Architecture)" = amd64
  ledger_record=$(python3 - "$work/versions.json" "$version" <<'PY'
import json, sys
record = json.load(open(sys.argv[1]))["versions"][sys.argv[2]]
print(record["source_commit"], record["sha256"], record["filename"])
PY
)
read -r source_commit ledger_deb_sha ledger_filename <<<"$ledger_record"
test "${#source_commit}" = 40
test "$ledger_deb_sha" = "$expected"
test "$ledger_filename" = "pool/main/x/xynoos-hypercore/xynoos-hypercore_${version}_amd64.deb"
printf 'VERIFIED RELEASE version=%s source=%s sha256=%s\n' "$version" "$source_commit" "$expected"
done

# Preserve operator files across each real package version transition.
test_home="$work/operator-home"
mkdir -p "$test_home/.config/xynoos" "$test_home/.local/state/xynoos"
printf 'operator-config\n' >"$test_home/.config/xynoos/operator.conf"
printf 'retained-evidence\n' >"$test_home/.local/state/xynoos/evidence.json"
before=$(find "$test_home" -type f -print0 | sort -z | xargs -0 sha256sum)

apt-get install -y --no-install-recommends "$package=$old_version"
test "$(dpkg-query -W -f='${Version}' "$package")" = "$old_version"
apt-get check
dpkg --verify "$package"
apt-get install -y --no-install-recommends "$package"
test "$(dpkg-query -W -f='${Version}' "$package")" = "$new_version"
apt-get check
dpkg --verify "$package"

if apt-get install -y --no-install-recommends "$package=$old_version"; then
  echo 'FAIL: implicit noninteractive downgrade was accepted'
  exit 1
fi
test "$(dpkg-query -W -f='${Version}' "$package")" = "$new_version"
apt-get install -y --allow-downgrades --no-install-recommends "$package=$old_version"
test "$(dpkg-query -W -f='${Version}' "$package")" = "$old_version"
apt-get check
dpkg --verify "$package"
apt-get install -y --no-install-recommends "$package"
test "$(dpkg-query -W -f='${Version}' "$package")" = "$new_version"
apt-get check

test "$(dpkg-query -S /usr/bin/xynoos)" = "$package: /usr/bin/xynoos"
test "$(stat -c '%U:%G:%a' /usr/bin/xynoos)" = 'root:root:755'
while IFS= read -r path; do
  if test -f "$path"; then
    case "$(dpkg-query -S "$path")" in "$package: "$path) ;; *) echo "FAIL: foreign owner: $path"; exit 1 ;; esac
  fi
done < <(dpkg-query -L "$package" | sed '/^$/d')
dpkg --verify "$package"
after=$(find "$test_home" -type f -print0 | sort -z | xargs -0 sha256sum)
test "$before" = "$after"
apt-get purge -y "$package"
test ! -e /usr/bin/xynoos
test ! -s <(dpkg --audit)
test "$(find "$test_home" -type f | wc -l)" -eq 2
printf 'PASS: real indexed releases, signature/index/package hashes, install, upgrade, explicit downgrade, re-upgrade, dpkg ownership and data preservation\n'
