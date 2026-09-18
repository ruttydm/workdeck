# Changed alpha source reload

The translated test
`source_controller::tests::changed_alpha_reload_retires_source_and_loads_replacement_text`
covers baseline `src/ui/hooks/useTerminalReview.test.tsx` bytes
`[52019, 54269)`, lines 1569–1623. Both source and native fixtures use twelve
TypeScript alpha lines with alpha8 changed to 800 initially, then 900 after
reload, and context radius three.

The initial reader supplies exactly `first\n` for the new side and no old text.
After expanding the leading gap, the test verifies loaded text and expansion.
It reloads the changed fixture and installs a new reader, checks that source
status and expansion are absent, then reopens the gap and verifies exactly
`second\n` and invocation of the replacement reader. The native worker is
drained explicitly in place of React flushing; the case has no frame assertions.

The source test passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14:
one test and sixteen assertions per pin. The native focused test passed in
0.75 seconds. No production code changed. The preceding unchanged-file reload
case remains unmapped, including its replacement-reader ownership semantics.

All 1,213 TUI library tests passed (8.69 seconds), with formatting and diff
checks passing. Strict audit validated ledger structure and evidence before
rejecting incomplete coverage: 1,257 files, 1,418 records, 453 translated-test
records, 275 unmapped records, and 92 pending upstream commits. This mapping
adds 2,250 verified bytes without claiming full subsystem or product parity.
