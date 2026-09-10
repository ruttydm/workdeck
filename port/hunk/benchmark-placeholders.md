# Benchmark directory placeholders

Hunk's empty `benchmarks/release/.gitkeep` and `benchmarks/results/.gitkeep`
blobs contain no payload bytes. They keep output directories visible in a
source checkout. Workdeck's Rust benchmark commands create those directories
when writing release or result artifacts (`xtask/src/benchmark.rs`), so no
runtime or source file is required for the empty blobs. The ledger records the
zero-byte records as Rust-generated replacements rather than silently dropping
them.

Source: Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, MIT, Modem Labs Inc.;
see `THIRD_PARTY_NOTICES`.
