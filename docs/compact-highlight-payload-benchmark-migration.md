# Compact-highlight-payload benchmark migration

Workdeck ports Hunk's `benchmarks/compact-highlight-payload.ts` (7,804 bytes, 216 lines) from
both pinned trees with SHA-256
`640dfe2046b97f304508effcaee25704c42fb8ab0b56a33bb8f057d105856e4a`. Verification reads the
source with `git show`; no TypeScript worker or JavaScript runtime is retained.
The shipped benchmark path uses no JavaScript runtime.

Run it with:

```sh
cargo xtask benchmark compact-highlight-payload
WORKDECK_COMPACT_HIGHLIGHT_LINES=1000 WORKDECK_COMPACT_HIGHLIGHT_SAMPLES=3 \
  cargo xtask benchmark compact-highlight-payload
```

The native diagnostic creates the same large TypeScript addition, warms the production Ratatui
syntect/Oniguruma highlighter and native worker, deep-clones the raw token response, encodes and
validates the compact UTF-16 range payload, clones the transfer equivalent, and decodes every
line. It also measures inline and worker-backed operation latency and reports medians and p95s.
The compact payload is the same production typed-array-equivalent protocol used by Workdeck's
worker cache; line lengths and ranges are validated before decoding.

The source's interval stall probe is represented by the synchronous native operation duration,
because Rust has no JavaScript event loop or `structuredClone` transfer list. Metrics are labeled
as native token/compact work and never claim to be HAST or JavaScript heap measurements.
