# Initial cursor navigation

The two test bodies at bytes 57799–59368 (exclusive end), lines 1724–1771,
of pinned `src/ui/hooks/useTerminalReview.test.tsx` are translated into
`source_controller::tests::initial_alpha_cursor_is_seeded_at_selected_hunk_without_source_reader`
and `source_controller::tests::alpha_cursor_steps_one_row_and_clamps_at_stream_start`.

The fixtures preserve the source's twelve TypeScript lines, three context lines,
alpha runtime ID, embedded diff metadata and absence of a source reader. Initial
placement uses the change to alpha8; stepping uses changes to line1 and line12.
The tests exercise the real ReviewApp cursor initialization and movement path,
asserting the selected file/hunk and forward/backward/top-clamped cursor identity.

Validation: both Rust tests passed. The corresponding source tests passed on both
main 2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd using disposable Bun 1.3.14
oracle checkouts. Source filters were `seeds the current line at the selected hunk`
(one test, four assertions per pin) and `moves the current line one row at a time`
(one test, twelve assertions per pin).

This mapping does not cover the shared source helpers, reveal-request counters,
pixel-level indicator rendering or the remaining hook implementation. These remain
subject to their own ledger records and parity evidence.

The full TUI library suite passed: 1,189 tests, zero failures (8.47 seconds).
Formatting and diff checks passed. Strict port audit still fails with 273 unmapped
intervals across 1,257 files and 11 cached pending upstream commits. Splitting off
these two tests adds one translated interval without eliminating the remaining
unmapped interval; this is not a product-completion percentage.
