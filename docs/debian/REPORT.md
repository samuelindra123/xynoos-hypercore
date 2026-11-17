# Phase 6 engineering report

Audit date: 2026-11-17. This report will be updated with CI evidence before delivery.

| Item | Classification and evidence |
|---|---|
| A. Baseline | PASS: clean integration/resource-intelligence at b8c78b72f32e844e64eade5c35147e973bb82ab1; fetched all branch refs |
| PR strategy | PASS: #34 open, draft, unmerged; base main c83d06c519b2ecf659b6e804df00bc449812914a; three checks SUCCESS. New branch starts at its head and targets it |
| B. Architecture | Implemented: debhelper 13, quilt source, one amd64 binary, documentation, no scripts |
| C. Debian | NOT TESTED yet: Trixie build and Trixie/Bookworm runtime jobs pending |
| D. Rust | NOT TESTED yet: candidate minimum 1.82 from APIs/dependency manifests; CI validates it and 1.100.0; Bookworm 1.63 unsupported |
| E. Dependencies | NOT TESTED yet: dh_shlibdeps generates constraints; no new Cargo dependencies; runtime limited to libc/libgcc |
| F. Size | NOT TESTED: measurements pending real artifact |
| G. Reproducibility | NOT TESTED: two clean-root/path comparison pending |
| H. Lifecycle | NOT TESTED: automated matrix implemented; see LIFECYCLE.md |
| I. Upgrade/rollback | NOT TESTED: same-payload older-version fixture only; historical package unavailable |
| J. CI | NOT TESTED: workflow not yet pushed |
| K. Security | PASS by inspection: no privileged hooks, services or tuning; installer unchanged; Debian manager refuses; behavioral tests pending |
| L. MX | NOT TESTED: no MX environment available |
| M. N5000 | NOT TESTED: no physical access; exact protocol in HARDWARE.md |
| N. Blockers | Pending CI; physical/MX validation; signed package distribution deliberately absent |
| O. Commands | Exact installation and verification in MX-LINUX.md; lifecycle/recovery in LIFECYCLE.md |

Local lightweight checks: formatting, Python compilation, two source-validation
tests, shell syntax, changelog parsing and diff whitespace PASS. Heavy builds and
installation tests are delegated to CI. No primary desktop installation is run.
No package size, runtime compatibility or reproducibility result is invented.

Existing release.yml, installer, key, manager and release scripts are preserved.
The inherited Phase 5 artifact workflow references the legacy binary and unpinned
actions; it is outside this change. No existing workflow is modified. Existing
example filename collision warnings are documented in Phase 5. New CI must expose
failures; none are hidden with continue-on-error or lintian overrides.
