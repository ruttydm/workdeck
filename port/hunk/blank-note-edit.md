# Blank note edits

Native composer saves now reject blank edits while retaining the active draft,
its body and the original saved note. New empty drafts retain their separate
discard behavior. The error uses pinned Hunk's message: an edited review note
cannot be blank; cancel or delete it instead.

`source_controller::tests::blank_note_edit_retains_draft_and_original_note`
checks whitespace-only input, unchanged persisted note and state revision,
retained draft body, exact error text, and successful correction and resave with
the same note ID. This supplements runtime integration evidence and does not
change a ledger disposition.

The pinned main `core/review/intents.test.ts` case `rejects blank edits without
retiring the draft` is the source basis for the failure semantics.
It passed under disposable Bun 1.3.14 (one test, two assertions). All 1,238 native
TUI library tests passed in 8.88 seconds; formatting and diff checks passed.
