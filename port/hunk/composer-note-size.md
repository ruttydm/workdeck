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

## Exact native save boundary

`composer_enforces_exact_semantic_note_byte_boundary` uses a fixed clock and
identical source/app state to calculate the semantic metadata overhead. It
successfully saves a note whose serialized semantic representation is exactly
262,144 bytes, rejects the same note with one additional ASCII byte, and verifies
that the rejected draft retains its ID and full body without persisting a note.
The focused test passed in 0.81 seconds; formatting and diff checks passed.

Both pinned main and stable `core/review/noteSize.test.ts` suites were rerun under
disposable Bun 1.3.14: four tests and six assertions passed per pin. Those tests
verify framing, field-order independence, combined-field overflow and multibyte
byte counting. This is not yet a frozen differential fixture for the complete
native projected note; no additional source interval is mapped.
