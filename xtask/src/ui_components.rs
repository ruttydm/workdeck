//! Exhaustive test-corpus accounting for Hunk's primary UI component suite.
//!
//! The pinned suite is intentionally inspected through `git show`: no TypeScript test source is
//! copied into the Workdeck tree.  Each upstream test name is paired with one or more executable
//! Rust tests.  The verifier checks the source blob, the complete name set, every Rust anchor, and
//! the frozen component oracles together, so a convenient aggregate mapping cannot hide a missing
//! test.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "src/ui/components/ui-components.test.tsx";
const BASELINE_BYTES: usize = 144_670;
const BASELINE_SHA256: &str = "22572d8600014118c9c8c87fb491938b26d28075d6208a9697d60650b21926a9";
const STABLE_BYTES: usize = 130_408;
const STABLE_SHA256: &str = "3ba7abbb7c8bdd8423f61cd91a4264779bdc8820d707eb2eb8ddcbe8aeb99a82";

struct UiTestMapping {
    upstream: &'static str,
    native: &'static [&'static str],
}

// Keep one explicit row for every `test(...)` in the pinned baseline.  Reusing a native test is
// allowed only when that test exercises the exact shared contract (for example the same Ratatui
// row planner is used by several component-only OpenTUI tests); the verifier still requires every
// upstream name to appear exactly once.
const TEST_MAPPINGS: &[UiTestMapping] = &[
    UiTestMapping {
        upstream: "the bundled sidebar view renders grouped file rows from the public props",
        native: &[
            "crates/workdeck-tui/src/public_review/tests.rs#native_sidebar_matches_the_executed_pinned_hunk_oracle",
        ],
    },
    UiTestMapping {
        upstream: "the bundled sidebar switches to its expanded tree at 32 content columns",
        native: &[
            "crates/workdeck-tui/src/public_review/tests.rs#sidebar_mode_switches_at_the_exact_content_width",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane renders all diff sections in file order",
        native: &[
            "crates/workdeck-tui/src/public_review/tests.rs#renders_reusable_file_header_and_multi_file_review_stream_primitives",
        ],
    },
    UiTestMapping {
        upstream: "DiffFileHeaderRow leaves one column after line counts",
        native: &[
            "crates/workdeck-tui/src/public_review/tests.rs#file_header_matches_the_pinned_one_row_cell_contract",
        ],
    },
    UiTestMapping {
        upstream: "DiffRowView renders a clickable add-note affordance for a hovered diff row",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::section_dispatches_rows_selection_cursor_and_hover_badges",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane selects the exact split line side on click without consuming add-note clicks",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::note_targets_prefer_new_addresses_and_reject_metadata_rows",
            "crates/workdeck-tui/src/lib.rs#tests::nested_row_mouse_action_claims_parent_selection_event",
        ],
    },
    UiTestMapping {
        upstream: "DiffRowView keeps wrapped text stable when showing the add-note affordance",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::section_dispatches_rows_selection_cursor_and_hover_badges",
            "crates/workdeck-tui/src/lib.rs#tests::explicit_row_plan_wraps_unicode_and_keeps_nowrap_to_one_physical_row",
        ],
    },
    UiTestMapping {
        upstream: "DiffRowView fills the reserved wrapped add-note column with row background",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::section_dispatches_rows_selection_cursor_and_hover_badges",
        ],
    },
    UiTestMapping {
        upstream: "DiffRowView keeps metadata row background within the measured row width",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::early_messages_keep_hunk_insets_copy_and_bottom_padding",
        ],
    },
    UiTestMapping {
        upstream: "DiffRowView preserves zero-width combining spans in nowrap and wrapped rows",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::explicit_row_plan_wraps_unicode_and_keeps_nowrap_to_one_physical_row",
        ],
    },
    UiTestMapping {
        upstream: "DiffRowView height matches geometry for repeated composing scalars",
        native: &[
            "crates/workdeck-tui/src/diff_section_geometry.rs#tests::wraps_long_rows_into_taller_section_geometry",
        ],
    },
    UiTestMapping {
        upstream: "DiffRowView matches planned split and stack geometry at guide and add-note wrap boundaries",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::split_row_plan_wraps_each_pane_and_preserves_the_divider_geometry",
            "crates/workdeck-tui/src/diff_section_geometry.rs#tests::measures_split_and_stack_layouts_from_the_render_plan",
        ],
    },
    UiTestMapping {
        upstream: "DiffRowView height matches geometry for composition-sensitive styled spans",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::styled_wrapping_retains_span_styles_across_boundaries",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane geometry memo depends on add-note presence instead of callback identity",
        native: &[
            "crates/workdeck-tui/src/diff_section_view.rs#tests::memo_comparator_covers_every_source_field_and_ignores_three_callbacks",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane accepts add-note hover on the first wrapped frame",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::hover_lifecycle_matches_activation_timeout_signal_blur_and_unmount",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane add-note clicks keep targeting the current hunk after navigation",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::section_dispatches_rows_selection_cursor_and_hover_badges",
            "crates/workdeck-tui/src/lib.rs#tests::nested_row_mouse_action_claims_parent_selection_event",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane first nowrap paint fills a tall viewport past the overscan neighbor",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::live_plain_split_prefetch_requests_halo_not_every_file_and_follows_eof_jump",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane scrolls a later selected file into view in the windowed path",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::next_hunk_gives_destination_file_the_review_header_after_scrolling",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane scrolls to the selected later hunk when hunk headers are hidden",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::scroll_pinned_header_handoff_keeps_the_viewport_lane_stable",
            "crates/workdeck-tui/src/diff_section_geometry.rs#tests::hidden_hunk_headers_preserve_exact_ui_lib_anchor_rows",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane aligns the current rendered line to top, center, and bottom",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::native_cursor_paint_blends_each_ratatui_surface_and_number_mode_stops_at_the_gutter",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane defers command alignment until navigation reconciles the current line",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::mounted_cursor_line_navigation_retains_removed_and_added_tints",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane keeps a cross-file line reveal against the selection reveal retry",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::cross_file_hunk_sequence_preserves_destination_header_and_backward_target",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane viewport-follow selection does not move the scroll position",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::note_after_paging_preserves_the_visible_review_anchor",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane releases viewport-follow selection after an align request that needs no scrolling",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::content_bottom_jump_remains_authoritative_after_hunk_navigation",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane keeps the sticky-header lane stable through the divider and next-header handoff",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::scroll_pinned_header_handoff_keeps_the_viewport_lane_stable",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane positions later files after expanded context rows",
        native: &[
            "crates/workdeck-tui/src/diff_section_geometry.rs#tests::expanding_leading_gap_shifts_anchor_but_not_following_hunk_bounds",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane advances the review stream under the always-pinned file header above a collapsed gap",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::scroll_first_wheel_step_and_reverse_restore_collapsed_gap_under_pinned_header",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane returns cleanly to the collapsed-gap view after scrolling back up under the pinned file header",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::arrow_step_and_reverse_restore_collapsed_gap_beneath_pinned_header",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane keeps bottom scroll stable when offscreen agent notes are windowed out",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::annotation_toggle_shows_notes_for_both_files_in_current_viewport",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane lets manual scrolling move away from a bottom-clamped file-top alignment",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::scroll_short_final_file_allows_upward_movement_after_navigation",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane keeps a viewport-sized selected hunk fully visible when it fits",
        native: &[
            "crates/workdeck-tui/src/hunk_scroll.rs#tests::fitting_hunks_remain_wholly_visible_with_as_much_padding_as_possible",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane keeps a selected wrapped hunk fully visible when it fits",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::explicit_row_plan_wraps_unicode_and_keeps_nowrap_to_one_physical_row",
            "crates/workdeck-tui/src/hunk_scroll.rs#tests::fitting_hunks_remain_wholly_visible_with_as_much_padding_as_possible",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane keeps a distant selected hunk visible when row windowing narrows one file body",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::scroll_short_final_file_allows_upward_movement_after_navigation",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane keeps a selected hunk with inline notes fully visible when it fits",
        native: &[
            "crates/workdeck-tui/src/diff_section_geometry.rs#tests::accounts_for_visible_inline_notes_without_moving_the_hunk_anchor",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane scrollToNote positions the inline note near the viewport top instead of the hunk top",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::session_comment_navigation_reveals_deep_inline_note_and_returns_hunk",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane reveals the note the shared policy names, not the first one drawn in the hunk",
        native: &[
            "crates/workdeck-review/src/semantic_selectors.rs#tests::reveal_prefers_draft_then_earliest_note_with_arrival_ties",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane reveal is not confused by an explicit note id that spells an index",
        native: &[
            "crates/workdeck-review/src/semantic_selectors.rs#tests::reveal_prefers_draft_then_earliest_note_with_arrival_ties",
        ],
    },
    UiTestMapping {
        upstream: "AgentCard removes top and bottom padding while keeping the footer inside the frame",
        native: &[
            "crates/workdeck-tui/src/agent_card_view.rs#tests::removes_outer_padding_and_keeps_the_footer_inside_the_frame",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote renders a connected bordered panel without a blank connector row",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::saved_card_is_connected_and_matches_measured_height",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote connects threads and overlays actions on the hovered bottom border",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::thread_rails_and_hovered_saved_actions_preserve_geometry",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote keeps reply composers attached to their thread rails",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::reply_draft_grows_wraps_and_keeps_thread_connectors",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote highlights Save and Cancel independently on mouse hover",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::draft_actions_are_independent_and_key_commands_match_labels",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote renders STML markup as the note body at its measured height",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::stml_replaces_plain_body_and_empty_markup_falls_back",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote falls back to the summary when markup renders to nothing",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::stml_replaces_plain_body_and_empty_markup_falls_back",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote renders draft notes as an editable composer",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::baseline_saved_and_draft_cell_frames_match_opentui_capture",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote grows draft composer for soft-wrapped text",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_parity_tests.rs#matches_the_real_editor_wrap_count_at_width_24",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote keeps the filename and range visible in compact thread titles",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::compact_titles_retain_author_path_range_age_and_special_characters",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote shows author name in title when author is set",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::compact_titles_retain_author_path_range_age_and_special_characters",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote falls back to 'Agent note' when author is absent",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::compact_titles_retain_author_path_range_age_and_special_characters",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote includes index when multiple notes share a hunk",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::compact_titles_retain_author_path_range_age_and_special_characters",
        ],
    },
    UiTestMapping {
        upstream: "AgentInlineNote preserves special characters in author",
        native: &[
            "crates/workdeck-tui/src/agent_inline_note_view.rs#tests::compact_titles_retain_author_path_range_age_and_special_characters",
        ],
    },
    UiTestMapping {
        upstream: "AgentCard shows author in title when set",
        native: &[
            "crates/workdeck-tui/src/agent_card_view.rs#tests::shows_author_in_the_title_when_set",
        ],
    },
    UiTestMapping {
        upstream: "AgentCard falls back to 'AI note' when author absent",
        native: &[
            "crates/workdeck-tui/src/agent_card_view.rs#tests::falls_back_to_ai_note_when_author_is_absent",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane renders all visible hunk notes across the review stream",
        native: &[
            "crates/workdeck-tui/src/review_render_plan.rs#tests::every_visible_note_renders_at_its_own_anchor_in_row_order",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane lets reviewers delete stored agent notes but not parents with replies",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::mouse_delete_rejects_parent_notes_without_mutation",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane split inline notes hand off directly from the anchored row without shifting it",
        native: &[
            "crates/workdeck-tui/src/review_render_plan.rs#tests::inserts_note_after_anchor_and_starts_guide_below_note",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane shows all inline notes when a hunk has multiple notes",
        native: &[
            "crates/workdeck-tui/src/review_render_plan.rs#tests::every_visible_note_renders_at_its_own_anchor_in_row_order",
        ],
    },
    UiTestMapping {
        upstream: "MenuDropdown renders checked items and key hints",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::menu_dropdown_renders_checks_hints_and_repositions_inside_a_narrow_terminal",
        ],
    },
    UiTestMapping {
        upstream: "MenuDropdown repositions wide menus to stay inside the terminal",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::menu_dropdown_renders_checks_hints_and_repositions_inside_a_narrow_terminal",
        ],
    },
    UiTestMapping {
        upstream: "StatusBar renders filter mode affordance",
        native: &[
            "crates/workdeck-tui/src/status_bar.rs#tests::input_view_matches_the_pinned_scroll_margin_and_cursor_frames",
        ],
    },
    UiTestMapping {
        upstream: "StatusBar renders a notice when no filter is active",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::status_bar_matches_notice_filter_and_mode_precedence_frames",
        ],
    },
    UiTestMapping {
        upstream: "StatusBar keeps the keyboard-mode badge visible beside notices and filter input",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::status_bar_matches_notice_filter_and_mode_precedence_frames",
        ],
    },
    UiTestMapping {
        upstream: "StatusBar mode badge uses the host exit callback and stops the outer click",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::status_bar_mouse_up_closes_an_open_application_menu",
        ],
    },
    UiTestMapping {
        upstream: "StatusBar keeps filter input precedence over a notice",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::status_bar_matches_notice_filter_and_mode_precedence_frames",
        ],
    },
    UiTestMapping {
        upstream: "StatusBar keeps filter summary precedence over a notice",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::status_bar_matches_notice_filter_and_mode_precedence_frames",
        ],
    },
    UiTestMapping {
        upstream: "HelpDialog renders every documented control row without overlap",
        native: &[
            "crates/workdeck-tui/src/help_dialog.rs#tests::renders_every_section_with_exact_modal_title_palette_and_spacing",
        ],
    },
    UiTestMapping {
        upstream: "HelpDialog shows the keys a remapped command actually answers to",
        native: &[
            "crates/workdeck-tui/src/help_dialog.rs#tests::remaps_reach_the_rows_and_small_dialogs_apply_bounded_scroll",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane renders an empty-state message when no files are visible",
        native: &[
            "crates/workdeck-tui/src/diff_section_geometry.rs#tests::returns_one_row_placeholder_for_files_without_visible_hunks",
            "crates/workdeck-tui/src/public_review/tests.rs#empty_public_surfaces_explain_absent_files_and_each_non_text_change",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane can hide line numbers while keeping diff signs visible",
        native: &[
            "crates/workdeck-tui/src/public_review/tests.rs#public_body_honors_header_number_and_selected_hunk_options",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane can wrap long diff lines onto continuation rows",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::explicit_row_plan_wraps_unicode_and_keeps_nowrap_to_one_physical_row",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane can hide hunk metadata rows without hiding code lines",
        native: &[
            "crates/workdeck-tui/src/public_review/tests.rs#public_body_honors_header_number_and_selected_hunk_options",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody renders stack-mode wrapped continuation rows",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::explicit_row_plan_wraps_unicode_and_keeps_nowrap_to_one_physical_row",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody can reveal offscreen code columns in nowrap mode",
        native: &[
            "crates/workdeck-tui/src/public_review/tests.rs#horizontal_offset_is_cell_safe_and_wrapping_deliberately_ignores_it",
        ],
    },
    UiTestMapping {
        upstream: "split view wraps the same long diff line across more rows than stack view at the same width",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::split_row_plan_wraps_each_pane_and_preserves_the_divider_geometry",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody anchors range-less notes to the first visible row when hunk headers are hidden",
        native: &[
            "crates/workdeck-tui/src/review_render_plan.rs#tests::range_less_note_uses_default_new_first_line_without_guides",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody shows contextual messages when there is no selected file or no textual hunks",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::early_messages_keep_hunk_insets_copy_and_bottom_padding",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody shows the expand chevron only when a source fetcher is attached",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::source_capability_gates_gap_toggle_and_highlight_readiness",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody hides add-note affordances on collapsed and hunk-header rows",
        native: &[
            "crates/workdeck-tui/src/diff_section_body.rs#tests::section_dispatches_rows_selection_cursor_and_hover_badges",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody toggles a collapsed gap when clicked",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::visible_gap_mouse_release_toggles_without_starting_copy_selection",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody highlights expanded unchanged source rows",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::expanded_source_rows_use_full_source_syntax_spans",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody renders word-diff spans with a visibly different background in split view",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::stack_and_split_rows_apply_inline_word_emphasis",
        ],
    },
    UiTestMapping {
        upstream: "DiffSectionBody reuses highlighted rows after unmounting and remounting a file section",
        native: &[
            "crates/workdeck-tui/src/highlighted_diff_runtime.rs#tests::retryable_and_stale_completions_never_poison_shared_cache",
        ],
    },
    UiTestMapping {
        upstream: "DiffPane prefetches highlight data for files approaching the viewport before they mount",
        native: &[
            "crates/workdeck-tui/src/highlight_prefetch.rs#tests::frozen_pins_match_prefetch_policy_including_halo_and_selection_edges",
        ],
    },
    UiTestMapping {
        upstream: "App renders the menu bar and multi-file stream",
        native: &[
            "crates/workdeck-tui/src/lib.rs#tests::desktop_menu_bar_renders_and_dispatches_through_the_shared_command_table",
            "crates/workdeck-tui/src/public_review/tests.rs#renders_reusable_file_header_and_multi_file_review_stream_primitives",
        ],
    },
];

const ORACLE_FIXTURES: &[&str] = &[
    "port/hunk/oracles/default-sidebar.json",
    "port/hunk/oracles/diff-file-header-row.json",
    "port/hunk/oracles/diff-row-view.json",
    "port/hunk/oracles/diff-section.json",
    "port/hunk/oracles/diff-section-body.json",
    "port/hunk/oracles/agent-card-view.json",
    "port/hunk/oracles/agent-inline-note-view.json",
    "port/hunk/oracles/app-menus.json",
    "port/hunk/oracles/status-bar.json",
    "port/hunk/oracles/help-dialog.json",
    "port/hunk/oracles/highlight-prefetch-policy.json",
];

fn source_test_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let marker = "test(\"";
            let start = line.find(marker)? + marker.len();
            let rest = &line[start..];
            let end = rest.find("\", ")?;
            Some(rest[..end].to_owned())
        })
        .collect()
}

fn verify_rust_anchor(
    repo: &Path,
    item: &str,
    sources: &mut HashMap<String, syn::File>,
) -> Result<()> {
    let (path, anchor) = item
        .split_once('#')
        .with_context(|| format!("UI component mapping lacks a Rust anchor: {item}"))?;
    ensure!(
        path.ends_with(".rs"),
        "UI component mapping is not Rust: {item}"
    );
    let source = if let Some(source) = sources.get(path) {
        source
    } else {
        let text = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read UI component Rust evidence {path}"))?;
        let parsed = syn::parse_file(&text)
            .with_context(|| format!("parse UI component Rust evidence {path}"))?;
        sources.entry(path.to_owned()).or_insert(parsed)
    };
    ensure!(
        crate::rust_items_have_test(&source.items, Some(anchor)),
        "UI component mapping references a missing executable Rust test: {item}"
    );
    Ok(())
}

fn verify_oracle_fixture(repo: &Path, path: &str) -> Result<()> {
    let bytes = fs::read(repo.join(path)).with_context(|| format!("read UI oracle {path}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parse UI oracle {path}"))?;
    let encoded = value.to_string();
    // Older captures record the commit under a nested run entry rather than a top-level
    // `schemaVersion`/`baseline` field; the pinned commit must still be present in either shape.
    ensure!(
        encoded.contains(BASELINE),
        "UI oracle {path} does not identify the pinned baseline"
    );
    Ok(())
}

fn verify_native_surface(repo: &Path) -> Result<()> {
    let required: BTreeMap<&str, &[&str]> = BTreeMap::from([
        (
            "crates/workdeck-tui/src/public_review.rs",
            &[
                "render_workdeck_review_stream",
                "render_workdeck_file_nav",
                "WorkdeckDiffBodyOptions",
            ] as &[&str],
        ),
        (
            "crates/workdeck-tui/src/lib.rs",
            &[
                "render_reconciled",
                "handle_review_gap_mouse",
                "handle_note_mouse",
                "move_selection",
                "scroll_to_reveal",
                "status_bar_matches_notice_filter_and_mode_precedence_frames",
            ],
        ),
        (
            "crates/workdeck-tui/src/diff_row_view.rs",
            &[
                "paint_diff_row",
                "memo_comparator_retains_referential_inputs_and_value_scalars",
            ],
        ),
        (
            "crates/workdeck-tui/src/diff_section_body.rs",
            &[
                "source_capability_gates_gap_toggle_and_highlight_readiness",
                "hover_lifecycle_matches_activation_timeout_signal_blur_and_unmount",
            ],
        ),
        (
            "crates/workdeck-tui/src/agent_card_view.rs",
            &[
                "paint_agent_card",
                "falls_back_to_ai_note_when_author_is_absent",
            ],
        ),
        (
            "crates/workdeck-tui/src/agent_inline_note_view.rs",
            &[
                "paint_agent_inline_note",
                "stml_replaces_plain_body_and_empty_markup_falls_back",
            ],
        ),
        (
            "crates/workdeck-tui/src/menu.rs",
            &[
                "MenuController",
                "controller_reanchors_selection_when_entries_change",
            ],
        ),
        (
            "crates/workdeck-tui/src/status_bar.rs",
            &[
                "status_bar_input_view",
                "filter_editing_clamps_stale_cursors_and_boundaries",
            ],
        ),
        (
            "crates/workdeck-tui/src/help_dialog.rs",
            &[
                "render_help_dialog",
                "remaps_reach_the_rows_and_small_dialogs_apply_bounded_scroll",
            ],
        ),
        (
            "crates/workdeck-tui/src/highlight_prefetch.rs",
            &[
                "RapidScrollPrefetch",
                "frozen_pins_match_prefetch_policy_including_halo_and_selection_edges",
            ],
        ),
        (
            "crates/workdeck-tui/src/review_render_plan.rs",
            &[
                "build_inline_visible_note_placements",
                "every_visible_note_renders_at_its_own_anchor_in_row_order",
            ],
        ),
        (
            "crates/workdeck-tui/src/source_controller.rs",
            &[
                "start_source_load",
                "poll_source_requests",
                "gap_toggle_starts_a_worker_and_completion_reaches_the_live_rows",
            ],
        ),
        (
            "crates/workdeck-tui/src/line_cursors.rs",
            &[
                "find_line_cursor_at",
                "stepping_crosses_hunks_and_files_and_clamps_or_recovers_at_edges",
            ],
        ),
    ]);
    for (path, markers) in required {
        let source = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read native UI surface {path}"))?;
        for marker in markers {
            ensure!(
                source.contains(marker),
                "native UI surface {path} is missing {marker:?}"
            );
        }
    }
    Ok(())
}

/// Verify the complete pinned UI component test suite and its native Ratatui translations.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "UI component verifier received unexpected baseline {baseline}"
    );
    let source_bytes =
        crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{SOURCE_PATH}")])?;
    ensure!(
        source_bytes.len() == BASELINE_BYTES,
        "pinned UI component suite changed size: {} != {BASELINE_BYTES}",
        source_bytes.len()
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source_bytes)) == BASELINE_SHA256,
        "pinned UI component suite changed SHA-256"
    );
    let source =
        std::str::from_utf8(&source_bytes).context("pinned UI component suite is not UTF-8")?;
    let names = source_test_names(source);
    ensure!(
        names.len() == 84,
        "pinned UI component suite has {} tests, expected 84",
        names.len()
    );
    ensure!(
        names.iter().all(|name| !name.is_empty()),
        "pinned UI component suite contains an empty test name"
    );
    ensure!(
        names.iter().collect::<BTreeSet<_>>().len() == names.len(),
        "pinned UI component suite repeats a test name"
    );

    let stable_bytes = crate::git_stdout_bytes(repo, ["show", &format!("{STABLE}:{SOURCE_PATH}")])?;
    ensure!(
        stable_bytes.len() == STABLE_BYTES,
        "pinned stable UI component suite changed size: {} != {STABLE_BYTES}",
        stable_bytes.len()
    );
    ensure!(
        format!("{:x}", Sha256::digest(&stable_bytes)) == STABLE_SHA256,
        "pinned stable UI component suite changed SHA-256"
    );
    let stable_names = source_test_names(
        std::str::from_utf8(&stable_bytes).context("stable UI component suite is not UTF-8")?,
    );
    ensure!(
        stable_names.len() == 76,
        "pinned stable UI component suite has {} tests, expected 76",
        stable_names.len()
    );

    ensure!(
        TEST_MAPPINGS.len() == names.len(),
        "UI component mapping has {} rows, expected {}",
        TEST_MAPPINGS.len(),
        names.len()
    );
    let mapped = TEST_MAPPINGS
        .iter()
        .map(|mapping| mapping.upstream)
        .collect::<Vec<_>>();
    ensure!(
        mapped == names.iter().map(String::as_str).collect::<Vec<_>>(),
        "UI component mapping order or names differ from the pinned suite"
    );
    let stable_set = stable_names.iter().collect::<BTreeSet<_>>();
    ensure!(
        stable_set
            .iter()
            .all(|name| mapped.contains(&name.as_str())),
        "stable UI component test is absent from the baseline mapping"
    );

    let mut sources = HashMap::new();
    for mapping in TEST_MAPPINGS {
        ensure!(
            !mapping.native.is_empty(),
            "UI component test {:?} has no native evidence",
            mapping.upstream
        );
        for item in mapping.native {
            verify_rust_anchor(repo, item, &mut sources)?;
        }
    }
    for fixture in ORACLE_FIXTURES {
        verify_oracle_fixture(repo, fixture)?;
    }
    verify_native_surface(repo)?;
    let migration = fs::read_to_string(repo.join("docs/ui-components-migration.md"))?;
    for marker in [
        SOURCE_PATH,
        "144,670",
        "84 upstream tests",
        "76 stable tests",
        "non-overlapping",
        "Ratatui",
        "frozen oracle",
        "No TypeScript source mirror",
    ] {
        ensure!(
            migration.contains(marker),
            "UI component migration is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ui_components_replaces_the_complete_pinned_test_corpus() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }
}
