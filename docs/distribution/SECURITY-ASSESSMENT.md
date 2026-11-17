# PR #40 internal security assessment

**Assessment type:** internal engineering review, performed for merge readiness. This is not an independent external audit, penetration test, maintainer approval, or production authorization.

**Reviewed baseline:** current distribution candidate `1022da3627cb73acecf2ef02292992ee5a459aa0`, based on `main` `be09510d6bc43861514a74224b1bacb54fbc281d`.

**Follow-up in this PR:** `repository/check_signing_key.py` now validates the imported secret-key shape before publication/renewal/promotion can use it. This code is not merged or deployed until normal PR review.

## Scope and method

Reviewed `.github/workflows/apt-publish.yml`, `apt-promote.yml`, `apt-refresh.yml`, `debian-package.yml`, `studio-pages.yml`, `repository/signing.sh`, `archive.py`, `release.py`, `promote.py`, `promote-gate.py`, `source-gate.py`, `bundle.py`, `state.sh`, the Debian package rules and lifecycle tests, the existing signed standalone installer/manager, and PR #14/#15 safety boundaries. The review covered trust roots, signature/index/package binding, key exposure, workflow permissions, privilege boundaries, ownership, downgrade behavior, rollback, archive retention and provenance.

The review was static, with local test-key checks and hosted CI evidence below. No production credentials were available or used. No live publication, install, key use, privileged host mutation, or physical acceptance was performed.

## Findings

### F-01 — Primary private key could be loaded into the publication runner

**Severity:** Medium before remediation; implementation and exact-head CI remediation pass are complete, pending human review.

`repository/signing.sh` imported the configured secret into an ephemeral `GNUPGHOME`, but did not check that the uploaded export contained only the delegated signing subkey. An owner could accidentally configure an export containing the primary private key. The protected publish job would then have access to that key as well as repository write permission.

**Reproduction:** `python3 -m unittest discover -s repository -p 'test_signing_key.py' -v` generates isolated test-only GnuPG material. It verifies that a primary-stub plus one expiring signing subkey is accepted, a full primary secret is rejected, and a mismatched fingerprint is rejected. No production identity is generated or handled.

**Remediation:** `repository/signing.sh` runs `repository/check_signing_key.py` immediately after import and clears the armored-key environment variable first. The checker requires exactly one primary matching the independently configured full fingerprint, an unavailable primary secret stub, exactly one available signing-capable secret subkey, and explicit future expiry for both public primary and signing subkey. It reads GnuPG's machine-readable secret-key fields, including the primary stub marker documented in [GnuPG DETAILS](https://github.com/gpg/gnupg/blob/master/doc/DETAILS). The primary stays in owner-controlled offline storage.

### F-02 — Production signing and owner approval are not configured

**Classification:** **OWNER ACTION REQUIRED — publication gate**, not an implementation security defect. No current secret exposure is evidenced. This blocks production publication, but does not require the unconfigured credentials to be merged into source.

At the initial assessment, GitHub had no `apt-preview` environment, repository Actions secrets, or fingerprint variable. The environment has since been created with a custom branch rule permitting only `main` and a required reviewer set to the sole repository owner. Self-review prevention is disabled because no second collaborator exists; this is an owner approval gate, not independent review. No production signing has occurred, and the publish job still cannot complete because secrets and fingerprint are absent.

