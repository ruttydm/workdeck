# Hidden-file line reveal

Pinned `useTerminalReview.ts` lines 739–760 resolve `revealLine` from measured
visible cursors, then from `visibleFilesRef.current`. If the file is hidden by
the filter, neither lookup succeeds and the source returns `none` without moving
selection. This contract is derived from inspection of the pinned source, not a
newly captured differential oracle.

The native extension reveal handler previously validated against every document
file and mutated selection before looking for a rendered row. The regression
`source_controller::tests::extension_line_reveal_cannot_select_a_filter_hidden_file`
failed: alpha moved from old line 1/hunk 0 to new line 12/hunk 1 while hidden.

The handler now checks the existing file filter before mutating selection. Hidden
targets take the existing missing-line warning path, preserving selection, scroll
and the absent visible cursor. The test also clears the filter and confirms the
same target then resolves normally. General hunk selection and headless review
state are unchanged; this check belongs to visible terminal line reveal.

No source interval is newly mapped. Full reveal return outcomes, request counting
and missing-stop fallback remain open parity requirements.

Validation: all 1,196 TUI library tests passed after the runtime fix (8.54 seconds).
The strengthened focused test also passed after adding the clear-filter recovery
assertions. Formatting and diff checks passed.
