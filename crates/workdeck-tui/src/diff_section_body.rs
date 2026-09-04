//! Stateful composition of one file's planned review rows.
//!
//! This is a native Ratatui reimplementation of Hunk's
//! `src/ui/diff/DiffSectionBody.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Highlight acquisition stays in
//! the native highlight runtime and inline-note painting stays in the note
//! component; this boundary owns their inputs, row planning, viewport
//! windowing, dispatch, and hover-only add-note lifecycle.

use std::collections::{HashMap, HashSet};

use workdeck_core::{DiffFile, ReviewSide};
use workdeck_diff::{
    DEFAULT_TAB_WIDTH, HighlightedDiffCode, HighlightedSourceCode, MeasuredRowBounds,
    VisibleBodyBounds, resolve_visible_row_window,
};
use workdeck_extension_api::ValidatedLineHighlight;
use workdeck_review::{ExpandedSourceStatus, LayoutMode};

use crate::{
    AppTheme, BuildDiffSectionRowPlanOptions, CodeRowLineTarget, CopySelectedRowRange,
    CursorHighlight, DEFAULT_HUNK_GAP, DiffRowInteractionIdentity, DiffRowViewOptions,
    DiffSectionGeometry, DiffSectionRowPlan, PaintedCodeCellLine, PaintedCodeCellRun,
    PaintedDiffRow, PlannedReviewRow, PlannedReviewRowLayoutOptions, PlannedRowIdentity,
    VisibleAgentNote, build_diff_section_row_plan, build_line_highlight_paint_index, diff_message,
    fit_planned_row_text, paint_diff_row, planned_review_row_visible, planned_row_matches_cursor,
    review_row_id, spans_for_highlighted_source_line,
};

pub const ADD_NOTE_IDLE_HIDE_DELAY_MS: u64 = 2_000;

/// The note insertion address represented by one visible hover affordance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveAddNoteAffordance {
    pub hunk_index: usize,
    pub target: Option<CodeRowLineTarget>,
}

/// Host-owned callback identities replacing React function references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSectionBodyInteractionIdentity {
    /// The section's one stable shared row-hover callback is always present.
    pub hover_row: u64,
    pub start_user_note: Option<u64>,
    pub toggle_gap: Option<u64>,
}

impl Default for DiffSectionBodyInteractionIdentity {
    fn default() -> Self {
        Self {
            hover_row: 1,
            start_user_note: None,
            toggle_gap: None,
        }
    }
}

/// One observable update emitted to Hunk's optional affordance callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddNoteAffordanceUpdate {
    Unchanged,
    Set(ActiveAddNoteAffordance),
    Clear,
}

/// Effect of routing one mouse-over event through the shared row handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSectionHoverEffect {
    pub notify_hover: bool,
    pub affordance: AddNoteAffordanceUpdate,
}

/// Hover-only controller state retained by the owning TUI shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffSectionBodyState {
    hovered_row_key: Option<String>,
    hover_idle_deadline_ms: Option<u64>,
    previous_hover_clear_signal: u64,
}

