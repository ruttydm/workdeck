# Expanded-source extension reveal

The extension reveal path previously required membership in an original patch
hunk before consulting measured rows. This rejected loaded, expanded source
lines even while those lines were visible and navigable by the cursor.

`source_controller::tests::extension_reveal_reaches_loaded_source_outside_original_hunk`
loads two leading source lines through a real deferred source reader, expands
them, moves to the original changed line, then reveals new source line 1.
Before the fix the action reported `found no new line 1` despite an exact
measured target. It now resolves the measured cursor first and uses the existing
loaded-source-aware selection path. Original-hunk fallback remains for targets
without measured cursors. Disabled-cursor and hidden-file behavior remain guarded.

This follows the pinned hook's `findLineCursorAt`-before-hunk-lookup ordering.
Evidence is source inspection plus a native failing-then-passing regression,
not a new frozen source oracle. Full hook and protocol mappings remain open.

All 1,208 TUI library tests passed (9.49 seconds), including the regression,
with formatting and diff checks passing. No ledger disposition changed.

The regression also now collapses the loaded gap, then attempts to reveal its
formerly visible line. It verifies a missing-target warning, unchanged selection
and scroll, and no reopened gap. This ensures cached source availability is not
mistaken for current row visibility. The extended focused test passed (0.73
seconds), along with formatting and diff checks. No further runtime change or
source mapping was needed for this transition.
