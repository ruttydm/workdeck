# Interaction profile after empty-registry projection bypass

- Checkout: `04cc22d3`; production code is `7bddee4a` (the intervening commit records measurements).
- Optimized command: `target/release/xtask benchmark interaction-diagnostic`.
- macOS `sample` attached to process 19895 for a requested one second at one-millisecond intervals.
- Raw local sample: `/tmp/workdeck-scroll-profile.UZmBSs/sample.txt`.
- The first attempted diagnostic process had already completed; its terminal exit and absent PID were verified before starting the sampled process.
- This short, perturbed sample includes construction, navigation and scrolling. Counts are not wall-time percentages and sampled timings are not benchmark acceptance evidence.

Collapsed top-of-stack observations include 37 `_xzm_free`, 32 `_platform_memmove`,
23 `_malloc_zone_malloc`, 20 `review_content_digest`, 15 syntax parser samples,
14 JSON IndexMap insertion samples and 10 `review_gap_geometry_for_file` samples.
These aggregate counts do not isolate scrolling from setup or navigation.

Stacks beneath `render_review` show `build_review_rows_with_chrome` entering
highlight prefetch, compact syntax decoding, semantic file projection, gap geometry
and source-gap row construction. This directs inspection toward row construction;
it does not establish that any single operation accounts for the measured scroll cost.

Source inspection at this revision confirms that `render_review` caches plain
section layouts for content height and prefetch selection, but then the shared row
builder still walks each visible-by-filter file, computes its gap geometry and
constructs offscreen geometry rows. Viewport painting avoids full offscreen cell
styling, not all offscreen file planning. A subsequent optimization should reuse
cached section geometry while preserving exact row offsets, navigation, source
expansion and hit-test data. Existing full-cell comparisons must remain authoritative;
neither reducing rendered content nor silently dropping offscreen navigation metadata
would be a valid performance fix. No source-ledger or benchmark gate is completed.
