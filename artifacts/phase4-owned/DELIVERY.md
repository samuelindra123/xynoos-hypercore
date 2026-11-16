# Phase 4 owned experiment delivery

Current package: `xynoos-owned-d97b27c60080-linux-x86_64.tar.gz` (use `PACKAGE.json`, not older manifests).

- Package source commit: `d97b27c6008091f1b04feaa8df138ff00df5aa60`.
- Implementation/evidence commit: `d951c5b400e178c96217fbd7c8dba7d384bb64fb`.
- Archive SHA256: `61f8c2680d4fd783642fe126b57309f736b58d2ef0bff7556e7d2f55632831bc`.
- Worker SHA256: `4ced2f4498a0df87cf66a76d18b30fae37d6c81261a41adc1e7828e6a6fe8ff6`.
- Static x86_64 musl, independently bounded cooperative worker only.
- Source/measurement hashes verified against packaged worker; no ELF interpreter.
- Internal checksums and extracted 18-arm smoke passed, with six effective active actions.
- Controlled mode and unauthorized active execution refused.
- All final workspace/Clippy/rustdoc/release/Python/lifecycle/compile-fail gates passed; final Python pass contains 49 tests.
- Post-measurement missing-HWM reporting fix leaves every measured summary value unchanged.
- Additional six-repeat deterministic MVP simulation regression passed; it is not active physical evidence.

Two mixed Codespaces sessions contain 36 measured arms (plus six warmups),
with equivalent output and byte-verified restoration. Entropy control contains
18 measured arms (plus three warmups), no saving and explicit no-benefit outcomes.
Cancelled active warmup and injected low-memory refusal are preserved separately;
neither contributes to successful performance quantiles.

Result: approximately 66% lower steady RSS for this cooperative mixed corpus,
roughly 77% more worker CPU and 9–10% slower completion versus baseline. Peak
memory does not improve; worst measured active request was about 34 ms. This is
a memory-retention tradeoff, not a desktop speedup.

Phase 4 **PARTIAL**: N5000 active physical evidence is missing. GO only for the
scoped, disposable owned-workload evaluation in `docs/PHASE4-OWNED-N5000.md`.
Physical kernel/cgroup mutation remains disabled/NO-GO. Next: two comparable
N5000 six-triple mixed sessions, 128 requests per arm, then matched entropy control;
keep any low-memory refusal and failed run. Full report and safety review are in
`docs/reports/PHASE4-OWNED-PERFORMANCE.md` and `PHASE4-OWNED-SAFETY-REVIEW.md`.

Archives are local build artifacts, intentionally ignored by git. Their checksums,
source provenance, builder, raw results and validation evidence are committed.
Rebuild from the recorded source revision with `python3 scripts/package-owned.py`
in a fresh checkout; transfer the current archive, sidecar and PACKAGE.json.
