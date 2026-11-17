# Compatibility and physical acceptance

| Environment | Status |
|---|---|
| Debian 13 amd64 | PASS: clean build and container lifecycle; no desktop validation |
| MX based on Debian 13 amd64 | NOT TESTED; init-independent design only |
| Debian 12 amd64 | PASS: container lifecycle; stock compiler unsupported |
| Other Debian derivatives | NOT TESTED |
| ARM/arm64, i386 and other architectures | NOT TESTED and not declared |
| N5000 / UHD 605 / 4 GB | NOT TESTED for this package |

Generic x86-64 is selected, without native or AVX requirements. This is a build
property, not physical certification. No daemon means zero resident package
processes after commands exit. It does not establish zero system overhead.
Archive size and Installed-Size are recorded by CI; dpkg Installed-Size is an
estimate in KiB and differs from filesystem allocation. No performance benefit
is claimed. Cloud measurements cannot substitute for laptop results.

Perform lifecycle/fault tests only in disposable VMs, never on the primary laptop.
After review, an operator may perform this nondestructive physical acceptance:

1. Record UTC time, `uname -a`, `cat /etc/os-release`, `dpkg --print-architecture`,
   `lscpu`, `free -b`, `lspci -nnk` if installed, firmware, init and graphics driver.
   Retain artifact hashes, source commit, package metadata and exact build run.
2. Before installation, record `cat /proc/swaps`, readable cpufreq policy governors,
   swap/ZRAM sizes, GPU settings and `type -a xynoos`. Record existing configuration
   and evidence digests without modifying them. Note read failures explicitly.
3. Inspect package contents and verify checksums. With operator authorization,
   install using MX-LINUX.md. Record APT output, `dpkg --verify`, file modes,
   `dpkg-query -W -f='${Installed-Size}\n' xynoos-hypercore` and
   `du -sk /usr/bin/xynoos /usr/share/doc/xynoos-hypercore`.
4. As an ordinary user, run absolute `/usr/bin/xynoos` version, doctor, status,
   resources, system snapshot and bounded memory/diagnose commands from the guide.
   Retain stdout and stderr separately. Confirm valid schema v1, exact source and
   version, no evidence-save errors, no new HOME files except redirected output.
5. Compare reported CPU topology/policy, i915 identity/frequency and sensor units
   with readable sysfs. Missing Intel utilization remains unsupported, not zero.
   Do not install monitoring dependencies or relax perf permissions for the test.
6. If `/usr/bin/time` is already available, run 20 sequential bounded resource
   commands, recording elapsed/user/system time and maximum RSS for each:
   `/usr/bin/time -v /usr/bin/xynoos resources --json`. Repeat at idle and during
   ordinary browser/video use, recording workload details. Do not generate memory
   pressure or compile on the 4 GB laptop. Summarize median, range and variability;
   time's RSS is process high-water, not system memory consumption.
7. Record the same readable settings after commands. Governor/frequency changes
   caused by the normal power manager must be distinguished from package activity;
   do not infer causality from changing frequency. Confirm no Xynoos process remains,
   no service/startup entry exists, and unrelated data digests are unchanged.
8. Stop on instability, thermal concern, memory pressure or reduced responsiveness.
   Record a FAIL with raw evidence. Missing interfaces are recorded, not repaired.

Acceptance requires reviewed raw captures, measurements and no unauthorized writes.
Cloud CI does not mark this protocol PASS. Package removal/reinstallation, downgrade,
interruption and fault injection belong in a disposable MX VM before desktop use.
