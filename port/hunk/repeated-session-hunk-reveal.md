# Repeated session hunk reveal

Pinned `useTerminalReview.ts` sends an explicit reveal request from `selectHunk`
even when the target is already selected. Its attention-mark fallback invokes
that same action. The native session fallback instead passed `navigate` a
predicate requiring selection to change, so a successful repeated reveal left
the viewport displaced.

The extended regression
`session_review_controller::tests::cursor_off_agent_highlight_reports_hunk_fallback`
selects the hunk, displaces a one-row viewport to the last valid row, and repeats
the attention-mark reveal. Before the fix it returned `hunk` but kept scroll 1
instead of the expected hunk placement at 0.

Explicit session hunk navigation and attention-mark fallback now reposition after
successful selection, including unchanged selection. Ordinary directional
navigation is unchanged. The test verifies unchanged selection and restored
viewport placement. Source behavior here is established by pinned-source
inspection; no new source test interval or full protocol parity is claimed.

Validation: the first full TUI run passed 1,197 tests but failed the atomic-save
watcher test at its `reload_pending` assertion. That unchanged test passed in
isolation, then the full suite passed all 1,198 tests (8.42 seconds). This is an
observed intermittent watcher-test failure, not proof its cause is fixed.
Formatting and diff checks passed; watcher code and test expectations were not
changed by this commit.
