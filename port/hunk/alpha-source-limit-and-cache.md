# Alpha source limit and cache

Two complete baseline hook tests are translated in
`crates/workdeck-tui/src/source_controller.rs`:

- Bytes `[47710, 48441)`, lines 1441–1464:
  `alpha_gap_reports_too_large_source_status`. The source reader throws
  `SourceTextTooLargeError(5)`; the Rust reader returns the corresponding
  typed TooLarge error. Both assert exactly error status with reason too-large.
  The source limit value is not exposed by this status assertion; this test
  does not establish provider byte-limit enforcement.
- Bytes `[48441, 49784)`, lines 1465–1506:
  `alpha_gap_reopening_reuses_first_read`. The reader increments on every call,
  returns new-side `read-N\n` text, and returns no old text. Both tests open,
  close and reopen the leading gap, assert loaded `read-1\n`, and verify the
  reader count stays at its first-open value.

Both use the source twelve-line TypeScript alpha fixture with alpha8 changed
to 800 and context three. Native worker completion draining replaces React
flushes; owned state replaces renderer cleanup. Neither source case asserts
terminal frames. The runtime hook and surrounding command tests remain unmapped.

Both source tests passed under disposable Bun 1.3.14 on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`: two tests and ten assertions
per pin. The four native `alpha_gap_` tests passed in 0.77 seconds.

All 1,217 TUI library tests passed (9.96 seconds), with formatting and diff
checks passing. Strict audit validated ledger structure and evidence before
rejecting incomplete coverage: 1,257 files, 1,422 records, 457 translated-test
records, 275 unmapped records and 92 pending upstream commits. These mappings
add 2,074 verified bytes; full product parity remains unproven.
