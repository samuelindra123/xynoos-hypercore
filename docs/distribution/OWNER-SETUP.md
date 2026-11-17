# Owner setup: signed experimental Debian distribution

Status: **BLOCKED** until [#36](https://github.com/samuelindra123/xynoos-hypercore/issues/36) is satisfied. No production archive key/fingerprint or Debian prerelease is currently claimed. Never substitute the standalone Ed25519 key. The source/publisher PR must be reviewed and merged normally before dispatching publication from main.

## Dedicated OpenPGP identity

On a trusted owner workstation, use `gpg --full-generate-key` with the maintainer's approved real identity, a strong passphrase and an explicit expiry. Use a v4 RSA primary key (4096 bits, certification) and a separate signing subkey (RSA 4096, bounded expiry) via `gpg --edit-key FINGERPRINT` → `addkey`. Do not copy a sample identity. Record and independently verify the full **40-character uppercase primary fingerprint** with `gpg --with-colons --fingerprint`. The publisher rejects shortened/mismatched fingerprints and keys with multiple primaries.

Keep the primary private key offline under the owner's secured key-management process. Export only the protected signing subkey (primary private stub) for Actions. Use an owner-controlled temporary directory, `umask 077`, a file outside the checkout, and `gpg --armor --export-secret-subkeys FULL_FINGERPRINT > PRIVATE_EXPORT`. Never print/cat that file, attach it to an issue, upload it as an artifact or place it in the repository. Delete the temporary export after secure secret configuration. Protect offline backups and retain revocation material. Review rotation before changing the public trust anchor; existing clients do not automatically trust a new key.

The protected workflows now run `repository/check_signing_key.py` after import. It rejects an available primary private key, a wrong/multiple primary, anything other than one available signing subkey, and absent/expired primary or subkey expiry. The test suite covers delegated-subkey acceptance and primary-key/fingerprint refusal using explicitly disposable test-only identities. Never substitute a fixture identity for the approved production key.

## GitHub configuration

**Current GitHub configuration (2026-11-17):** environment `apt-preview` has been created. Its custom deployment branch policy permits only `main`, and its required reviewer is the sole repository owner/collaborator (`samuelindra123`). Self-review prevention is disabled because there is no second collaborator; do not represent this as independent review. No secrets or fingerprint variable are configured. Before publication, verify this policy, add an independent authorized reviewer if one is available, and add only the approved production values below. PR validation receives no signing secrets.

| Name | Scope | Purpose |
| --- | --- | --- |
| XYNOOS_APT_PRIVATE_KEY | apt-preview environment secret | ASCII-armored private signing subkey export, including public primary key/stub |
| XYNOOS_APT_PASSPHRASE | apt-preview environment secret | Strong signing-key passphrase; must be nonempty |
| XYNOOS_APT_FINGERPRINT | repository Actions variable | Independently approved full primary fingerprint; public pin used by both apt-preview and github-pages jobs |

Use GitHub's secret form or `gh secret set XYNOOS_APT_PRIVATE_KEY --env apt-preview < PRIVATE_EXPORT`. Set `XYNOOS_APT_PASSPHRASE` through the hidden interactive secret prompt (never a literal command argument or shell history). Set the public variable using `gh variable set XYNOOS_APT_FINGERPRINT --body FULL_FINGERPRINT`. Do not configure differing environment overrides for that pin. The original `XYNOOS_RELEASE_SIGNING_KEY`, if later configured, belongs to the standalone Ed25519 pipeline and is independent.

Keep Pages source **GitHub Actions**. The existing github-pages environment is used by the one combined Studio/APT deployment. Publication needs scoped GITHUB_TOKEN contents:write for immutable releases and the append-only **distribution/apt-history** branch. If a ruleset requires PRs on that branch, use the normal required PR process; do not add bypasses. Never relax protection on main. Attestation jobs use contents:read, id-token:write, attestations:write; publication additionally reads attestations. No registry/packages permission is required.

As of 2026-11-17, `main` has active branch protection: PR required, one approval required after the last push, required current validation checks, stale review dismissal, conversation resolution, admin enforcement, and force-push/deletion disabled. Active repository rulesets protect `debian/*` tags from update/deletion and `distribution/apt-history` from non-fast-forward/deletion; both have no bypass actors. Preserve these settings. The remaining human review action is to obtain an independent security reviewer and the required PR #40 approval; this internal assessment does not satisfy either requirement.

Before publication, enable GitHub immutable releases where the repository supports them. Do not add Actions or maintainers as protection bypasses. Workflow checks reject routine duplicate-version/tag writes and use normal fast-forward pushes, but repository code cannot prevent an owner/admin from editing release assets or changing repository settings. Include these controls in the periodic archive recovery/retention review tracked by [#43](https://github.com/samuelindra123/xynoos-hypercore/issues/43).

Stable is intentionally **not enabled**. A later stable channel must have its own protected environment, signing identity/approval policy, suite metadata and acceptance checks; the preview workflow hard-rejects stable versions. Do not repurpose preview as stable.

## First publication

Wait for exact-main CLI, Debian and archive validation to pass. Read current main SHA and `debian/changelog` version; dispatch **Publish signed Debian preview** (`apt-publish.yml`) on main with those exact `source_sha` and `version` inputs. The SHA must equal the workflow source SHA: attestations cannot identify one commit while the package builds another. The source must be a normal main PR merge, with current required source CI passing.

The workflow reuses clean pinned Debian builds, reproducibility and lifecycle tests; attests every release subject; independently verifies those attestations; imports secrets only into temporary GNUPGHOME; validates/signs history; tests APT in a disposable Debian container; creates a new prerelease/tag; pushes public signed history normally; and deploys combined Pages. A missing key, passphrase, fingerprint, signature, dependency, attestation or check fails publication. No unsigned fallback is allowed.

Debian release tags use `debian/` plus Debian version with `~` replaced by `-` because Git ref names cannot contain a tilde. Example mapping: package `0.1.0~preview.3-1` → tag `debian/0.1.0-preview.3-1`. Existing standalone v0.1.0-preview.3 is never overwritten. To change any published package bytes, bump debian/changelog; preserve Cargo/Debian upstream-version agreement.

## Public acceptance and installation instructions

The combined Pages job performs HTTPS byte-for-byte Studio/asset read-back, then key fingerprint, InRelease/Release.gpg, indexes, by-hash and every historical package digest verification. Its status is not considered successful solely because deploy-pages returned a URL. Record the workflow/run, source SHA, real fingerprint, release URL and public verification in #36.

Only then replace the pending text in docs/distribution/INSTALLATION.md, docs/wiki/Native-APT-Installation.md, README and Studio with the actual verified bootstrap. The intended source is exactly:

`deb [arch=amd64 signed-by=/usr/share/keyrings/xynoos-archive-keyring.gpg] https://samuelindra123.github.io/xynoos-hypercore/apt preview main`

First create `/usr/share/keyrings` root-owned 0755, download the actual binary public key over HTTPS to a user temporary file, compare its full fingerprint through an independently trusted channel, and install it root-owned 0644 at `/usr/share/keyrings/xynoos-archive-keyring.gpg`. Write the stanza root-owned 0644 to `/etc/apt/sources.list.d/xynoos.list`, then normal apt update/install. Do not use apt-key, insecure repositories, signature disabling or unverified URLs. This paragraph is a publication acceptance specification, not an assertion that those public files exist now.

## Failure recovery and renewal

If an immutable release was published but history push or Pages failed, do not rerun a build at the same version or delete/replace assets. Use **Promote existing attested Debian preview** with the original source SHA/version. It downloads the existing public prerelease, verifies tag/ancestry and all original attestations/checksums, checks any existing version is byte/source-identical, and appends or re-signs only the verified state before combined Pages deployment. Failed normal pushes remain failed; fix the required branch/ruleset process instead of force-pushing.

**Renew signed preview metadata** runs weekly, sets Valid-Until 30 days ahead and retains all package/index history. A missed expiry may be renewed only after checking the old signature, fingerprint, entire digest chain and ledger; no new package is admitted through expired metadata. Clients never disable expiry checks. GitHub may disable inactive scheduled workflows or require environment review; monitor renewals and dispatch manually when needed. Key expiry/revocation requires reviewed owner rotation, not a blind re-sign.

## Wiki

See [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). If the Wiki Git remote is uninitialized, open the repository Wiki on GitHub and save its first Home page. From a clean, committed and pushed checkout, run `python3 scripts/publish-wiki.py`. It clones existing Wiki history, copies 25 maintained pages plus sidebar/footer, commits, pushes normally and checks remote HEAD. It does not delete extra owner pages. Verify public Home, installation, safety and sidebar links before describing the Wiki as published.

## Owner actions still required

1. Arrange an independent security review and provide the required human approval on PR #40 after reviewing its exact current head and checks.
2. Review the existing `apt-preview` environment’s `main`-only deployment policy and owner approval gate; add a second authorized reviewer if available.
3. Configure the approved dedicated OpenPGP signing-subkey export and passphrase as environment secrets, and set the independently verified full public fingerprint as the repository variable. Never send private key material or passphrases in chat, issues, or commits.
4. After merge, run the exact-main publication workflow; verify signatures, package indexes, hashes, public HTTP read-back, and installation before advertising APT commands.
5. Initialize the GitHub Wiki by saving its first Home page, then publish the maintained sources and verify public pages.

Repository branch protection and tag/history rulesets above are already configured and independently read back. Do not weaken them to perform these actions.
