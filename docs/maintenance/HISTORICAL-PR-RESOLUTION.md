# Historical pull request resolution — 2026-11-17

Audited current main: `be09510d6bc43861514a74224b1bacb54fbc281d`.
All original branches, commits, benchmark outputs and historical package provenance are retained.
No history rewrite or branch deletion is authorized or performed.

## Decisions and exact heads

| PR | Audited head → review head | Decision | Tracking |
| --- | --- | --- | --- |
| #15 | `3c3402efad45d23eb9f047d085e21cf05d468a7e` → `f481c6f7fbe4faae07f0894222641ea6c7f9f438` | Open, Ready for Review; merged current main into feature branch; all current CI passes; physical acceptance remains blocked | #17, #18, #19, #20, #38 |
| #14 | `ef39a4ddfd35dbda390dcb7457af2b2e45322bbe` → `582de23590b4746a20c1db2aa4de90a37f283b37` | Open, Ready for Review; historical tests pass; current-main CLI conflicts remain | #15, #21, #22, #38 |
| #16 | `283ef0573f92671917bc52a6738a887d4893a1fb` | Superseded by #24 and newer main documentation; close with explanation | #17–#23 retain backlog |
| #32 | `18b35416724bd7787363078ab0d8e43f90842c60` | Superseded by #34 merge `8b5499b7058723e310f5cec56a8e7f7978583cf5`; close with content evidence | #20, #21 retain host/physical limits |

No review submissions requesting changes or unresolved review threads were found on any of these four PRs. Old head CI is distinct from current review-head CI. Absence of a check is not a passing check.

## PR15 safety review

Reviewed adapter authorization and private non-clone permits; descriptor I/O and alternate DisposableCgroup write sinks; child identity/permissions/emptiness and low-write latches; systemd unit/invocation/PID/start-time binding; coordinator transitions/recovery; worker release, termination and reaping; fixture and compile-fail coverage. The old mutation-sink report was checked against source, not accepted as a substitute for review.

Production adapter permit guard is always false. Descriptor controller writes, child creation/removal and alternate DisposableCgroup constructor/place/write/cleanup/drop independently refuse physical mutations. Generic caller-provided backends can perform arbitrary work and are not sandboxed. The explicit unit-preflight launcher requests a systemd transient service, so it is an external mutation path and must not be represented as read-only.

Checks followed by writes are sequential: held descriptors and identity checks reduce substitution risk but do not establish exclusive delegation or eliminate TOCTOU. Ambiguous operations latch unresolved state and stop automatic recovery. Controller progress/latches remain in memory; SIGKILL, abort, power loss and unreaped workers lack durable physical recovery. Therefore the original physical acceptance criteria do not pass. #17–#20 remain open; #38 tracks integration with current source, installer, packaging and Studio.

A concrete boundedness issue was fixed: repeated adapter observations previously grew evidence vectors without bound. Backported PR14's existing 128-record cap and regression into #15. Capacity exhaustion refuses and preserves history. Physical guards remain closed. Fresh pinned hosted CI exercises all Rust targets, compile-fail doctests, formatting, strict Clippy/rustdoc, release build and Python/launcher fixtures. It runs no ignored physical tests or live systemd launcher.

## PR14 inventory and implementation review

The exact current #14 head `582de23590b4746a20c1db2aa4de90a37f283b37` compared with current main `be09510d6bc43861514a74224b1bacb54fbc281d` changes **208 paths: 129,129 added and 69 removed text lines**. [Machine-readable inventory](pr14-file-inventory.json) separates 109,862 historical artifact lines, 14,218 source/test lines, 4,769 documentation/evidence lines and 280 workflow/workspace/configuration lines. This exact-head inventory replaces the earlier 169-path snapshot.

The 169-path comparison contains no new binary archives. The old backup branch used by #32 contains additional historical archive paths; it must not be merged wholesale. Existing artifacts on #14 retain exact source-specific names and manifests; generated output is evidence, not product implementation or a current release.

