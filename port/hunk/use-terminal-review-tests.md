# `useTerminalReview` test migration

The pinned `src/ui/hooks/useTerminalReview.test.tsx` suite is exercised by the
native review controller.  React/OpenTUI's test harness and fixture builders
were translated into deterministic `ReviewApp` fixtures; the behavior remains
owned by the same review/session state machine rather than a compatibility
runtime.

The following previously open byte intervals are now covered by executable Rust
tests.  The intervals are disjoint and are recorded separately in the ledger:

| Pinned bytes | Native executable coverage |
| --- | --- |
| `0-10122` | filter preservation, reload hunk clamping, and fixture identity (`source_controller` tests `filter_hides_alpha_cursor_and_clearing_restores_it`, `reload_hunk_clamp_preserves_existing_file_only_selection`) |
| `11068-11787` | selection-only navigation retains the review document (`selection_only_alpha_navigation_retains_document_allocation`) |
| `30319-32187` | pointer-targeted edit/reply drafts preserve the viewport (`pointer_targeted_new_draft_preserves_viewport_with_cursor_off`, `note_edit_and_reply_do_not_replace_an_active_draft`) |
| `41191-41832` | gap toggling without a source reader is a no-op (`alpha_gap_without_reader_does_not_expand_or_load`) |
| `40137-41191` | Strict-mode-equivalent source loading settles from loading to loaded (`alpha_gap_toggle_loads_exact_source_and_collapses`, `selected_alpha_gap_reads_new_side_once`) |
| `41832-44852` | catalog-declared note, gap, and draft actions (`alpha_catalog_commands_toggle_notes_start_draft_and_expand_next_gap`) |
| `59368-63915` | line movement, reveal request coalescing, hunk-boundary selection, and note targeting (`counted_alpha_cursor_movement_reveals_nearest_row_and_carries_hunk_selection`, `pointer_targeted_new_draft_preserves_viewport_with_cursor_off`) |
| `66557-74363` | cross-file and measured/unmeasured line reveals, filtered targets, and attention marks (`cross_file_beta_line_reveal_resolves_before_another_frame`, `extension_line_reveal_reads_current_rows_after_reload`, `extension_line_reveal_cannot_select_a_filter_hidden_file`, `cursor_off_line_reveal_uses_containing_hunk_placement`, `filter_hides_alpha_cursor_and_clearing_restores_it`) |
| `80317-82411` | exact-line navigation and hunk fallback (`extension_line_reveal_reads_current_rows_after_reload`, `cursor_off_line_reveal_uses_containing_hunk_placement`) |

The source bytes are read from the pinned Git blob by the ledger audit; no
TypeScript mirror is added to the Workdeck tree.  Native tests run under both
stack and split geometry where the source suite varied by layout.
