#!/bin/bash
set -euo pipefail
test -f /.dockerenv
suite=$1
case "$suite" in trixie|bookworm) ;; *) exit 2 ;; esac
export DEBIAN_FRONTEND=noninteractive
rm -f /etc/apt/sources.list.d/debian.sources
printf 'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20261116T000000Z %s main\n' "$suite" > /etc/apt/sources.list
apt-get -o Acquire::Retries=2 -o Acquire::http::Timeout=30 update
apt-get install -y --no-install-recommends python3
touch /run/xynoos-package-test
export XYNOOS_DISPOSABLE_TEST=1
cd /artifacts
sha256sum -c SHA256SUMS
python3 /work/packaging/lifecycle.py /artifacts/first/*.deb "/results/lifecycle-$suite.json"
