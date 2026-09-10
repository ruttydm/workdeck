# Saved-note timestamps

Native composer saves now record creation timestamps in UTC ISO 8601 format with
three fractional digits. Editing retains the original creation timestamp and sets
the update timestamp, including when the body is unchanged. The existing untimed
`ReviewState::edit_comment_summary` API retains its behavior; composer saves use
the timestamp-aware variant. No existing saved note ID or creation time is rewritten.

`source_controller::tests::saved_note_timestamps_preserve_creation_across_edits`
uses a fixed millisecond clock, checks `2023-11-14T22:13:20.000Z` on creation,
then edits at `.123Z` precision and again with unchanged text at a later second.
It verifies identity, preserved creation time, updated timestamps and equality
between the returned note and persisted state after each edit.

Source basis: pinned main `useTerminalReview.ts` supplies an ISO timestamp on
save, and `core/review/intents.ts` sets creation time on create and update time on
edit. The pinned main source test `prefills an edit draft and replaces the saved
note without changing its identity` passed under disposable Bun 1.3.14 (one test,
six assertions). This supplements runtime integration evidence; it does not add
a source ledger mapping or claim complete note lifecycle parity.

Verification: all 1,237 TUI library tests passed in 9.58 seconds, and all 178
review library tests passed in 0.02 seconds. Formatting and diff checks passed.

## Author and tag defaults

Pinned main `core/review/intents.ts` creates notes with `author: "user"` and no
tags. Native newly saved notes now match those defaults instead of omitting the
author and inventing a `user` tag. Existing persisted metadata is not rewritten.
The fixed-clock save/edit regression now also checks author and empty tags on
creation and unchanged author/tags across both edits. No ledger mapping changes.
All 1,242 TUI library tests passed in 8.80 seconds with these defaults; formatting
and diff checks passed.
