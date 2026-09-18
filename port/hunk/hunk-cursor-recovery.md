# Hunk navigation and cursor recovery

The two source test bodies at bytes 63915–65494 (exclusive end), lines
1914–1961, of pinned `src/ui/hooks/useTerminalReview.test.tsx` are translated
to `source_controller::tests::hunk_navigation_carries_alpha_cursor_to_selected_hunk`
and `source_controller::tests::reload_recovers_alpha_cursor_when_selected_hunk_is_retired`.

Both use the source's twelve-line TypeScript alpha fixture with three context
lines, embedded metadata and no reader. The first invokes real hunk navigation
and verifies selection and cursor advance together. The second explicitly selects
hunk 1 through the native host navigation callback, reloads the source's one-hunk
replacement, and checks cursor recovery to alpha hunk 0.

Both source tests passed in disposable Bun 1.3.14 checkouts of main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (two tests, eighteen assertions per pin).
Filters: `carries the current line along when hunk` and
`recovers the current line when a reload`. Both native tests passed, alongside
the four other alpha-cursor regression tests.

Shared source helpers, surrounding reveal-request tests and the full hook runtime
remain unmapped. This mapping covers only these two complete test bodies.

Full TUI validation passed: 1,193 tests, zero failures (8.90 seconds), plus
formatting and diff checks. Strict audit still exits 1 with 274 unmapped intervals
and 11 cached pending upstream commits. The unmapped count increased because
extracting these 1,579 covered bytes leaves two disjoint unmapped intervals in
place of one; no additional bytes were declared incomplete or waived.
