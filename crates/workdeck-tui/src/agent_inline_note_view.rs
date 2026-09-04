//! Native inline review-note card painting and interaction geometry.
//!
//! This is a Ratatui reimplementation of Hunk's
//! `src/ui/components/panes/AgentInlineNote.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. OpenTUI editor and mouse
//! callbacks become explicit shell-owned draft events and action hit records;
//! the cell geometry, titles, thread rails, STML styles, and hover overlays
//! remain deterministic renderer output.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::DateTime;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use workdeck_core::{AgentAnnotation, DiffFile, ReviewSide};
use workdeck_diff::sanitize_terminal_line;
use workdeck_review::LayoutMode;

use crate::agent_note_geometry::draft_editor_visual_lines;
use crate::{
    AppTheme, VisibleAgentNoteActions, VisibleAgentNoteThread, agent_note_box_layout,
    annotation_range_label, draft_visual_line_count, file_label, fit_text, inline_note_title,
    measure_text_width, pad_text, ratatui_theme_color, slice_text_by_width, wrap_text,
};

/// One semantic control rendered into a note border.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentInlineNoteAction {
    Reply,
    Edit,
    Delete,
    Save,
    Cancel,
    Close,
}

/// Host-routed hit area for one declarative action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentInlineNoteActionHit {
    pub action: AgentInlineNoteAction,
    pub row: usize,
    pub column_start: usize,
    pub width: usize,
}

/// OpenTUI text attributes retained for a final Ratatui span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedAgentInlineNoteRun {
    pub text: String,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
    pub strike: bool,
    pub action: Option<AgentInlineNoteAction>,
}

impl PaintedAgentInlineNoteRun {
    fn plain(
        text: impl Into<String>,
        foreground: Option<String>,
        background: Option<String>,
    ) -> Self {
        Self {
            text: text.into(),
            foreground,
            background,
            bold: false,
            italic: false,
            underline: false,
            dim: false,
            strike: false,
            action: None,
        }
    }

    #[must_use]
    pub fn ratatui_span(&self) -> Span<'static> {
        let mut style = Style::default();
        if let Some(foreground) = self.foreground.as_deref() {
            style = style.fg(ratatui_theme_color(foreground));
        }
        if let Some(background) = self.background.as_deref() {
            style = style.bg(ratatui_theme_color(background));
        }
        for (enabled, modifier) in [
            (self.bold, Modifier::BOLD),
            (self.dim, Modifier::DIM),
            (self.italic, Modifier::ITALIC),
            (self.underline, Modifier::UNDERLINED),
            (self.strike, Modifier::CROSSED_OUT),
        ] {
            if enabled {
                style = style.add_modifier(modifier);
            }
        }
        Span::styled(self.text.clone(), style)
    }
}

/// One complete terminal row in an inline note card.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaintedAgentInlineNoteLine {
    pub runs: Vec<PaintedAgentInlineNoteRun>,
}

impl PaintedAgentInlineNoteLine {
    #[must_use]
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[must_use]
    pub fn width(&self) -> usize {
        measure_text_width(&self.text())
    }

    #[must_use]
    pub fn ratatui_line(&self) -> Line<'static> {
        Line::from(
            self.runs
                .iter()
                .map(PaintedAgentInlineNoteRun::ratatui_span)
                .collect::<Vec<_>>(),
        )
    }
}

/// Current text-area facts owned by the note composer controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentInlineNoteDraft<'a> {
    pub body: &'a str,
    pub focused: bool,
    pub notify_focus: bool,
    pub notify_blur: bool,
}

/// Complete paint inputs for one saved note or draft composer.
#[derive(Debug, Clone, Copy)]
pub struct AgentInlineNoteViewOptions<'a> {
    pub annotation: &'a AgentAnnotation,
    pub anchor_side: Option<ReviewSide>,
    pub file: Option<&'a DiffFile>,
    pub layout: LayoutMode,
    pub note_count: usize,
    pub note_index: usize,
    pub draft: Option<AgentInlineNoteDraft<'a>>,
    pub actions: Option<VisibleAgentNoteActions>,
    pub legacy_close: bool,
    pub thread: Option<&'a VisibleAgentNoteThread>,
    pub thread_depth: Option<usize>,
    pub theme: &'a AppTheme,
    pub width: usize,
    pub now_ms: i64,
}

impl<'a> AgentInlineNoteViewOptions<'a> {
    #[must_use]
    pub fn new(
        annotation: &'a AgentAnnotation,
        layout: LayoutMode,
        theme: &'a AppTheme,
        width: usize,
    ) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_millis()).ok())
            .unwrap_or(i64::MAX);
        Self {
            annotation,
            anchor_side: None,
            file: None,
            layout,
            note_count: 1,
            note_index: 0,
            draft: None,
            actions: None,
            legacy_close: false,
            thread: None,
            thread_depth: None,
            theme,
            width,
            now_ms,
        }
    }
}

/// Mouse-only saved-action state retained by the TUI shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentInlineNoteViewState {
    actions_hovered: bool,
    hovered_action: Option<AgentInlineNoteAction>,
}

impl AgentInlineNoteViewState {
    #[must_use]
    pub const fn actions_hovered(&self) -> bool {
        self.actions_hovered
    }

    #[must_use]
    pub const fn hovered_action(&self) -> Option<AgentInlineNoteAction> {
        self.hovered_action
    }

    /// Mirror the dependency-reset effect when draft/action capabilities change.
    pub fn synchronize(&mut self, is_draft: bool, has_saved_actions: bool) {
        if is_draft || !has_saved_actions {
            self.actions_hovered = false;
        }
        self.hovered_action = None;
    }