**Evidence:** GitHub API read-back of `apt-preview` confirms its required reviewer and `main` deployment branch policy; repository Actions secrets and variables remain empty. Public Studio root returned HTTP 200, while current public APT `InRelease` and `Release.gpg` endpoints return 404. Issue [#36](https://github.com/samuelindra123/xynoos-hypercore/issues/36) records the exact remaining owner setup.

**Remediation:** Owner configures only the approved delegated key/passphrase as environment secrets and independently pins the approved full fingerprint as a repository Actions variable. Add a second environment reviewer if available. Then normal PR review/merge and exact-main workflow gates must pass before dispatch. Never use the standalone Ed25519 key for APT.

### F-03 — GitHub release and history immutability depend partly on repository settings

**Severity:** Medium residual control gap; key Git ref protections are now configured.

The code refuses duplicate Debian version/tag publication, checks release asset names and bytes after upload, and uses normal fast-forward-only history pushes. Initially, `main` was also unprotected. During this assessment, branch protection was enabled for `main`, requiring PRs, one approval after the last push, passing required checks, conversation resolution, admin enforcement, and no force-push/deletion. Active repository rulesets now prohibit updates/deletion of `debian/*` tags and non-fast-forward/deletion of `distribution/apt-history` with no bypass actors. These protect the refs but do not prevent an owner/admin from editing GitHub release assets or changing repository settings. APT clients detect changed metadata/packages unless validly resigned, but signed metadata cannot prove that repository retention policy was followed.

**Reproduction/evidence:** Source inspection of `repository/release.py` and `repository/state.sh`; GitHub read-back of `/branches/main/protection` and repository rulesets `27465026` and `27465046`. `repository/release.py` verifies an unused tag and uploaded assets, then reads asset bytes back; it does not set GitHub's immutable-release state.

**Recommendation:** Before first publication, enable GitHub immutable releases where available. Debian tag and history-ref protections are active. Limit recovery to normal reviewed operations. Add a periodic owner-visible history/release digest audit. Operational rehearsal remains in [#43](https://github.com/samuelindra123/xynoos-hypercore/issues/43).

### F-04 — Publisher token has repository-wide `contents:write`

**Severity:** Low residual privilege breadth; constrained by job and environment.

The publish, promote, and refresh jobs receive `contents:write` because they create prereleases and update archive history. GitHub token permissions are repository-scoped rather than branch-scoped; workflow code with that token could attempt other content/tag writes if compromised. The publish token is only available to protected jobs, and `main` still depends on repository protection.

**Reproduction:** Read the `permissions` blocks in `apt-publish.yml`, `apt-promote.yml`, and `apt-refresh.yml`. Build jobs receive read-only access; the attestation job has only contents read, OIDC identity and attestation write; secrets are only passed to protected signing jobs.

**Recommendation:** Keep `apt-preview` restricted to protected main and mandatory owner approval. Retain the active protection/rulesets; do not grant bypass to Actions. Keep `persist-credentials: false` on checkout and continue avoiding PATs.

### F-05 — Reproducible package build uses a privileged disposable build container

**Severity:** Low residual CI isolation risk.

`debian-package.yml` runs the source build in a Docker `--privileged` container to perform the isolated sbuild/reproducibility process. The checked-out source is mounted read-only, build output is in a separate volume, the hosted runner is ephemeral, and this build job has no signing secrets or write token. A malicious source change could still exercise a larger kernel/container attack surface during PR CI.

**Reproduction:** Inspect the `Two isolated builds` step in `.github/workflows/debian-package.yml`; it runs on a pinned Debian image with `--privileged` and does not receive production credentials.

**Recommendation:** Retain read-only source mounts, secret-free PR validation, pinned images and ephemeral hosted runners. Evaluate a rootless/user-namespace sbuild or a dedicated isolated builder with only the minimum capabilities needed. Do not move signing secrets into the build job.

### F-06 — Direct `.deb` authenticity is separate from local APT installation

**Severity:** Informational boundary, documented.

`apt install ./file.deb` does not authenticate where the local package came from. Direct downloads must verify the immutable release's checksums and GitHub attestations against this repository, the publication workflow, `refs/heads/main`, and the exact source digest before invoking root package installation. APT repository installation instead trusts the independently pinned OpenPGP key and signed metadata chain.

**Evidence:** `docs/distribution/INSTALLATION.md`, `docs/debian/SECURITY.md`, and the release preparation/verifier describe these separate trust paths. No direct Debian release asset currently exists.

**Remediation:** Preserve these instructions. Publish exact runnable direct-download commands only after an actual attested `.deb` release exists and has been read back.

## Controls that passed source review

- The APT archive pins a full primary fingerprint; `InRelease` and detached `Release.gpg` signatures authenticate release metadata. SHA256 records bind Packages indexes and retained `.deb` files; ledger/index equality, by-hash integrity, duplicate versions, wrong architecture, extra package files, symlinks and expiry are checked.
- New package publication requires exact current-main source SHA, matching Debian/Cargo version, a normal merged PR, passing source workflows and no existing immutable version/tag. GitHub attestations are verified for each payload subject before signing material enters the publish step.
- Build and attest jobs do not receive production signing secrets. The signing directory is created under `RUNNER_TEMP` with mode 0700 and cleaned on exit. No private keys are committed or uploaded by the workflow.
- The Debian package contains `/usr/bin/xynoos` and documentation, with no maintainer scripts, triggers, setuid/capabilities, systemd services, kernel settings, swap/ZRAM changes or CPU/GPU controls. CI validates archive modes/ownership and `dpkg` ownership.
- Debian execution uses APT/dpkg authority; package-mode CLI refuses standalone update/rollback/uninstall. The standalone manager is separately signed and refuses foreign APT-owned links before networking. Both paths avoid overwriting the other manager's files.
- Package downgrades require explicit APT authorization. The current CI downgrade test is a same-payload version fixture; actual two-release upgrade/downgrade remains **NOT TESTED** and is handled by [#41](https://github.com/samuelindra123/xynoos-hypercore/issues/41).
- PR #15's production physical-mutation permits remain closed. Its generic caller-supplied backends are not a sandbox, and systemd preflight has external effects; neither is represented as production-safe. PR #14's rollback journal is simulation-only and not durable physical recovery.

## Validation evidence

PR #40 candidate exact-head runs at `1022da3627cb73acecf2ef02292992ee5a459aa0`:

- [Debian build, Rust 1.82/1.100, reproducibility, package lifecycle and signed APT fixture](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44104262269)
- [Archive signature/integrity/adversarial and signing-key preflight tests](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44104262160)
- [Studio and combined Pages validation](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44104262300)
- [CLI/distribution validation](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44104262078)
- [Phase 5 workspace validation](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44104262589)

All listed current-candidate runs passed. These are CI tests with explicitly disposable fixture keys where applicable. They are not production-key tests, external approval, public APT publication, a real two-release rollback test, or physical hardware acceptance.

## Maintainer security decision

**APPROVE FOR MAINTAINER MERGE**, conditional on all exact-head required CI passing, no unresolved conversations, and retaining the protected-main status checks and force-push/deletion restrictions. No unresolved CRITICAL or HIGH implementation security finding was identified. The online-primary-key import risk is addressed by code and passing exact-head CI. This is an internal maintainer assessment, not an independent external audit or fabricated GitHub approval.

Production publication remains **BLOCKED** until the owner configures the approved production signing identity/fingerprint in the existing protected `apt-preview` environment, then verifies the public signed archive. Approval to merge the source is not approval to publish packages.
