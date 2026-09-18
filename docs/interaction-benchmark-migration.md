# Interaction benchmark migration

The pinned interaction workload and shared helper are identical at the baseline
(`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`) and stable
(`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`) pins:

- `benchmarks/interaction-latency.ts`: 2,478 bytes, 73 lines,
  SHA-256 `7de485fea4d96bb8bf64b03b885f17544363de729b8def65b960f638bc1a03e0`.
- `benchmarks/lib/interaction.ts`: 3,042 bytes, 87 lines,
  SHA-256 `81ab8d3098519e612a956ff1cb8debfdeb4eed61905cdc52da7d1284447e1807`.

`cargo xtask benchmark interaction-diagnostic` is the native replacement, and
the normal runner accepts the compatibility selector
`--script interaction-latency.ts`. It renders the same 240x28 review stream,
measures six navigation presses and eight wheel ticks on fresh sessions, and
emits median/p95 timing metrics plus native RSS/allocator snapshots. The
Ratatui cell buffer and real input handlers remain the authority; dispatch and
render timings are retained in the diagnostic report instead of being replaced
by a synthetic model.

Rust tests execute the frozen metric/count oracle at both pins and drive real
navigation, wheel scrolling, frame passes, memory snapshots, and cleanup. The
native process does not expose a JavaScript heap, so no JavaScript heap counters
are fabricated. The dual-pin verifier checks exact source bytes, line counts,
hashes, helper/workload markers, runner dispatch, and this documentation.
