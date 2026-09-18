//! Host-owned painting for extension file views.
//!
//! This is the native Ratatui counterpart of Hunk's
//! `src/ui/components/panes/FileView.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. The original in-process React
//! row callback becomes a bounded declarative component painter, while row
//! identity, selection, cursor paint, virtualization, fallback, and failure
//! attribution remain explicit host behavior.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use workdeck_core::DiffFile;
use workdeck_diff::{MeasuredRowBounds, VisibleBodyBounds, resolve_visible_row_index_window};
use workdeck_extension_api::{
    ExtensionFileViewLayout, ExtensionFileViewRowComponent, ExtensionFileViewSpan,
    ExtensionFileViewTone, ExtensionPaintTheme, ExtensionTextAttribute, FileViewRowFailure,
    ValidatedFileViewLayout, ViewNode,
};
use workdeck_review::{LayoutMode, PlannedFileViewRow};

use crate::{
    AgentInlineNoteViewOptions, AgentInlineNoteViewState, AppTheme, CursorHighlight,
    FileViewGeometry, PlannedRowIdentity, VisibleAgentNoteActions,
    cursor_line_highlight_background, paint_agent_inline_note, planned_row_matches_cursor,
    ratatui_theme_color, review_row_id, to_extension_paint_theme, wrap_styled_spans,
};

/// Host identity that determines whether native row-local state may survive a repaint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileViewPaintIdentity<'a> {
    pub extension_id: &'a str,
    pub view_id: &'a str,
    pub registration_identity: u64,
    pub layout_generation: u64,
}

impl Default for FileViewPaintIdentity<'static> {
    fn default() -> Self {
        Self {
            extension_id: "native",
            view_id: "file-view",
            registration_identity: 0,
            layout_generation: 0,
        }
    }
}

/// Immutable bounded properties delivered to one custom row painter.
#[derive(Debug, Clone, Copy)]
pub struct FileViewComponentPaintRequest<'a> {
    pub component: &'a ExtensionFileViewRowComponent,
    pub content: &'a ViewNode,
    pub width: usize,
    pub height: usize,
    pub selected: bool,
    pub row_index: usize,
    pub theme: &'a ExtensionPaintTheme,
}

/// One mounted row and the exact terminal lines it owns.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintedFileViewRow {
    pub key: String,
    pub stable_key: String,
    pub review_row_id: String,
    pub paint_identity: Option<String>,
    pub row_id: Option<String>,
    pub row_index: Option<usize>,
    pub top: usize,
    pub height: usize,
    pub selected: bool,
    pub cursor_highlighted: bool,
    pub custom: bool,
    pub toggle_expanded_on_left_mouse_up: bool,
    pub lines: Vec<Line<'static>>,
}

/// Windowed file-view output. Spacer rows retain the complete measured body geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintedFileView {
    pub body_height: usize,
    pub top_spacer_height: usize,
    pub bottom_spacer_height: usize,
    pub rows: Vec<PaintedFileViewRow>,
    pub failures: Vec<FileViewRowFailure>,
}

impl PaintedFileView {
    #[must_use]
    pub fn lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::with_capacity(self.body_height);
        lines.extend((0..self.top_spacer_height).map(|_| Line::default()));
        lines.extend(self.rows.iter().flat_map(|row| row.lines.iter().cloned()));
        lines.extend((0..self.bottom_spacer_height).map(|_| Line::default()));
        lines
    }
}

/// All immutable inputs for painting one alternate file presentation.
#[derive(Debug, Clone, Copy)]
pub struct FileViewViewOptions<'a> {
    pub file: &'a DiffFile,
    pub resolved: &'a ValidatedFileViewLayout,
    pub geometry: &'a FileViewGeometry<'a>,
    pub cursor_highlight: Option<&'a CursorHighlight>,
    pub selected_hunk_index: Option<usize>,
    pub theme: &'a AppTheme,
    pub visible_body_bounds: Option<VisibleBodyBounds>,
    pub width: usize,
    pub identity: FileViewPaintIdentity<'a>,
    pub expanded_row_ids: &'a BTreeSet<String>,
    pub now_ms: i64,
}

/// Report whether one symbolic row belongs to the selected hunk's inclusive row range.
#[must_use]
pub fn is_file_view_row_selected(
    layout: &ExtensionFileViewLayout,
    row_index: usize,
    selected_hunk_index: Option<usize>,
) -> bool {
    selected_hunk_index
        .and_then(|index| layout.hunk_rows.get(index))
        .is_some_and(|hunk| row_index >= hunk.start_row && row_index <= hunk.end_row)
}

fn file_view_tone_color(tone: Option<ExtensionFileViewTone>, theme: &AppTheme) -> &str {
    match tone {
        Some(ExtensionFileViewTone::Muted) => &theme.muted,
        Some(ExtensionFileViewTone::Accent) => &theme.accent,
        Some(ExtensionFileViewTone::AccentMuted) => &theme.accent_muted,
        Some(ExtensionFileViewTone::Syntax) => &theme.syntax_colors.default,
        Some(ExtensionFileViewTone::Added) => &theme.file_new,
        Some(ExtensionFileViewTone::Removed) => &theme.file_deleted,
        None => &theme.text,
    }
}

