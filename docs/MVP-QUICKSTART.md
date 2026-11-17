# Xynoos HyperCore MVP

This build integrates live read-only Linux telemetry and deterministic policy
simulation. It can compress and verify a small test process's own pages. It is
not a desktop-wide memory optimizer. Physical kernel control remains locked.
No root access, daemon, service, kernel module or global swap changes are needed.

From a checkout with a Rust toolchain and the existing Cargo dependencies:

```sh
cargo build --workspace --release
./target/release/xynoos-cli mvp observe --count 5 --interval-ms 1000
./target/release/xynoos-cli mvp observe --count 1 --human
./target/release/xynoos-cli mvp simulate
./target/release/xynoos-cli mvp simulate --human
```

JSON is the default; observe emits one object per sample. Simulation emits one
report with a bounded list of steps. Decisions are deterministic for a given
trace; measured durations and RSS vary. Observation failures appear explicitly
in the telemetry contract and produce policy backoff. Normal missing telemetry
does not crash the CLI.

To exercise durable **simulation-only** evidence, choose a new file:

```sh
./target/release/xynoos-cli mvp simulate --trace fixtures/mvp-trace.json --journal /tmp/xynoos-mvp-new.jsonl
./target/release/xynoos-cli mvp recovery --journal /tmp/xynoos-mvp-new.jsonl --human
./target/release/xynoos-cli mvp recovery --human
```

The simulation command explicitly authorizes private buffer work. Journal files
are created with mode 0600, never overwritten and never automatically removed.
Recovery diagnostics read evidence and capabilities; they never replay writes.
Physical journals and crash recovery are not implemented or admitted.

The controlled-experiment admission interface is reviewable but always refuses:

```sh
./target/release/xynoos-cli mvp controlled --authorize
```

It emits a typed refusal and exits nonzero. There is no feature, environment
variable or authorization flag that enables physical writes in this build.

Reproduce the small comparison (new output filename required):

```sh
python3 scripts/benchmark_mvp.py --repeats 12 --output /tmp/xynoos-mvp-benchmark-new.json
cargo test --workspace
sh scripts/tests/test-018e-launcher.sh
python3 scripts/quality_mvp.py --output /tmp/xynoos-mvp-gates-new.txt
```

For the static Linux x86_64 package, verify `SHA256SUMS` after extraction, then:

```sh
./xynoos-mvp observe --count 1 --human
./xynoos-mvp simulate --human
./xynoos-mvp recovery --human
```

The package entry point accepts observe, simulate and read-only recovery only.
It contains no enabled physical experiment interface. A musl binary removes
glibc version requirements; target system behavior remains to be validated.

See [architecture](MVP-ARCHITECTURE.md), [safety and recovery](MVP-SAFETY-RECOVERY.md),
[benchmark method](MVP-BENCHMARK.md), [results](reports/MVP-RESULTS.md), and the
[N5000 validation guide](MVP-N5000-VALIDATION.md).
