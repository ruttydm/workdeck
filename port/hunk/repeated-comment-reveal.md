# Repeated comment reveal

`source_controller::tests::batch_first_reveal_repositions_an_already_selected_hunk`
selects alpha hunk 1, adds a comment without reveal, records the hunk's normal
reveal position, scrolls away, then adds another batch with first-comment reveal.
The test initially failed with scroll 0 instead of 4: the controller required
selection to change before executing the requested reveal.

Single-comment and first-batch-comment explicit reveal now navigate after a
successful hunk selection even if it is unchanged. Ordinary directional
navigation retains its change guard. Pinned Hunk's `addLiveCommentBatch` invokes
`selectHunk` whenever first reveal is requested and a first entry exists;
`selectHunk` submits the reveal intent without an unchanged-selection guard
(`src/ui/hooks/useTerminalReview.ts`, lines 549–563 and 1203–1216).

This is supplemental regression coverage, not a new source-test mapping or
complete runtime-hook port. The source anchor is
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. No ledger disposition changed.

After the fix all 1,230 TUI library tests passed (9.66 seconds), with formatting
and diff checks passing.

The regression test also covers the single-comment path: after scrolling away,
adding with reveal disabled keeps scroll at zero; another addition with reveal
enabled returns to the expected hunk position while retaining hunk selection.
The extended focused test passed (0.74 seconds), with formatting and diff checks
passing. This adds no source-ledger mapping.
