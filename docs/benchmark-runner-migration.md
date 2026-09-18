# Benchmark runner migration

The pinned Hunk orchestrator `benchmarks/run.ts` is identical at the baseline
(`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`) and stable
(`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`) pins: 5,568 bytes, 217 lines, and
SHA-256 `0a86be0791cf64a61771b451e4a38aa2e77ebfa51cc2dd5bc083fb3b4cc17891`.

`cargo xtask benchmark run` is the native replacement. It preserves samples,
explicit workload selection, opt-in competitors and huge fixtures, child
process output/error handling, metric parsing and deduplication, percentile
aggregation, runtime/package provenance, JSON reports, and output-directory
resolution. Workload names retain the `.ts` selectors as compatibility labels,
but dispatch only to Rust benchmark owners; there is no Bun child process.

The runner drains child stdout and stderr concurrently, forwards diagnostics,
keeps failure exit codes actionable, and validates the entire workload selection
before creating a report. The native report records Cargo package metadata and
host platform/architecture while leaving the historical Bun field absent. Rust
tests cover both pinned option/parser oracles, duplicate metrics, fractional
sampling, process-pipe drainage, signal exits, failure propagation, and report
round trips. The verifier reads both source blobs through Git and checks exact
bytes, line count, hash, markers, native dispatch, and this documentation.
