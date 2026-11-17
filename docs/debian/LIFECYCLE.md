# Package lifecycle and recovery

Install or upgrade an inspected, authenticated local package using `sudo apt
install ./PACKAGE.deb`. Retain the exact prior `.deb`, source and SHA256 separately
before upgrading. APT does not provide the signed installer's retained versions
or atomic symlink rollback. Processes already running can retain the old inode.
Close and reopen the command to use the new package. No service needs restarting.

Downgrades are refused by noninteractive APT unless explicitly allowed. After
checking the prior package and compatibility, run:

```sh
sudo apt install --allow-downgrades ./PRIOR-PACKAGE.deb
dpkg --verify xynoos-hypercore
/usr/bin/xynoos --version --json
```

Interactive APT can prompt for confirmation; raw dpkg permits downgrades. This
package adds no custom enforcement. There is no configuration or evidence migration.
JSON consumers must check schema_version; future compatibility is not promised.
The existing signed manager cannot roll back a Debian package.

```sh
sudo apt remove xynoos-hypercore
sudo apt purge xynoos-hypercore
sudo apt install ./PACKAGE.deb
sudo apt install --reinstall ./PACKAGE.deb
```

Remove deletes package-owned files. Purge currently has the same result because
there are no conffiles or maintainer scripts. User evidence, configuration, signed
installations and unrelated files are never touched. The package neither owns nor
creates any HOME path. It does not prune data. Avoid automatic recursive deletion.

After interruption, inspect `dpkg --audit`, then use `sudo dpkg --configure -a`
and, if dependencies are broken, review `sudo apt --fix-broken install`. Read the
proposed transaction first; APT may remove an unconfigurable package. Reinstall
the inspected `.deb` if files are damaged. Do not delete dpkg locks while a package
manager is running. The package has no scripts to resume; dpkg owns recovery.
Power loss, disk corruption and arbitrary termination remain untested.

The automated matrix runs only in marked disposable Debian containers:

| Test | Method / scope |
|---|---|
| Clean install and dependencies | APT installs the real artifact |
| Execution and CLI | nobody runs bounded commands; JSON and identity checked |
| Ownership and permissions | All archive members; dpkg file ownership |
| Integrity | dpkg verification detects appended bytes; reinstall restores |
| Upgrade | Older Debian revision fixture with same payload, then real artifact |
| Downgrade | Noninteractive refusal, then explicit allow-downgrades |
| Remove, purge, reinstall | Binary absent/present and dpkg audit clean |
| Preservation | Byte comparison of user config, evidence and unrelated data |
| Interrupted boundary | Unpack, check unpacked state, configure and audit |
| Dependency failure | Fixture with nonexistent dependency; APT and dpkg refusal |

Fixtures are explicitly synthetic and never published. No historical version is
available. This does not test cross-version migrations, power loss or a kill in
the middle of unpack. Tests verify the final clean state after failure recovery.
Do not execute lifecycle.py on a desktop: it requires a container marker and
explicit environment guard, and performs destructive package operations.

CI calls ci-lifecycle.sh, which creates the guard in a disposable container.
To reproduce, use the exact pinned image from the workflow, mount the checkout
read-only at `/work`, extracted artifacts read-only at `/artifacts`, and an empty
results directory at `/results`; run `bash /work/packaging/ci-lifecycle.sh trixie`.
No privileged container is needed for lifecycle tests. The script's APT source
changes are confined to the disposable container. Never run that script on a host.
