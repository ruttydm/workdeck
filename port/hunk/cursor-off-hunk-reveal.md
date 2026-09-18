# Cursor-off hunk reveal placement

The native extension line-reveal handler now uses containing-hunk selection and
placement when current-line painting is disabled. Previously it selected the
requested line and applied line placement even in that mode. The regression
`source_controller::tests::cursor_off_line_reveal_uses_containing_hunk_placement`
initially failed with scroll row 7 versus the corresponding hunk selection's row 4.
It compares selection and scroll against the actual hunk navigation path in an
eight-row review area, using the pinned two-hunk alpha contents.

The source test `falls back to the containing hunk when nothing measured a row for the line`
passed on both main 2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd using disposable Bun 1.3.14
checkouts (one test, seven assertions per pin). Its harness suppresses cursor
publication, corresponding to the source's cursor-off rendering behavior.

This fixes native cursor-off placement but is not a complete mapping of that
source test: the native fixture has two hunks, and public return outcomes and
reveal-request counts still need separate implementation/evidence. Arbitrary
missing-stop situations beyond cursor-off mode remain to be qualified.

After the runtime fix all 1,197 TUI library tests passed (8.55 seconds).
The strengthened focused test comparing full selection also passed. Formatting
and diff checks passed. Source ledger dispositions are unchanged.