fn file_view_text_style(span: &ExtensionFileViewSpan, theme: &AppTheme) -> Style {
    let mut style =
        Style::default().fg(ratatui_theme_color(file_view_tone_color(span.tone, theme)));
    for attribute in &span.attributes {
        style = style.add_modifier(match attribute {
            ExtensionTextAttribute::Bold => Modifier::BOLD,
            ExtensionTextAttribute::Italic => Modifier::ITALIC,
            ExtensionTextAttribute::Underline => Modifier::UNDERLINED,
            ExtensionTextAttribute::Strikethrough => Modifier::CROSSED_OUT,
        });
    }
    style
}

/// Paint a symbolic fallback row through the theme-neutral extension span contract.
#[must_use]
pub fn paint_symbolic_file_view_row(
    spans: &[ExtensionFileViewSpan],
    theme: &AppTheme,
    width: usize,
) -> Vec<Line<'static>> {
    let spans = spans
        .iter()
        .map(|span| Span::styled(span.text.clone(), file_view_text_style(span, theme)))
        .collect::<Vec<_>>();
    wrap_styled_spans(spans, width.max(1))
        .into_iter()
        .map(Line::from)
        .collect()
}

fn selected_component_content(
    component: &ExtensionFileViewRowComponent,
    selected: bool,
    expanded: bool,
) -> &ViewNode {
    if selected && expanded {
        component
            .selected_expanded_content
            .as_ref()
            .or(component.expanded_content.as_ref())
            .or(component.selected_content.as_ref())
            .unwrap_or(&component.content)
    } else if expanded {
        component
            .expanded_content
            .as_ref()
            .unwrap_or(&component.content)
    } else if selected {
        component
            .selected_content
            .as_ref()
            .unwrap_or(&component.content)
    } else {
        &component.content
    }
}

