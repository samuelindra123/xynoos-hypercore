# Xynoos Engineering and Repository Governance Rules

Completed work must be committed, pushed and independently verified on GitHub. Never leave a completed local-only task. Create every new PR open and Ready for Review (`draft=false`); convert sufficiently implemented surviving drafts to Ready. PR descriptions must explain behavior/implementation, validation evidence and remaining limitations. Search issues before documenting every blocker or owner action; reuse existing issues where appropriate.

Use normal protected PR operations. Never force-push, rewrite shared history, delete existing branches, bypass protection or merge unfinished/failing work. Close only genuinely superseded PRs, with exact replacement commits and preserved evidence. Verify actual heads, bases, reviews, unresolved threads and checks before merge decisions. Only claim a push, merge, test or publication after independent read-back.

Preserve the standalone signed installer, its pinned Ed25519 trust anchor, branding, Studio and immutable release history. Debian builds use the debian-package feature and APT/dpkg authority; standalone builds use private user paths. Never allow either installation manager to overwrite foreign files. Do not enable services or mutate global kernel/swap/ZRAM/CPU/GPU settings as a packaging side effect.

Production APT metadata requires a dedicated approved OpenPGP identity, protected environment secrets and an independently pinned public fingerprint. Never commit/log/upload private keys or publish unsigned fallback metadata. If signing configuration is absent, push implementation and document the exact owner setup in issues. Preview is not stable. Retain package/history bytes; new bytes require a new version. Use exact-source builds and independently verify attestations before release publication.

The only Pages deployment must combine current Studio at root with verified retained /apt history. A missing state branch must not erase a live archive. Public HTTP read-back is required before claiming deployment or promoting installation commands. Wiki sources live in docs/wiki and must also be pushed to the actual Wiki when initialized/authorized; never claim local sources mean published pages.

Run heavy builds on GitHub Actions. Destructive package lifecycle tests run only in explicitly disposable Debian containers, never on the owner's physical laptop. Keep implemented, CI/cloud/physical-tested, not-implemented and not-verified statuses distinct. Never claim proven N5000/desktop gains, universal GPU support or production readiness without evidence. See docs/distribution/OWNER-SETUP.md and REPORT.md plus the live tracking issues.

---

## Permanent Repository Development & Milestone Governance Rules (Rules 1–10)

### Rule 1 — Main is the verified integration branch
`main` must represent the latest successfully integrated engineering state.
Do not develop directly on `main`.

### Rule 2 — Every milestone has its own development branch
Use a descriptive branch name (e.g. `feat/hypercore-milestone-f-daemon`, `feat/hypercore-milestone-g-description`).
Create new milestone branches from the latest verified `main`.
Do not create a new milestone branch from an older unmerged milestone branch as the normal workflow.

### Rule 3 — Every milestone has a corresponding pull request
All implementation changes must be reviewed through a pull request.
The PR must contain:
- Engineering objective.
- Implementation scope.
- Linked issues and milestones.
- Implementation details and architecture changes.
- Automated tests and verification evidence.
- Risks and failure modes.
- Benchmark evidence where applicable.
- Rollback strategy.
- Known limitations.

### Rule 4 — Heavy execution belongs to GitHub Actions
Never perform heavy compilation, stress testing, large artifact generation, or expensive workloads on the local developer laptop.
Local operations are restricted to lightweight inspection, editing, git/gh operations, documentation, and lightweight check runs.

### Rule 5 — No indefinite pull request accumulation
A pull request must end in one of the following states:
- Successfully merged via Squash and Merge.
- Closed with a documented reason, replacement reference, or preserved evidence.
- Temporarily blocked with a genuine, documented reason.
- Draft only when future active implementation genuinely requires it.
Do not leave completed, verified pull requests open.

### Rule 6 — Squash and Merge is mandatory
Every successfully completed milestone must be merged using Squash and Merge (`gh pr merge <PR_NUMBER> --squash`).
Do not stop after reporting that CI is green.

### Rule 7 — Sequential milestone integration
The mandatory milestone delivery workflow is:
`main` → Milestone branch → Implementation → Pull request → CI validation → Fix failures → Engineering audit → Squash and Merge → Post-merge `main` validation → Issue closure → Branch cleanup → Next milestone.
Do not start the next dependent milestone before the preceding milestone has been integrated and `main` has passed all required validation.

### Rule 8 — Evidence-based completion
Never claim a milestone or feature is production-ready merely because CI passed.
Distinguish clearly:
- Implementation completion (code written and passing local syntax checks).
- CI verification (clean compilation, tests, benchmarks, and package builds in hosted runners).
- Main integration (squash-merged into `main` with passing post-merge verification).
- Real kernel validation (tested against Linux cgroup v2 and PSI controllers).
- Physical hardware validation (measured on target physical hardware such as Intel Pentium Silver N5000).
- Production readiness (hardened, soak-tested, security-audited, and approved for release).

### Rule 9 — Safe operational defaults
HyperCore must prioritize:
- Observe-only behavior by default.
- Explicit authorization for intrusive operations.
- Fail-safe, fail-closed behavior on telemetry or permission failures.
- Predictable, strictly bounded resource usage (memory, CPU, disk).
- Automatic rollback of transient kernel changes.
- System stability over aggressive memory reclaiming.
- Accurate, evidence-backed performance claims.

### Rule 10 — No integration with the separate XYNOOS Linux project
Keep XYNOOS HyperCore independent.
Do not integrate it into the separate XYNOOS Linux distribution during this task.
