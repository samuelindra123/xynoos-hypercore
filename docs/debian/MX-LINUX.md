# MX Linux operator guide

MX based on Debian 13 amd64 is a target, not a tested environment. Container
success does not certify MX, XFCE, sysvinit, systemd or physical hardware.
Check `/etc/os-release` and `dpkg --print-architecture`. No init dependency exists.

Download `debian-amd64-unsigned` from the exact successful packaging workflow
run on PR #35 and extract it into a new inspection directory. CI artifacts are
unsigned and may contain unreviewed code: authenticate the repository, commit and
run through a trusted channel before installing. There is no public download or
release URL for this package. For the signed preview use ../INSTALL.md instead.

From the extracted artifact root:

```sh
sha256sum -c SHA256SUMS
dpkg-deb --info first/xynoos-hypercore_0.1.0~preview.3-1_amd64.deb
dpkg-deb --contents first/xynoos-hypercore_0.1.0~preview.3-1_amd64.deb
sudo apt install ./first/xynoos-hypercore_0.1.0~preview.3-1_amd64.deb
dpkg-query -W -f='${Package} ${Version} ${Architecture} ${Depends}\n' xynoos-hypercore
dpkg-query -S /usr/bin/xynoos
dpkg --verify xynoos-hypercore
stat -c '%U:%G %a %n' /usr/bin/xynoos
/usr/bin/xynoos --version --json
/usr/bin/xynoos doctor --json
/usr/bin/xynoos resources --json
/usr/bin/xynoos diagnose --count 2 --interval-ms 100 --json > xynoos-diagnostic.json
```

Only installation/removal requires root. Run observations as your ordinary user.
No runtime network, restart, logout or shell modification is required. Diagnostics
write stdout only in this build; choose a private location for retained data.
`dpkg --verify` checks stored file digests, not signatures or all permissions.
An empty result is expected; independently inspect ownership and modes.

An existing `~/.local/bin/xynoos` can shadow `/usr/bin/xynoos`. Inspect `type -a
xynoos` and use the absolute path. Do not remove or rewrite the user installation.
Its update, rollback and uninstall commands manage that user installation only.
The Debian executable refuses them and points to APT. APT never manages user
versions or evidence. Two installations can coexist but are independently updated.

If dependencies cannot be satisfied, stop. Do not use force-depends, mix suites,
or copy the executable out to bypass APT. See LIFECYCLE.md for removal and rollback.
Missing proc/sys interfaces are normal in restricted environments; do not change
kernel permissions to obtain telemetry. No optimization benefit is claimed.
