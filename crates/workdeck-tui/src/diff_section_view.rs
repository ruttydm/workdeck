//! Native composition boundary for one file in the continuous review stream.
//!
//! This is the Ratatui reimplementation of Hunk's
//! `src/ui/components/panes/DiffSection.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. The parent stream retains
//! scroll ownership; this boundary owns the section id and panel, inter-file
//! separator, optional header, raw-diff versus extension-file-view route, and
//! the source component's explicit memoization contract.

use ratatui::style::Style;
use ratatui::text::Line;
use workdeck_core::ReviewSide;

use crate::{AppTheme, diff_section_id, ratatui_theme_color};

/// The two layouts accepted after the parent resolves `auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedDiffSectionLayout {
    Split,
    Stack,
}

/// Stable host callback identities replacing React function references.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffSectionInteractionIdentity {
    pub hover: u64,
    pub mouse_scroll: Option<u64>,
    pub file_view_row_failure: Option<u64>,
    pub active_add_note_affordance_change: Option<u64>,
    pub start_user_note_at_hunk: Option<u64>,
    pub row_plan_change: Option<u64>,
    pub select: u64,
    pub toggle_gap: u64,
}

/// Complete identity/value signature used by Hunk's explicit section comparator.
///
/// Values ending in `_identity` preserve JavaScript reference equality. The
/// comparator deliberately ignores `hover`, `select`, and `toggle_gap`, just as
/// the source does, while retaining the other callback identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSectionMemoInput {
    pub code_horizontal_offset: usize,
    pub expanded_gap_keys_identity: u64,
    pub extension_line_highlights_identity: Option<u64>,
    pub file_identity: u64,
    pub file_view_identity: Option<u64>,
    pub offload_large_diff: bool,
    pub header_label_width: usize,
    pub header_stats_width: usize,
    pub layout: ResolvedDiffSectionLayout,
    pub selected_hunk_index: isize,
    pub copy_selected_row_ranges_identity: Option<u64>,
    pub copy_selected_side: Option<ReviewSide>,
    pub cursor_highlight_identity: Option<u64>,
    pub should_load_highlight: bool,
    pub section_geometry_identity: Option<u64>,
    pub separator_width: usize,
    pub show_line_numbers: bool,
    pub show_hunk_headers: bool,
    pub source_status_identity: Option<u64>,
    pub tab_width: u16,
    pub hunk_gap: usize,
    pub wrap_lines: bool,
    pub show_header: bool,
    pub separator_height: usize,
    pub hover_active: bool,
    pub hover_clear_signal: u64,
    pub theme_identity: u64,
    pub visible_agent_notes_identity: u64,
    pub visible_body_bounds_identity: Option<u64>,
    pub view_width: usize,
    pub interactions: DiffSectionInteractionIdentity,
}