    pub fn enter_card(&mut self) {
        self.actions_hovered = true;
    }

    pub fn leave_card(&mut self) {
        self.actions_hovered = false;
        self.hovered_action = None;
    }

    pub fn hover_action(&mut self, action: Option<AgentInlineNoteAction>) {
        self.hovered_action = action;
    }
}

/// One complete card plus host-owned editor and action metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedAgentInlineNote {
    pub lines: Vec<PaintedAgentInlineNoteLine>,
    pub action_hits: Vec<AgentInlineNoteActionHit>,
    pub box_width: usize,
    pub box_left: usize,
    pub content_width: usize,
    pub draft_rows: usize,
    pub draft_focused: bool,
    pub draft_notifies_focus: bool,
    pub draft_notifies_blur: bool,
}

impl PaintedAgentInlineNote {
    #[must_use]
    pub fn action_at(&self, row: usize, column: usize) -> Option<AgentInlineNoteAction> {
        self.action_hits
            .iter()
            .find(|hit| {
                hit.row == row
                    && (hit.column_start..hit.column_start.saturating_add(hit.width))
                        .contains(&column)
            })
            .map(|hit| hit.action)
    }

    #[must_use]
    pub fn ratatui_lines(&self) -> Vec<Line<'static>> {
        self.lines
            .iter()
            .map(PaintedAgentInlineNoteLine::ratatui_line)
            .collect()
    }
}

/// Composer command routed from the native keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentInlineNoteDraftCommand {
    Newline,
    Save,
    Cancel,
}

/// Preserve the component's explicit Ctrl-J newline binding and visible save/cancel keys.
#[must_use]
pub fn agent_inline_note_draft_command(
    key: &str,
    control: bool,
) -> Option<AgentInlineNoteDraftCommand> {
    match (key, control) {
        ("j", true) => Some(AgentInlineNoteDraftCommand::Newline),
        ("s", true) => Some(AgentInlineNoteDraftCommand::Save),
        ("escape", false) => Some(AgentInlineNoteDraftCommand::Cancel),
        _ => None,
    }
}

/// Lay out optional STML only for saved notes, falling back on empty output.
#[must_use]
pub fn agent_inline_note_markup_lines(
    annotation: &AgentAnnotation,
    content_width: usize,
) -> Option<Vec<workdeck_markup::StmlLine>> {
    annotation
        .markup
        .as_deref()
        .filter(|_| annotation.source.as_deref() != Some("user-draft"))
        .map(|markup| workdeck_markup::layout_stml_cached(markup, content_width))
        .filter(|layout| !layout.lines.is_empty())
        .map(|layout| layout.lines.clone())
}

fn wrap_note_text(text: &str, width: usize) -> Vec<String> {
    text.split('\n')
        .flat_map(|line| wrap_text(&sanitize_terminal_line(line), width))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteBodyKind {
    Summary,
    Rationale,
}

fn plain_body_lines(annotation: &AgentAnnotation, width: usize) -> Vec<(NoteBodyKind, String)> {
    let mut lines = wrap_note_text(&annotation.summary, width)
        .into_iter()
        .map(|line| (NoteBodyKind::Summary, line))
        .collect::<Vec<_>>();
    if let Some(rationale) = annotation.rationale.as_deref() {
        lines.extend(
            wrap_note_text(rationale, width)
                .into_iter()
                .map(|line| (NoteBodyKind::Rationale, line)),
        );
    }
    lines
}

/// Compact age suffix used by semantic thread titles.
#[must_use]
pub fn short_review_note_age(created_at: Option<&str>, now_ms: i64) -> String {
    let Some(timestamp) = created_at
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
    else {
        return String::new();
    };
    let elapsed_minutes = now_ms.saturating_sub(timestamp).max(0) / 60_000;
    if elapsed_minutes < 1 {
        return "now".into();
    }
    if elapsed_minutes < 60 {
        return format!("{elapsed_minutes}m");
    }
    let elapsed_hours = elapsed_minutes / 60;
    if elapsed_hours < 24 {
        return format!("{elapsed_hours}h");
    }
    let elapsed_days = elapsed_hours / 24;
    if elapsed_days < 7 {
        return format!("{elapsed_days}d");
    }
    let elapsed_weeks = elapsed_days / 7;
    if elapsed_weeks < 52 {
        return format!("{elapsed_weeks}w");
    }
    format!("{}y", elapsed_weeks / 52)
}

fn fit_trailing_text(text: &str, width: usize, overflow_marker: &str) -> String {
    let measured_width = measure_text_width(text);
    if measured_width <= width {
        return text.to_owned();
    }
    let marker = fit_text(overflow_marker, width, Some(""));
    let marker_width = measure_text_width(&marker);
    let tail_width = width.saturating_sub(marker_width);
    format!(
        "{marker}{}",
        slice_text_by_width(text, measured_width.saturating_sub(tail_width), tail_width).text
    )
}

fn threaded_title(annotation: &AgentAnnotation, now_ms: i64) -> String {
    let author = sanitize_terminal_line(annotation.author.as_deref().unwrap_or_default().trim());
    let label = if annotation.source.as_deref() == Some("user") {
        "Your note".into()
    } else if author.is_empty() {
        "Agent note".into()
    } else {
        author
    };
    let age = short_review_note_age(annotation.created_at.as_deref(), now_ms);
    if age.is_empty() {
        label
    } else {
        format!("{label} · {age}")
    }
}

fn compact_thread_title(
    annotation: &AgentAnnotation,
    file: &DiffFile,
    saved_title_budget: usize,
    now_ms: i64,
) -> String {
    let author = threaded_title(annotation, now_ms);
    let path = file_label(file);
    let range = annotation_range_label(annotation, None);
    let content_budget = saved_title_budget.saturating_sub(6);
    let range_budget = measure_text_width(&range).min(3.max(content_budget * 40 / 100));
    let remaining_before_range = content_budget.saturating_sub(range_budget);
    let path_budget = measure_text_width(&path).min(6.max(remaining_before_range * 55 / 100));
    let author_budget = remaining_before_range.saturating_sub(path_budget);
    format!(
        " {} · {} {} ",
        fit_text(&author, author_budget, Some("…")),
        fit_trailing_text(&path, path_budget, "..."),
        fit_text(&range, range_budget, Some("…")),
    )
}

fn push_run(line: &mut PaintedAgentInlineNoteLine, run: PaintedAgentInlineNoteRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(previous) = line.runs.last_mut().filter(|previous| {
        previous.foreground == run.foreground
            && previous.background == run.background
            && previous.bold == run.bold
            && previous.italic == run.italic
            && previous.underline == run.underline
            && previous.dim == run.dim
            && previous.strike == run.strike
            && previous.action == run.action
    }) {
        previous.text.push_str(&run.text);
    } else {
        line.runs.push(run);
    }
}

fn push_plain(
    line: &mut PaintedAgentInlineNoteLine,
    text: impl Into<String>,
    foreground: Option<&str>,
    background: Option<&str>,
) {
    push_run(
        line,
        PaintedAgentInlineNoteRun::plain(
            text,
            foreground.map(str::to_owned),
            background.map(str::to_owned),
        ),
    );
}

fn finish_line(line: &mut PaintedAgentInlineNoteLine, width: usize, theme: &AppTheme) {
    let padding = width.saturating_sub(line.width());
    push_plain(line, " ".repeat(padding), None, Some(&theme.panel));
}

struct ThreadGutter<'a> {
    thread: Option<&'a VisibleAgentNoteThread>,
    visual_depth: usize,
    box_left: usize,
}

