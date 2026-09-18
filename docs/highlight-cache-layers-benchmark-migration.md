# Highlight cache-layers benchmark migration

The pinned Hunk workload `benchmarks/highlight-cache-layers.ts` is identical at
the baseline (`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`) and stable
(`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`) pins: 2,584 bytes, 74 lines, and
SHA-256 `6f83f074bd40f38d87b05421248caf1b8289052df346f4e93b9975a92a56f11a`.

`cargo xtask benchmark highlight-cache-layers` creates eight unique 8,000-line
TypeScript diffs through Workdeck's canonical parser. It records the cold
highlight, an immediate decoded terminal-cache hit, and a final revisit after
seven other files exceed the 60,000-line terminal-cache budget (the terminal cache). The compact
worker LRU remains resident (within its 8 MiB limit), so the final metric is a
real worker-owned cache hit after terminal eviction. The metric stream preserves
`cold_ms`, `main_cache_hit_ms`, `worker_cache_hit_after_main_eviction_ms`,
`files=8`, and `changed_lines=8000`.

The Rust path uses the production syntect/Oniguruma worker, immutable
alias-context metadata, and the Ratatui review cache boundaries. It drops the
worker after measurement and executes with no JavaScript runtime. Tests assert
unique inputs, actual terminal-cache eviction, stable worker results, payload
retention, and argument validation. The dual-pin verifier checks exact source
bytes, line count, hash, markers, dispatch, and this documentation.
