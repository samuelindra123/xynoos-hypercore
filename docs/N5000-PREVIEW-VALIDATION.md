# OpenCode protocol — Debian 13 / N5000 / 4 GB

Use only the official command shown in the README **after its published-release
verification is recorded**. Do not clone or build. Do not authorize optimization.

1. Install with that one line. Record the output, `xynoos --version --json` and
   `xynoos doctor --json`. The installation must report signature + SHA256 VERIFIED,
   the intended preview version/channel and source commit. Stop on verification
   failure. Host prerequisites are curl, Python 3.9+, OpenSSL 3 and CA certificates.
2. Run `xynoos doctor` and `xynoos status`. Record actual MemAvailable, swap/ZRAM,
   PSI and unavailable interfaces. Do not hide current memory pressure or bypass
   a safety refusal. Active optimization is unavailable in this preview.
3. Use Firefox and VS Code normally. Capture read-only evidence:
   `xynoos diagnose --count 30 --interval-ms 1000 --json > xynoos-laptop.json`.
   Also use `/usr/bin/time -v` around that command when available, recording user
   CPU, system CPU, elapsed time and maximum RSS. Calculate process CPU overhead as
   `(user_seconds + system_seconds) / elapsed_seconds * 100`; this is whole-command
   process overhead, not an inferred desktop improvement. Include the diagnostic
   collector's child process costs. Do not start synthetic stress workloads.
4. Run `xynoos update`. If a newer published preview exists, verify its version and
   origin, then disconnect network and run `xynoos rollback`. Confirm the prior
   verified version is restored. If already current or no previous version exists,
   record that truthful result; never fabricate an upgrade. A pinned older version
   may be installed into a separate disposable HOME to exercise a real upgrade.
5. Perform uninstall/reinstall only in a separate disposable HOME using that same
   public bootstrap and explicit HOME/XDG variables. Put a marker in its Xynoos
   state directory, uninstall, confirm the executable and versions are removed
   while the marker remains, then reinstall and verify again. Do not repoint HOME
   for Firefox/VS Code or delete the real user's evidence.
6. Record version, origin, kernel/distro, pressure, overhead, lifecycle results and
   limitations. Desktop performance benefit: **NOT PROVEN**. Stop here.

No sysctl, swap, ZRAM, governor, system service, cgroup mutation, process migration,
or unrelated application modification is authorized by this protocol.
