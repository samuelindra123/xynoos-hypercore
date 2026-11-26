# XYNOOS HyperCore Standalone Distribution

This archive contains pre-compiled binaries and an explicit standalone installation script for Linux.

## Safe Manual Installation

1. Verify SHA-256 Checksum:
   ```bash
   sha256sum -c SHA256SUMS
   ```

2. Extract Archive:
   ```bash
   tar -xzf xynoos-hypercore-standalone-x86_64.tar.gz
   cd xynoos-hypercore-standalone
   ```

3. Dry-Run Verification (Inspect without modifying filesystem):
   ```bash
   ./install.sh --dry-run
   ```

4. Install to System (Requires root for `/usr/local/bin` and `/var/lib/xynoos`):
   ```bash
   sudo ./install.sh
   ```
   Or install to a user-controlled directory:
   ```bash
   ./install.sh --prefix "$HOME/.local"
   ```

## Security Model

- **No Remote Shell Pipes**: Never run `curl | sudo sh`. Always inspect downloaded payloads.
- **Explicit Permissions**: Binaries are installed with mode `0755`. State directory is restricted to `0755`.
- **Observe-Only by Default**: Installation does not activate background services or alter kernel tuning.
- **Uninstallation**: Run `./install.sh --uninstall` to remove all standalone binaries cleanly.