impl DiffSectionBodyState {
    #[must_use]
    pub fn new(hover_clear_signal: u64) -> Self {
        Self {
            previous_hover_clear_signal: hover_clear_signal,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn hovered_row_key(&self) -> Option<&str> {
        self.hovered_row_key.as_deref()
    }

    fn clear(&mut self) -> AddNoteAffordanceUpdate {
        self.hovered_row_key = None;
        self.hover_idle_deadline_ms = None;
        AddNoteAffordanceUpdate::Clear
    }

    /// Mirror the `hoverActive` and explicit-clear effects owned by Hunk's component.
    pub fn synchronize(
        &mut self,
        hover_active: bool,
        hover_clear_signal: u64,
    ) -> AddNoteAffordanceUpdate {
        let signal_changed = self.previous_hover_clear_signal != hover_clear_signal;
        self.previous_hover_clear_signal = hover_clear_signal;
        if !hover_active || signal_changed {
            self.clear()
        } else {
            AddNoteAffordanceUpdate::Unchanged
        }
    }

    /// Terminal focus left Workdeck; hide every hover-only affordance immediately.
    pub fn terminal_blur(&mut self) -> AddNoteAffordanceUpdate {
        self.clear()
    }

    /// Cancel the component-owned timer during unmount without firing callbacks.
    pub fn unmount(&mut self) {
        self.hover_idle_deadline_ms = None;
    }

    /// Route a row mouse-over through the one stable section callback.
    pub fn hover_row(
        &mut self,
        row_key: &str,
        affordances: &HashMap<String, ActiveAddNoteAffordance>,
        now_ms: u64,
        notify_hover: bool,
    ) -> DiffSectionHoverEffect {
        let affordance = affordances.get(row_key).copied();
        let update = if let Some(affordance) = affordance {
            self.hovered_row_key = Some(row_key.to_owned());
            self.hover_idle_deadline_ms = Some(now_ms.saturating_add(ADD_NOTE_IDLE_HIDE_DELAY_MS));
            AddNoteAffordanceUpdate::Set(affordance)
        } else {
            self.clear()
        };
        DiffSectionHoverEffect {
            notify_hover,
            affordance: update,
        }
    }

    /// Fire the idle timer if its deadline has elapsed.
    pub fn advance_time(&mut self, now_ms: u64) -> AddNoteAffordanceUpdate {
        if self
            .hover_idle_deadline_ms
            .is_some_and(|deadline| now_ms >= deadline)
        {
            self.clear()
        } else {
            AddNoteAffordanceUpdate::Unchanged
        }
    }
}

/// Highlight work requested by the section's two original hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSectionHighlightPolicy {
    pub should_load_diff: bool,
    pub offload_large_diff: bool,
    pub should_load_source: bool,
}

/// Complete shell-owned inputs for one native section-body render.
#[derive(Debug, Clone, Copy)]
pub struct DiffSectionBodyOptions<'a> {
    pub code_horizontal_offset: usize,
    pub copy_selected_row_ranges: Option<&'a HashMap<String, CopySelectedRowRange>>,
    pub copy_selected_side: Option<ReviewSide>,
    pub cursor_highlight: Option<&'a CursorHighlight>,
    pub expanded_gap_keys: &'a HashSet<String>,
    pub extension_line_highlights: &'a [ValidatedLineHighlight],
    pub file: Option<&'a DiffFile>,
    pub layout: LayoutMode,
    pub interactions: DiffSectionBodyInteractionIdentity,
    pub show_line_numbers: bool,
    pub show_hunk_headers: bool,
    pub source_status: ExpandedSourceStatus<'a>,
    /// Native provider capability corresponding to Hunk's optional source-fetch callback.
    pub source_fetcher_available: bool,
    pub tab_width: u16,
    pub hunk_gap: usize,
    pub wrap_lines: bool,
    pub theme: &'a AppTheme,
    pub visible_agent_notes: &'a [VisibleAgentNote],
    pub notify_hover: bool,
    pub width: usize,
    pub selected_hunk_index: usize,
    pub section_geometry: Option<&'a DiffSectionGeometry>,
    pub should_load_highlight: bool,
    pub offload_large_diff: bool,
    pub scrollable: bool,
    pub visible_body_bounds: Option<VisibleBodyBounds>,
    pub highlighted_diff: Option<&'a HighlightedDiffCode>,
    pub highlighted_source: Option<&'a HighlightedSourceCode>,
}

impl<'a> DiffSectionBodyOptions<'a> {
    #[must_use]
    pub fn new(
        file: Option<&'a DiffFile>,
        layout: LayoutMode,
        theme: &'a AppTheme,
        expanded_gap_keys: &'a HashSet<String>,
    ) -> Self {
        Self {
            code_horizontal_offset: 0,
            copy_selected_row_ranges: None,
            copy_selected_side: None,
            cursor_highlight: None,
            expanded_gap_keys,
            extension_line_highlights: &[],
            file,
            layout,
            interactions: DiffSectionBodyInteractionIdentity::default(),
            show_line_numbers: true,
            show_hunk_headers: true,
            source_status: ExpandedSourceStatus::Pending,
            source_fetcher_available: file.is_some_and(source_snapshot_available),
            tab_width: DEFAULT_TAB_WIDTH,
            hunk_gap: DEFAULT_HUNK_GAP,
            wrap_lines: false,
            theme,
            visible_agent_notes: &[],
            notify_hover: false,
            width: 80,
            selected_hunk_index: 0,
            section_geometry: None,
            should_load_highlight: true,
            offload_large_diff: false,
            scrollable: true,
            visible_body_bounds: None,
            highlighted_diff: None,
            highlighted_source: None,
        }
    }
}

