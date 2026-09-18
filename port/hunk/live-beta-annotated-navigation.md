# Live beta annotated navigation

`source_controller::tests::live_beta_comment_updates_annotated_navigation_without_reload`
uses the source single-line alpha and beta fixtures, inserts an unrevealed
new-side line-1 beta comment, checks its live summary, and navigates to beta's
annotated hunk. It removes the comment, returns to alpha, and verifies annotated
navigation no longer moves to beta. Generation remains unchanged throughout.

The test initially failed: selection stayed on alpha. The terminal navigation
index excluded agent comments while note display was off (the default), unlike
Hunk's annotated navigation. Navigation now includes active comments regardless
of display visibility, while still excluding orphaned comments and respecting
the file filter. Rendering visibility is unchanged.

The source case `live comment mutations update annotated navigation without remounting the app`
passed on main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` under disposable Bun 1.3.14:
one test and 23 assertions per pin.

This remains supplemental evidence: the source also asserts projected visible
file annotations and the scroll-to-note request. This native test does not yet
cover those assertions, so the full source case and runtime hook remain unmapped.

After the fix all 1,227 TUI library tests passed (8.90 seconds), with formatting
and diff checks passing. No ledger disposition changed.

## Filtering and visibility boundaries

The same test now filters the stream to alpha after adding beta's live comment.
Annotated navigation must preserve selection and scroll while retaining the
comment. After clearing the filter, navigation reaches beta, but agent-note
display stays off. This distinguishes navigation membership from both file
filtering and note rendering visibility. The focused test passed (0.74 seconds),
with formatting and diff checks passing. No additional source interval is mapped.

## Reveal intent and annotation projection

The native regression now asserts `review_reveal.scroll_to_note` after annotated
navigation. It also checks the production `saved_extension_annotations` projection:
beta has exactly one annotation with the inserted summary, and the projection
is empty after removal. The focused test passed in 0.75 seconds. The source case
was rerun successfully on both pinned baselines (one test, 23 assertions each).

These checks close the previously untested reveal flag and annotation-map
contents, but do not yet assert the merged visible-file object's agent metadata.
The complete source interval remains unmapped; no runtime change was necessary.

## Completed merged-file assertions

The regression now runs the production `merge_file_annotations_borrowed` step
after projecting stored notes. It asserts beta's merged agent annotation summaries
equal exactly `["Check beta rename"]`, then verifies beta's agent metadata is
absent after removal. The focused test passed in 0.76 seconds.

Together with the existing empty/one/empty live-summary counts, selected beta
path, hunk zero, note-reveal flag and unchanged generation, this covers every
assertion in source bytes 15894–18025 (lines 481–538). The test uses a single
native application throughout; Rust ownership replaces the source harness's
React mount/flush/destroy scaffolding. Only this complete test interval is now
mapped. The runtime hook and other source-test intervals remain separate work.
All 1,254 native TUI library tests passed in 9.80 seconds; formatting and diff
checks passed. The strict audit refresh was started after this mapping.

The strict audit completed after commit `538dc185`, exiting 1 at the incomplete
coverage gate: 1,257 baseline files, 1,440 interval records, 473 translated-test
records, 277 unmapped records, five tracked stable-only commits and 92 pending
upstream commits. This is not an audit pass; gates after the incomplete-coverage
check remain unproven. No fresh upstream fetch was part of this audit.
