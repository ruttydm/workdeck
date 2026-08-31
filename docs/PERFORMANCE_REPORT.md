# Workdeck Performance Contract

## Budgets

At the deterministic 74-project/75-repository/109-worktree fixture scale:

| Metric | Budget |
| --- | ---: |
| Cold usable launch | ≤ 1.2s |
| Warm shell paint | ≤ 350ms |
| First actionable Inbox content | ≤ 300ms |
| Complete incremental Inbox scan | ≤ 7s |
| Cancellation acknowledgement | ≤ 150ms |
| Cached Git graph | ≤ 250ms |
| Cold Git graph | ≤ 750ms |
| Area switch p95 | ≤ 50ms |
| First diff viewport | ≤ 250ms |
| Cached search after debounce | ≤ 50ms |
| Main-thread scrolling p95 | ≤ 8ms |
| Idle memory after full fixture | ≤ 350MB |

## Design controls

- Rendering consumes immutable API snapshots and performs no I/O.
- Native requests cross a bounded queue and four bounded request workers.
- SQLite has one write owner.
- Git, providers, syntax, search, and artifacts are cancellable and bounded.
- Inbox emits progressive task events rather than blocking initial content.
- Git history and provider output are paged and capped.
- Lists and trees retain only visible/bounded working sets; the full fixture is not expanded into DOM rows.
- Dioxus resources are keyed by stable selection and explicit refresh generation.
- Cacheable reads coalesce by semantic identity and preserve request IDs/revisions; the cache is bounded to 96 completed and 24 in-flight entries.
- Startup prewarms the highest-value available unread worktree, while stale-while-revalidate keeps an existing graph or review interactive during refresh.
- Commit and PR preparation return the complete immutable change set directly to the three-column surface, eliminating the previous second review load.
- Tree-sitter highlight queries compile once per language and are shared from a bounded process cache; normalization produces immutable line-local spans before the snapshot reaches Dioxus.
- Search uses a 120ms cancellable debounce and a normalized 30-second result cache; Git/PR/CI/artifact/review data use explicit domain TTLs and mutation-aware invalidation.

## Deterministic measurements

`scripts/scale-fixture-gates.sh` validates the exact archived scale fixture without touching the live catalog. Playwright exercises 76 deterministic light/dark responsive visual states plus interaction and Axe coverage. Warm-navigation regressions use a DOM mutation observer to prove that revisiting Git and pull requests never mounts a loading or diff-preparation state, and a refresh regression proves that the current selected commit and diff remain present throughout revalidation. Rust tests cover cache hits, request-ID rebinding, in-flight coalescing, bounded abandoned reads, invalidation generations, cancellation, bounded history, search, provider subprocesses, and artifact lifecycle.

## Final measurements

The current renderer evidence was measured after the three-column PR/commit cutover and bounded cache/prewarm pass. It is not yet bound to the next packaged executable; exact native RSS and launch evidence must be regenerated after packaging.

| Run | Usable renderer | First Inbox after shell | Area switch p95 | First diff |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 314.24ms | 3.37ms | 40.40ms | 119.44ms |
| 2 | 264.49ms | 2.95ms | 40.50ms | 112.14ms |
| 3 | 262.59ms | 3.00ms | 40.50ms | 119.21ms |

Renderer timings come from three fresh browser contexts against the Dioxus web target and `FixtureWorkdeckClient::polished()` at 74/75/109 scale. They measure the same Rust renderer, snapshots, reducers, and component paths without native Computer Use transport overhead. Area switching observes the destination DOM mutation and then waits for its first animation frame, avoiding an artificial extra-frame tax while still measuring painted availability.

The independent packaged CLI scale and bounded-search harness completed in 126ms. Rust tests separately enforce cancellation, bounded worker queues, paged history, secure artifact lifecycle, and generation-safe results.

Current renderer evidence is in `artifacts/performance/web-renderer.json`. `scripts/profile-packaged-launch.sh` rejects stale executable hashes or over-budget packaged runs.

Performance numbers are release evidence, not evergreen documentation; rebuilding the executable invalidates the native report.
