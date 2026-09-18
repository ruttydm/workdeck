# Degraded markup feedback

`source_controller::tests::alpha_degraded_markup_returns_agent_render_notes`
translates the pinned hook case with the single-line alpha fixture and explicit
experimental launch authority. A `<sparkline>` comment returns render feedback
containing `unknown tag`; the following `<box border>` comment returns no markup
notes. Both use the real session mutation route with reveal disabled.

Both pinned main and stable source cases passed under disposable Bun 1.3.14
(one test, four assertions per pin). The native focused test passed in 0.75
seconds; formatting and diff checks passed.

Only hook-test bytes 19832–21306, lines 600–652, are mapped. This case checks
feedback presence, not exhaustive STML rendering or live-width parity.
Strict audit still fails on incomplete coverage: 1,257 files, 1,436 intervals,
469 translated-test records, 277 unmapped records and 92 pending upstream commits.