impl ThreadGutter<'_> {
    fn text(&self, top: bool) -> String {
        let Some(thread) = self.thread.filter(|_| self.visual_depth > 0) else {
            return " ".repeat(self.box_left);
        };
        let connector_width = self.visual_depth * 2;
        let connector_left = self.box_left.saturating_sub(connector_width);
        let displayed = if self.visual_depth > 1 {
            let count = self.visual_depth - 1;
            let start = thread.ancestor_has_next_sibling.len().saturating_sub(count);
            &thread.ancestor_has_next_sibling[start..]
        } else {
            &[]
        };
        let mut text = " ".repeat(connector_left);
        for depth in 0..self.visual_depth {
            let segment = if depth < self.visual_depth - 1 {
                if displayed.get(depth).copied().unwrap_or(false) {
                    "│ "
                } else {
                    "  "
                }
            } else if top {
                if thread.has_next_sibling.unwrap_or(false) {
                    "├─"
                } else {
                    "╰─"
                }
            } else if thread.has_next_sibling.unwrap_or(false) {
                "│ "
            } else {
                "  "
            };
            text.push_str(segment);
        }
        text
    }
}

fn push_thread_gutter(
    line: &mut PaintedAgentInlineNoteLine,
    gutter: &ThreadGutter<'_>,
    top: bool,
    theme: &AppTheme,
) {
    push_plain(
        line,
        gutter.text(top),
        Some(&theme.muted),
        Some(&theme.panel),
    );
}

fn body_line(
    gutter: &ThreadGutter<'_>,
    content: Vec<PaintedAgentInlineNoteRun>,
    content_width: usize,
    width: usize,
    theme: &AppTheme,
) -> PaintedAgentInlineNoteLine {
    let mut line = PaintedAgentInlineNoteLine::default();
    push_thread_gutter(&mut line, gutter, false, theme);
    push_plain(&mut line, "│", Some(&theme.note_border), Some(&theme.panel));
    push_plain(&mut line, " ", None, Some(&theme.panel));
    let used = content
        .iter()
        .map(|run| measure_text_width(&run.text))
        .sum::<usize>();
    for run in content {
        push_run(&mut line, run);
    }
    push_plain(
        &mut line,
        " ".repeat(content_width.saturating_sub(used)),
        None,
        Some(&theme.panel),
    );
    push_plain(&mut line, " ", None, Some(&theme.panel));
    push_plain(&mut line, "│", Some(&theme.note_border), Some(&theme.panel));
    finish_line(&mut line, width, theme);
    line
}

#[derive(Debug, Clone, Copy)]
struct ActionItem {
    action: AgentInlineNoteAction,
    key_label: &'static str,
    label: &'static str,
}

fn saved_action_items(actions: Option<VisibleAgentNoteActions>) -> Vec<ActionItem> {
    let Some(actions) = actions else {
        return Vec::new();
    };
    [
        actions.reply.then_some(ActionItem {
            action: AgentInlineNoteAction::Reply,
            key_label: "r",
            label: "reply",
        }),
        actions.edit.then_some(ActionItem {
            action: AgentInlineNoteAction::Edit,
            key_label: "e",
            label: "edit",
        }),
        actions.delete.then_some(ActionItem {
            action: AgentInlineNoteAction::Delete,
            key_label: "d",
            label: "delete",
        }),
    ]
    .into_iter()
    .flatten()
    .collect()
}

struct BottomBorderOptions<'a> {
    gutter: &'a ThreadGutter<'a>,
    items: &'a [ActionItem],
    box_width: usize,
    width: usize,
    theme: &'a AppTheme,
    hovered_action: Option<AgentInlineNoteAction>,
    row: usize,
}

