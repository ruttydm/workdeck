# Resize-memory benchmark migration

Workdeck ports Hunk's `benchmarks/resize-memory.ts` (10,992 bytes, 309 lines) from both pinned
trees with SHA-256
`bc379ed71d4bcd1c941b4d953f6a1c8fb47c4d10e3dd08eb342c9e945db5ab52`. The source is verified
directly with `git show` by `cargo xtask verify` and `cargo xtask port audit`; no TypeScript
mirror is retained.

Run the native workload with:

```sh
cargo xtask benchmark resize-memory
cargo xtask benchmark resize-memory --file-count 12 --widths 80,120,160 --cycles 1 --no-gc
```

The Rust implementation builds one deterministic one-hunk-per-file stream, mounts a single real
Ratatui review, renders the first frame, resizes the native cell buffer through every requested
width for every cycle, renders twice after each resize, and reports total and p95 resize latency.
Finite non-negative option parsing, width-list validation, truncation, minimums, repeated-option
order, JSON output, and retained-memory thresholds follow the source contract.

Hunk's Bun `process.memoryUsage()` fields cannot be reproduced in a Rust process; there is no JavaScript heap. RSS is measured
on supported platforms, and `heapUsedBytes` is populated only from native allocator usage when
available; heap-total, external, and ArrayBuffer fields remain `null`. The `gc` flag is retained
for report compatibility but never forces garbage collection. This keeps the diagnostic honest
while preserving the source metric names and Ratatui resize behavior.
