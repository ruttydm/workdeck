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

This is supplemental evidence, not a ledger mapping. The source's absent
measured-line setup is not reproduced here. The source
test and runtime hook remain unmapped; those obligations are not waived.

After the gap-policy fix, all 1,218 TUI library tests passed (10.78 seconds).
Formatting and diff checks passed. No ledger disposition changed.

## Catalog authority regression test

`app_commands::tests::note_and_gap_dispatch_follow_catalog_effects_not_command_identity`
checks the three shipped review-effect declarations from the source case.
It then substitutes each of the three effects for each of the three command
IDs (nine combinations), verifying dispatch follows the declaration rather
than the ID. This guards the architectural requirement beyond checking the
current default outcomes. The focused test passed, along with formatting and
diff checks. The remaining measured-line obligation still prevents
mapping the full source test; no ledger disposition changed.

All 1,219 TUI library tests passed after this addition (8.60 seconds).

## Real draft cancellation input

The sequence now sends Escape through `ReviewApp::handle_key` instead of
clearing the composer directly. It asserts the draft is absent, focus is back
in Review, and no comments were persisted before executing the gap command.
The focused test passed (0.75 seconds). This closes the direct-cleanup test
shortcut; it does not reproduce the source harness's unpublished line cursors.

All 1,219 TUI library tests passed after the input change (8.63 seconds),
with formatting and diff checks passing. Ledger dispositions are unchanged.
