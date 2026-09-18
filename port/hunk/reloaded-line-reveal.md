# Reloaded line reveal qualification

`source_controller::tests::extension_line_reveal_reads_current_rows_after_reload`
retains the native reveal function before replacing a twelve-line, two-hunk alpha
document with the source's thirty-line, three-hunk fixture. It invokes the reveal
after reload for new line 30 and verifies hunk 2, side, line, synchronized selection,
and viewport visibility. A subsequent invalid line 999 must preserve cursor and
selection while reporting a new warning. Successful navigation preserves the
existing `review reloaded` status rather than requiring an empty status.

The related pinned source tests passed under disposable Bun 1.3.14 on both
main 2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd: `reveals one line of another file`,
`falls back to the containing hunk`, and `a revealLine reference held`
(three tests, twenty-nine assertions per pin).

This is supplemental native qualification, not a full translation of those three
tests. A Rust function pointer does not reproduce React's captured closure or
pre-measurement effect timing. Cross-file fixture parity, missing-stop fallback,
return outcomes and exact reveal-request counting still need separate evidence.
No source ledger interval is mapped by this test.

The full TUI library suite passed all 1,195 tests (8.75 seconds). Formatting and
diff checks passed. No production runtime behavior changed in this qualification.
