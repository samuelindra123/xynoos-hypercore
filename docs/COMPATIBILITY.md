# Preview compatibility

| Environment | Evidence and limits |
| --- | --- |
| Linux x86_64 / Ubuntu 24.04 Codespaces | Disposable HOME installer/security regressions; static musl CLI; published lifecycle read-back is required for every release. |
| Debian 13 / XFCE / Intel Pentium Silver N5000 / 4 GB | User-reported physical preview.2 test: published binary, Ed25519, SHA256, CLI, telemetry, isolated update and rollback worked. Normal HOME install failed on existing bin 0775 and new private directory 0500. Preview.3 physical installation is **PENDING**, not PASS. |
| Other Linux x86_64 distributions | Compatibility targets with Python 3.9+, curl, OpenSSL 3 Ed25519 and CA certificates; no physical certification. |
| Other OS or CPU architectures | Refused; no verified release binary. |

The baseline binary uses static musl and baseline x86_64 CPU flags. Neither a Rust
installation nor sudo is required. Kernel mutation remains disabled.

Preview.3 creates new directories as 0700, verifies ownership and permissions, and
sets modes through no-follow file descriptors. Regressions cover umask 077 and
actual Linux default ACLs that produce 0500 or overly permissive defaults. Plain
umask 077 alone produced 0700 in Codespaces; the laptop's inherited ACLs have not
been independently inspected. Do not attribute its 0500 outcome to umask alone.

Existing user-owned bin directories at 0755/0700 are accepted unchanged. Mode 0775
fails closed because group members could replace executables; the error gives an
explicit manual remedy. Existing private Xynoos directories must be exactly 0700.
No existing directory is chmodded automatically. Symlink, foreign ownership,
traversal, hardlink and unexpected-content protections remain enabled. Root-owned
sticky shared ancestors such as /tmp are allowed; that exception never applies to
the final executable or private Xynoos directory.

[Installation and trust](INSTALL.md) · [regression evidence](reports/PREVIEW-3-INSTALLER.md)
· [physical validation handoff](N5000-PREVIEW-VALIDATION.md).
