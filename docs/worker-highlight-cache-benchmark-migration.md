# Worker highlight-cache benchmark migration

The pinned Hunk workload `benchmarks/worker-highlight-cache.ts` is identical at
the baseline (`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`) and stable
(`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`) pins: 1,831 bytes, 56 lines, and
SHA-256 `48996b748cea282b08e605e64c5549724045d3537c0051aafdd535cef7246582`.

`cargo xtask benchmark worker-highlight-cache` constructs the same 8,000-line
TypeScript diff through Workdeck's canonical parser and submits it to the
production native syntect/Oniguruma worker. The first sample settles the cold
compact payload. The benchmark then drops only the decoded terminal cache and
submits the identical immutable metadata again, so `worker_cache_hit_ms` is a
real worker-owned LRU revisit. `compact_payload_bytes` is measured from the
native compact payload retained by that LRU, and `changed_lines` remains 8,000.

The native implementation keeps alias-context/partial patch semantics, emits
the source metric names, and disposes the worker after the measurement. It uses
Ratatui/Workdeck production data paths with no JavaScript runtime executed or
embedded. Executable Rust tests assert worker settlement, cache
eviction boundaries, stable syntax-run counts, payload retention, and argument
validation. The dual-pin verifier checks exact source bytes, line count, hash,
markers, dispatch, and this documentation.
