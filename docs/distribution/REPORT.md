# Distribution ecosystem validation and delivery record

Audit date: 2026-11-17. Development base after verified normal prerequisite merges: main `be09510d6bc43861514a74224b1bacb54fbc281d`. Initial main was `c83d06c519b2ecf659b6e804df00bc449812914a`. Original source, branding, signed installer, Ed25519 public trust anchor and v0.1.0-preview.1–3 release history are preserved. The package is **xynoos-hypercore 0.1.0~preview.3-1 amd64**, Cargo upstream 0.1.0-preview.3.

## Repository and PR decisions

#34 was Draft at b8c78b72f32e844e64eade5c35147e973bb82ab1. Exact-head CLI/Phase5 CI passed; no requested changes/unresolved threads; read-only implementation reviewed. Marked Ready for Review and merged normally at **8b5499b7058723e310f5cec56a8e7f7978583cf5**. #33's two commits were included and GitHub marked it merged. #35 was Ready at 266f1b3b3017a2262b064967aa09c79cfa0430a2; retargeted to main, rechecked its exact-head Debian/Rust/lifecycle CI and clean comparison, then merged normally at **630ca19ae847525e401ddf8e0733bac21105cd1f**. No protection bypass or direct main push was used. PR #40 follow-up is proposed at a new exact head and remains **OPEN / Ready for Review / blocked pending required human review**; #14 and #15 are also open and unmerged.

Historical #14/#15 remain open Ready for Review with explicit blockers in #38/#17–#22. #15 received a pushed bounded-evidence fix/current CI. #16 closed as superseded by #24 after all eight files were checked; #32 closed as superseded by #34 after all sixteen file contents were checked. The audit/inventories were pushed and merged through #39 at **be09510d6bc43861514a74224b1bacb54fbc281d**, with all CLI, Phase5 and Debian checks passing at audit head 3ffb7f5ed5cd62a4a624f5aa2a992a08728119bb. Original branches and evidence remain. See [historical record](../maintenance/HISTORICAL-PR-RESOLUTION.md) and the live PR comments for subsequent exact-head validation.

## Implementation delivered

- Dedicated publish workflow selects exact reviewed/main source/version, reuses clean pinned offline Debian sbuild/reproducibility/lifecycle checks, prepares source/binary release material and scoped Cargo CycloneDX SBOM, attests all subjects and independently verifies workflow/source provenance before signing/publication.
- Dedicated OpenPGP preview identity configuration, protected environment secrets, independently pinned public fingerprint, signed InRelease/Release.gpg and SHA256 chain. No production key generated, unsigned fallback, private-key artifact or secret logging.
- Append-only apt-ftparchive repository with preview/main/amd64 metadata, all-version package indexes, signed version ledger, immutable pool assets, retained by-hash indexes and normal push conflict protection. Recovery promotes existing attested bytes. Weekly expiry renewal admits no packages through expired state.
- Single reusable Pages deployment preserves website bytes at root plus verified history under /apt; website-only deployments cannot erase packages. Public HTTP read-back is required after deployment.
- 25 maintained Wiki pages, sidebar/footer and normal-history publisher; README/Studio channel/status/security/development links without an unverified installation bootstrap.
- Internal pre-merge security assessment and signing-key preflight. This is engineering review evidence, not independent security approval.

## Validation matrix