/// The two early-return messages rendered with one column of horizontal inset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedDiffSectionMessage {
    pub text: String,
    pub horizontal_inset: usize,
    pub bottom_padding: usize,
    pub foreground: String,
}

/// One mounted logical row in the section's windowed body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintedDiffSectionRow {
    InlineNote {
        row_id: String,
        planned_row: Box<PlannedReviewRow>,
        clears_hover_on_mouse_over: bool,
    },
    HunkGap {
        row_id: String,
        height: usize,
        background: String,
        clears_hover_on_mouse_over: bool,
    },
    Diff {
        row_id: String,
        painted: PaintedDiffRow,
    },
}

impl PaintedDiffSectionRow {
    #[must_use]
    pub fn row_id(&self) -> &str {
        match self {
            Self::InlineNote { row_id, .. }
            | Self::HunkGap { row_id, .. }
            | Self::Diff { row_id, .. } => row_id,
        }
    }
}

/// Content selected by Hunk's early-return and normal-row branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintedDiffSectionContent {
    Message(PaintedDiffSectionMessage),
    Rows {
        top_spacer_height: usize,
        rows: Vec<PaintedDiffSectionRow>,
        bottom_spacer_height: usize,
        spacer_background: String,
    },
}

/// Immutable result consumed by the Ratatui shell and geometry/effect owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedDiffSectionBody {
    pub content: PaintedDiffSectionContent,
    pub row_plan: DiffSectionRowPlan,
    pub row_plan_highlighted: bool,
    pub highlight_policy: DiffSectionHighlightPolicy,
    pub affordances: HashMap<String, ActiveAddNoteAffordance>,
    pub scrollable: bool,
}

fn line_target(side: ReviewSide, line: usize) -> CodeRowLineTarget {
    CodeRowLineTarget { side, line }
}

/// Resolve Hunk's new-side-first note target for a split or stack code row.
#[must_use]
pub fn add_note_affordance_for_row(
    row: &workdeck_diff::DiffRow,
) -> Option<ActiveAddNoteAffordance> {
    match row {
        workdeck_diff::DiffRow::SplitLine {
            hunk_index,
            left,
            right,
            ..
        } => Some(ActiveAddNoteAffordance {
            hunk_index: *hunk_index,
            target: right
                .line_number
                .map(|line| line_target(ReviewSide::New, line))
                .or_else(|| {
                    left.line_number
                        .map(|line| line_target(ReviewSide::Old, line))
                }),
        }),
        workdeck_diff::DiffRow::StackLine {
            hunk_index, cell, ..
        } => Some(ActiveAddNoteAffordance {
            hunk_index: *hunk_index,
            target: cell
                .new_line_number
                .map(|line| line_target(ReviewSide::New, line))
                .or_else(|| {
                    cell.old_line_number
                        .map(|line| line_target(ReviewSide::Old, line))
                }),
        }),
        workdeck_diff::DiffRow::Collapsed { .. } | workdeck_diff::DiffRow::HunkHeader { .. } => {
            None
        }
    }
}

fn add_note_affordances(
    planned_rows: &[PlannedReviewRow],
) -> HashMap<String, ActiveAddNoteAffordance> {
    planned_rows
        .iter()
        .filter_map(|planned_row| {
            let row = planned_row.diff_row()?;
            Some((
                diff_row_key(row).to_owned(),
                add_note_affordance_for_row(row)?,
            ))
        })
        .collect()
}

fn diff_row_key(row: &workdeck_diff::DiffRow) -> &str {
    match row {
        workdeck_diff::DiffRow::Collapsed { key, .. }
        | workdeck_diff::DiffRow::HunkHeader { key, .. }
        | workdeck_diff::DiffRow::SplitLine { key, .. }
        | workdeck_diff::DiffRow::StackLine { key, .. } => key,
    }
}

