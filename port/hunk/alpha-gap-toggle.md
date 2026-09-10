# Alpha lazy gap toggle

Baseline bytes `[37312, 38639)`, lines 1141–1178 of
`src/ui/hooks/useTerminalReview.test.tsx`, contain the complete source case
`toggleGap flips per-file expansion state and lazily loads source text`.
The translated test is
`source_controller::tests::alpha_gap_toggle_loads_exact_source_and_collapses`.

Both fixtures use twelve TypeScript alpha lines, with alpha8 changed to 800,
context radius three, and an installed source reader. The reader returns exactly
`alpha\nbeta\ngamma\n` for the new side and no text for the old side.
The test toggles the leading gap, waits for the actual worker completion,
checks expansion, the reader invocation and exact loaded status/text, then
toggles again and checks collapse. It additionally verifies no eager read
before the first toggle. Native owned state and completion draining replace
the source React mount/flush/destroy scaffolding; there are no source frame
assertions in this interval.

The source test passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` using disposable Bun 1.3.14,
one test and ten assertions per pin. The focused Rust test passed in 0.75
seconds. Formatting and diff checks passed. This mapping does not cover the
surrounding runtime hook, rendering, or other source-loading cases.

All 1,212 TUI library tests passed (8.61 seconds). Strict audit validated the
ledger structure and evidence before rejecting incomplete coverage: 1,257
files, 1,417 records, 452 translated-test records, 275 unmapped records and
92 pending upstream commits. The release gate remains unmet.
