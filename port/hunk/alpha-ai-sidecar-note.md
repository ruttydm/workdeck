# Alpha AI sidecar note

`source_controller::tests::alpha_sidecar_annotation_is_exposed_as_noneditable_ai_note`
translates baseline hook test bytes `[24732, 26212)`, lines 769–821.
Both fixtures use the single-line alpha change and an ai:1 sidecar annotation
with source ai, new range [1, 1], author assistant, the summary
Prefer a named constant., and rationale It documents the changed value.

The native test checks every source output field: note ID, source, file path,
new range, author, non-editability, and body joining summary and rationale with
two newlines. Native dimensions are set to 80 by four, as in the source harness.
The source has no frame assertion; owned state replaces React flush/destroy.
No runtime implementation changed.

The source test passed under disposable Bun 1.3.14 on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`: one test and two assertions
per pin. The focused native test passed (0.74 seconds), with formatting passing.

All 1,231 TUI library tests passed (9.00 seconds). Strict audit validated ledger
structure and evidence before rejecting incomplete coverage: 1,257 files,
1,431 records, 464 translated-test records, 277 unmapped records and 92 pending
upstream commits. This mapping adds 1,480 verified bytes; full parity remains
incomplete.
