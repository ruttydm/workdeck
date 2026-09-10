# Alpha catalog command sequence

`source_controller::tests::alpha_catalog_commands_toggle_notes_start_draft_and_expand_next_gap`
uses the source two-hunk alpha fixture (twelve TypeScript lines, changes at
lines 1 and 12, context three). It invokes public extension command dispatch
to toggle notes twice, start a new note at new line 1 of hunk 0, then expand
the selected hunk's nearest available gap. The final expansion must be only
the leading gap of hunk 1.

The initial native test failed waiting for a source completion: the terminal
gap handler only considered the selected hunk's leading gap or its own trailing
gap. It did not search subsequent leading gaps as the source/shared selector
does. The handler now scans forward from the selected hunk and falls back to
the trailing gap. The command dispatcher also now lowers StartDraft,
ToggleNoteVisibility and ToggleSelectedGap from their catalog review effects
instead of matching their command IDs independently.

The source case `runs the review effect the catalog declares for the note layer,
the nearest gap, and a new note` passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14,
one test and eighteen assertions per pin.

This is supplemental evidence, not a ledger mapping. Native draft cancellation
in this test uses direct state cleanup, and the source's absent measured-line
setup and catalog declaration assertions are not reproduced here. The source
test and runtime hook remain unmapped; those obligations are not waived.

After the gap-policy fix, all 1,218 TUI library tests passed (10.78 seconds).
Formatting and diff checks passed. No ledger disposition changed.