fn apply_row_background(lines: &mut [Line<'static>], background: &str) {
    let background = ratatui_theme_color(background);
    for line in lines {
        line.style = line.style.bg(background);
    }
}

fn fixed_height(mut lines: Vec<Line<'static>>, height: usize) -> Vec<Line<'static>> {
    lines.truncate(height);
    lines.resize_with(height, Line::default);
    lines
}

/// Paint with an injected component renderer, used by the subprocess adapter and parity tests.
#[must_use]
pub fn paint_file_view_with<P>(
    options: FileViewViewOptions<'_>,
    mut paint_component: P,
) -> PaintedFileView
where
    P: for<'a> FnMut(FileViewComponentPaintRequest<'a>) -> Result<Vec<Line<'static>>, String>,
{
    debug_assert_eq!(
        options.resolved.layout.rows.len(),
        options.resolved.row_heights.len()
    );
    let measured = options
        .geometry
        .row_bounds
        .iter()
        .map(|bounds| MeasuredRowBounds {
            key: bounds.key.clone(),
            top: bounds.top,
            height: bounds.height,
        })
        .collect::<Vec<_>>();
    let window = options.visible_body_bounds.map_or_else(
        || workdeck_diff::VisibleRowIndexWindow {
            bottom_spacer_height: 0,
            end_index: options.geometry.file_view_rows.len(),
            start_index: 0,
            top_spacer_height: 0,
        },
        |visible| {
            resolve_visible_row_index_window(options.geometry.body_height, &measured, visible)
        },
    );
    let public_theme = to_extension_paint_theme(options.theme);
    let mut failures = Vec::new();
    let mut rows = Vec::with_capacity(window.end_index.saturating_sub(window.start_index));

    for plan_index in window.start_index..window.end_index {
        let planned = &options.geometry.file_view_rows[plan_index];
        let bounds = &options.geometry.row_bounds[plan_index];
        match planned {
            PlannedFileViewRow::InlineNote {
                key,
                stable_key,
                annotation,
                anchor_side,
                note,
                note_count,
                note_index,
                ..
            } => {
                let actions = note.has_actions.then_some(VisibleAgentNoteActions {
                    edit: true,
                    reply: true,
                    delete: true,
                });
                let mut note_options = AgentInlineNoteViewOptions::new(
                    annotation,
                    LayoutMode::Stack,
                    options.theme,
                    options.width,
                );
                note_options.anchor_side = Some(*anchor_side);
                note_options.file = Some(options.file);
                note_options.note_count = *note_count;
                note_options.note_index = *note_index;
                note_options.actions = actions;
                note_options.thread_depth = Some(note.thread_depth);
                note_options.now_ms = options.now_ms;
                let lines = fixed_height(
                    paint_agent_inline_note(&AgentInlineNoteViewState::default(), note_options)
                        .ratatui_lines(),
                    bounds.height,
                );
                rows.push(PaintedFileViewRow {
                    key: key.clone(),
                    stable_key: stable_key.clone(),
                    review_row_id: review_row_id(key),
                    paint_identity: None,
                    row_id: None,
                    row_index: None,
                    top: bounds.top,
                    height: bounds.height,
                    selected: false,
                    cursor_highlighted: false,
                    custom: false,
                    toggle_expanded_on_left_mouse_up: false,
                    lines,
                });
            }
            PlannedFileViewRow::FileViewRow {
                key,
                stable_key,
                stable_alias_keys,
                row,
                row_index,
            } => {
                let selected = is_file_view_row_selected(
                    &options.resolved.layout,
                    *row_index,
                    options.selected_hunk_index,
                );
                let cursor_highlighted = planned_row_matches_cursor(
                    &PlannedRowIdentity {
                        stable_key: stable_key.clone(),
                        stable_alias_keys: stable_alias_keys.clone(),
                    },
                    options.cursor_highlight,
                );
                let expanded = options.expanded_row_ids.contains(&row.id);
                let mut lines = if let Some(component) = &row.component {
                    let content = selected_component_content(component, selected, expanded);
                    match paint_component(FileViewComponentPaintRequest {
                        component,
                        content,
                        width: options.width.max(1),
                        height: component.height,
                        selected,
                        row_index: *row_index,
                        theme: &public_theme,
                    }) {
                        Ok(mut lines) if !lines.is_empty() => {
                            if let Some(prefix) = &component.selection_prefix
                                && let Some(line) = lines.first_mut()
                            {
                                let style = line
                                    .spans
                                    .iter()
                                    .find(|span| !span.content.is_empty())
                                    .map_or_else(Style::default, |span| span.style);
                                line.spans.insert(
                                    0,
                                    Span::styled(
                                        if selected {
                                            prefix.selected.clone()
                                        } else {
                                            prefix.unselected.clone()
                                        },
                                        style,
                                    ),
                                );
                            }
                            lines
                        }
                        Ok(_) => {
                            paint_symbolic_file_view_row(&row.spans, options.theme, options.width)
                        }
                        Err(message) => {
                            failures.push(FileViewRowFailure {
                                extension_id: options.identity.extension_id.to_owned(),
                                view_id: options.identity.view_id.to_owned(),
                                file_id: options.file.runtime_id.clone(),
                                file_path: options.file.path.clone(),
                                row_id: row.id.clone(),
                                layout_generation: options.identity.layout_generation,
                                message,
                            });
                            paint_symbolic_file_view_row(&row.spans, options.theme, options.width)
                        }
                    }
                } else {
                    paint_symbolic_file_view_row(&row.spans, options.theme, options.width)
                };
                lines = fixed_height(lines, bounds.height);
                let base_background = if selected {
                    &options.theme.selected_hunk
                } else {
                    &options.theme.panel
                };
                let background = if cursor_highlighted {
                    cursor_line_highlight_background(base_background, options.theme)
                } else {
                    base_background.clone()
                };
                apply_row_background(&mut lines, &background);
                let custom = row.component.is_some();
                rows.push(PaintedFileViewRow {
                    key: key.clone(),
                    stable_key: stable_key.clone(),
                    review_row_id: review_row_id(key),
                    paint_identity: custom.then(|| {
                        format!(
                            "{}:{}:{}:{}",
                            options.file.runtime_id,
                            options.identity.registration_identity,
                            options.identity.layout_generation,
                            row.id
                        )
                    }),
                    row_id: Some(row.id.clone()),
                    row_index: Some(*row_index),
                    top: bounds.top,
                    height: bounds.height,
                    selected,
                    cursor_highlighted,
                    custom,
                    toggle_expanded_on_left_mouse_up: row
                        .component
                        .as_ref()
                        .is_some_and(|component| component.toggle_expanded_on_left_mouse_up),
                    lines,
                });
            }
        }
    }

    PaintedFileView {
        body_height: options.geometry.body_height,
        top_spacer_height: window.top_spacer_height,
        bottom_spacer_height: window.bottom_spacer_height,
        rows,
        failures,
    }
}

/// Paint through Workdeck's validated declarative native-extension tree.
#[must_use]
pub fn paint_file_view(options: FileViewViewOptions<'_>) -> PaintedFileView {
    let theme = options.theme;
    paint_file_view_with(options, |request| {
        let mut lines = Vec::new();
        crate::flatten_file_view_component(request.content, 0, theme, &mut lines);
        Ok(lines)
    })
}

/// Minimal host state that mirrors React keyed mount survival for native stateful rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileViewComponentMountState {
    next_token: u64,
    mounted: BTreeMap<String, u64>,
}

impl FileViewComponentMountState {
    /// Retain only visible keyed rows and return their stable mount tokens in paint order.
    pub fn synchronize(&mut self, painted: &PaintedFileView) -> Vec<u64> {
        let visible = painted
            .rows
            .iter()
            .filter_map(|row| row.paint_identity.clone())
            .collect::<BTreeSet<_>>();
        self.mounted
            .retain(|identity, _| visible.contains(identity));
        painted
            .rows
            .iter()
            .filter_map(|row| row.paint_identity.as_ref())
            .map(|identity| {
                *self.mounted.entry(identity.clone()).or_insert_with(|| {
                    self.next_token = self.next_token.saturating_add(1);
                    self.next_token
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
