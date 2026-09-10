# Filter cursor restoration

Source bytes 65494–66557 (exclusive end), lines 1962–1996, of pinned
`src/ui/hooks/useTerminalReview.test.tsx` are translated to
`source_controller::tests::filter_hides_alpha_cursor_and_clearing_restores_it`.

The native fixture preserves both source files' TypeScript contents, alpha/beta
runtime IDs, paths, three-line context and embedded metadata without source
readers. The real filter input handler types `beta`, which clears the visible
alpha cursor while retaining document selection. Clearing through Escape restores
the original cursor. This verifies the entire source test's cursor transition,
plus unchanged underlying selection, without claiming pointer or cell geometry.

The source test `clears the line cursor while its selected file is filtered out`
passed in both disposable Bun 1.3.14 checkouts: main
2c00f4358b89cfc0a6b04459ffc538ba601aa3c2 and stable
4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd (one test, ten assertions per pin).
The native regression test passed. Shared helpers and surrounding hook tests
remain separately accounted for; no full-hook runtime mapping is claimed.

The full TUI library suite passed all 1,194 tests (8.39 seconds), with formatting
and diff checks passing. Strict audit still exits 1 with 274 unmapped intervals,
1,410 records across 1,257 files and 11 cached pending upstream commits.
