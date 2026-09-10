# Composer note size enforcement

Composer create/reply and edit saves now measure the complete semantic note
before storage mutation, using the existing shared 256 KiB limit. The new
`ReviewComment::semantic_note` projection preserves IDs, parent, source, author,
range anchor, content and timestamps while omitting persistence-only fields from
the size representation. This avoids measuring Workdeck's different persistence
JSON instead of Hunk's semantic model.

`source_controller::tests::oversized_note_saves_preserve_drafts_without_mutation`
checks a create body of 256 KiB of multibyte text and an equally large ASCII edit.
Both exceed the whole-note limit once metadata is included. Failed saves retain
the draft and leave saved notes and state revision unchanged; correcting the
create body then saves normally.

This is supplemental runtime validation. Exact boundary-size oracle vectors and
all save error presentations are not proven here, and no ledger mapping changes.

Failed saves also restore the internal draft ID after attempted saved-ID
assignment. All 1,243 TUI library tests passed in 8.98 seconds, and all 178 review
library tests passed in 0.03 seconds. Formatting and diff checks passed.
