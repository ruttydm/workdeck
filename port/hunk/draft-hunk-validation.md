# Draft hunk validation

Create/reply saves now require the target hunk to exist before calculating an
anchor or adding a note. Missing file targets were already rejected; a missing
hunk previously reached the permissive anchor calculation. The source basis is
pinned main's `planUserNoteCreation`, which requires both file and hunk.

`source_controller::tests::note_save_rejects_missing_hunk_without_consuming_draft`
deliberately gives a new draft an invalid hunk index and verifies retained draft
identity/body, unchanged state revision, no saved note and an explanatory error.
This tests save-boundary validation rather than claiming a normal UI navigation
can create such a draft. No source interval is newly mapped.
The focused regression passed in 0.78 seconds, followed by all 1,246 TUI library
tests in 9.05 seconds. Formatting and diff checks passed.
