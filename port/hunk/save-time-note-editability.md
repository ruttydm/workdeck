# Save-time note editability

Composer edit saves now recheck the current stored note's `editable` flag before
changing its body or timestamp. This matches pinned main's user-note update
intent, which rejects a note that is no longer editable. The generic Workdeck
storage editing API remains unchanged; the review composer enforces this rule.

`source_controller::tests::note_save_rechecks_editability_without_consuming_draft`
opens an edit, replaces the stored note with a non-editable version, and attempts
to save. It verifies retained draft ID/body, unchanged stored note and revision,
and an error explaining that the note is not editable. This is supplemental
runtime evidence; no source interval is newly mapped.
The focused regression passed in 0.88 seconds, then all 1,245 TUI library tests
passed in 9.95 seconds. Formatting and diff checks passed.
