# Ordered alpha comment batch

`source_controller::tests::alpha_comment_batch_preserves_order_and_reveals_first_hunk`
translates baseline hook test bytes `[22449, 23694)`, lines 693–732.
Both tests use the twelve-line two-hunk alpha fixture and request-1 with
two comments: Later hunk note on hunk 1, then Earlier hunk note on hunk 0.
First-comment reveal is enabled. Assertions cover applied hunk order [1, 0],
two stored comments, selected hunk 1, and live summary order matching input.
Owned native state replaces React flush/destroy scaffolding; this source test
does not assert frames or scroll coordinates. No runtime code changed.

The source case passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14:
one test and eight assertions per pin. The focused native test passed in
0.75 seconds, with formatting and diff checks passing.

All 1,229 TUI library tests passed (8.69 seconds). Strict audit validated
ledger structure and evidence before rejecting incomplete coverage: 1,257
files, 1,430 records, 463 translated-test records, 277 unmapped records and
92 pending upstream commits. The mapping adds 1,245 verified source bytes;
full product parity remains incomplete.
