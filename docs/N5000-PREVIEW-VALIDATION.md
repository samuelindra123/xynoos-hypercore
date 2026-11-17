# OpenCode handoff — preview.3 / Debian 13 / N5000

After publication and GitHub read-back, give OpenCode this prompt. Do not modify
source, build locally, run optimization experiments or claim PASS in advance.

> Validate published v0.1.0-preview.3 on this physical Debian 13 / Intel N5000
> laptop. Do not modify source code. Record OS/kernel, `umask`, ownership/modes and
> ACLs (if getfacl is installed) for HOME, ~/.local, ~/.local/bin and any existing
> Xynoos data/state directories. If bin is 0775, run the installer once and confirm
> the actionable refusal leaves its mode unchanged; inspect and explicitly apply
> `chmod go-w -- "$HOME/.local/bin"` only if owned by this user and shared write
> is not intentional. For a pre-existing Xynoos private directory at 0500, inspect
> and explicitly apply only the printed 0700 remedy; never recursively chmod HOME.
> Under `umask 077`, run the exact versioned installation command below. Stop on
> any signature/checksum/ownership/symlink failure. Export PATH and capture
> `xynoos --version --json`, `xynoos doctor`, `xynoos status`, and
> `xynoos diagnose --count 2 --interval-ms 100 --json`. Confirm version preview.3,
> signature + SHA256 verification, source commit, kernel mutation disabled,
> ~/.local/bin/xynoos pointing into this user's verified installation, private
> Xynoos directories 0700, metadata 0600 and installed binary 0700. Compare read-only
> before/after snapshots of sysctl, swap/ZRAM, governor and service state; report
> any unexpected system or unrelated file modification. Preserve user data and
> diagnostics. Report PASS/FAIL per check with actual output and limitations.
> Do not uninstall from the real HOME or change system settings.

```sh
umask 077
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/samuelindra123/xynoos-hypercore/v0.1.0-preview.3/install.sh | sh -s -- --version v0.1.0-preview.3
export PATH="$HOME/.local/bin:$PATH"
```

The HTTPS bootstrap initially trusts GitHub/TLS/repository control. For an
independently pinned source digest, use the [auditable alternative](INSTALL.md#pinned-auditable-alternative)
and confirm the pin through a separately trusted channel. Codespaces lifecycle
results do not establish physical installation PASS or desktop performance benefit.
