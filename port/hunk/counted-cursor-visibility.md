# Counted cursor visibility qualification

`source_controller::tests::counted_alpha_cursor_movement_reveals_nearest_row_and_carries_hunk_selection`
uses the pinned two-hunk alpha fixture with no source reader. In stack and split
layouts it checks a four-cursor move against the rendered cursor list, visibility
in an eight-row review area, unchanged scrolling for a zero-distance move,
bottom clamping after repeated movement, synchronized hunk selection, and a
large reverse move clamped to the first cursor with nearest placement.

Both pinned Hunk checkouts passed these source test filters under disposable
Bun 1.3.14: `requests a reveal every time`, `moves several rendered lines`, and
`carries hunk selection along` (three tests, sixty assertions per pin).
Pins: main 2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd.

The native test passed. It validates cursor and viewport state, not a terminal
cell oracle or the exact number of source reveal requests. Hunk increments a
React reveal-request object consumed by its viewport; Rust currently applies
cursor selection and nearest scrolling synchronously in `step_diff_line`.
An architectural difference alone is not proof of semantic parity. No additional
source interval is mapped by this qualification; request/lifecycle behavior
needs separately executable evidence before the corresponding tests can be mapped.

Full native TUI validation: 1,190 library tests passed, zero failures, 8.45 seconds.
Workspace formatting and diff checks passed. The source ledger is unchanged.