const fn diff_row_hunk_index(row: &workdeck_diff::DiffRow) -> usize {
    match row {
        workdeck_diff::DiffRow::Collapsed { hunk_index, .. }
        | workdeck_diff::DiffRow::HunkHeader { hunk_index, .. }
        | workdeck_diff::DiffRow::SplitLine { hunk_index, .. }
        | workdeck_diff::DiffRow::StackLine { hunk_index, .. } => *hunk_index,
    }
}

fn source_snapshot_available(file: &DiffFile) -> bool {
    file.sources.old.is_some() || file.sources.new.is_some()
}

fn row_matches_cursor(row: &PlannedReviewRow, cursor: Option<&CursorHighlight>) -> bool {
    let PlannedReviewRow::DiffRow {
        stable_key,
        stable_alias_keys,
        ..
    } = row
    else {
        return false;
    };
    planned_row_matches_cursor(
        &PlannedRowIdentity {
            stable_key: stable_key.clone(),
            stable_alias_keys: stable_alias_keys.clone(),
        },
        cursor,
    )
}

fn message(
    text: &str,
    options: &DiffSectionBodyOptions<'_>,
    bottom_padding: usize,
) -> PaintedDiffSectionMessage {
    PaintedDiffSectionMessage {
        text: fit_planned_row_text(text, options.width.saturating_sub(2).max(1)),
        horizontal_inset: 1,
        bottom_padding,
        foreground: options.theme.muted.clone(),
    }
}

fn measured_row_bounds(geometry: &DiffSectionGeometry) -> Vec<MeasuredRowBounds> {
    geometry
        .row_bounds
        .iter()
        .map(|row| MeasuredRowBounds {
            key: row.key.clone(),
            top: row.bounds.top,
            height: row.bounds.height,
        })
        .collect()
}

