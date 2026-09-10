# Unchanged alpha reader reload

The native test
`source_controller::tests::unchanged_alpha_reload_with_fresh_reader_preserves_loaded_gap`
initially failed: installing a new host reader reset completed source status
even after an identical-content reload. Both pinned source tests retained the
loaded status and the leading gap's expansion.

`install_source_loader` now replaces the host reader without discarding loaded
text when the existing binding has the same nonempty content identity and is
not a VCS binding. Pending/failed/unavailable statuses still follow the normal
binding path. VCS capability installation retains its separate runtime identity
and attestation checks. Changed-content retirement remains covered by the
changed-alpha test and existing stale-completion tests.

The translated-test interval is exactly baseline bytes `[50888, 52019)`,
lines 1536–1568 of `src/ui/hooks/useTerminalReview.test.tsx`. The alpha fixture
contains twelve TypeScript lines and changes alpha8 to 800. Both tests expand
the leading gap, load `first\n`, reload identical content with a fresh reader,
and assert loaded status and expansion survive. Native completion draining and
owned app lifetime replace React flush/destroy scaffolding; the source case
does not assert frames. The runtime hook remains unmapped.

Source runs under disposable Bun 1.3.14 passed on main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`: one test and six assertions per
pin. After the fix all 1,214 TUI library tests passed (8.61 seconds).

Formatting and diff checks passed. Strict audit validated ledger structure
and evidence before rejecting incomplete coverage: 1,257 files, 1,418 records,
454 translated-test records, 274 unmapped records, and 92 pending upstream
commits. Full parity remains incomplete.
