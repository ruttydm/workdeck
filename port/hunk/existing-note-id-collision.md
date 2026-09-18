# Existing note ID collision

The native composer sequence starts at zero in a new app. An existing review
containing `user-note-1` therefore collided with the first newly opened draft.
`source_controller::tests::new_draft_does_not_collide_with_existing_user_note_id`
reproduced this by transferring a saved note into a fresh app, then opening a
new draft. Its distinct-ID assertion failed before the fix.

New-note and reply allocation now checks current persisted comment IDs and skips
occupied identifiers. Existing IDs, parent references and schemas are untouched.
The counter wraps rather than saturating, so reaching its numeric limit cannot
trap allocation on one occupied value. The test verifies the existing note is
unchanged and a second note with its intended body is saved.

This fixes a native collision independently of the remaining Hunk timestamp-ID
compatibility requirement. It does not claim matching source ID formatting,
cross-process persistence or full hook parity. No source ledger mapping changes.

All 1,210 TUI library tests passed (8.50 seconds), plus formatting and diff checks.
