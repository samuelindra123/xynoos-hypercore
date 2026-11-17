#!/bin/bash
# Run only in the privileged, disposable CI container. No signing or publication.
set -euo pipefail
test -f /.dockerenv
test "$(dpkg --print-architecture)" = amd64
trap 'status=$?; python3 /work/packaging/build-logs.py /out; exit "$status"' EXIT
export DEBIAN_FRONTEND=noninteractive
snapshot=http://snapshot.debian.org/archive/debian/20261116T000000Z
security=http://snapshot.debian.org/archive/debian-security/20261116T000000Z
rm -f /etc/apt/sources.list.d/debian.sources
printf 'deb [check-valid-until=no] %s trixie main\n' "$snapshot" > /etc/apt/sources.list
printf 'deb [check-valid-until=no] %s trixie-updates main\ndeb [check-valid-until=no] %s trixie-security main\n' "$snapshot" "$security" >> /etc/apt/sources.list
printf 'Acquire::Retries "2";\nAcquire::http::Timeout "30";\n' > /etc/apt/apt.conf.d/80xynoos-timeouts
apt-get update
apt-get install -y --no-install-recommends sbuild mmdebstrap uidmap debhelper dpkg-dev lintian python3 ca-certificates
dpkg-query -W > /out/bootstrap-packages.txt
useradd --create-home --uid 1000 builder
mkdir -p /out/first /out/second
chown -R builder:builder /out
tree=$(find /out/source -mindepth 1 -maxdepth 1 -type d -name 'xynoos-hypercore-*')
test -n "$tree"
# The source export contains no .git and never reads a developer HOME.
cd "$tree"
dpkg-buildpackage -S -us -uc -d
mmdebstrap --variant=buildd --architectures=amd64 \
  --include=cargo,rustc,debhelper,python3,ca-certificates \
  --aptopt='Acquire::Check-Valid-Until "false"' \
  --aptopt='Acquire::Retries "2"' \
  --aptopt='Acquire::http::Timeout "30"' \
  --customize-hook='echo "Acquire::Check-Valid-Until \"false\";" > "$1/etc/apt/apt.conf.d/99snapshot"' \
  trixie /srv/trixie-amd64.tar "$snapshot" \
  "deb [check-valid-until=no] $snapshot trixie-updates main" \
  "deb [check-valid-until=no] $security trixie-security main"
sha256sum /srv/trixie-amd64.tar > /out/build-root.sha256
for pass in first second; do
  # Each invocation extracts a fresh build root; no build cache is shared.
  runuser -u builder -- sbuild --verbose --chroot-mode=unshare --chroot=/srv/trixie-amd64.tar \
    --dist=trixie --arch=amd64 --no-enable-network --no-run-lintian \
    --build-dir="/out/$pass" --build-path="/build/$pass" \
    /out/source/*.dsc
done
cd /out
lintian --fail-on error,warning first/*.deb source/*.dsc | tee lintian.txt
python3 /work/packaging/artifacts.py /out
