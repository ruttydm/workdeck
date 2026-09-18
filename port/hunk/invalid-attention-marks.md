# Invalid attention marks

Source bytes 74363–75739 (exclusive end), lines 2208–2252, of pinned
`src/ui/hooks/useTerminalReview.test.tsx` are translated into
`source_controller::tests::invalid_alpha_attention_marks_leave_review_unchanged`.
The fixture preserves the source's two-hunk alpha contents, metadata and absence
of a reader. The native session attention-mark entry point rejects missing.ts,
uncovered new line 9001 and empty range [4, 4) with the exact source error messages.
After each rejection the mark map stays empty; selection and cursor are unchanged.

The source test passed under disposable Bun 1.3.14 on both main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, six assertions per pin).
The corresponding native regression passed. Only this complete test body is
mapped; shared helpers, mark rendering and the remaining hook implementation
retain separate coverage requirements.

All 1,200 TUI library tests passed (8.50 seconds), with formatting and diff checks
passing. Strict audit still exits 1: 1,257 files, 1,412 records, 275 unmapped
intervals and 11 cached upstream commits. Extracting this covered test leaves two
unmapped neighbors where there was one; the higher interval count is not lost
coverage or a product-completion percentage.