/// Build and paint one file section's visible planned-row window.
#[must_use]
pub fn paint_diff_section_body(
    state: &DiffSectionBodyState,
    options: DiffSectionBodyOptions<'_>,
) -> PaintedDiffSectionBody {
    assert_ne!(
        options.layout,
        LayoutMode::Auto,
        "section body requires a resolved layout"
    );
    let source_text_for_highlight = match options.source_status {
        ExpandedSourceStatus::Loaded(text) if !options.expanded_gap_keys.is_empty() => Some(text),
        ExpandedSourceStatus::Pending
        | ExpandedSourceStatus::Loading
        | ExpandedSourceStatus::Loaded(_)
        | ExpandedSourceStatus::Error(_) => None,
    };
    let source_line_spans = |line: Option<&str>, source_line_number: usize| {
        spans_for_highlighted_source_line(
            line,
            options
                .highlighted_source
                .and_then(|source| source.lines.get(source_line_number))
                .and_then(Option::as_ref),
            options.theme,
            options.tab_width,
        )
    };
    let row_plan = build_diff_section_row_plan(BuildDiffSectionRowPlanOptions {
        expanded_keys: options.expanded_gap_keys,
        file: options.file,
        highlighted_diff: options.highlighted_diff,
        layout: options.layout,
        show_hunk_headers: options.show_hunk_headers,
        source_line_spans: Some(&source_line_spans),
        source_status: options.source_status,
        tab_width: options.tab_width,
        hunk_gap: options.hunk_gap,
        theme: options.theme,
        visible_agent_notes: options.visible_agent_notes,
    });
    let row_plan_highlighted = options.highlighted_diff.is_some()
        && (source_text_for_highlight.is_none() || options.highlighted_source.is_some());
    let highlight_policy = DiffSectionHighlightPolicy {
        should_load_diff: options.should_load_highlight,
        offload_large_diff: options.offload_large_diff,
        should_load_source: options.should_load_highlight && !options.expanded_gap_keys.is_empty(),
    };
    let affordances = add_note_affordances(&row_plan.planned_rows);

    let content = if let Some(file) = options.file {
        if file.hunks.is_empty() {
            PaintedDiffSectionContent::Message(message(diff_message(file), &options, 1))
        } else {
            let bounds;
            let window = if let (Some(geometry), Some(visible_bounds)) =
                (options.section_geometry, options.visible_body_bounds)
            {
                bounds = measured_row_bounds(geometry);
                resolve_visible_row_window(
                    &row_plan.planned_rows,
                    geometry.body_height,
                    &bounds,
                    visible_bounds,
                )
            } else {
                workdeck_diff::VisibleRowWindow {
                    bottom_spacer_height: 0,
                    rows: &row_plan.planned_rows,
                    top_spacer_height: 0,
                }
            };
            let line_highlights = build_line_highlight_paint_index(
                file,
                options.extension_line_highlights,
                options.tab_width,
                match options.source_status {
                    ExpandedSourceStatus::Loaded(text) => Some(text),
                    ExpandedSourceStatus::Pending
                    | ExpandedSourceStatus::Loading
                    | ExpandedSourceStatus::Error(_) => None,
                },
            );
            let gap_toggle = options
                .source_fetcher_available
                .then_some(options.interactions.toggle_gap)
                .flatten();
            let rows = window
                .rows
                .iter()
                .filter(|planned_row| {
                    planned_review_row_visible(
                        planned_row,
                        PlannedReviewRowLayoutOptions {
                            show_hunk_headers: options.show_hunk_headers,
                            layout: options.layout,
                            width: options.width,
                        },
                    )
                })
                .filter_map(|planned_row| {
                    let row_id = review_row_id(planned_row.key());
                    match planned_row {
                        PlannedReviewRow::InlineNote { .. } => {
                            Some(PaintedDiffSectionRow::InlineNote {
                                row_id,
                                planned_row: Box::new(planned_row.clone()),
                                clears_hover_on_mouse_over: true,
                            })
                        }
                        PlannedReviewRow::HunkGap { height, .. } => {
                            Some(PaintedDiffSectionRow::HunkGap {
                                row_id,
                                height: *height,
                                background: options.theme.panel.clone(),
                                clears_hover_on_mouse_over: true,
                            })
                        }
                        PlannedReviewRow::DiffRow { row, .. } => {
                            let selected = diff_row_hunk_index(row) == options.selected_hunk_index;
                            let copy_selected_row_range = options
                                .copy_selected_row_ranges
                                .and_then(|ranges| ranges.get(planned_row.key()));
                            let is_cursor_row =
                                row_matches_cursor(planned_row, options.cursor_highlight);
                            paint_diff_row(DiffRowViewOptions {
                                planned_row: Some(planned_row),
                                row: None,
                                width: options.width,
                                line_number_digits: row_plan.line_number_digits,
                                show_line_numbers: options.show_line_numbers,
                                show_hunk_headers: options.show_hunk_headers,
                                wrap_lines: options.wrap_lines,
                                code_horizontal_offset: options.code_horizontal_offset,
                                theme: options.theme,
                                selected,
                                copy_selected_row_range,
                                copy_selected_side: options.copy_selected_side,
                                cursor_highlight: is_cursor_row
                                    .then_some(options.cursor_highlight)
                                    .flatten(),
                                line_highlights: line_highlights.as_ref(),
                                anchor_id: None,
                                note_guide_side: None,
                                show_add_note_badge: options.interactions.start_user_note.is_some()
                                    && state.hovered_row_key() == Some(diff_row_key(row))
                                    && affordances.contains_key(diff_row_key(row)),
                                interactions: DiffRowInteractionIdentity {
                                    hover_row: Some(options.interactions.hover_row),
                                    start_user_note: options.interactions.start_user_note,
                                    toggle_gap: gap_toggle,
                                },
                            })
                            .map(|painted| PaintedDiffSectionRow::Diff { row_id, painted })
                        }
                    }
                })
                .collect();
            PaintedDiffSectionContent::Rows {
                top_spacer_height: window.top_spacer_height,
                rows,
                bottom_spacer_height: window.bottom_spacer_height,
                spacer_background: options.theme.panel.clone(),
            }
        }
    } else {
        PaintedDiffSectionContent::Message(message("No file selected.", &options, 0))
    };

    PaintedDiffSectionBody {
        content,
        row_plan,
        row_plan_highlighted,
        highlight_policy,
        affordances,
        scrollable: options.scrollable,
    }
}

