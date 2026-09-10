# Active draft ownership

New-note, edit and reply composer opening now reject a second draft with pinned
Hunk's `A review note draft is already active.` message. The keyboard edit/reply
route only reveals a composer after successful opening; rejection does not
reveal or scroll the existing draft.

`source_controller::tests::note_edit_and_reply_do_not_replace_an_active_draft`
creates a saved root and then an unsaved draft, invokes New/Edit/Reply through
the built-in command router, and checks unchanged draft ID/body, saved-note
count, selection and scroll with the exact error message for each command.

The source guards are pinned main `core/review/intents.ts` lines 400, 455 and
492. This is supplemental runtime evidence; no ledger disposition changes.
All 1,242 TUI library tests passed in 9.32 seconds after all three guards and
keyboard rejection handling were added. Formatting and diff checks passed.
