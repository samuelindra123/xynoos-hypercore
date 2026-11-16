# Developer preview implementation review

Scope: new read-only CLI facade, rootless distribution and release automation on
main after merged branding #24 and website #25. PR #14 targets #15's safety branch;
#15 remains open against main; #16 remains open against the optimization branch.
None is merged to deliver this preview. Existing active kernel-control code is not
exposed through the shipped product commands. The optional empty-cgroup kernel test
remains ignored and was not run. No uncommitted laptop evidence was altered.

Self-review, not an independent security audit:

- The bootstrap explicitly starts with GitHub/TLS trust; its pinned SHA256 verifies
  installer source integrity. Independently pinned Ed25519 signatures authenticate
  release metadata before binary execution. The installer does not pretend to be a
  Sigstore certificate-chain verifier.
- Release discovery uses numeric SemVer components, exact preview/stable filters,
  draft exclusion and required platform assets. No latest-release endpoint, shell
  evaluation, metadata-provided command, URL, key or path is accepted.
- Installation uses private version directories, strict owners/permissions,
  nofollow exclusive file creation, filesystem sync, a persistent flock inode,
  atomically replaced current link and a pre-switch recovery journal. Same-UID
  adversaries, root and filesystem corruption remain outside this model.
- Downloads are byte/time bounded. Raw ELF installation avoids archive extraction.
  Archive entries are independently validated for package publication.
- Unknown files/collisions cause preservation/refusal. Uninstall preserves evidence
  and lock directories. Rollback verifies retained signature/digest offline.
- GitHub publication has no PR trigger and gates exact main/merged PR/check status.
  Build/attestation and private-key signing are separate jobs; action references are
  immutable commits. Publishing verifies source and attestations before mutation.
- Memory pressure is reported as observed, with no telemetry admission floor.
  Diagnostics distinguish counter observations from unproven bottleneck inference.

Validation records are captured separately with exact release/source identities.
Hardware certification, active optimization and desktop benefit are not claimed.
