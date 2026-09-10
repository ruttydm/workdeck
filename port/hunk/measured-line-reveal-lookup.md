# Exact measured-line reveal lookup

Native extension and session reveal paths now check the current row plan for the
exact file index, side and line before treating navigation as a measured-line
reveal. The former cursor-off-only condition could not distinguish other absent
rows from measured ones. The shared `has_measured_review_line` helper explicitly
returns false with cursor painting off and otherwise searches current geometry
cursor targets without using selection's fallback-to-first-hunk-row behavior.

The lookup feeds extension fallback placement, session navigation outcome, and
attention-mark reveal. Existing visible-file and valid-hunk checks remain in
place. The source contract is the pinned hook's exact `findLineCursorAt` lookup
before its containing-hunk fallback.

`source_controller::tests::measured_line_lookup_excludes_gaps_hidden_files_and_disabled_cursor`
checks valid old/new targets, collapsed context, invalid file/line targets,
filter-hidden content and disabled cursor mode against the real two-hunk fixture.
Existing session fallback, cross-file navigation and mark tests exercise callers.
This is not full request-counter, custom file-view or terminal-cell oracle parity;
no additional ledger mapping is claimed.

All 1,207 TUI library tests passed (8.56 seconds), plus formatting and diff checks.
