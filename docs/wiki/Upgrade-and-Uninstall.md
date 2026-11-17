# Upgrade and Uninstall

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

For an already configured, verified APT installation:

```sh
sudo apt update
sudo apt upgrade
apt-cache policy xynoos-hypercore
apt-cache madison xynoos-hypercore
sudo apt remove xynoos-hypercore
```

Select a retained version from actual policy output for an explicit downgrade using APT's `--allow-downgrades` and exact version selection. Use `apt-mark hold xynoos-hypercore` to hold a tested installation and `apt-mark unhold xynoos-hypercore` to resume updates; document/review holds because they also block security updates. No historical package upgrade is proven until two real versions exist. Current lifecycle fixtures use identical payload with an explicitly older control version.

Removal/purge leave user-owned evidence and unrelated data. Remove the Xynoos source-list/keyring only as a separate explicit repository-removal step; check no other source uses that key. For standalone installs use its signed `xynoos update`, `rollback` and `uninstall` at the user path; offline standalone rollback has separate published evidence. See [lifecycle scope](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/debian/LIFECYCLE.md).