| Area | Classification and evidence |
| --- | --- |
| Existing main source/package | **PASS**: main 630ca19 [Debian CI 44078933266](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44078933266), [CLI 44078933106](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44078933106). Existing package report retains lifecycle/provenance/reproducibility scope. |
| Archive engine/signature chain | **PASS locally and in hosted initial-head archive tests**: real GPG test identity, InRelease/Release.gpg, signed index/ledger checksums, SHA256 packages, real APT HTTP retrieval/download of two indexed versions. Production key is not the test identity. |
| Negative security | **PASS locally**: missing key, wrong fingerprint, invalid signature, modified package/index, duplicate version and wrong architecture rejected. Missing dependencies have existing Debian lifecycle coverage. |
| New exact-head CI | Initial head f642efb passed [Studio 44083445480](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44083445480), [CLI 44083445541](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44083445541) and [Phase5 44083445521](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44083445521). Archive tests passed, then its shallow-checkout whitespace check failed; checkout depth was corrected. Final-current-head package/archive run links/results are recorded in #40. |
| Signed APT installation/upgrade/downgrade/purge | Hosted disposable Debian 13 workflow added. Results must be read from exact PR CI. Older version is an explicitly labeled same-payload fixture; real historical upgrades remain **NOT TESTED**. |
| Production signing-key shape | Follow-up validates the imported identity is the pinned primary stub plus one expiring signing subkey; local test-key tests pass, exact-head hosted CI is pending. |
| Public Studio root | **PASS before changes**: target root returned HTTP 200 during audit. Current local/site checks preserve assets/animation/SEO; new content deployment is **BLOCKED** until reviewed merge/Pages execution. |
| Public APT/key/index/.deb | **BLOCKED**: no Actions signing secrets; production fingerprint absent (#36). Initial InRelease request returned HTTP 503; no usable signed archive was established. No public signature/install success is claimed. |
| Debian GitHub Release | **BLOCKED**: none published. Existing standalone [preview.3](https://github.com/samuelindra123/xynoos-hypercore/releases/tag/v0.1.0-preview.3) remains available; it is not a .deb release. |
| Artifact attestations for new .deb | Implemented, **NOT TESTED for publication** until protected main workflow runs; no claim of verified released provenance. |
| Wiki | **BLOCKED**: has_wiki=true but authenticated Wiki Git remote returns Repository not found (#37). 27 maintained Markdown files prepared; actual public Wiki pages are not claimed published. |
| Debian 13 amd64 | Existing package lifecycle **PASS in CI**; new APT channel lifecycle tracked in current PR CI. |
| Debian 12 amd64 | Existing package runtime lifecycle **PASS in CI**; stock compiler below MSRV. |
| MX / physical N5000/UHD 605 / other GPU vendors | **NOT TESTED** for new package/APT channel. No physical performance benefit/production readiness claim. Existing #20/#21 and hardware acceptance docs retain requirements. |
| Stable / other architectures / retention drills | **NOT IMPLEMENTED or NOT TESTED**, tracked as distribution follow-ups. Preview history is retained without automatic pruning. |

## Public endpoints and owner actions

Intended APT URL: `https://samuelindra123.github.io/xynoos-hypercore/apt/` (**not yet verified live**). Public fingerprint/signature: **BLOCKED**. Native bootstrap is withheld until HTTPS acceptance. Package versions/upgrades use ordinary APT after real repository configuration; see [installation/status](INSTALLATION.md), [owner setup](OWNER-SETUP.md) and [archive operations](MAINTENANCE.md).

Wiki location: `https://github.com/samuelindra123/xynoos-hypercore/wiki` (**publication blocked**). Sources list all 25 requested pages under docs/wiki plus navigation/footer. The actual Wiki repository must be initialized/access-enabled, pushed normally and publicly checked.

Owner actions: independent security reviewer and required PR #40 human approval; create the protected `apt-preview` environment; configure the approved signing-subkey/passphrase secrets and full public fingerprint; run and independently verify publication after a normal merge; initialize the Wiki with its first Home page before publishing maintained sources. No credentials were provided and no owner is asked to post key material in chat. Repository branch protection and tag/history rulesets are already enabled and read back. #36/#37 record exact steps. Remaining hardware, historical upgrade, stable/architecture and maintenance acceptance stay in the linked GitHub Issues.

## Outstanding tracked work

[#36](https://github.com/samuelindra123/xynoos-hypercore/issues/36) signing setup/first publication/verified commands; [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37) Wiki initialization and actual push; [#38](https://github.com/samuelindra123/xynoos-hypercore/issues/38) historical integration; [#41](https://github.com/samuelindra123/xynoos-hypercore/issues/41) two-real-version upgrades/rollback; [#42](https://github.com/samuelindra123/xynoos-hypercore/issues/42) physical MX/N5000 package/APT acceptance; [#43](https://github.com/samuelindra123/xynoos-hypercore/issues/43) maintenance/rotation/retention drills; [#44](https://github.com/samuelindra123/xynoos-hypercore/issues/44) future stable/architecture acceptance. Existing #17–#22 retain safety/telemetry/performance/provenance scope. No duplicated physical-performance claim is opened.

The actual clean/pushed Wiki publication attempt at f642efba7dcdab1cb2842253ec1d6cafcbe5803d failed at authenticated clone with exit 128 / Repository not found. No pages were published; #37 has the evidence and owner initialization steps.

Internal assessment: [PR #40 security readiness](SECURITY-ASSESSMENT.md). It is not an independent audit and leaves production signing/ref-protection setup to the owner. The real two-version test runner is `repository/real-two-version-lifecycle.sh`; its procedure is in [Debian lifecycle](../debian/LIFECYCLE.md). It remains **NOT TESTED** until two actual signed releases exist.