fn render_bottom_border(
    options: BottomBorderOptions<'_>,
) -> (PaintedAgentInlineNoteLine, Vec<AgentInlineNoteActionHit>) {
    let available_items_width = options.box_width.saturating_sub(4);
    let full_items_width = options
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| item.key_label.len() + 1 + item.label.len() + usize::from(index > 0))
        .sum::<usize>();
    let show_labels = full_items_width <= available_items_width;
    let item_widths = options
        .items
        .iter()
        .map(|item| item.key_label.len() + usize::from(show_labels) * (1 + item.label.len()))
        .collect::<Vec<_>>();
    let items_width = item_widths.iter().sum::<usize>() + options.items.len().saturating_sub(1);
    let inner_width = options.box_width.saturating_sub(2);
    let leading_width = inner_width.saturating_sub(items_width.saturating_add(2));
    let mut line = PaintedAgentInlineNoteLine::default();
    let mut hits = Vec::with_capacity(options.items.len());
    push_thread_gutter(&mut line, options.gutter, false, options.theme);
    push_plain(
        &mut line,
        format!("╰{} ", "─".repeat(leading_width)),
        Some(&options.theme.note_border),
        Some(&options.theme.panel),
    );
    for (index, (item, item_width)) in options.items.iter().zip(item_widths).enumerate() {
        if index > 0 {
            push_plain(&mut line, " ", None, Some(&options.theme.panel));
        }
        let column_start = line.width();
        let hovered = options.hovered_action == Some(item.action);
        let background = if hovered {
            &options.theme.accent_muted
        } else {
            &options.theme.panel
        };
        let mut key_run = PaintedAgentInlineNoteRun::plain(
            item.key_label,
            Some(options.theme.note_title_text.clone()),
            Some(background.clone()),
        );
        key_run.action = Some(item.action);
        push_run(&mut line, key_run);
        if show_labels {
            let mut label_run = PaintedAgentInlineNoteRun::plain(
                format!(" {}", item.label),
                Some(if hovered {
                    options.theme.text.clone()
                } else {
                    options.theme.muted.clone()
                }),
                Some(background.clone()),
            );
            label_run.action = Some(item.action);
            push_run(&mut line, label_run);
        }
        hits.push(AgentInlineNoteActionHit {
            action: item.action,
            row: options.row,
            column_start,
            width: item_width,
        });
    }
    push_plain(
        &mut line,
        " ╯",
        Some(&options.theme.note_border),
        Some(&options.theme.panel),
    );
    finish_line(&mut line, options.width, options.theme);
    (line, hits)
}

fn draft_text_lines(body: &str, width: usize) -> Vec<String> {
    draft_editor_visual_lines(body, width)
}

fn stml_theme(theme: &AppTheme) -> workdeck_markup::StmlThemeColors {
    workdeck_markup::StmlThemeColors {
        accent: theme.accent.clone(),
        accent_muted: theme.accent_muted.clone(),
        added_sign_color: theme.added_sign_color.clone(),
        removed_sign_color: theme.removed_sign_color.clone(),
        file_modified: theme.file_modified.clone(),
        muted: theme.muted.clone(),
        panel_alt: theme.panel_alt.clone(),
        text: theme.text.clone(),
        panel: theme.panel.clone(),
        note_border: theme.note_border.clone(),
        background: theme.background.clone(),
    }
}