/// Native equivalent of the source `memo(DiffSectionComponent, comparator)`.
#[must_use]
pub fn same_diff_section_inputs(
    previous: DiffSectionMemoInput,
    next: DiffSectionMemoInput,
) -> bool {
    previous.code_horizontal_offset == next.code_horizontal_offset
        && previous.expanded_gap_keys_identity == next.expanded_gap_keys_identity
        && previous.extension_line_highlights_identity == next.extension_line_highlights_identity
        && previous.file_identity == next.file_identity
        && previous.file_view_identity == next.file_view_identity
        && previous.offload_large_diff == next.offload_large_diff
        && previous.header_label_width == next.header_label_width
        && previous.header_stats_width == next.header_stats_width
        && previous.layout == next.layout
        && previous.selected_hunk_index == next.selected_hunk_index
        && previous.copy_selected_row_ranges_identity == next.copy_selected_row_ranges_identity
        && previous.copy_selected_side == next.copy_selected_side
        && previous.cursor_highlight_identity == next.cursor_highlight_identity
        && previous.should_load_highlight == next.should_load_highlight
        && previous.section_geometry_identity == next.section_geometry_identity
        && previous.separator_width == next.separator_width
        && previous.show_line_numbers == next.show_line_numbers
        && previous.show_hunk_headers == next.show_hunk_headers
        && previous.source_status_identity == next.source_status_identity
        && previous.tab_width == next.tab_width
        && previous.hunk_gap == next.hunk_gap
        && previous.wrap_lines == next.wrap_lines
        && previous.show_header == next.show_header
        && previous.separator_height == next.separator_height
        && previous.hover_active == next.hover_active
        && previous.hover_clear_signal == next.hover_clear_signal
        && previous.interactions.mouse_scroll == next.interactions.mouse_scroll
        && previous.interactions.file_view_row_failure == next.interactions.file_view_row_failure
        && previous.interactions.active_add_note_affordance_change
            == next.interactions.active_add_note_affordance_change
        && previous.interactions.start_user_note_at_hunk
            == next.interactions.start_user_note_at_hunk
        && previous.interactions.row_plan_change == next.interactions.row_plan_change
        && previous.theme_identity == next.theme_identity
        && previous.visible_agent_notes_identity == next.visible_agent_notes_identity
        && previous.visible_body_bounds_identity == next.visible_body_bounds_identity
        && previous.view_width == next.view_width
}

/// Exact inter-file separator geometry and palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSectionSeparatorPlan {
    pub height: usize,
    pub content_width: usize,
    pub blank_rows: usize,
    pub left_padding: usize,
    pub right_padding: usize,
    pub rule: String,
    pub foreground: String,
    pub background: String,
}

impl DiffSectionSeparatorPlan {
    /// Materialize fixed-width terminal rows. The last row owns the divider;
    /// every preceding row is panel-colored vertical spacing.
    #[must_use]
    pub fn lines(&self) -> Vec<Line<'static>> {
        if self.height == 0 {
            return Vec::new();
        }
        let row_width = self
            .left_padding
            .saturating_add(self.content_width)
            .saturating_add(self.right_padding);
        let panel = Style::default().bg(ratatui_theme_color(&self.background));
        let divider = Style::default()
            .fg(ratatui_theme_color(&self.foreground))
            .bg(ratatui_theme_color(&self.background));
        let mut lines = Vec::with_capacity(self.height);
        lines.extend((0..self.blank_rows).map(|_| Line::styled(" ".repeat(row_width), panel)));
        lines.push(Line::styled(
            format!(
                "{}{}{}",
                " ".repeat(self.left_padding),
                self.rule,
                " ".repeat(self.right_padding)
            ),
            divider,
        ));
        lines
    }
}

/// The child selected by the section facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSectionBodyRoute {
    FileView {
        uses_zero_geometry_fallback: bool,
    },
    DiffBody {
        /// Always false: the continuous parent review stream owns scrolling.
        scrollable: bool,
    },
}

/// Renderer-neutral description of the source component's outer box and children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSectionViewPlan {
    pub id: String,
    pub full_width: bool,
    pub column: bool,
    pub background: String,
    pub overflow_visible: bool,
    pub separator: Option<DiffSectionSeparatorPlan>,
    pub show_header: bool,
    pub body: DiffSectionBodyRoute,
}

#[derive(Debug, Clone, Copy)]
pub struct DiffSectionViewOptions<'a> {
    pub file_id: &'a str,
    pub has_file_view: bool,
    pub has_section_geometry: bool,
    pub separator_width: usize,
    pub show_header: bool,
    pub separator_height: usize,
    pub theme: &'a AppTheme,
}

