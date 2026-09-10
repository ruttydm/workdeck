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

## Frozen projection oracle

`semantic-note-size-oracle.json` records identical outputs from both pinned
versions of `src/core/review/noteSize.ts`, executed with disposable Bun 1.3.14.
The input includes parent identity, old/new ranges, preferred line, two hunk
indices, author, timestamps and a body containing quotes, a backslash, newline,
tab, emoji and CJK text. Both report 399 serialized UTF-8 bytes and acceptance.

`workdeck_review::tests::persistence_projection_matches_pinned_semantic_size_oracle`
constructs a native persistence note with that content and additional native
file/line fields, then checks its complete projected JSON, byte count and limit
result against the frozen output. The focused test passed, along with formatting
and diff checks. This closes one projected-note differential vector, not the
complete note fixture matrix or any additional ledger interval.
All 179 review library tests passed in 0.02 seconds with the oracle test included.
To recapture in a disposable pinned checkout, import `reviewNoteByteLength` and
`reviewNoteWithinSizeLimit` from `src/core/review/noteSize.ts`, pass the committed
fixture's `note` object unchanged, and compare both results with `bytes` and
`withinLimit`. No original runtime is required by the committed Rust test.
