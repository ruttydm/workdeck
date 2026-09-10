# Alpha session hunk fallback

`source_controller::tests::alpha_session_navigation_without_cursor_rows_reports_hunk_fallback`
uses the pinned two-hunk alpha fixture and invokes the native session navigation
entry point for new line 12 with cursor painting disabled. It asserts response
hunk 1, the `hunk` reveal outcome and synchronized selected hunk 1.

The final pinned hook case, `navigate line targets fall back to the hunk when no row is measured`,
passed under disposable Bun 1.3.14 on main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, four assertions per pin).
The native test also passed.

This qualifies the matching fixture through the production cursor-off path.
The source harness can independently suppress published cursors; the native
test does not yet exercise every reason that measured rows might be unavailable.
No additional source ledger mapping is claimed by this supplemental test.

All 1,203 TUI library tests passed (9.45 seconds), with formatting and diff
checks passing. Production code is unchanged by this qualification.
