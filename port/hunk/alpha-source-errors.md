# Alpha source errors

Two complete baseline hook tests are translated:

- Bytes `[45969, 46636)`, lines 1386–1407:
  `source_controller::tests::alpha_null_source_sets_error_status`.
- Bytes `[46636, 47710)`, lines 1408–1440:
  `source_controller::tests::alpha_rejected_source_sets_error_and_logs_file_context`.

Both use the twelve-line alpha TypeScript fixture, alpha8 changed to 800,
context three, and toggle the leading gap through the real controller. A null
reader result and a rejected read both produce error status without a too-large
reason. The rejected reader reports `source unavailable`. A child test process
captures the real controller's stderr and asserts the file path, runtime ID and
error detail, replacing the source console.error interception without changing
global logging in concurrently running native tests. The child's successful
exit also proves its source-status assertion passed.

Native completion draining and owned state replace React flush/destroy
scaffolding. These cases contain no frame assertions. No production code changed.
The source tests passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` using disposable Bun 1.3.14:
two tests and eight assertions per pin. Nine focused native alpha tests passed
(0.80 seconds), with formatting and diff checks passing.

All 1,222 TUI library tests passed (8.88 seconds). Strict audit validated
ledger structure and evidence before rejecting incomplete coverage: 1,257
files, 1,425 records, 460 translated-test records, 275 unmapped records and
92 pending upstream commits. These mappings add 1,741 verified source bytes;
full product parity remains incomplete.