Reviewed Engine::evaluate refusal/authorization/freshness/reset logic; typed cooperative policy; private fixed-corpus compression and reversal; simulation trace bounds and simulation-only journal; CLI modes; owned/desktop/physical-named read-only harnesses, process lifecycle and independent oracles; corresponding fixtures and results. Compression operates on private participating buffers, verifies candidate bytes before committing, and reverses/compares owned data. It cannot reclaim arbitrary application pages. CRC32 is accidental-damage detection, not provenance authentication. CPU/AS/output/request/watchdog bounds do not establish hard real-time behavior. Simulation journal recovery never authorizes physical writes. Reports acknowledge CPU/latency costs and the laptop study's failure to establish a repeatable acceptable benefit. No desktop-wide or N5000 gain is claimed.

### Recovery disposition

| Historical implementation | Value | Current-main decision |
|---|---|---|
| Cooperative policy enum and private owned-buffer compression/round-trip verification | Useful only for explicit participating-process experiments; it does not control unrelated process pages or demonstrate system-wide gains. | Retain as historical research evidence. No distro/runtime integration: its new `xynoos-engine` API duplicates current CLI/product boundaries and depends on an unreviewed orchestration layer. Re-evaluate under a separately scoped feature proposal with workload evidence. |
| `Engine::evaluate`, simulation journal and telemetry-gated proposal flow | Makes refusal/authorization/freshness/reset behavior inspectable; journal is process-local simulation only. | No physical-control recovery. Keep simulation separate from kernel mutation and preserve current read-only product contract. |
| `xynoos-mvp` binary, old CLI dispatch, benchmark commands and docs | Demonstrates experimental commands and source-specific studies. | Do not replace current CLI, Debian feature or README. Current main already contains maintained commands, packaging and the signed standalone installer. Preserve old reports under their exact commit only. |
| cgroup/systemd controllers and transaction helpers | Rich identity/cleanup fixtures, but writes and systemd transient service operations require explicit authorization and remain physically disabled or externally mutating. | No kernel mutation, privileged service setup or controller integration. Keep PR #15 isolated pending #17–#20 and exact current-main acceptance. |
| New external dependencies in #14 | The compared lockfile adds `nix 0.29.0`, `bitflags`, and `cfg_aliases` for the new engine path; existing dependencies also gain new internal edges. | Do not reintroduce without a narrowly reviewed use case, API review, locked-source provenance and CI on the supported MSRV. |

No PR #14 product code was copied into current main during this recovery. Its only safety-related bounded-evidence idea was already backported to PR #15 with a 128-record limit and refusal-preserves-history regression; #15 remains unmerged, so this is not current-main behavior.

After retargeting #14 to main, a real git merge-tree reports conflicts in **Cargo.lock, README.md, crates/xynoos-cli/Cargo.toml, crates/xynoos-cli/src/main.rs**. Its original #15 dependency is still unmerged. Exact-head hosted validation can run; a final integrated merge candidate cannot be claimed validated while these conflicts remain. #38 records the additive reconciliation/extraction criteria, including preservation of current installer/packaging/site tests. The safe bounded-evidence subset was backported into #15; wider engine extraction requires dependency-aware review and is not falsely called complete.

## PR16: all eight files

| File | Current-main comparison and disposition |
| --- | --- |
| assets/branding/xynoos-banner.svg | Byte-identical to current main; retained by #24 |
| assets/branding/xynoos-logo.svg | Byte-identical; retained by #24 |
| assets/branding/xynoos-mark.svg | Byte-identical; retained by #24 |
| assets/logo.svg | Byte-identical; retained by #24 |
| README.md | Newer main preserves branding, evidence limits, architecture/safety/contribution links and adds published signed installer plus Debian status. Old MVP commands depend on unmerged #14; do not overwrite current README. |
| docs/README.md | Replaced by newer main documentation index with available install, safety, evidence and compatibility links. Old index links unmerged MVP pages. |
| docs/PROJECT-BACKLOG.md | Proposed issues were actually created as #17–#23. Old draft-only PR instructions are obsolete; preserve actionable risks through those issues. |
| docs/ROADMAP.md | Research gates and evidence-first direction retained in current README, docs/RESEARCH-HISTORY.md, #17–#23 and the current Studio. Old “website/packaging future” wording is obsolete. Original file remains accessible at its immutable commit. |

