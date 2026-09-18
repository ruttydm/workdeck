# Gap commands require source access

Pinned `src/ui/hooks/useTerminalReview.ts` checks `file.sourceFetcher` before
both renderer-addressed `toggleGap` and selected-hunk gap commands. The native
shared toggle previously mutated expansion state even when its file contained
only embedded diff metadata with no fetch capability.

`gap_commands_without_source_access_leave_review_unchanged` reproduced the
mismatch: the initial run failed because `expanded_gaps` was not empty. The
shared `toggle_source_gap_target` now checks file identity and the same source
presentation availability used by the renderer before changing expansion,
cursor restoration, pending reveal, scrolling, or status.

The native regression exercises both entry points with the pinned collapsed-top
fixture and checks unchanged expansion, restore points, selection, cursor,
scroll, status, and final frame. It passed after the change. This does not claim
that every source-fetch lifecycle or protocol route is complete.

The regression also covers a patch-only file with no addressable gap. The
selected-gap command previously invented an unavailable-source status in that
case; it now returns without an effect, matching the source command's absent
intent behavior. The existing editor/keybinding test still checks the editor
request and `z` routing, with the no-op status expectation corrected.

Source validation used Bun 1.3.14 only in disposable oracle trees. On both main
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`, the original test
`toggleGap is a no-op for files without a source fetcher` passed: one test,
five assertions, zero failures per pin. This is source test evidence, not a
frozen terminal-cell comparison. No source interval is newly mapped; the full
hook remains explicitly unmapped in the ledger.

Final scoped validation: all 1,180 TUI library tests passed with zero failures,
ignored tests, or filters in 12.98 seconds; formatting and diff checks passed.
Full-workspace, strict Clippy, benchmark, and release gates were not rerun for
this change and remain separate requirements.