/// Convert an early-return message to the same renderer-neutral run format as diff rows.
#[must_use]
pub fn painted_diff_section_message_line(
    message: &PaintedDiffSectionMessage,
) -> PaintedCodeCellLine {
    PaintedCodeCellLine {
        runs: vec![
            PaintedCodeCellRun {
                text: " ".repeat(message.horizontal_inset),
                foreground: None,
                background: None,
            },
            PaintedCodeCellRun {
                text: message.text.clone(),
                foreground: Some(message.foreground.clone()),
                background: None,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{FileSourceSnapshots, SourceOrigin, SourceSnapshot};
    use workdeck_diff::{
        FileComparisonOptions, FileSnapshot, StackLineCell, StackLineKind, diff_from_file_snapshots,
    };

    use crate::{DiffSectionGeometryCache, DiffSectionGeometryOptions, resolve_theme};

    fn file(before: &str, after: &str) -> DiffFile {
        let mut file = diff_from_file_snapshots(
            FileSnapshot {
                cache_key: "section-body:before",
                contents: before,
                name: "src/main.rs",
            },
            FileSnapshot {
                cache_key: "section-body:after",
                contents: after,
                name: "src/main.rs",
            },
            FileComparisonOptions { context_radius: 0 },
        )
        .unwrap();
        file.runtime_id = "runtime:main".into();
        file
    }

    fn target_row() -> workdeck_diff::DiffRow {
        workdeck_diff::DiffRow::StackLine {
            key: "line".into(),
            file_id: "file".into(),
            hunk_index: 3,
            cell: StackLineCell {
                kind: StackLineKind::Addition,
                sign: "+".into(),
                old_line_number: Some(4),
                new_line_number: Some(5),
                move_kind: None,
                spans: Vec::new(),
            },
            is_expansion_row: false,
            expanded_gap_key: None,
        }
    }

    #[test]
    fn note_targets_prefer_new_addresses_and_reject_metadata_rows() {
        assert_eq!(
            add_note_affordance_for_row(&target_row()),
            Some(ActiveAddNoteAffordance {
                hunk_index: 3,
                target: Some(CodeRowLineTarget {
                    side: ReviewSide::New,
                    line: 5,
                }),
            })
        );
        let workdeck_diff::DiffRow::StackLine { mut cell, .. } = target_row() else {
            unreachable!();
        };
        cell.new_line_number = None;
        let old_only = workdeck_diff::DiffRow::StackLine {
            key: "old".into(),
            file_id: "file".into(),
            hunk_index: 3,
            cell,
            is_expansion_row: false,
            expanded_gap_key: None,
        };
        assert_eq!(
            add_note_affordance_for_row(&old_only)
                .unwrap()
                .target
                .unwrap()
                .side,
            ReviewSide::Old
        );
        assert!(
            add_note_affordance_for_row(&workdeck_diff::DiffRow::HunkHeader {
                key: "header".into(),
                file_id: "file".into(),
                hunk_index: 0,
                text: "@@".into(),
            })
            .is_none()
        );
    }

    #[test]
    fn hover_lifecycle_matches_activation_timeout_signal_blur_and_unmount() {
        let target = ActiveAddNoteAffordance {
            hunk_index: 2,
            target: Some(CodeRowLineTarget {
                side: ReviewSide::New,
                line: 8,
            }),
        };
        let affordances = HashMap::from([("row".into(), target)]);
        let mut state = DiffSectionBodyState::new(7);
        assert_eq!(
            state.hover_row("row", &affordances, 100, true),
            DiffSectionHoverEffect {
                notify_hover: true,
                affordance: AddNoteAffordanceUpdate::Set(target),
            }
        );
        assert_eq!(state.hovered_row_key(), Some("row"));
        assert_eq!(
            state.advance_time(2_099),
            AddNoteAffordanceUpdate::Unchanged
        );
        assert_eq!(state.advance_time(2_100), AddNoteAffordanceUpdate::Clear);
        assert_eq!(state.hovered_row_key(), None);

        let _ = state.hover_row("row", &affordances, 3_000, false);
        assert_eq!(state.synchronize(true, 8), AddNoteAffordanceUpdate::Clear);
        let _ = state.hover_row("row", &affordances, 4_000, false);
        assert_eq!(state.terminal_blur(), AddNoteAffordanceUpdate::Clear);
        let _ = state.hover_row("row", &affordances, 5_000, false);
        state.unmount();
        assert_eq!(
            state.advance_time(u64::MAX),
            AddNoteAffordanceUpdate::Unchanged
        );
        assert_eq!(
            state.hover_row("metadata", &affordances, 6_000, true),
            DiffSectionHoverEffect {
                notify_hover: true,
                affordance: AddNoteAffordanceUpdate::Clear,
            }
        );
        assert_eq!(state.synchronize(false, 8), AddNoteAffordanceUpdate::Clear);
    }

    #[test]
    fn early_messages_keep_hunk_insets_copy_and_bottom_padding() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let expanded = HashSet::new();
        let mut options = DiffSectionBodyOptions::new(None, LayoutMode::Stack, &theme, &expanded);
        options.width = 12;
        let body = paint_diff_section_body(&DiffSectionBodyState::default(), options);
        let PaintedDiffSectionContent::Message(message) = body.content else {
            panic!("missing file did not render a message");
        };
        assert_eq!(message.text, "No file s…");
        assert_eq!(message.horizontal_inset, 1);
        assert_eq!(message.bottom_padding, 0);
        assert_eq!(
            painted_diff_section_message_line(&message).text(),
            " No file s…"
        );

        let empty = file("same\n", "same\n");
        let mut options =
            DiffSectionBodyOptions::new(Some(&empty), LayoutMode::Stack, &theme, &expanded);
        options.width = 80;
        let body = paint_diff_section_body(&DiffSectionBodyState::default(), options);
        let PaintedDiffSectionContent::Message(message) = body.content else {
            panic!("empty file did not render a message");
        };
        assert_eq!(message.text, "No textual hunks to render for this file.");
        assert_eq!(message.bottom_padding, 1);
    }

    #[test]
    fn section_dispatches_rows_selection_cursor_and_hover_badges() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let expanded = HashSet::new();
        let file = file("old\n", "new\n");
        let mut options =
            DiffSectionBodyOptions::new(Some(&file), LayoutMode::Stack, &theme, &expanded);
        options.width = 24;
        options.show_line_numbers = false;
        options.interactions.start_user_note = Some(9);
        let first = paint_diff_section_body(&DiffSectionBodyState::default(), options);
        let row_key = first.affordances.keys().next().unwrap().clone();
        let mut state = DiffSectionBodyState::default();
        let _ = state.hover_row(&row_key, &first.affordances, 0, false);
        let body = paint_diff_section_body(&state, options);
        let PaintedDiffSectionContent::Rows { rows, .. } = body.content else {
            panic!("diff did not render rows");
        };
        assert!(
            rows.iter()
                .all(|row| row.row_id().starts_with("review-row:"))
        );
        assert!(rows.iter().any(|row| {
            matches!(
                row,
                PaintedDiffSectionRow::Diff {
                    painted: PaintedDiffRow::Code(code),
                    ..
                } if code.row_key == row_key && code.add_note_hit.is_some()
            )
        }));
        assert!(body.affordances.contains_key(&row_key));
        assert_eq!(body.row_plan.line_number_digits, 1);
    }

    #[test]
    fn baseline_cell_oracle_matches_direct_opentui_capture() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let expanded = HashSet::new();
        let file = file("old\n", "new\n");
        let mut options =
            DiffSectionBodyOptions::new(Some(&file), LayoutMode::Stack, &theme, &expanded);
        options.width = 24;
        options.show_line_numbers = false;
        options.should_load_highlight = false;
        options.scrollable = false;
        let body = paint_diff_section_body(&DiffSectionBodyState::default(), options);
        let PaintedDiffSectionContent::Rows { rows, .. } = body.content else {
            panic!("diff did not render rows");
        };
        let captured = rows
            .iter()
            .filter_map(|row| match row {
                PaintedDiffSectionRow::Diff { painted, .. } => Some(painted.lines()),
                PaintedDiffSectionRow::InlineNote { .. }
                | PaintedDiffSectionRow::HunkGap { .. } => None,
            })
            .flatten()
            .map(|line| {
                line.runs
                    .iter()
                    .map(|run| {
                        (
                            run.text.clone(),
                            run.foreground.clone(),
                            run.background.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            captured,
            vec![
                vec![
                    ("▌".into(), Some("#878c92".into()), Some("#272b31".into())),
                    (
                        "@@ -1,1 +1,1 @@".into(),
                        Some("#adaeb1".into()),
                        Some("#272b31".into())
                    ),
                    ("        ".into(), None, Some("#272b31".into())),
                ],
                vec![
                    ("▌".into(), Some("#f85149".into()), Some("#1e2329".into())),
                    ("- ".into(), Some("#f85149".into()), Some("#3c1e21".into())),
                    (
                        "old                  ".into(),
                        Some("#e6edf3".into()),
                        Some("#3c1e21".into())
                    ),
                ],
                vec![
                    ("▌".into(), Some("#2ea043".into()), Some("#1e2329".into())),
                    ("+ ".into(), Some("#2ea043".into()), Some("#12251d".into())),
                    (
                        "new                  ".into(),
                        Some("#e6edf3".into()),
                        Some("#12251d".into())
                    ),
                ],
            ]
        );
    }

    #[test]
    fn measured_window_preserves_spacers_and_scroll_policy() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let expanded = HashSet::new();
        let file = file("one\ntwo\nthree\n", "one\nTWO\nthree\nfour\n");
        let mut cache = DiffSectionGeometryCache::default();
        let mut geometry_options =
            DiffSectionGeometryOptions::new(&file, LayoutMode::Stack, &theme);
        geometry_options.width = 30;
        let geometry = cache.measure(geometry_options);
        let mut options =
            DiffSectionBodyOptions::new(Some(&file), LayoutMode::Stack, &theme, &expanded);
        options.width = 30;
        options.section_geometry = Some(&geometry);
        options.visible_body_bounds = Some(VisibleBodyBounds { top: 1, height: 1 });
        options.scrollable = false;
        let body = paint_diff_section_body(&DiffSectionBodyState::default(), options);
        let PaintedDiffSectionContent::Rows {
            top_spacer_height,
            rows,
            bottom_spacer_height,
            ..
        } = body.content
        else {
            panic!("diff did not render rows");
        };
        assert_eq!(top_spacer_height, 1);
        assert_eq!(rows.len(), 1);
        assert!(bottom_spacer_height > 0);
        assert!(!body.scrollable);
    }

    #[test]
    fn source_capability_gates_gap_toggle_and_highlight_readiness() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut expanded = HashSet::new();
        expanded.insert("before:0".into());
        let mut file = file("zero\none\ntwo\n", "zero\nONE\ntwo\n");
        let mut unavailable_options =
            DiffSectionBodyOptions::new(Some(&file), LayoutMode::Stack, &theme, &expanded);
        unavailable_options.interactions.toggle_gap = Some(7);
        let unavailable =
            paint_diff_section_body(&DiffSectionBodyState::default(), unavailable_options);
        let PaintedDiffSectionContent::Rows {
            rows: unavailable_rows,
            ..
        } = unavailable.content
        else {
            panic!("diff did not render rows");
        };
        assert!(unavailable_rows.iter().all(|row| !matches!(
            row,
            PaintedDiffSectionRow::Diff {
                painted: PaintedDiffRow::Metadata(meta),
                ..
            } if meta.gap_toggle_hit.is_some()
        )));

        let source = "zero\none\ntwo\n";
        file.sources = FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                source.into(),
                SourceOrigin::WorkingTree,
                true,
            )),
            new: None,
        };
        let mut options =
            DiffSectionBodyOptions::new(Some(&file), LayoutMode::Stack, &theme, &expanded);
        options.source_status = ExpandedSourceStatus::Loaded(source);
        options.interactions.toggle_gap = Some(7);
        options.offload_large_diff = true;
        let body = paint_diff_section_body(&DiffSectionBodyState::default(), options);
        assert_eq!(
            body.highlight_policy,
            DiffSectionHighlightPolicy {
                should_load_diff: true,
                offload_large_diff: true,
                should_load_source: true,
            }
        );
        assert!(!body.row_plan_highlighted);
        let PaintedDiffSectionContent::Rows { rows, .. } = body.content else {
            panic!("diff did not render rows");
        };
        assert!(rows.iter().any(|row| {
            matches!(
                row,
                PaintedDiffSectionRow::Diff {
                    painted: PaintedDiffRow::Metadata(meta),
                    ..
                } if meta.gap_toggle_hit.is_some()
            )
        }));
    }
}