All four identical SVGs were parsed as XML, checked for viewBox and rejected script/foreignObject/event-handler content. Current main retains original identities unchanged. #24 merge already extracted the useful branding independently of #14. No unique implementation is discarded by closing #16; its old roadmap/backlog record stays at the original commit and live issues.

## PR32: all sixteen files

[Content comparison with SHA256 and full textual deltas](pr32-content-comparison.json) binds every affected file to #32 head and audited main. Nine are byte-identical. Remaining differences were inspected at content level:

| Path | Verification |
| --- | --- |
| .github/workflows/phase5-validation.yml | Main retains format/tests/Clippy, adds integration branch/release/resources smoke. |
| Cargo.lock | Main retains new aura→pressureiq dependency; old backup engine edges are unrelated to PR32 and deliberately excluded. |
| crates/xynoos-aura/Cargo.toml | memory policy's pressureiq dependency retained; current package metadata adds MSRV and preserves workspace. |
| crates/xynoos-aura/src/lib.rs | memory module retained; main preserves its own module set and adds resource policy. |
| crates/xynoos-aura/src/memory.rs | Byte-identical |
| crates/xynoos-cli/Cargo.toml | memory command's aura/serde dependencies retained alongside current product package; old engine dependency excluded. |
| crates/xynoos-cli/src/main.rs | memory dispatch retained; entire memory_command body identical after removing whitespace. Other differences preserve current product CLI rather than backup mvp entry. |
| crates/xynoos-cli/tests/memory.rs | Byte-identical |
| crates/xynoos-linux-control/tests/linux.rs | Byte-identical including PSI isolation fix |
| crates/xynoos-linux-control/tests/transactions.rs | PSI/meminfo deterministic fixture wrapper retained verbatim; older backup-only tests remain out of scope. |
| crates/xynoos-pressureiq/src/intelligence.rs | Byte-identical including malformed-PSI and slow-first-collection reliability fixes |
| crates/xynoos-pressureiq/src/lib.rs | intelligence module retained; main additionally exports hardware observations. |
| docs/PHASE5-MEMORY-INTELLIGENCE.md | Byte-identical |
| docs/reports/phase5/benchmark.txt | Byte-identical |
| docs/reports/phase5/human-example.txt | Byte-identical |
| docs/reports/phase5/live-example.json | Byte-identical |

No unique valuable PR32 change remains to port. Close as superseded by #34; never merge its backup base. #34 exact-head CI passed before its normal merge; #35/current-main packaging also tested the integrated memory code. Physical N5000 calibration, MX acceptance and GPU driver coverage remain unverified.

## Validation record

PASS: 16-path content comparison; memory CLI whitespace-normalized equality; 4 SVG byte/XML checks; full 169-path inventory; merge-tree conflict detection; review-thread/review inspection; normal pushes of safety fix and fresh CI workflows.

Fresh #14 exact-head historical validation [44081420250](https://github.com/samuelindra123/xynoos-hypercore/actions/runs/44081420250) **PASS**: all Rust stages, 68 Python tests and launcher fixtures. Historical code was adjusted to use deterministic PSI fixtures in two happy-path protocol tests; dedicated refusal paths remain intact. #14 still has unresolved current-main merge-tree conflicts in Cargo.lock, README and CLI files. #15's previous source-integration checks failed because its historical base omitted current Debian/product work and used `u64::is_multiple_of`, unavailable to Rust 1.82. PR #15 has now merged current main into its feature branch and replaced those calls with MSRV-compatible remainder checks; exact-head package, CLI and safety CI is pending. This merge candidate preserves unconditional physical refusal. It does not satisfy #17–#20 physical safety acceptance or constitute an independent safety review.
