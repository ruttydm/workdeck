# Memory benchmark migration

The pinned Hunk workload `benchmarks/memory.ts` is identical at the baseline
(`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`) and stable
(`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`) pins: 2,490 bytes, 72 lines, and
SHA-256 `a538b3bd2dbebe4c43d2e880c463f5f5f347b1905d7ac51986c9c70b23be2c0c`.

`cargo xtask benchmark memory` builds the same 120-file by 120-line review
fixture, measures native bootstrap and row planning, renders a 240x28 Ratatui
frame, and drives six real next-hunk navigation presses. It emits the source
metric names for fixture, planning, first-frame, navigation, file, and line
counts, plus native RSS/allocator snapshots at each retained-memory boundary.
The normal benchmark runner accepts `--script memory.ts` as a compatibility
selector and dispatches only to Rust.

The native process has no JavaScript heap, so `heapUsed` is not fabricated. The
benchmark remains diagnostic, owns its renderer and fixture lifetimes, and does
not write repository state. Executable Rust tests assert the complete workload
shape, real planning/rendering/navigation behavior, viewport, counts, and
memory observations. The dual-pin verifier checks exact source bytes, line
count, hash, markers, dispatch, and this documentation.
