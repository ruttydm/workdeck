# Navigation-memory benchmark migration

Workdeck ports Hunk's `benchmarks/navigation-memory.ts` (12,791 bytes, 339 lines) from both
pinned trees with SHA-256
`d1aee6872a77a79414dd68e877c0a1b5279b402e6a0ce1373c352c56d7f86ef9`. The source is read through
`git show` by `cargo xtask verify` and `cargo xtask port audit`; no TypeScript mirror is kept.

Run the native workload with:

```sh
cargo xtask benchmark navigation-memory
cargo xtask benchmark navigation-memory --navigations 24 --file-count 12 --mode forward --no-gc
```

The Rust workload constructs the same one-hunk-per-file stream, mounts one real Ratatui
review, renders the first frame, dispatches `]`/`[` navigation for bounce or forward traversal,
and samples after bootstrap, first frame, navigation checkpoints, and destruction. Numeric
options retain Hunk's finite non-negative parsing, truncation, minimums, repeated-option order,
and threshold checks. `--json-out` writes the complete sample report.

Hunk's Bun `process.memoryUsage()` exposes JavaScript heap, external, and ArrayBuffer counters.
Rust has no JavaScript heap, so those fields are `null`; on macOS `heapUsedBytes` is backed by
native allocator (malloc-zone) usage, while all supported platforms report current process RSS. The report
labels this explicitly and never presents RSS as a JavaScript heap measurement. No forced
garbage collection is performed even when the compatibility `gc` option is enabled.

The source metric names (`navigation_heap_*` and `navigation_rss_*`) are emitted whenever the
corresponding native measurement exists. The workload is diagnostic and remains subject to the
same configured growth/slope thresholds as the source.