/// Compose one file section without taking terminal-scroll ownership.
#[must_use]
pub fn plan_diff_section(options: DiffSectionViewOptions<'_>) -> DiffSectionViewPlan {
    let separator = (options.separator_height > 0).then(|| DiffSectionSeparatorPlan {
        height: options.separator_height,
        content_width: options.separator_width,
        blank_rows: options.separator_height.saturating_sub(1),
        left_padding: 1,
        right_padding: 1,
        rule: "─".repeat(options.separator_width),
        foreground: options.theme.border.clone(),
        background: options.theme.panel.clone(),
    });
    let body = if options.has_file_view {
        DiffSectionBodyRoute::FileView {
            uses_zero_geometry_fallback: !options.has_section_geometry,
        }
    } else {
        DiffSectionBodyRoute::DiffBody { scrollable: false }
    };
    DiffSectionViewPlan {
        id: diff_section_id(options.file_id),
        full_width: true,
        column: true,
        background: options.theme.panel.clone(),
        overflow_visible: true,
        separator,
        show_header: options.show_header,
        body,
    }
}

/// Convenience used by both the live shell and embeddable review renderer.
#[must_use]
pub fn diff_section_separator_lines(
    separator_height: usize,
    separator_width: usize,
    theme: &AppTheme,
) -> Vec<Line<'static>> {
    plan_diff_section(DiffSectionViewOptions {
        file_id: "",
        has_file_view: false,
        has_section_geometry: false,
        separator_width,
        show_header: false,
        separator_height,
        theme,
    })
    .separator
    .map_or_else(Vec::new, |separator| separator.lines())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve_theme;

    fn memo_input() -> DiffSectionMemoInput {
        DiffSectionMemoInput {
            code_horizontal_offset: 1,
            expanded_gap_keys_identity: 2,
            extension_line_highlights_identity: Some(3),
            file_identity: 4,
            file_view_identity: Some(5),
            offload_large_diff: false,
            header_label_width: 40,
            header_stats_width: 12,
            layout: ResolvedDiffSectionLayout::Split,
            selected_hunk_index: -1,
            copy_selected_row_ranges_identity: Some(6),
            copy_selected_side: Some(ReviewSide::New),
            cursor_highlight_identity: Some(7),
            should_load_highlight: true,
            section_geometry_identity: Some(8),
            separator_width: 78,
            show_line_numbers: true,
            show_hunk_headers: true,
            source_status_identity: Some(9),
            tab_width: 4,
            hunk_gap: 1,
            wrap_lines: false,
            show_header: true,
            separator_height: 2,
            hover_active: true,
            hover_clear_signal: 10,
            theme_identity: 11,
            visible_agent_notes_identity: 12,
            visible_body_bounds_identity: Some(13),
            view_width: 80,
            interactions: DiffSectionInteractionIdentity {
                hover: 14,
                mouse_scroll: Some(15),
                file_view_row_failure: Some(16),
                active_add_note_affordance_change: Some(17),
                start_user_note_at_hunk: Some(18),
                row_plan_change: Some(19),
                select: 20,
                toggle_gap: 21,
            },
        }
    }

    #[test]
    fn plans_exact_outer_box_header_and_body_routes() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let raw = plan_diff_section(DiffSectionViewOptions {
            file_id: "file:1",
            has_file_view: false,
            has_section_geometry: true,
            separator_width: 8,
            show_header: true,
            separator_height: 0,
            theme: &theme,
        });
        assert_eq!(raw.id, "diff-section:file:1");
        assert!(raw.full_width && raw.column && raw.overflow_visible);
        assert_eq!(raw.background, theme.panel);
        assert!(raw.separator.is_none());
        assert!(raw.show_header);
        assert_eq!(
            raw.body,
            DiffSectionBodyRoute::DiffBody { scrollable: false }
        );

        let file_view = plan_diff_section(DiffSectionViewOptions {
            has_file_view: true,
            has_section_geometry: false,
            show_header: false,
            ..DiffSectionViewOptions {
                file_id: "file:1",
                has_file_view: false,
                has_section_geometry: true,
                separator_width: 8,
                show_header: true,
                separator_height: 0,
                theme: &theme,
            }
        });
        assert!(!file_view.show_header);
        assert_eq!(
            file_view.body,
            DiffSectionBodyRoute::FileView {
                uses_zero_geometry_fallback: true
            }
        );
    }

    #[test]
    fn numeric_separator_height_places_the_rule_on_the_last_padded_row() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let plan = plan_diff_section(DiffSectionViewOptions {
            file_id: "file:2",
            has_file_view: false,
            has_section_geometry: false,
            separator_width: 4,
            show_header: true,
            separator_height: 3,
            theme: &theme,
        });
        let separator = plan.separator.expect("separator");
        assert_eq!(separator.height, 3);
        assert_eq!(separator.blank_rows, 2);
        assert_eq!(separator.rule, "────");
        let lines = separator.lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].to_string(), "      ");
        assert_eq!(lines[1].to_string(), "      ");
        assert_eq!(lines[2].to_string(), " ──── ");
        assert_eq!(lines[2].style.fg, Some(ratatui_theme_color(&theme.border)));
        assert_eq!(lines[2].style.bg, Some(ratatui_theme_color(&theme.panel)));
    }

    #[test]
    fn memo_comparator_covers_every_source_field_and_ignores_three_callbacks() {
        let previous = memo_input();
        assert!(same_diff_section_inputs(previous, previous));

        macro_rules! changed {
            ($field:ident, $value:expr) => {{
                let mut next = previous;
                next.$field = $value;
                assert!(
                    !same_diff_section_inputs(previous, next),
                    "{} must invalidate",
                    stringify!($field)
                );
            }};
        }
        changed!(code_horizontal_offset, 2);
        changed!(expanded_gap_keys_identity, 22);
        changed!(extension_line_highlights_identity, Some(23));
        changed!(file_identity, 24);
        changed!(file_view_identity, Some(25));
        changed!(offload_large_diff, true);
        changed!(header_label_width, 41);
        changed!(header_stats_width, 13);
        changed!(layout, ResolvedDiffSectionLayout::Stack);
        changed!(selected_hunk_index, 0);
        changed!(copy_selected_row_ranges_identity, Some(26));
        changed!(copy_selected_side, Some(ReviewSide::Old));
        changed!(cursor_highlight_identity, Some(27));
        changed!(should_load_highlight, false);
        changed!(section_geometry_identity, Some(28));
        changed!(separator_width, 79);
        changed!(show_line_numbers, false);
        changed!(show_hunk_headers, false);
        changed!(source_status_identity, Some(29));
        changed!(tab_width, 8);
        changed!(hunk_gap, 2);
        changed!(wrap_lines, true);
        changed!(show_header, false);
        changed!(separator_height, 3);
        changed!(hover_active, false);
        changed!(hover_clear_signal, 30);
        changed!(theme_identity, 31);
        changed!(visible_agent_notes_identity, 32);
        changed!(visible_body_bounds_identity, Some(33));
        changed!(view_width, 81);

        for mutate in [
            |input: &mut DiffSectionInteractionIdentity| input.mouse_scroll = Some(34),
            |input: &mut DiffSectionInteractionIdentity| {
                input.file_view_row_failure = Some(35);
            },
            |input: &mut DiffSectionInteractionIdentity| {
                input.active_add_note_affordance_change = Some(36);
            },
            |input: &mut DiffSectionInteractionIdentity| {
                input.start_user_note_at_hunk = Some(37);
            },
            |input: &mut DiffSectionInteractionIdentity| input.row_plan_change = Some(38),
        ] {
            let mut next = previous;
            mutate(&mut next.interactions);
            assert!(!same_diff_section_inputs(previous, next));
        }

        let mut ignored = previous;
        ignored.interactions.hover = 40;
        ignored.interactions.select = 41;
        ignored.interactions.toggle_gap = 42;
        assert!(same_diff_section_inputs(previous, ignored));
    }
}