/// Paint one saved inline note or editable draft composer.
#[must_use]
pub fn paint_agent_inline_note(
    state: &AgentInlineNoteViewState,
    options: AgentInlineNoteViewOptions<'_>,
) -> PaintedAgentInlineNote {
    assert_ne!(
        options.layout,
        LayoutMode::Auto,
        "inline note requires a resolved layout"
    );
    let thread_depth = options
        .thread_depth
        .or_else(|| options.thread.map(|thread| thread.depth))
        .unwrap_or(0);
    let geometry = agent_note_box_layout(
        options.anchor_side,
        options.layout,
        options.width,
        thread_depth,
    );
    let gutter = ThreadGutter {
        thread: options.thread,
        visual_depth: thread_depth.min(3),
        box_left: geometry.box_left,
    };
    let location_title = format!(
        "{} - {}",
        inline_note_title(options.annotation, options.note_index, options.note_count),
        annotation_range_label(options.annotation, options.file)
    );
    let mut lines = Vec::new();
    let mut hits = Vec::new();

    if let Some(draft) = options.draft {
        let draft_inner_width = geometry.box_width.saturating_sub(2).max(1);
        let draft_content_width = draft_inner_width.saturating_sub(2).max(1);
        let draft_rows = draft_visual_line_count(draft.body, draft_content_width);
        let title = fit_text(
            &format!(" {location_title} "),
            geometry.box_width.saturating_sub(4),
            Some("…"),
        );
        let suffix = format!(
            "{}╮",
            "─".repeat(
                geometry
                    .box_width
                    .saturating_sub(3 + title.encode_utf16().count())
            )
        );
        let mut top = PaintedAgentInlineNoteLine::default();
        push_thread_gutter(&mut top, &gutter, true, options.theme);
        push_plain(
            &mut top,
            "╭─",
            Some(&options.theme.note_border),
            Some(&options.theme.panel),
        );
        push_plain(
            &mut top,
            title,
            Some(&options.theme.note_title_text),
            Some(&options.theme.panel),
        );
        push_plain(
            &mut top,
            suffix,
            Some(&options.theme.note_border),
            Some(&options.theme.panel),
        );
        finish_line(&mut top, options.width, options.theme);
        lines.push(top);
        lines.push(body_line(
            &gutter,
            Vec::new(),
            draft_content_width,
            options.width,
            options.theme,
        ));

        let mut draft_lines = draft_text_lines(draft.body, draft_content_width);
        draft_lines.resize(draft_rows, String::new());
        draft_lines.truncate(draft_rows);
        for (index, text) in draft_lines.into_iter().enumerate() {
            let placeholder = draft.body.is_empty() && index == 0;
            let text = if placeholder {
                fit_text("Write a note…", draft_content_width, Some("…"))
            } else {
                text
            };
            lines.push(body_line(
                &gutter,
                vec![PaintedAgentInlineNoteRun::plain(
                    text,
                    Some(if placeholder {
                        options.theme.muted.clone()
                    } else {
                        options.theme.text.clone()
                    }),
                    Some(options.theme.panel.clone()),
                )],
                draft_content_width,
                options.width,
                options.theme,
            ));
        }
        let action_items = [
            ActionItem {
                action: AgentInlineNoteAction::Save,
                key_label: "^S",
                label: "save",
            },
            ActionItem {
                action: AgentInlineNoteAction::Cancel,
                key_label: "Esc",
                label: "cancel",
            },
        ];
        let (bottom, bottom_hits) = render_bottom_border(BottomBorderOptions {
            gutter: &gutter,
            items: &action_items,
            box_width: geometry.box_width,
            width: options.width,
            theme: options.theme,
            hovered_action: state.hovered_action,
            row: draft_rows + 2,
        });
        lines.push(bottom);
        hits.extend(bottom_hits);
        return PaintedAgentInlineNote {
            lines,
            action_hits: hits,
            box_width: geometry.box_width,
            box_left: geometry.box_left,
            content_width: draft_content_width,
            draft_rows,
            draft_focused: draft.focused,
            draft_notifies_focus: draft.notify_focus,
            draft_notifies_blur: draft.notify_blur,
        };
    }

    let close_width = usize::from(options.legacy_close) * 3;
    let close_gap_width = usize::from(options.legacy_close);
    let saved_title_budget = geometry
        .box_width
        .saturating_sub(4 + close_gap_width + close_width);
    let title_text = if let (Some(_), Some(file)) = (options.thread, options.file) {
        compact_thread_title(options.annotation, file, saved_title_budget, options.now_ms)
    } else {
        let title = if options.thread.is_some() {
            format!(
                "{} · {}",
                threaded_title(options.annotation, options.now_ms),
                annotation_range_label(options.annotation, options.file)
            )
        } else {
            location_title
        };
        fit_text(&format!(" {title} "), saved_title_budget, Some("…"))
    };
    let title_width = measure_text_width(&title_text);
    let suffix_width = geometry
        .box_width
        .saturating_sub(3 + title_width + close_gap_width + close_width);
    let mut top = PaintedAgentInlineNoteLine::default();
    push_thread_gutter(&mut top, &gutter, true, options.theme);
    push_plain(
        &mut top,
        "╭─",
        Some(&options.theme.note_border),
        Some(&options.theme.panel),
    );
    push_plain(
        &mut top,
        title_text,
        Some(&options.theme.note_title_text),
        Some(&options.theme.panel),
    );
    push_plain(
        &mut top,
        "─".repeat(suffix_width),
        Some(&options.theme.note_border),
        Some(&options.theme.panel),
    );
    if options.legacy_close {
        push_plain(&mut top, " ", None, Some(&options.theme.panel));
        let column_start = top.width();
        let mut close = PaintedAgentInlineNoteRun::plain(
            "[x]",
            Some(options.theme.note_title_text.clone()),
            Some(options.theme.panel.clone()),
        );
        close.action = Some(AgentInlineNoteAction::Close);
        push_run(&mut top, close);
        hits.push(AgentInlineNoteActionHit {
            action: AgentInlineNoteAction::Close,
            row: 0,
            column_start,
            width: 3,
        });
    }
    push_plain(
        &mut top,
        "╮",
        Some(&options.theme.note_border),
        Some(&options.theme.panel),
    );
    finish_line(&mut top, options.width, options.theme);
    lines.push(top);
    lines.push(body_line(
        &gutter,
        vec![PaintedAgentInlineNoteRun::plain(
            " ".repeat(geometry.content_width),
            Some(options.theme.text.clone()),
            Some(options.theme.panel.clone()),
        )],
        geometry.content_width,
        options.width,
        options.theme,
    ));

    if let Some(markup) = agent_inline_note_markup_lines(options.annotation, geometry.content_width)
    {
        let colors = stml_theme(options.theme);
        for markup_line in markup {
            let content = markup_line
                .spans
                .into_iter()
                .map(|span| PaintedAgentInlineNoteRun {
                    text: span.text,
                    foreground: workdeck_markup::resolve_stml_color(
                        span.style.fg.as_deref(),
                        &colors,
                    )
                    .or_else(|| Some(options.theme.text.clone())),
                    background: workdeck_markup::resolve_stml_color(
                        span.style.bg.as_deref(),
                        &colors,
                    )
                    .or_else(|| Some(options.theme.panel.clone())),
                    bold: span.style.bold == Some(true),
                    italic: span.style.italic == Some(true),
                    underline: span.style.underline == Some(true),
                    dim: span.style.dim == Some(true),
                    strike: span.style.strike == Some(true),
                    action: None,
                })
                .collect();
            lines.push(body_line(
                &gutter,
                content,
                geometry.content_width,
                options.width,
                options.theme,
            ));
        }
    } else {
        for (kind, text) in plain_body_lines(options.annotation, geometry.content_width) {
            lines.push(body_line(
                &gutter,
                vec![PaintedAgentInlineNoteRun::plain(
                    pad_text(&text, geometry.content_width),
                    Some(
                        match kind {
                            NoteBodyKind::Summary => &options.theme.text,
                            NoteBodyKind::Rationale => &options.theme.muted,
                        }
                        .clone(),
                    ),
                    Some(options.theme.panel.clone()),
                )],
                geometry.content_width,
                options.width,
                options.theme,
            ));
        }
    }

    let action_items = saved_action_items(options.actions);
    let bottom_row = lines.len();
    if state.actions_hovered && !action_items.is_empty() {
        let (bottom, bottom_hits) = render_bottom_border(BottomBorderOptions {
            gutter: &gutter,
            items: &action_items,
            box_width: geometry.box_width,
            width: options.width,
            theme: options.theme,
            hovered_action: state.hovered_action,
            row: bottom_row,
        });
        lines.push(bottom);
        hits.extend(bottom_hits);
    } else {
        let mut bottom = PaintedAgentInlineNoteLine::default();
        push_thread_gutter(&mut bottom, &gutter, false, options.theme);
        push_plain(
            &mut bottom,
            format!("╰{}╯", "─".repeat(geometry.box_width.saturating_sub(2))),
            Some(&options.theme.note_border),
            Some(&options.theme.panel),
        );
        finish_line(&mut bottom, options.width, options.theme);
        lines.push(bottom);
    }

    PaintedAgentInlineNote {
        lines,
        action_hits: hits,
        box_width: geometry.box_width,
        box_left: geometry.box_left,
        content_width: geometry.content_width,
        draft_rows: 0,
        draft_focused: false,
        draft_notifies_focus: false,
        draft_notifies_blur: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{AgentAnnotationConfidence, FileChangeKind, LineRange};

    use crate::{measure_agent_inline_note_height, resolve_theme};

    fn annotation(summary: &str) -> AgentAnnotation {
        AgentAnnotation {
            id: None,
            old_range: None,
            new_range: Some(LineRange { start: 2, end: 4 }),
            summary: summary.into(),
            rationale: None,
            markup: None,
            tags: Vec::new(),
            confidence: None::<AgentAnnotationConfidence>,
            source: None,
            title: None,
            author: None,
            created_at: None,
            updated_at: None,
            editable: false,
        }
    }

    fn file(path: &str) -> DiffFile {
        DiffFile {
            key: "file".into(),
            runtime_id: "file".into(),
            path: path.into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: None,
            stats: Default::default(),
            flags: Default::default(),
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: Vec::new(),
            content_identity: String::new(),
            sources: Default::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        }
    }

    #[test]
    fn saved_card_is_connected_and_matches_measured_height() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut note = annotation("Summary line");
        note.rationale = Some("Rationale line.".into());
        let mut options = AgentInlineNoteViewOptions::new(&note, LayoutMode::Split, &theme, 96);
        options.anchor_side = Some(ReviewSide::New);
        options.legacy_close = true;
        let painted = paint_agent_inline_note(&AgentInlineNoteViewState::default(), options);
        assert_eq!(painted.lines.len(), 5);
        assert!(painted.lines[0].text().trim_start().starts_with('╭'));
        assert!(painted.lines[0].text().contains("Agent note - R2–R4"));
        assert!(painted.lines[0].text().contains("[x]"));
        assert!(
            painted.lines[1]
                .text()
                .contains("│                                              │")
        );
        assert!(painted.lines[2].text().contains("Summary line"));
        assert!(painted.lines[3].text().contains("Rationale line."));
        assert!(painted.lines[4].text().trim_start().starts_with('╰'));
        assert_eq!(
            painted.lines.len(),
            measure_agent_inline_note_height(
                &note,
                Some(ReviewSide::New),
                LayoutMode::Split,
                96,
                0,
            )
        );
        assert_eq!(painted.action_at(0, 94), Some(AgentInlineNoteAction::Close));
    }

    #[test]
    fn baseline_saved_and_draft_cell_frames_match_opentui_capture() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut saved_note = annotation("Summary");
        saved_note.rationale = Some("Why".into());
        let mut saved_options =
            AgentInlineNoteViewOptions::new(&saved_note, LayoutMode::Stack, &theme, 34);
        saved_options.anchor_side = Some(ReviewSide::New);
        saved_options.legacy_close = true;
        let saved = paint_agent_inline_note(&AgentInlineNoteViewState::default(), saved_options);
        assert_eq!(
            saved
                .lines
                .iter()
                .map(PaintedAgentInlineNoteLine::text)
                .collect::<Vec<_>>(),
            vec![
                "    ╭─ Agent note - R2–R4 ─── [x]╮",
                "    │                            │",
                "    │ Summary                    │",
                "    │ Why                        │",
                "    ╰────────────────────────────╯",
            ]
        );
        let top_padding = &saved.lines[1].runs;
        assert!(top_padding.iter().any(|run| {
            run.text.contains("                          ")
                && run.foreground.as_deref() == Some(theme.text.as_str())
        }));

        let mut draft_note = annotation("Draft reply");
        draft_note.source = Some("user-draft".into());
        draft_note.new_range = Some(LineRange { start: 2, end: 2 });
        let mut draft_options =
            AgentInlineNoteViewOptions::new(&draft_note, LayoutMode::Stack, &theme, 40);
        draft_options.anchor_side = Some(ReviewSide::New);
        draft_options.draft = Some(AgentInlineNoteDraft {
            body: "Draft reply",
            focused: true,
            notify_focus: false,
            notify_blur: false,
        });
        let draft = paint_agent_inline_note(&AgentInlineNoteViewState::default(), draft_options);
        assert_eq!(
            draft
                .lines
                .iter()
                .map(PaintedAgentInlineNoteLine::text)
                .collect::<Vec<_>>(),
            vec![
                "    ╭─ Draft note - R2 ────────────────╮",
                "    │                                  │",
                "    │ Draft reply                      │",
                "    ╰────────────── ^S save Esc cancel ╯",
            ]
        );
        let draft_text_run = draft.lines[2]
            .runs
            .iter()
            .find(|run| run.text == "Draft reply")
            .unwrap();
        assert_eq!(
            draft_text_run.foreground.as_deref(),
            Some(theme.text.as_str())
        );
        assert!(draft.lines[2].runs.iter().any(|run| {
            run.text.contains("                      ") && run.foreground.is_none()
        }));
    }

    #[test]
    fn thread_rails_and_hovered_saved_actions_preserve_geometry() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut note = annotation("Reviewer note");
        note.source = Some("user".into());
        note.new_range = Some(LineRange { start: 2, end: 2 });
        let thread = VisibleAgentNoteThread {
            note_id: "child".into(),
            parent_id: Some("parent".into()),
            depth: 2,
            has_next_sibling: Some(true),
            ancestor_has_next_sibling: vec![false, true],
        };
        let actions = VisibleAgentNoteActions {
            edit: true,
            reply: true,
            delete: true,
        };
        let mut options = AgentInlineNoteViewOptions::new(&note, LayoutMode::Stack, &theme, 60);
        options.anchor_side = Some(ReviewSide::New);
        options.thread = Some(&thread);
        options.actions = Some(actions);
        let resting = paint_agent_inline_note(&AgentInlineNoteViewState::default(), options);
        assert!(resting.lines[0].text().contains("│ ├─╭─ Your note"));
        assert!(resting.lines.last().unwrap().text().contains("│ ╰"));
        assert!(!resting.lines.last().unwrap().text().contains("reply"));

        let mut state = AgentInlineNoteViewState::default();
        state.enter_card();
        state.hover_action(Some(AgentInlineNoteAction::Reply));
        let hovered = paint_agent_inline_note(&state, options);
        assert_eq!(resting.lines.len(), hovered.lines.len());
        assert!(
            hovered
                .lines
                .last()
                .unwrap()
                .text()
                .contains("r reply e edit d delete")
        );
        let reply = hovered
            .lines
            .last()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.action == Some(AgentInlineNoteAction::Reply))
            .unwrap();
        let edit = hovered
            .lines
            .last()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.action == Some(AgentInlineNoteAction::Edit))
            .unwrap();
        assert_eq!(
            reply.background.as_deref(),
            Some(theme.accent_muted.as_str())
        );
        assert_ne!(reply.background, edit.background);
        assert_eq!(
            hovered.action_at(
                hovered.lines.len() - 1,
                hovered
                    .action_hits
                    .iter()
                    .find(|hit| hit.action == AgentInlineNoteAction::Delete)
                    .unwrap()
                    .column_start
            ),
            Some(AgentInlineNoteAction::Delete)
        );
    }

    #[test]
    fn reply_draft_grows_wraps_and_keeps_thread_connectors() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let body = "This draft note is long enough to soft wrap inside the composer without manually inserted newlines.";
        let mut note = annotation(body);
        note.source = Some("user-draft".into());
        note.title = Some("Reply".into());
        note.new_range = Some(LineRange { start: 2, end: 2 });
        let thread = VisibleAgentNoteThread {
            note_id: "draft".into(),
            parent_id: Some("parent".into()),
            depth: 2,
            has_next_sibling: Some(false),
            ancestor_has_next_sibling: vec![false, true],
        };
        let mut options = AgentInlineNoteViewOptions::new(&note, LayoutMode::Stack, &theme, 48);
        options.anchor_side = Some(ReviewSide::New);
        options.thread = Some(&thread);
        options.draft = Some(AgentInlineNoteDraft {
            body,
            focused: true,
            notify_focus: true,
            notify_blur: true,
        });
        let painted = paint_agent_inline_note(&AgentInlineNoteViewState::default(), options);
        assert!(painted.lines[0].text().contains("│ ╰─╭─ Reply - R2"));
        assert!(painted.lines.iter().all(|line| line.text().contains('│')));
        assert!(painted.draft_rows > 2);
        assert!(
            painted
                .lines
                .iter()
                .any(|line| line.text().contains("This draft"))
        );
        assert!(
            painted
                .lines
                .iter()
                .any(|line| line.text().contains("newlines."))
        );
        assert!(
            painted
                .lines
                .last()
                .unwrap()
                .text()
                .contains("^S save Esc cancel")
        );
        assert!(painted.draft_focused);
        assert!(painted.draft_notifies_focus);
        assert!(painted.draft_notifies_blur);
        assert_eq!(
            painted.lines.len(),
            measure_agent_inline_note_height(
                &note,
                Some(ReviewSide::New),
                LayoutMode::Stack,
                48,
                2,
            )
        );
    }

    #[test]
    fn draft_actions_are_independent_and_key_commands_match_labels() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut note = annotation("Draft reply");
        note.source = Some("user-draft".into());
        note.new_range = Some(LineRange { start: 2, end: 2 });
        let mut options = AgentInlineNoteViewOptions::new(&note, LayoutMode::Stack, &theme, 60);
        options.draft = Some(AgentInlineNoteDraft {
            body: "Draft reply",
            focused: true,
            notify_focus: false,
            notify_blur: false,
        });
        let mut state = AgentInlineNoteViewState::default();
        state.hover_action(Some(AgentInlineNoteAction::Save));
        let painted = paint_agent_inline_note(&state, options);
        let save = painted
            .lines
            .last()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.action == Some(AgentInlineNoteAction::Save))
            .unwrap();
        let cancel = painted
            .lines
            .last()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.action == Some(AgentInlineNoteAction::Cancel))
            .unwrap();
        assert_eq!(
            save.background.as_deref(),
            Some(theme.accent_muted.as_str())
        );
        assert_ne!(save.background, cancel.background);
        assert_eq!(
            agent_inline_note_draft_command("j", true),
            Some(AgentInlineNoteDraftCommand::Newline)
        );
        assert_eq!(
            agent_inline_note_draft_command("s", true),
            Some(AgentInlineNoteDraftCommand::Save)
        );
        assert_eq!(
            agent_inline_note_draft_command("escape", false),
            Some(AgentInlineNoteDraftCommand::Cancel)
        );
    }

    #[test]
    fn stml_replaces_plain_body_and_empty_markup_falls_back() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut note = annotation("Plain fallback summary");
        note.markup = Some(
            "<h2>Refactor</h2><list><item>keep <b>hot path</b> allocation-free</item></list><box border border-style=\"double\">shape</box>".into(),
        );
        let options = AgentInlineNoteViewOptions::new(&note, LayoutMode::Stack, &theme, 60);
        let painted = paint_agent_inline_note(&AgentInlineNoteViewState::default(), options);
        let frame = painted
            .lines
            .iter()
            .map(PaintedAgentInlineNoteLine::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(frame.contains("Refactor"));
        assert!(frame.contains("• keep hot path allocation-free"));
        assert!(frame.contains('╔'));
        assert!(frame.contains("║shape"));
        assert!(frame.contains('╚'));
        assert!(!frame.contains("Plain fallback summary"));
        assert_eq!(
            painted.lines.len(),
            measure_agent_inline_note_height(&note, None, LayoutMode::Stack, 60, 0)
        );

        note.markup = Some("<!-- only a comment -->".into());
        let options = AgentInlineNoteViewOptions::new(&note, LayoutMode::Stack, &theme, 60);
        let fallback = paint_agent_inline_note(&AgentInlineNoteViewState::default(), options);
        assert!(
            fallback
                .lines
                .iter()
                .any(|line| line.text().contains("Plain fallback summary"))
        );
    }

    #[test]
    fn compact_titles_retain_author_path_range_age_and_special_characters() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let path = "src/a/very/long/review/path/that/must/be/truncated.ts";
        let file = file(path);
        let mut note = annotation("Summary line");
        note.source = Some("user".into());
        note.new_range = Some(LineRange { start: 20, end: 24 });
        note.created_at = Some("2026-04-15T00:00:00.000Z".into());
        let thread = VisibleAgentNoteThread {
            note_id: "note".into(),
            parent_id: None,
            depth: 0,
            has_next_sibling: None,
            ancestor_has_next_sibling: Vec::new(),
        };
        let mut options = AgentInlineNoteViewOptions::new(&note, LayoutMode::Stack, &theme, 60);
        options.file = Some(&file);
        options.thread = Some(&thread);
        options.now_ms = DateTime::parse_from_rfc3339("2026-04-15T02:00:00.000Z")
            .unwrap()
            .timestamp_millis();
        let painted = paint_agent_inline_note(&AgentInlineNoteViewState::default(), options);
        let title = painted.lines[0].text();
        assert!(title.contains("Your note · 2h"));
        assert!(title.contains("..."));
        assert!(title.contains("truncated.ts"));
        assert!(title.contains("R20–R24"));

        note.source = None;
        note.author = Some("prism (arbiter)".into());
        note.created_at = None;
        let mut options = AgentInlineNoteViewOptions::new(&note, LayoutMode::Split, &theme, 96);
        options.note_count = 2;
        options.legacy_close = true;
        let title =
            paint_agent_inline_note(&AgentInlineNoteViewState::default(), options).lines[0].text();
        assert!(title.contains("prism (arbiter) note 1/2"));
    }

    #[test]
    fn age_buckets_and_hover_resets_match_the_component_effects() {
        let base = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        let at = |minutes: i64| base + minutes * 60_000;
        assert_eq!(short_review_note_age(Some("invalid"), base), "");
        assert_eq!(
            short_review_note_age(Some("2026-01-01T00:00:00Z"), at(0)),
            "now"
        );
        assert_eq!(
            short_review_note_age(Some("2026-01-01T00:00:00Z"), at(59)),
            "59m"
        );
        assert_eq!(
            short_review_note_age(Some("2026-01-01T00:00:00Z"), at(60)),
            "1h"
        );
        assert_eq!(
            short_review_note_age(Some("2026-01-01T00:00:00Z"), at(1_440)),
            "1d"
        );
        assert_eq!(
            short_review_note_age(Some("2026-01-01T00:00:00Z"), at(10_080)),
            "1w"
        );
        assert_eq!(
            short_review_note_age(Some("2026-01-01T00:00:00Z"), at(524_160)),
            "1y"
        );

        let mut state = AgentInlineNoteViewState::default();
        state.enter_card();
        state.hover_action(Some(AgentInlineNoteAction::Edit));
        state.synchronize(false, true);
        assert!(state.actions_hovered());
        assert_eq!(state.hovered_action(), None);
        state.synchronize(true, true);
        assert!(!state.actions_hovered());
        state.enter_card();
        state.leave_card();
        assert!(!state.actions_hovered());
        assert_eq!(state.hovered_action(), None);
    }
}
