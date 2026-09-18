# Alpha gap without a source reader

Baseline `src/ui/hooks/useTerminalReview.test.tsx` bytes `[41191, 41832)`,
lines 1249–1268, contain the complete no-source-fetcher gap-toggle test.
`source_controller::tests::alpha_gap_without_reader_does_not_expand_or_load`
uses the same twelve-line TypeScript alpha fixture (alpha8 changed to 800,
context three), removes executable source capability, and toggles its leading
gap. It checks the source assertions: no expansion and no source status.
It additionally checks no loader, pending reveal, completed source request,
or selection mutation. Embedded DiffMetadata does not grant read authority.

The source test passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14,
one test and five assertions per pin. The native focused test passed in
0.74 seconds, with formatting and diff checks passing. Owned native state
replaces React flush/destroy scaffolding; this source test has no frame
assertions. No runtime implementation changed. Adjacent tests, including
StrictMode lifecycle behavior, remain unmapped.

All 1,215 TUI library tests passed (8.79 seconds). Strict audit validated the
ledger and evidence before rejecting incomplete coverage: 1,257 files, 1,420
records, 455 translated-test records, 275 unmapped records, and 92 pending
upstream commits. The unmapped interval count increases because this interior
641-byte mapping splits an existing interval; verified byte coverage increases.
