# Contributing to XYNOOS HyperCore

Thank you for contributing to XYNOOS HyperCore! This document outlines our permanent development governance, engineering rules, and submission process.

## Permanent Repository Development Rules (Rules 1–10)

1. **Main is the verified integration branch**: `main` represents the latest successfully integrated engineering state. Direct development on `main` is prohibited.
2. **Every milestone has its own branch**: Branch descriptive milestone branches (e.g., `feat/hypercore-milestone-f-daemon`) directly from the latest verified `main`.
3. **Every milestone has a pull request**: All changes must be submitted via a GitHub Pull Request with clear objectives, linked issues, implementation details, tests, and limitations.
4. **Heavy execution belongs to GitHub Actions**: Heavy compilation, workspace test suites, packaging, sbuild, and benchmarks must run on GitHub Actions. Local operations on low-power developer laptops (e.g., Intel N5000, 4 GB RAM) are restricted to editing, docs, and lightweight validation.
5. **No indefinite pull request accumulation**: PRs must end in Merged, Closed with documented rationale, or Draft with active ongoing work. Do not leave verified PRs open.
6. **Squash and Merge is mandatory**: All completed milestone PRs are integrated using GitHub Squash and Merge (`gh pr merge <PR> --squash`) with clean, descriptive commit messages.
7. **Sequential milestone integration**: Follow the pipeline: `main` → branch → PR → CI → audit → Squash and Merge → verify `main` → close issues → clean branches → next milestone.
8. **Evidence-based completion**: Strictly distinguish implementation completion, CI verification, main integration, real kernel validation, physical hardware testing, and production readiness. Never claim unverified hardware gains.
9. **Safe operational defaults**: HyperCore defaults to Observe-Only mode. Intrusive operations require explicit authorization, bounded resources, and automated rollback.
10. **Independence from XYNOOS Linux**: Keep XYNOOS HyperCore modular and independent from the separate XYNOOS Linux distribution.

## Safety & Code Standards

- `#![forbid(unsafe_code)]` is strictly enforced across all memory and daemon crates.
- MSRV: Rust 1.82.0 (compatible with Debian trixie toolchain).
- Format code with `cargo fmt --check` and lint with strict `clippy`.
- Packaging builds must never mutate foreign files or activate intrusive services upon installation.
