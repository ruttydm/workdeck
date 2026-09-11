# Release benchmark capture migration

The pinned `scripts/run-release-benchmark.ts` launcher is replaced by the
native `cargo xtask benchmark release-run` command. It reads the Workdeck Cargo
version, accepts `--version`, `--samples`, and `--out`, applies
`WORKDECK_RELEASE_BENCHMARK_SAMPLES` when samples are omitted, and writes the
versioned JSON snapshot under `benchmarks/release/` (or the explicit output
path). The command delegates to the Rust benchmark runner, so every workload,
metric aggregation, and child-process boundary remains native.

No package-manager runtime is started. The release gate consumes the resulting
snapshot with `cargo xtask benchmark compare-release`, and the verifier checks
the complete pinned baseline and stable source blobs (2,792 bytes / 100 lines,
SHA-256 `b476ac2c808c87df48b1c2963c2285cc03390bb4447f88ac036e23cda577aceb`).
