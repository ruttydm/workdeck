//! Reusable, Workdeck-branded Ratatui review primitives.
//!
//! These are the native counterpart of Hunk's public OpenTUI component package. They render into
//! the caller's Ratatui buffer and deliberately do not own terminal setup, application chrome,
//! scrolling policy, or key bindings.

use super::{
    ReviewOptions, ReviewStreamChrome, build_review_rows_with_chrome, file_header,
    max_file_header_stats_width, resolve_theme,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use std::collections::BTreeSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use workdeck_core::{
    BUNDLED_SHIKI_THEME_IDS, Changeset, ChangesetSource, DiffFile, FileChangeKind, FileStats,
    ReviewSelection,
};
use workdeck_diff::{
    HighlightCache, PatchError, VisibleBodyBounds, find_max_line_number, format_terminal_path,
    parse_patch, resolve_visible_row_index_window, unit_row_bounds,
};
use workdeck_review::LayoutMode;

pub type WorkdeckDiffFile = DiffFile;
pub type WorkdeckDiffFileInput = DiffFile;
pub type WorkdeckDiffStats = FileStats;
pub type WorkdeckDiffLayout = LayoutMode;
pub type WorkdeckDiffThemeName = &'static str;
pub use workdeck_diff::{
    FileComparisonOptions as WorkdeckFileComparisonOptions, FileSnapshot as WorkdeckFileSnapshot,
    diff_from_file_snapshots as diff_from_workdeck_file_snapshots,
};

/// The complete public theme-name catalog exposed by the pinned Hunk component package.
pub const WORKDECK_DIFF_THEME_NAMES: &[WorkdeckDiffThemeName] = BUNDLED_SHIKI_THEME_IDS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckDiffSelection {
    pub file_id: String,
    pub hunk_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckDiffBodyOptions {
    pub layout: LayoutMode,
    pub theme: String,
    pub show_line_numbers: bool,
    pub show_hunk_headers: bool,
    pub tab_width: u16,
    pub wrap_lines: bool,
    pub horizontal_offset: usize,
    pub highlight: bool,
    pub selected_hunk_index: Option<usize>,
    pub hunk_gap: u16,
}

impl Default for WorkdeckDiffBodyOptions {
    fn default() -> Self {
        Self {
            layout: LayoutMode::Split,
            theme: "github-dark-default".into(),
            show_line_numbers: true,
            show_hunk_headers: true,
            tab_width: 4,
            wrap_lines: false,
            horizontal_offset: 0,
            highlight: true,
            selected_hunk_index: Some(0),
            hunk_gap: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckDiffViewOptions {
    pub body: WorkdeckDiffBodyOptions,
    pub scrollable: bool,
    pub vertical_offset: usize,
}

impl Default for WorkdeckDiffViewOptions {
    fn default() -> Self {
        Self {
            body: WorkdeckDiffBodyOptions::default(),
            scrollable: true,
            vertical_offset: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckDiffFileHeaderOptions {
    pub theme: String,
    pub selected: bool,
}

impl Default for WorkdeckDiffFileHeaderOptions {
    fn default() -> Self {
        Self {
            theme: "github-dark-default".into(),
            selected: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckReviewStreamOptions {
    pub body: WorkdeckDiffBodyOptions,
    pub selection: Option<WorkdeckDiffSelection>,
    pub show_file_headers: bool,
    pub show_file_separators: bool,
    pub file_gap: u16,
    pub vertical_offset: usize,
}

impl Default for WorkdeckReviewStreamOptions {
    fn default() -> Self {
        Self {
            body: WorkdeckDiffBodyOptions::default(),
            selection: None,
            show_file_headers: true,
            show_file_separators: true,
            file_gap: 1,
            vertical_offset: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckFileNavOptions {
    pub selected_file_id: Option<String>,
    pub theme: String,
}

impl Default for WorkdeckFileNavOptions {
    fn default() -> Self {
        Self {
            selected_file_id: None,
            theme: "github-dark-default".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckHunkRow {
    pub hunk_index: usize,
    pub row: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkdeckDiffRenderMap {
    pub hunk_rows: Vec<WorkdeckHunkRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckFileNavHit {
    pub file_id: String,
    pub row: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkdeckFileNavRenderMap {
    pub file_rows: Vec<WorkdeckFileNavHit>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkdeckReviewStreamRenderMap {
    pub file_rows: Vec<WorkdeckFileNavHit>,
    pub hunk_rows: Vec<(String, WorkdeckHunkRow)>,
}

/// Count visible additions and deletions from the provider-neutral hunk model.
#[must_use]
pub fn count_workdeck_diff_stats(file: &DiffFile) -> WorkdeckDiffStats {
    let mut stats = FileStats::default();
    for line in file.hunks.iter().flat_map(|hunk| &hunk.lines) {
        match line.kind {
            workdeck_core::DiffLineKind::Addition => stats.additions += 1,
            workdeck_core::DiffLineKind::Deletion => stats.deletions += 1,
            workdeck_core::DiffLineKind::Context => {}
        }
    }
    stats.truncated = file.stats.truncated;
    stats
}

/// Refresh the stable identities of one provider-neutral public diff file.
#[must_use]
pub fn create_workdeck_diff_file(mut input: WorkdeckDiffFileInput) -> WorkdeckDiffFile {
    input.refresh_identity();
    input
}

/// Parse unified patch text into the public Workdeck diff-file model.
pub fn create_workdeck_diff_files_from_patch(
    patch_text: &str,
    source_id: &str,
) -> Result<Vec<WorkdeckDiffFile>, PatchError> {
    Ok(parse_patch(
        patch_text,
        source_id,
        source_id,
        ChangesetSource::Patch {
            label: source_id.to_owned(),
        },
    )?
    .files)
}

/// Render one diff body without app chrome, navigation, or terminal ownership.
pub fn render_workdeck_diff_body(
    area: Rect,
    buffer: &mut Buffer,
    file: Option<&WorkdeckDiffFileInput>,
    options: &WorkdeckDiffBodyOptions,
) -> WorkdeckDiffRenderMap {
    let (mut lines, map) = workdeck_diff_body_rows(file, options, area.width);
    apply_public_theme(&mut lines, &options.theme);
    Paragraph::new(lines).render(area, buffer);
    map
}

/// Render one diff body with caller-controlled vertical scrolling.
pub fn render_workdeck_diff_view(
    area: Rect,
    buffer: &mut Buffer,
    file: Option<&WorkdeckDiffFileInput>,
    options: &WorkdeckDiffViewOptions,
) -> WorkdeckDiffRenderMap {
    let (mut lines, mut map) = workdeck_diff_body_rows(file, &options.body, area.width);
    apply_public_theme(&mut lines, &options.body.theme);
    let offset = if options.scrollable {
        options.vertical_offset
    } else {
        0
    };
    let row_bounds = unit_row_bounds(lines.len());
    let window = resolve_visible_row_index_window(
        lines.len(),
        &row_bounds,
        VisibleBodyBounds {
            top: i64::try_from(offset).unwrap_or(i64::MAX),
            height: i64::from(area.height),
        },
    );
    map.hunk_rows = map
        .hunk_rows
        .into_iter()
        .filter_map(|mut hit| {
            let source_row = usize::from(hit.row);
            if !(window.start_index..window.end_index).contains(&source_row) {
                return None;
            }
            let row = source_row.checked_sub(window.start_index)?;
            hit.row = u16::try_from(row).ok()?;
            Some(hit)
        })
        .collect();
    Paragraph::new(
        lines
            .drain(window.start_index..window.end_index)
            .collect::<Vec<_>>(),
    )
    .render(area, buffer);
    map
}

/// Render the compact one-line public file header. The returned rectangle is the caller's click
/// target for the original component's `onSelect` behavior.
pub fn render_workdeck_diff_file_header(
    area: Rect,
    buffer: &mut Buffer,
    file: &WorkdeckDiffFileInput,
    options: &WorkdeckDiffFileHeaderOptions,
) -> Rect {
    let mut line = file_header(
        file,
        options.selected,
        usize::from(area.width),
        max_file_header_stats_width(std::slice::from_ref(file)),
        &resolve_theme(Some(&options.theme), None, &[]),
    );
    apply_public_theme(std::slice::from_mut(&mut line), &options.theme);
    Paragraph::new(line).render(area, buffer);
    Rect { height: 1, ..area }
}

/// Render a top-to-bottom multi-file review stream without app chrome or terminal ownership.
pub fn render_workdeck_review_stream(
    area: Rect,
    buffer: &mut Buffer,
    files: &[WorkdeckDiffFileInput],
    options: &WorkdeckReviewStreamOptions,
) -> WorkdeckReviewStreamRenderMap {
    if files.is_empty() {
        let palette = public_palette(&options.body.theme);
        Paragraph::new(Line::styled(
            " No files to render.",
            Style::default().fg(palette.muted).bg(palette.panel),
        ))
        .render(area, buffer);
        return WorkdeckReviewStreamRenderMap::default();
    }

    let active = resolve_selection(files, options.selection.as_ref());
    let mut lines = Vec::new();
    let mut map = WorkdeckReviewStreamRenderMap::default();
    for (file_index, file) in files.iter().enumerate() {
        if file_index > 0 && options.show_file_separators && options.file_gap > 0 {
            lines.extend((1..options.file_gap).map(|_| Line::default()));
            lines.push(separator_line(area.width, &options.body.theme));
        }
        if options.show_file_headers {
            map.file_rows.push(WorkdeckFileNavHit {
                file_id: public_file_id(file).to_owned(),
                row: u16::try_from(lines.len()).unwrap_or(u16::MAX),
            });
            let mut header = file_header(
                file,
                active
                    .as_ref()
                    .is_some_and(|selection| selection.file_id == public_file_id(file)),
                usize::from(area.width),
                max_file_header_stats_width(std::slice::from_ref(file)),
                &resolve_theme(Some(&options.body.theme), None, &[]),
            );
            apply_public_theme(std::slice::from_mut(&mut header), &options.body.theme);
            lines.push(header);
        }

        let mut body_options = options.body.clone();
        body_options.selected_hunk_index = active.as_ref().and_then(|selection| {
            (selection.file_id == public_file_id(file)).then_some(selection.hunk_index)
        });
        let body_start = lines.len();
        let (mut body, body_map) = workdeck_diff_body_rows(Some(file), &body_options, area.width);
        apply_public_theme(&mut body, &body_options.theme);
        map.hunk_rows
            .extend(body_map.hunk_rows.into_iter().map(|mut hit| {
                hit.row = hit
                    .row
                    .saturating_add(u16::try_from(body_start).unwrap_or(u16::MAX));
                (public_file_id(file).to_owned(), hit)
            }));
        lines.append(&mut body);
    }

    let offset = options.vertical_offset;
    let row_bounds = unit_row_bounds(lines.len());
    let window = resolve_visible_row_index_window(
        lines.len(),
        &row_bounds,
        VisibleBodyBounds {
            top: i64::try_from(offset).unwrap_or(i64::MAX),
            height: i64::from(area.height),
        },
    );
    map.file_rows.retain_mut(|hit| {
        let source_row = usize::from(hit.row);
        if !(window.start_index..window.end_index).contains(&source_row) {
            return false;
        }
        let Some(row) = source_row.checked_sub(window.start_index) else {
            return false;
        };
        hit.row = u16::try_from(row).unwrap_or(u16::MAX);
        true
    });
    map.hunk_rows.retain_mut(|(_, hit)| {
        let source_row = usize::from(hit.row);
        if !(window.start_index..window.end_index).contains(&source_row) {
            return false;
        }
        let Some(row) = source_row.checked_sub(window.start_index) else {
            return false;
        };
        hit.row = u16::try_from(row).unwrap_or(u16::MAX);
        true
    });
    Paragraph::new(
        lines
            .drain(window.start_index..window.end_index)
            .collect::<Vec<_>>(),
    )
    .render(area, buffer);
    map
}

/// Render Hunk's adaptive flat/tree file navigator. Returned row mappings implement its
/// `onSelectFile` behavior without embedding a React callback in the renderer.
pub fn render_workdeck_file_nav(
    area: Rect,
    buffer: &mut Buffer,
    files: &[WorkdeckDiffFileInput],
    options: &WorkdeckFileNavOptions,
) -> WorkdeckFileNavRenderMap {
    let palette = public_palette(&options.theme);
    let entries = if area.width.saturating_sub(1) >= 32 {
        tree_sidebar_entries(files)
    } else {
        flat_sidebar_entries(files)
    };
    let stats_width = entries
        .iter()
        .filter_map(|entry| match entry {
            SidebarEntry::File(file) => Some(file.stats.width()),
            SidebarEntry::Group(_) | SidebarEntry::Directory { .. } => None,
        })
        .max()
        .unwrap_or_default();
    let mut lines = Vec::with_capacity(entries.len());
    let mut map = WorkdeckFileNavRenderMap::default();
    for entry in entries {
        match entry {
            SidebarEntry::Group(label) => lines.push(Line::styled(
                fit_nav_text(&label, usize::from(area.width).max(1)),
                Style::default().fg(palette.muted).bg(palette.panel),
            )),
            SidebarEntry::Directory { label, depth } => {
                lines.push(directory_line(
                    &label,
                    depth,
                    area.width,
                    stats_width,
                    palette,
                ));
            }
            SidebarEntry::File(file) => {
                map.file_rows.push(WorkdeckFileNavHit {
                    file_id: file.id.clone(),
                    row: u16::try_from(lines.len()).unwrap_or(u16::MAX),
                });
                lines.push(sidebar_file_line(
                    &file,
                    options.selected_file_id.as_deref() == Some(file.id.as_str()),
                    area.width,
                    stats_width,
                    palette,
                ));
            }
        }
    }
    Paragraph::new(lines).render(area, buffer);
    map
}

#[must_use]
pub fn workdeck_file_nav_selection_at(map: &WorkdeckFileNavRenderMap, row: u16) -> Option<&str> {
    map.file_rows
        .iter()
        .find(|hit| hit.row == row)
        .map(|hit| hit.file_id.as_str())
}

fn workdeck_diff_body_rows(
    file: Option<&DiffFile>,
    options: &WorkdeckDiffBodyOptions,
    width: u16,
) -> (Vec<Line<'static>>, WorkdeckDiffRenderMap) {
    let Some(file) = file else {
        return (
            vec![Line::styled(
                format!(
                    " {}",
                    fit_nav_text(
                        "No file selected.",
                        usize::from(width.saturating_sub(2).max(1))
                    )
                ),
                Style::default().fg(Color::DarkGray),
            )],
            WorkdeckDiffRenderMap::default(),
        );
    };
    if file.hunks.is_empty() {
        return (
            vec![Line::styled(
                format!(
                    " {}",
                    fit_nav_text(
                        empty_diff_message(file),
                        usize::from(width.saturating_sub(2).max(1))
                    )
                ),
                Style::default().fg(Color::DarkGray),
            )],
            WorkdeckDiffRenderMap::default(),
        );
    }

    let changeset = Changeset {
        id: "public-review".into(),
        title: "Public review".into(),
        source: ChangesetSource::Patch {
            label: "public-review".into(),
        },
        files: vec![file.clone()],
    };
    let selection = ReviewSelection {
        file_index: 0,
        hunk_index: options.selected_hunk_index,
        ..ReviewSelection::default()
    };
    let review_options = ReviewOptions {
        layout: options.layout,
        sidebar: false,
        line_numbers: options.show_line_numbers,
        tab_width: options.tab_width,
        hunk_headers: options.show_hunk_headers,
        wrap_lines: options.wrap_lines,
        horizontal_offset: options.horizontal_offset,
        line_number_digits: Some(max_line_number_digits(file)),
        highlight: options.highlight,
        file_gap: 0,
        hunk_gap: options.hunk_gap,
        theme: resolve_theme(Some(&options.theme), None, &[]),
        ..ReviewOptions::default()
    };
    let mut highlights = HighlightCache::default();
    let rows = build_review_rows_with_chrome(
        &changeset,
        &[],
        selection,
        options.layout,
        &review_options,
        width,
        &mut highlights,
        &BTreeSet::new(),
        ReviewStreamChrome {
            show_file_headers: false,
        },
        false,
    );
    let map = WorkdeckDiffRenderMap {
        hunk_rows: rows
            .hunk_tops
            .into_iter()
            .filter_map(|((_, hunk_index), row)| {
                Some(WorkdeckHunkRow {
                    hunk_index,
                    row: u16::try_from(row).ok()?,
                })
            })
            .collect(),
    };
    (rows.lines, map)
}

fn max_line_number_digits(file: &DiffFile) -> usize {
    find_max_line_number(file).to_string().len()
}

fn empty_diff_message(file: &DiffFile) -> &'static str {
    if file.change_kind == FileChangeKind::Renamed {
        "No textual hunks. This change only renames the file."
    } else if file.flags.binary {
        "Binary file skipped"
    } else if file.flags.too_large {
        "File too large to render automatically."
    } else if file.change_kind == FileChangeKind::Added {
        "No textual hunks. The file is marked as new."
    } else if file.change_kind == FileChangeKind::Deleted {
        "No textual hunks. The file is marked as deleted."
    } else {
        "No textual hunks to render for this file."
    }
}

fn resolve_selection(
    files: &[DiffFile],
    selection: Option<&WorkdeckDiffSelection>,
) -> Option<WorkdeckDiffSelection> {
    selection
        .filter(|selection| {
            files
                .iter()
                .any(|file| public_file_id(file) == selection.file_id)
        })
        .cloned()
        .or_else(|| {
            files.first().map(|file| WorkdeckDiffSelection {
                file_id: public_file_id(file).to_owned(),
                hunk_index: 0,
            })
        })
}

fn public_file_id(file: &DiffFile) -> &str {
    if file.runtime_id.is_empty() {
        &file.key
    } else {
        &file.runtime_id
    }
}

fn separator_line(width: u16, theme: &str) -> Line<'static> {
    let palette = public_palette(theme);
    Line::styled(
        format!(
            " {}",
            "─".repeat(usize::from(width.saturating_sub(2).max(1)))
        ),
        Style::default().fg(palette.muted).bg(palette.panel),
    )
}

#[derive(Debug)]
enum SidebarEntry {
    Group(String),
    Directory { label: String, depth: usize },
    File(SidebarFileEntry),
}

#[derive(Debug)]
struct SidebarFileEntry {
    id: String,
    name: String,
    depth: usize,
    stats: String,
    icon: &'static str,
    icon_color: Color,
}

fn flat_sidebar_entries(files: &[DiffFile]) -> Vec<SidebarEntry> {
    let mut entries = Vec::new();
    let mut active_group = None::<String>;
    for file in files {
        let path = format_terminal_path(&file.path);
        let group = posix_dirname(&path).to_owned();
        if active_group.as_deref() != Some(group.as_str()) {
            active_group = Some(group.clone());
            entries.push(SidebarEntry::Group(if group == "." {
                "./".into()
            } else {
                format!("{group}/")
            }));
        }
        entries.push(SidebarEntry::File(sidebar_file_entry(file, 0)));
    }
    entries
}

fn tree_sidebar_entries(files: &[DiffFile]) -> Vec<SidebarEntry> {
    let mut entries = Vec::new();
    let mut active_directories = Vec::<String>::new();
    for file in files {
        let path = format_terminal_path(&file.path);
        let directories = sidebar_directory_segments(posix_dirname(&path));
        let shared = active_directories
            .iter()
            .zip(&directories)
            .take_while(|(left, right)| left == right)
            .count();
        for (depth, segment) in directories.iter().enumerate().skip(shared) {
            entries.push(SidebarEntry::Directory {
                label: if segment.starts_with('/') {
                    segment.clone()
                } else {
                    format!("{segment}/")
                },
                depth,
            });
        }
        entries.push(SidebarEntry::File(sidebar_file_entry(
            file,
            directories.len(),
        )));
        active_directories = directories;
    }
    entries
}

fn sidebar_file_entry(file: &DiffFile, depth: usize) -> SidebarFileEntry {
    let path = format_terminal_path(&file.path);
    let previous = file.previous_path.as_deref().map(format_terminal_path);
    let name = match previous {
        Some(previous) if previous != path => {
            let previous_name = posix_basename(&previous);
            let next_name = posix_basename(&path);
            if previous_name == next_name {
                next_name.to_owned()
            } else {
                format!("{previous_name} -> {next_name}")
            }
        }
        _ => posix_basename(&path).to_owned(),
    };
    let mut stats = Vec::new();
    if let Some(agent) = &file.agent
        && !agent.annotations.is_empty()
    {
        stats.push(format!("*{}", agent.annotations.len()));
    }
    if file.stats.additions > 0 {
        stats.push(format!(
            "+{}{}",
            file.stats.additions,
            if file.stats.truncated { "+" } else { "" }
        ));
    }
    if file.stats.deletions > 0 {
        stats.push(format!("-{}", file.stats.deletions));
    }
    let (icon, icon_color) =
        if file.flags.untracked || file.change_kind == FileChangeKind::Untracked {
            ("?", Color::Yellow)
        } else {
            match file.change_kind {
                FileChangeKind::Added => ("A", Color::Green),
                FileChangeKind::Deleted => ("D", Color::Red),
                FileChangeKind::Renamed | FileChangeKind::Copied => ("R", Color::Cyan),
                FileChangeKind::Modified | FileChangeKind::TypeChanged => ("M", Color::Yellow),
                FileChangeKind::Untracked => ("?", Color::Yellow),
                FileChangeKind::Conflicted => ("", Color::White),
            }
        };
    SidebarFileEntry {
        id: public_file_id(file).to_owned(),
        name,
        depth,
        stats: stats.join(" "),
        icon,
        icon_color,
    }
}

fn directory_line(
    label: &str,
    depth: usize,
    width: u16,
    stats_width: usize,
    palette: PublicPalette,
) -> Line<'static> {
    let text_width = usize::from(width.saturating_sub(1)).max(1);
    let stats_section_width = usize::from(stats_width > 0) * (stats_width + 1);
    let indent = sidebar_indent(depth, text_width, stats_section_width + 1);
    let label_width = text_width
        .saturating_sub(1 + stats_section_width + indent)
        .max(1);
    Line::from(vec![
        Span::styled(" ", Style::default().bg(palette.panel)),
        Span::styled(" ".repeat(indent), Style::default().bg(palette.panel)),
        Span::styled(
            fit_nav_text(label, label_width),
            Style::default().fg(palette.muted).bg(palette.panel),
        ),
    ])
}

fn sidebar_file_line(
    file: &SidebarFileEntry,
    selected: bool,
    width: u16,
    stats_width: usize,
    palette: PublicPalette,
) -> Line<'static> {
    let row_bg = if selected {
        palette.panel_alt
    } else {
        palette.panel
    };
    let text_width = usize::from(width.saturating_sub(1)).max(1);
    let icon_width = usize::from(!file.icon.is_empty()) * 2;
    let stats_section_width = usize::from(stats_width > 0) * (stats_width + 1);
    let indent = sidebar_indent(file.depth, text_width, icon_width + stats_section_width + 1);
    let name_width = text_width
        .saturating_sub(1 + icon_width + stats_section_width + indent)
        .max(1);
    let mut spans = vec![Span::styled(
        " ",
        Style::default().bg(if selected {
            palette.accent
        } else {
            palette.panel
        }),
    )];
    if indent > 0 {
        spans.push(Span::styled(
            " ".repeat(indent),
            Style::default().bg(row_bg),
        ));
    }
    if !file.icon.is_empty() {
        let icon_color = match file.icon_color {
            Color::Green => palette.added,
            Color::Red => palette.removed,
            Color::Cyan => palette.accent,
            other => other,
        };
        spans.push(Span::styled(
            format!("{} ", file.icon),
            Style::default().fg(icon_color).bg(row_bg),
        ));
    }
    let fitted = fit_nav_text(&file.name, name_width);
    let padding = name_width.saturating_sub(fitted.width());
    spans.push(Span::styled(
        format!("{fitted}{}", " ".repeat(padding)),
        Style::default().fg(palette.text).bg(row_bg),
    ));
    if stats_section_width > 0 {
        spans.push(Span::styled(
            format!(" {:>stats_width$}", file.stats),
            Style::default()
                .fg(if selected {
                    palette.text
                } else {
                    palette.muted
                })
                .bg(row_bg),
        ));
    }
    Line::from(spans)
}

fn sidebar_indent(depth: usize, text_width: usize, reserved_width: usize) -> usize {
    depth
        .saturating_mul(2)
        .min(text_width.saturating_sub(reserved_width + 1))
}

fn sidebar_directory_segments(parent: &str) -> Vec<String> {
    if parent == "." {
        return Vec::new();
    }
    let root_len = parent
        .chars()
        .take_while(|character| *character == '/')
        .count();
    let mut segments = parent
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if root_len > 0 {
        segments.insert(0, "/".repeat(root_len));
    }
    segments
}

fn posix_dirname(path: &str) -> &str {
    path.rsplit_once('/').map_or(
        ".",
        |(directory, _)| {
            if directory.is_empty() { "/" } else { directory }
        },
    )
}

fn posix_basename(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

fn fit_nav_text(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if text.width() <= width {
        return text.to_owned();
    }
    if width == 1 {
        return "…".into();
    }
    let mut output = String::new();
    let mut used = 0_usize;
    for character in text.chars() {
        let character_width = character.width().unwrap_or_default();
        if used.saturating_add(character_width) > width - 1 {
            break;
        }
        output.push(character);
        used = used.saturating_add(character_width);
    }
    output.push('…');
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PublicPalette {
    panel: Color,
    panel_alt: Color,
    text: Color,
    muted: Color,
    accent: Color,
    added: Color,
    removed: Color,
    added_bg: Color,
    removed_bg: Color,
    selected_bg: Color,
}

fn public_palette(theme: &str) -> PublicPalette {
    let theme = resolve_theme(Some(theme), None, &[]);
    PublicPalette {
        panel: color_from_hex(&theme.background).unwrap_or(Color::Reset),
        panel_alt: color_from_hex(&theme.panel_alt).unwrap_or(Color::Reset),
        text: color_from_hex(&theme.text).unwrap_or(Color::White),
        muted: color_from_hex(&theme.muted).unwrap_or(Color::DarkGray),
        accent: color_from_hex(&theme.accent).unwrap_or(Color::Cyan),
        added: color_from_hex(&theme.added_sign_color).unwrap_or(Color::Green),
        removed: color_from_hex(&theme.removed_sign_color).unwrap_or(Color::Red),
        added_bg: color_from_hex(&theme.added_bg).unwrap_or(Color::Reset),
        removed_bg: color_from_hex(&theme.removed_bg).unwrap_or(Color::Reset),
        selected_bg: color_from_hex(&theme.selected_hunk).unwrap_or(Color::Reset),
    }
}

fn color_from_hex(value: &str) -> Option<Color> {
    (value.len() == 7 && value.starts_with('#')).then_some(())?;
    Some(Color::Rgb(
        u8::from_str_radix(&value[1..3], 16).ok()?,
        u8::from_str_radix(&value[3..5], 16).ok()?,
        u8::from_str_radix(&value[5..7], 16).ok()?,
    ))
}

fn apply_public_theme(lines: &mut [Line<'static>], theme: &str) {
    let palette = public_palette(theme);
    for line in lines {
        line.style.bg = Some(palette.panel);
        for span in &mut line.spans {
            span.style.fg = span.style.fg.map(|color| match color {
                Color::White | Color::Rgb(201, 209, 217) => palette.text,
                Color::DarkGray | Color::Rgb(139, 148, 158) => palette.muted,
                Color::Cyan => palette.accent,
                Color::Green | Color::Rgb(126, 231, 135) => palette.added,
                Color::Red | Color::Rgb(255, 123, 114) => palette.removed,
                other => other,
            });
            span.style.bg = Some(match span.style.bg.unwrap_or(Color::Reset) {
                Color::Reset => palette.panel,
                Color::Rgb(14, 54, 30) | Color::Rgb(34, 104, 58) => palette.added_bg,
                Color::Rgb(67, 24, 29) | Color::Rgb(126, 42, 49) => palette.removed_bg,
                Color::Rgb(45, 55, 72) => palette.selected_bg,
                other => other,
            });
        }
    }
}

#[cfg(test)]
mod tests;
