# Selected alpha gap

`source_controller::tests::selected_alpha_gap_reads_new_side_once` translates
the full baseline hook test at bytes `[44852, 45969)`, lines 1351–1385 of
`src/ui/hooks/useTerminalReview.test.tsx`.

Both fixtures use thirty plain lines, change line 5 to `line 5 changed`, and
parse with context three, file ID alpha and path alpha.ts. A source reader
records every requested side and returns the updated text only for new-side
reads. The selected-gap action must expand the leading gap of hunk 0 and
produce exactly one new-side read. Native worker completion draining and owned
app lifetime replace React flush/destroy scaffolding; this case has no frame
assertions. Neighboring command-effect and rejected-reader cases remain unmapped.

The source case passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` using disposable Bun 1.3.14:
one test and four assertions per pin. The native focused test passed in
0.76 seconds. Formatting passed. No production code changed in this step.

All 1,220 TUI library tests passed (8.79 seconds). Strict audit validated
ledger structure and evidence before rejecting incomplete coverage: 1,257
files, 1,424 records, 458 translated-test records, 276 unmapped records and
92 pending upstream commits. The interior mapping adds 1,117 verified bytes
while splitting one remaining interval into two; full parity is not achieved.
