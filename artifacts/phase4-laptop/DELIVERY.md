# Laptop cycle delivery

Implementation/source commit: `c28435c03c1d4e0842b7f8a1c46469e68d7b4a8c`.
Branch: `task-018-real-optimization`; existing draft PR #14; do not merge.

Workspace package:
`c28435c03c1d/xynoos-laptop-c28435c03c1d-linux-x86_64.tar.gz` (about 5.6 MiB).
SHA256:
`75acf00fa1438b9a436d26ddb13cd41a8bc6bd0d3a9423a2b932e9a7bad3fb44`.

Transfer that archive, its `.sha256` and `c28435c03c1d/PACKAGE.json` to the
physical laptop. Follow [exact OpenCode instructions](../../docs/PHASE4-LAPTOP-N5000.md).
The archive stays a workspace artifact, gitignored under the same convention as
previous distributions. Checksums, provenance, raw smoke evidence and validation
are committed. Rebuild from the source commit in a fresh checkout if the workspace
archive is unavailable; byte-identical rebuild/archive hashes are not claimed.

Both executables are static musl Linux x86_64, with no interpreter or NEEDED
libraries. Python 3.9+ is required for the harness. `SOURCE.tar.gz`'s embedded git
commit and every manifest source hash were independently verified. Extracted
internal SHA256 checks passed. Staged ABC smoke completed 21 arms including warmup;
extracted default AB smoke completed 14. Both reject kernel-controlled mode,
including with its authorization flag. See `c28435c03c1d/PACKAGE-VALIDATION.json`.

All requested workspace gates passed; the final targeted Python suite contains
19 passing admission/lifecycle/adversarial cases. Native full-size Codespaces
measurements are in session1, session2 and final-source-session; static smoke
measurements are functional validation and are not pooled with those timings.

The intervention did not demonstrate a reproducible acceptable benefit: active
paired p95 changes were +5.8%, -3.4% and -11.1%, while index completion increased
95.3%, 69.5% and 71.4%. No physical desktop benefit is established. Read
[the full result and recommendation](../../docs/reports/PHASE4-LAPTOP-RESULTS.md).

Physical kernel mutation remains **NO-GO**. The package never enables controllers,
writes memory.low/high, changes swap/ZRAM or touches unrelated applications.
Physical userspace active runs require independent validation and explicit
bounded authorization. Phase 4 remains PARTIAL pending real N5000 diagnosis and
matched application/desktop evidence, with acceptable background/recovery costs.
