# Live beta annotated navigation

`source_controller::tests::live_beta_comment_updates_annotated_navigation_without_reload`
uses the source single-line alpha and beta fixtures, inserts an unrevealed
new-side line-1 beta comment, checks its live summary, and navigates to beta's
annotated hunk. It removes the comment, returns to alpha, and verifies annotated
navigation no longer moves to beta. Generation remains unchanged throughout.

The test initially failed: selection stayed on alpha. The terminal navigation
index excluded agent comments while note display was off (the default), unlike
Hunk's annotated navigation. Navigation now includes active comments regardless
of display visibility, while still excluding orphaned comments and respecting
the file filter. Rendering visibility is unchanged.

The source case `live comment mutations update annotated navigation without remounting the app`
passed on main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14:
one test and 23 assertions per pin.

This remains supplemental evidence: the source also asserts projected visible
file annotations and the scroll-to-note request. This native test does not yet
cover those assertions, so the full source case and runtime hook remain unmapped.

After the fix all 1,227 TUI library tests passed (8.90 seconds), with formatting
and diff checks passing. No ledger disposition changed.

## Filtering and visibility boundaries

The same test now filters the stream to alpha after adding beta's live comment.
Annotated navigation must preserve selection and scroll while retaining the
comment. After clearing the filter, navigation reaches beta, but agent-note
display stays off. This distinguishes navigation membership from both file
filtering and note rendering visibility. The focused test passed (0.74 seconds),
with formatting and diff checks passing. No additional source interval is mapped.
