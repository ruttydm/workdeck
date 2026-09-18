# Invalid alpha comment batch

`source_controller::tests::invalid_alpha_comment_batch_is_atomic` translates
the complete baseline hook test at bytes `[23694, 24732)`, lines 733–768 of
`src/ui/hooks/useTerminalReview.test.tsx`.

The fixture matches the twelve-line alpha two-hunk diff. A request-2 batch
contains a valid alpha.ts hunk-0 note followed by a missing.ts hunk-0 note.
Both tests require the exact error `No diff file matches missing.ts.`, zero
stored comments and empty live-comment summaries. The native test additionally
asserts selection does not move despite requesting first-comment reveal.
Owned state replaces React flush/destroy scaffolding; no frame assertion is
present in the source case. No runtime implementation changed.

The source test passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14:
one test and six assertions per pin. The focused native test passed in
0.74 seconds, with formatting and diff checks passing.

All 1,228 TUI library tests passed (9.19 seconds). Strict audit validated
ledger structure and evidence before rejecting incomplete coverage: 1,257
files, 1,429 records, 462 translated-test records, 277 unmapped records and
92 pending upstream commits. This maps 1,038 bytes and splits the surrounding
unmapped interval; full parity remains incomplete.
