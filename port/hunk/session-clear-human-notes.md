# Session clear and human notes

`source_controller::tests::session_clear_alpha_human_notes_requires_explicit_opt_in`
recreates the two-hunk alpha fixture and the source session-clear sequence:
add an agent comment and save a human draft, remove the agent comment by ID,
add another agent comment and clear with default options, then add an agent
comment and clear with explicit `include_user: true`.

The test checks removal identity/source, all source removal-count assertions,
empty live-comment summaries, preservation of the complete saved human note
under default clearing, and empty comments/review summaries after inclusive
clearing. Existing runtime behavior passes; no production code was changed.

The pinned source test `session clear can include human user notes` passed on
main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`, one test and 25 assertions each,
using disposable Bun 1.3.14. The source test files were hash-checked against
the pinned Git blobs (`ed9cf33eed4e4a77a2ba4abf5d9f14b12df00635` and
`23517487f726d6b12cd8822cda7c7b57eb27fd16`, respectively).

The native focused test passed (0.76 seconds), as did all 1,211 TUI library
tests (8.72 seconds), formatting and diff checks. This evidence covers the
controller mutation sequence, not terminal frames, session transport, or the
complete note subsystem.

## Interval mapping review

The translated-test record covers exactly baseline bytes `[34240, 37312)`,
lines 1047–1140: the complete `session clear can include human user notes` test
and its following blank line. The source two-hunk alpha fixture matches the
native fixture's twelve lines and changes at lines 1 and 12. Native assertions
also check the surviving note's alpha path and user source, covering the source
`userNotesByFileId.alpha` checks rather than merely counting arbitrary comments.
The native owned app replaces React's asynchronous flush/destroy test scaffolding;
the case has no frame assertions. All mutation outcomes and source assertions
are represented. Surrounding source tests and the runtime hook remain unmapped.

The strengthened focused test passed (0.77 seconds), with formatting and diff
checks passing. Strict `cargo xtask port audit` validated the ledger structure,
blob coverage and evidence paths before failing its incompleteness gate:
1,257 files, 1,417 records, 451 translated-test records, 276 unmapped records,
and 92 pending upstream commits. Splitting the containing unmapped interval
increases its count by one while mapping 3,072 previously unmapped bytes.
This is not a passing strict audit or a product parity claim.
