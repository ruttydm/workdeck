# Annotation fixture correction and filtered-navigation fix

The deep-note interaction exposed an error in recent translated fixtures:
Hunk's JSON uses `newRange: [start, end]`, while direct deserialization into
Workdeck's internal `AgentAnnotation` requires `new_range: {start, end}`.
Unknown fields are retained in `extra`, so the earlier fixtures silently left
`new_range` unset. Their passing text checks did not prove the recorded ranges.
Earlier range claims in these fixtures' captures are superseded by this correction.
The public Workdeck schema was not changed to accommodate a test mistake.

All eight affected fixtures now assert the parsed `LineRange` and absence of a
stray `newRange` metadata field. Alpha/scroll/wrap defaults use line two; beta's
explicit viewport annotation stays on line one; the deep note uses line 62.

With correct ranges, seven tests passed and filtered session navigation failed:
it returned hidden alpha's hunk instead of the source no-annotated-hunks error.
That ledger interval was reopened. The relative-navigation candidate builder now
filters files with the existing shared matcher, matching pinned
`src/core/review/selectors.ts::selectReviewNavigationFiles`. This is distinct from
selection normalization: a hidden selection is retained, but relative moves walk
the visible stream. After the fix all eight tests passed (0 failures/ignored,
1,112 filtered, 1.09 seconds).

Affected tests in `crates/workdeck-tui/src/lib.rs`:

- `annotation_toggle_shows_notes_for_both_files_in_current_viewport`
- `arrow_keys_scroll_visible_lines_and_return_to_top_in_review_and_pager`
- `wrap_toggle_preserves_first_visible_added_line_after_arrow_scrolling`
- `layout_toggle_preserves_first_visible_source_line_after_arrow_scrolling`
- `tab_filter_input_renders_query_and_empty_match_message`
- `filter_displays_beta_but_preserves_hidden_selection_and_query_after_tab`
- `session_comment_navigation_keeps_active_filter_and_visible_files`
- `session_comment_navigation_reveals_deep_inline_note_and_returns_hunk`

The focused run used `CARGO_INCREMENTAL=0 cargo test -p workdeck-tui --lib --
--exact` followed by the eight fully qualified `tests::` names above. This correction
does not itself complete any whole source file or release gate.

After the runtime correction, `CARGO_INCREMENTAL=0 cargo test -p workdeck-tui
--lib` passed all 1,120 tests, with zero failures, ignored or filtered tests,
in 49.80 seconds. Formatting and TUI all-target Clippy with warnings denied
also passed. This verifies the corrected working tree based on `bbf3e7c6`, not
the uncorrected fixtures at that commit. It is not a workspace or platform-matrix gate.
