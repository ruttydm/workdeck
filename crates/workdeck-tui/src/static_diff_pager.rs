//! Non-interactive ANSI diff rendering for captured pager hosts.
//!
//! This is the Rust/Ratatui equivalent of Hunk's `staticDiffPager.ts`: parsing,
//! highlighting, alignment, gap geometry, and terminal sanitizing remain owned by
//! the shared review crates. This module only serializes that model without raw
//! mode or alternate-screen control sequences.

use std::ops::Range;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use workdeck_core::{
    ChangesetSource, CommonOptions, DiffFile, DiffLine, DiffLineKind, FileChangeKind,
    NamedCustomThemeConfig,
};
use workdeck_diff::{
    HighlightAppearance, HighlightCache, SanitizeOptions, SyntaxToken, expand_diff_tabs,
    format_terminal_path, normalize_diff_path, parse_patch, plan_split_line_pairs,
    resolve_split_cell_geometry, resolve_split_pane_widths, sanitize_terminal_line,
    sanitize_terminal_text, word_diff_ranges,
};
use workdeck_review::{review_gap_source_for_file, review_leading_gap, review_trailing_gap};

use crate::{
    AppTheme, RowCellKind, ThemeAppearance, diff_rail_marker, neutral_rail_color, resolve_theme,
    resolve_word_diff_highlight_bg, split_cell_palette, split_gutter_text, split_left_rail_color,
    split_right_rail_color, stack_cell_palette, stack_gutter_text, stack_rail_color,
    with_transparent_surfaces,
};

pub const DEFAULT_STATIC_DIFF_PAGER_WIDTH: usize = 120;
pub const MIN_STATIC_DIFF_PAGER_WIDTH: usize = 20;
const RESET: &str = "\u{1b}[0m";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticDiffPagerOutput {
    pub text: String,
    /// Sanitized diagnostic detail when rendering fell back to the input patch.
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StaticSpan {
    text: String,
    foreground: Option<String>,
    background: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaticLayout {
    Stack,
    Split,
}

/// Render patch text for embedded/captured pager hosts without ever entering the alternate screen.
///
/// Failures deliberately return sanitized original input. The CLI composition root owns warning
/// delivery so tests and embedders can inspect the reason without intercepting process stderr.
#[must_use]
pub fn render_static_diff_pager(
    text: &str,
    options: &CommonOptions,
    custom_themes: &[NamedCustomThemeConfig],
    terminal_columns: Option<usize>,
) -> StaticDiffPagerOutput {
    match try_render_static_diff_pager(text, options, custom_themes, terminal_columns) {
        Ok(text) => StaticDiffPagerOutput {
            text,
            fallback_reason: None,
        },
        Err(error) => StaticDiffPagerOutput {
            text: sanitize_terminal_text(text, SanitizeOptions::default()),
            fallback_reason: Some(sanitize_terminal_line(&error)),
        },
    }
}

fn try_render_static_diff_pager(
    text: &str,
    options: &CommonOptions,
    custom_themes: &[NamedCustomThemeConfig],
    terminal_columns: Option<usize>,
) -> Result<String, String> {
    let changeset = parse_patch(
        text,
        "patch:pager",
        "Patch review: pager",
        ChangesetSource::Patch {
            label: "pager".into(),
        },
    )
    // Hunk's patch bootstrap converts structural parse failures into an empty changeset; the
    // static adapter then emits this stable reason rather than leaking parser internals.
    .map_err(|_| "no files rendered".to_owned())?;
    if changeset.files.is_empty() {
        return Err("no files rendered".into());
    }

    let resolved = resolve_theme(options.theme.as_deref(), None, custom_themes);
    let theme = if options.transparent_background == Some(true) {
        with_transparent_surfaces(&resolved)
    } else {
        resolved
    };
    let width = terminal_columns
        .unwrap_or(DEFAULT_STATIC_DIFF_PAGER_WIDTH)
        .max(MIN_STATIC_DIFF_PAGER_WIDTH);
    let layout = if options.mode == Some(workdeck_core::InputLayoutMode::Split) {
        StaticLayout::Split
    } else {
        StaticLayout::Stack
    };
    let tab_width = options
        .tab_width
        .unwrap_or(workdeck_diff::DEFAULT_TAB_WIDTH);
    let show_line_numbers = options.line_numbers != Some(false);
    let show_hunk_headers = options.hunk_headers != Some(false);
    let mut highlighter = HighlightCache::default();
    let mut rendered = Vec::with_capacity(changeset.files.len());
    for file in &changeset.files {
        rendered.push(render_static_file(
            file,
            &theme,
            options,
            width,
            layout,
            tab_width,
            show_line_numbers,
            show_hunk_headers,
            &mut highlighter,
        ));
    }
    Ok(format!("{}\n", rendered.join("\n\n")))
}

#[allow(clippy::too_many_arguments)]
fn render_static_file(
    file: &DiffFile,
    theme: &AppTheme,
    _options: &CommonOptions,
    width: usize,
    layout: StaticLayout,
    tab_width: u16,
    show_line_numbers: bool,
    show_hunk_headers: bool,
    highlighter: &mut HighlightCache,
) -> String {
    let highlighted = (!file.flags.binary && !file.flags.too_large).then(|| {
        highlighter.highlight_with_syntax_theme(
            file,
            match theme.appearance {
                ThemeAppearance::Light => HighlightAppearance::Light,
                ThemeAppearance::Dark => HighlightAppearance::Dark,
            },
            theme.syntax_theme.as_deref(),
            &theme.syntax_scope_overrides,
        )
    });
    let stats = format!(
        "{} {}",
        color_text(
            &format!(
                "+{}{}",
                file.stats.additions,
                if file.stats.truncated { "+" } else { "" }
            ),
            Some(&theme.badge_added),
            None,
        ),
        color_text(
            &format!("-{}", file.stats.deletions),
            Some(&theme.badge_removed),
            None,
        )
    );
    let status = color_text(
        &format!("{}{}", file_status_label(file), file_mode_text(file)),
        Some(&theme.muted),
        None,
    );
    let header = format!(
        "{} {status} {stats}",
        color_text(&file_display_path(file), Some(&theme.text), None)
    );

    if file.hunks.is_empty() {
        return format!(
            "{header}\n{}",
            color_text(
                &format!("  {}", static_empty_diff_message(file)),
                Some(&theme.muted),
                None,
            )
        );
    }

    let max_rendered_line = file
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .flat_map(|line| [line.old_line, line.new_line])
        .flatten()
        .max()
        .unwrap_or(1);
    let digits = usize::try_from(max_rendered_line)
        .unwrap_or(usize::MAX)
        .to_string()
        .len()
        .max(file.stats.additions.to_string().len());
    let gap_source = review_gap_source_for_file(file);
    let mut rows = Vec::new();
    for (hunk_index, hunk) in file.hunks.iter().enumerate() {
        if let Some(gap) = review_leading_gap(&gap_source, hunk_index) {
            rows.push(render_header_like_row(
                &format!(
                    "··· {} unchanged {} ···",
                    gap.line_count,
                    if gap.line_count == 1 { "line" } else { "lines" }
                ),
                &theme.muted,
                &theme.panel_alt,
                theme,
            ));
        }
        if show_hunk_headers {
            rows.push(render_header_like_row(
                &hunk.formatted_header(),
                &theme.badge_neutral,
                &theme.panel_alt,
                theme,
            ));
        }
        let highlighted_hunk = highlighted
            .as_ref()
            .and_then(|highlighted| highlighted.get(hunk_index));
        match layout {
            StaticLayout::Stack => rows.extend(render_stack_hunk(
                hunk,
                highlighted_hunk,
                theme,
                digits,
                show_line_numbers,
                tab_width,
            )),
            StaticLayout::Split => rows.extend(render_split_hunk(
                hunk,
                highlighted_hunk,
                theme,
                digits,
                show_line_numbers,
                tab_width,
                width,
            )),
        }
    }
    if let Some(gap) = review_trailing_gap(&gap_source) {
        rows.push(render_header_like_row(
            &format!(
                "··· {} unchanged {} ···",
                gap.line_count,
                if gap.line_count == 1 { "line" } else { "lines" }
            ),
            &theme.muted,
            &theme.panel_alt,
            theme,
        ));
    }
    if rows.is_empty() {
        format!(
            "{header}\n{}",
            color_text(
                &format!("  {}", static_empty_diff_message(file)),
                Some(&theme.muted),
                None,
            )
        )
    } else {
        format!("{header}\n{}", rows.join("\n"))
    }
}

fn render_stack_hunk(
    hunk: &workdeck_core::DiffHunk,
    highlighted: Option<&workdeck_diff::HighlightedHunk>,
    theme: &AppTheme,
    digits: usize,
    show_line_numbers: bool,
    tab_width: u16,
) -> Vec<String> {
    let mut emphasis = vec![Vec::<Range<usize>>::new(); hunk.lines.len()];
    for pair in plan_split_line_pairs(&hunk.lines) {
        let (Some(old_index), Some(new_index)) = (pair.old_index, pair.new_index) else {
            continue;
        };
        if old_index == new_index {
            continue;
        }
        let ranges =
            expanded_word_ranges(&hunk.lines[old_index], &hunk.lines[new_index], tab_width);
        emphasis[old_index] = ranges.old;
        emphasis[new_index] = ranges.new;
    }
    hunk.lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let tokens = highlighted
                .and_then(|highlighted| highlighted.get(index))
                .and_then(|highlighted| highlighted.for_stack(line.kind));
            render_stack_line(
                line,
                tokens,
                &emphasis[index],
                theme,
                digits,
                show_line_numbers,
                tab_width,
            )
        })
        .collect()
}

fn render_stack_line(
    line: &DiffLine,
    tokens: Option<&Vec<SyntaxToken>>,
    emphasis: &[Range<usize>],
    theme: &AppTheme,
    digits: usize,
    show_line_numbers: bool,
    tab_width: u16,
) -> String {
    let kind = row_cell_kind(line.kind);
    let palette = stack_cell_palette(kind, theme, line.moved);
    let sign = line_sign(line.kind);
    let gutter = stack_gutter_text(
        sign,
        line.old_line,
        line.new_line,
        digits,
        show_line_numbers,
    );
    let gutter_width = if show_line_numbers { digits * 2 + 5 } else { 2 };
    let spans = line_spans(line, tokens, emphasis, theme, tab_width);
    let rail = stack_rail_color(kind, theme, true);
    format!(
        "{}{}{}{}",
        color_text(diff_rail_marker(), Some(&rail), Some(&theme.panel)),
        color_text(
            &pad_or_clip(&gutter, gutter_width),
            Some(palette.number_color),
            Some(palette.gutter_background),
        ),
        serialize_spans(&spans, palette.content_background),
        fill_remaining_line(palette.content_background),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_split_hunk(
    hunk: &workdeck_core::DiffHunk,
    highlighted: Option<&workdeck_diff::HighlightedHunk>,
    theme: &AppTheme,
    digits: usize,
    show_line_numbers: bool,
    tab_width: u16,
    width: usize,
) -> Vec<String> {
    let widths = resolve_split_pane_widths(width);
    plan_split_line_pairs(&hunk.lines)
        .into_iter()
        .map(|pair| {
            let old = pair.old_index.and_then(|index| hunk.lines.get(index));
            let new = pair.new_index.and_then(|index| hunk.lines.get(index));
            let ranges = old
                .zip(new)
                .filter(|(old, new)| !std::ptr::eq(*old, *new))
                .map(|(old, new)| expanded_word_ranges(old, new, tab_width));
            let old_tokens = pair
                .old_index
                .and_then(|index| highlighted.and_then(|lines| lines.get(index)))
                .and_then(|line| line.deletion.as_ref());
            let new_tokens = pair
                .new_index
                .and_then(|index| highlighted.and_then(|lines| lines.get(index)))
                .and_then(|line| line.addition.as_ref());
            format!(
                "{}{}",
                render_split_cell(
                    old,
                    old_tokens,
                    ranges.as_ref().map_or(&[], |ranges| ranges.old.as_slice()),
                    true,
                    widths.left_width,
                    theme,
                    digits,
                    show_line_numbers,
                    tab_width,
                ),
                render_split_cell(
                    new,
                    new_tokens,
                    ranges.as_ref().map_or(&[], |ranges| ranges.new.as_slice()),
                    false,
                    widths.right_width,
                    theme,
                    digits,
                    show_line_numbers,
                    tab_width,
                )
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn render_split_cell(
    line: Option<&DiffLine>,
    tokens: Option<&Vec<SyntaxToken>>,
    emphasis: &[Range<usize>],
    old_side: bool,
    width: usize,
    theme: &AppTheme,
    digits: usize,
    show_line_numbers: bool,
    tab_width: u16,
) -> String {
    let kind = line.map_or(RowCellKind::Empty, |line| row_cell_kind(line.kind));
    let moved = line.is_some_and(|line| line.moved);
    let palette = split_cell_palette(kind, theme, moved);
    let geometry = resolve_split_cell_geometry(width, digits, show_line_numbers, 1);
    let sign = line.map_or(' ', |line| line_sign(line.kind));
    let number = line.and_then(|line| {
        if old_side {
            line.old_line
        } else {
            line.new_line
        }
    });
    let gutter = split_gutter_text(sign, number, digits, show_line_numbers);
    let spans = line.map_or_else(Vec::new, |line| {
        line_spans(line, tokens, emphasis, theme, tab_width)
    });
    let rail = if old_side {
        split_left_rail_color(kind, theme, true)
    } else {
        split_right_rail_color(kind, theme, true)
    };
    format!(
        "{}{}{}",
        color_text(diff_rail_marker(), Some(&rail), Some(&theme.panel)),
        color_text(
            &pad_or_clip(&gutter, geometry.gutter_width),
            Some(palette.number_color),
            Some(palette.gutter_background),
        ),
        serialize_spans_fixed_width(&spans, palette.content_background, geometry.content_width,)
    )
}

fn expanded_word_ranges(
    old: &DiffLine,
    new: &DiffLine,
    tab_width: u16,
) -> workdeck_diff::WordDiffRanges {
    word_diff_ranges(
        &expand_diff_tabs(&sanitize_terminal_line(&old.content), tab_width, 0),
        &expand_diff_tabs(&sanitize_terminal_line(&new.content), tab_width, 0),
    )
}

fn line_spans(
    line: &DiffLine,
    tokens: Option<&Vec<SyntaxToken>>,
    emphasis: &[Range<usize>],
    theme: &AppTheme,
    tab_width: u16,
) -> Vec<StaticSpan> {
    let emphasis_background = match line.kind {
        DiffLineKind::Deletion => resolve_word_diff_highlight_bg(
            &theme.removed_content_bg,
            &theme.removed_bg,
            &theme.removed_sign_color,
        ),
        DiffLineKind::Addition => resolve_word_diff_highlight_bg(
            &theme.added_content_bg,
            &theme.added_bg,
            &theme.added_sign_color,
        ),
        DiffLineKind::Context => theme.context_content_bg.clone(),
    };
    let mut raw = Vec::new();
    let mut column = 0;
    if let Some(tokens) = tokens.filter(|tokens| !tokens.is_empty()) {
        for token in tokens {
            let safe = sanitize_terminal_line(&token.text);
            let text = expand_diff_tabs(&safe, tab_width, column);
            column = column.saturating_add(text.width());
            raw.push(StaticSpan {
                text,
                foreground: Some(format!(
                    "#{:02x}{:02x}{:02x}",
                    token.foreground.red, token.foreground.green, token.foreground.blue
                )),
                background: None,
            });
        }
    } else {
        raw.push(StaticSpan {
            text: expand_diff_tabs(&sanitize_terminal_line(&line.content), tab_width, 0),
            foreground: None,
            background: None,
        });
    }
    apply_emphasis(raw, emphasis, &emphasis_background)
}

fn apply_emphasis(
    spans: Vec<StaticSpan>,
    ranges: &[Range<usize>],
    emphasis_background: &str,
) -> Vec<StaticSpan> {
    if ranges.is_empty() {
        return spans;
    }
    let mut result: Vec<StaticSpan> = Vec::new();
    let mut offset = 0;
    for span in spans {
        for character in span.text.chars() {
            let background = ranges
                .iter()
                .any(|range| range.start <= offset && offset < range.end)
                .then(|| emphasis_background.to_owned());
            let same_style = result
                .last_mut()
                .filter(|last| last.foreground == span.foreground && last.background == background);
            if let Some(last) = same_style {
                last.text.push(character);
            } else {
                result.push(StaticSpan {
                    text: character.to_string(),
                    foreground: span.foreground.clone(),
                    background,
                });
            }
            offset += character.len_utf8();
        }
    }
    result
}

fn row_cell_kind(kind: DiffLineKind) -> RowCellKind {
    match kind {
        DiffLineKind::Context => RowCellKind::Context,
        DiffLineKind::Addition => RowCellKind::Addition,
        DiffLineKind::Deletion => RowCellKind::Deletion,
    }
}

fn line_sign(kind: DiffLineKind) -> char {
    match kind {
        DiffLineKind::Context => ' ',
        DiffLineKind::Addition => '+',
        DiffLineKind::Deletion => '-',
    }
}

fn render_header_like_row(
    text: &str,
    foreground: &str,
    background: &str,
    theme: &AppTheme,
) -> String {
    format!(
        "{}{}",
        color_text(
            diff_rail_marker(),
            Some(neutral_rail_color(theme)),
            Some(background),
        ),
        color_text(text.trim_end(), Some(foreground), Some(background)),
    )
}

fn ansi_color(kind: &str, hex: Option<&str>) -> String {
    let Some(hex) = hex.and_then(|hex| hex.strip_prefix('#')) else {
        return String::new();
    };
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return String::new();
    }
    let Ok(red) = u8::from_str_radix(&hex[0..2], 16) else {
        return String::new();
    };
    let Ok(green) = u8::from_str_radix(&hex[2..4], 16) else {
        return String::new();
    };
    let Ok(blue) = u8::from_str_radix(&hex[4..6], 16) else {
        return String::new();
    };
    let selector = if kind == "fg" { 38 } else { 48 };
    format!("\u{1b}[{selector};2;{red};{green};{blue}m")
}

fn color_text(text: &str, foreground: Option<&str>, background: Option<&str>) -> String {
    let safe = sanitize_terminal_line(text);
    if safe.is_empty() {
        return String::new();
    }
    let prefix = format!(
        "{}{}",
        ansi_color("fg", foreground),
        ansi_color("bg", background)
    );
    if prefix.is_empty() {
        safe
    } else {
        format!("{prefix}{safe}{RESET}")
    }
}

fn fill_remaining_line(background: &str) -> String {
    let background = ansi_color("bg", Some(background));
    if background.is_empty() {
        String::new()
    } else {
        format!("{background}\u{1b}[K{RESET}")
    }
}

fn serialize_spans(spans: &[StaticSpan], row_background: &str) -> String {
    spans
        .iter()
        .map(|span| {
            color_text(
                &span.text,
                span.foreground.as_deref(),
                span.background.as_deref().or(Some(row_background)),
            )
        })
        .collect()
}

fn serialize_spans_fixed_width(spans: &[StaticSpan], row_background: &str, width: usize) -> String {
    let mut remaining = width;
    let mut output = String::new();
    for span in spans {
        if remaining == 0 {
            break;
        }
        let text = take_width(&span.text, remaining);
        let used = text.width();
        if !text.is_empty() {
            output.push_str(&color_text(
                &text,
                span.foreground.as_deref(),
                span.background.as_deref().or(Some(row_background)),
            ));
            remaining = remaining.saturating_sub(used);
        }
    }
    if remaining > 0 {
        output.push_str(&color_text(
            &" ".repeat(remaining),
            None,
            Some(row_background),
        ));
    }
    output
}

fn pad_or_clip(text: &str, width: usize) -> String {
    let mut visible = take_width(text, width);
    visible.push_str(&" ".repeat(width.saturating_sub(visible.width())));
    visible
}

fn take_width(text: &str, width: usize) -> String {
    let mut used = 0_usize;
    text.chars()
        .take_while(|character| {
            let character_width = character.width().unwrap_or_default();
            if used.saturating_add(character_width) > width {
                false
            } else {
                used = used.saturating_add(character_width);
                true
            }
        })
        .collect()
}

fn file_status_label(file: &DiffFile) -> &'static str {
    if file.flags.too_large {
        return "skipped large file";
    }
    if file.flags.binary {
        return "binary";
    }
    match file.change_kind {
        FileChangeKind::Added => "new file",
        FileChangeKind::Deleted => "deleted",
        FileChangeKind::Renamed => {
            if file.hunks.is_empty() || (file.stats.additions == 0 && file.stats.deletions == 0) {
                "renamed"
            } else {
                "renamed modified"
            }
        }
        FileChangeKind::Untracked => "untracked",
        FileChangeKind::TypeChanged => "mode changed",
        FileChangeKind::Copied => "copied",
        FileChangeKind::Conflicted => "conflicted",
        FileChangeKind::Modified => "modified",
    }
}

fn file_mode_text(file: &DiffFile) -> String {
    let mut old_mode = None;
    let mut new_mode = None;
    for line in file.patch.lines() {
        if let Some(mode) = line.strip_prefix("new file mode ") {
            new_mode = Some(mode);
        } else if let Some(mode) = line.strip_prefix("deleted file mode ") {
            old_mode = Some(mode);
        } else if let Some(mode) = line.strip_prefix("old mode ") {
            old_mode = Some(mode);
        } else if let Some(mode) = line.strip_prefix("new mode ") {
            new_mode = Some(mode);
        }
    }
    match (old_mode, new_mode) {
        (Some(old), Some(new)) if old != new => format!(" {old}→{new}"),
        (_, Some(mode)) if file.change_kind == FileChangeKind::Added => format!(" {mode}"),
        (Some(mode), _) if file.change_kind == FileChangeKind::Deleted => format!(" {mode}"),
        _ => String::new(),
    }
}

fn file_display_path(file: &DiffFile) -> String {
    let path = normalize_diff_path(Some(&file.path)).unwrap_or_else(|| file.path.clone());
    let path = format_terminal_path(&path);
    file.previous_path
        .as_deref()
        .and_then(|previous| normalize_diff_path(Some(previous)))
        .map(|previous| format_terminal_path(&previous))
        .filter(|previous| previous != &path)
        .map_or_else(|| path.clone(), |previous| format!("{previous} → {path}"))
}

fn static_empty_diff_message(file: &DiffFile) -> &'static str {
    if file.flags.too_large {
        "Skipped because the file is too large to render."
    } else if file.flags.binary {
        "Binary file."
    } else {
        "No textual changes."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(text: &str) -> String {
        let mut output = String::new();
        let mut chars = text.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\u{1b}' && chars.peek() == Some(&'[') {
                chars.next();
                for value in chars.by_ref() {
                    if ('@'..='~').contains(&value) {
                        break;
                    }
                }
            } else {
                output.push(character);
            }
        }
        output
    }

    fn strip_intentional_ansi(text: &str) -> String {
        let characters = text.chars().collect::<Vec<_>>();
        let mut output = String::new();
        let mut index = 0;
        while index < characters.len() {
            if characters[index] == '\u{1b}' && characters.get(index + 1) == Some(&'[') {
                let mut end = index + 2;
                while characters
                    .get(end)
                    .is_some_and(|value| value.is_ascii_digit() || *value == ';')
                {
                    end += 1;
                }
                if characters
                    .get(end)
                    .is_some_and(|value| *value == 'm' || *value == 'K')
                {
                    index = end + 1;
                    continue;
                }
            }
            output.push(characters[index]);
            index += 1;
        }
        output
    }

    fn custom_theme(value: serde_json::Value) -> NamedCustomThemeConfig {
        serde_json::from_value(value).expect("custom theme fixture must deserialize")
    }

    fn assert_no_unsafe_terminal_controls(text: &str) {
        for unsafe_value in [
            "\u{1b}]52;c;SGVsbG8=\u{7}",
            "\u{1b}[2J",
            "\u{1b}Pqpayload\u{1b}\\",
            "\u{1b}_payload\u{1b}\\",
            "\u{1b}^payload\u{1b}\\",
            "\u{1b}Xpayload\u{1b}\\",
            "\u{7}",
            "\r",
            "\u{8}",
            "\u{1b}",
        ] {
            assert!(
                !text.contains(unsafe_value),
                "unsafe terminal control remained: {unsafe_value:?} in {text:?}"
            );
        }
    }

    fn patch() -> &'static str {
        "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-const value = 1;\n+const value = 2;\n"
    }

    #[test]
    fn renders_non_interactive_ansi_stack_output() {
        let output = render_static_diff_pager(patch(), &CommonOptions::default(), &[], None);
        assert_eq!(output.fallback_reason, None);
        let plain = strip_ansi(&output.text);
        assert!(plain.contains("a.ts modified +1 -1"));
        assert!(plain.contains("▌@@ -1 +1 @@\n"));
        assert!(plain.contains("▌1   -  const value = 1;"));
        assert!(plain.contains("▌  1 +  const value = 2;"));
        assert!(output.text.contains("\u{1b}[38;2;"));
        assert!(!output.text.contains("\u{1b}[?1049h"));
    }

    #[test]
    fn matches_frozen_hunk_cell_content_geometry_and_palette() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/static-diff-pager.json"
        ))
        .unwrap();
        let fixture = &oracle["fixture"];
        let patch = fixture["patch"].as_str().unwrap();
        let stack = render_static_diff_pager(
            patch,
            &CommonOptions::default(),
            &[],
            Some(DEFAULT_STATIC_DIFF_PAGER_WIDTH),
        );
        assert_eq!(
            strip_ansi(&stack.text),
            fixture["standard_plain"].as_str().unwrap()
        );
        let hunk_ansi = fixture["standard_ansi"].as_str().unwrap();
        for color in [
            "\u{1b}[38;2;230;237;243m",
            "\u{1b}[38;2;173;174;177m",
            "\u{1b}[38;2;119;193;133m",
            "\u{1b}[38;2;250;142;137m",
            "\u{1b}[48;2;60;30;33m",
            "\u{1b}[48;2;18;37;29m",
        ] {
            assert!(hunk_ansi.contains(color));
            assert!(
                stack.text.contains(color),
                "missing Hunk palette code {color:?}"
            );
        }

        let split = render_static_diff_pager(
            patch,
            &CommonOptions {
                mode: Some(workdeck_core::InputLayoutMode::Split),
                ..CommonOptions::default()
            },
            &[],
            Some(80),
        );
        assert_eq!(
            strip_ansi(&split.text),
            fixture["split_plain_width_80"].as_str().unwrap()
        );
    }

    #[test]
    fn honors_hidden_line_numbers_and_hunk_headers() {
        let options = CommonOptions {
            line_numbers: Some(false),
            hunk_headers: Some(false),
            ..CommonOptions::default()
        };
        let plain = strip_ansi(&render_static_diff_pager(patch(), &options, &[], None).text);
        assert!(!plain.contains("@@ -1 +1 @@"));
        assert!(plain.contains("▌- const value = 1;"));
        assert!(plain.contains("▌+ const value = 2;"));
    }

    #[test]
    fn honors_tab_stops() {
        let patch =
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-a\tb\n+a\tc\n";
        let width_four = strip_ansi(
            &render_static_diff_pager(
                patch,
                &CommonOptions {
                    tab_width: Some(4),
                    ..CommonOptions::default()
                },
                &[],
                None,
            )
            .text,
        );
        let width_eight = strip_ansi(
            &render_static_diff_pager(
                patch,
                &CommonOptions {
                    tab_width: Some(8),
                    ..CommonOptions::default()
                },
                &[],
                None,
            )
            .text,
        );
        assert!(width_four.contains("a   c"));
        assert!(width_eight.contains("a       c"));
    }

    #[test]
    fn honors_explicit_split_layout() {
        let output = render_static_diff_pager(
            patch(),
            &CommonOptions {
                mode: Some(workdeck_core::InputLayoutMode::Split),
                ..CommonOptions::default()
            },
            &[],
            Some(80),
        );
        let plain = strip_ansi(&output.text);
        let changed = plain
            .lines()
            .find(|line| line.contains("const value"))
            .expect("changed split row");
        assert!(changed.contains("▌1 - const value = 1;"), "{changed:?}");
        assert!(changed.contains("▌1 + const value = 2;"), "{changed:?}");
        assert!(!plain.contains("▌  1 +  const value = 2;"));
    }

    #[test]
    fn auto_layout_remains_stacked_at_wide_widths() {
        let plain = strip_ansi(
            &render_static_diff_pager(
                patch(),
                &CommonOptions {
                    mode: Some(workdeck_core::InputLayoutMode::Auto),
                    ..CommonOptions::default()
                },
                &[],
                Some(200),
            )
            .text,
        );
        assert!(plain.contains("▌1   -  const value = 1;"));
        assert!(plain.contains("▌  1 +  const value = 2;"));
    }

    #[test]
    fn extends_stacked_row_backgrounds_to_host_edge() {
        let patch =
            "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-short\n+also short\n";
        let output = render_static_diff_pager(patch, &CommonOptions::default(), &[], None);
        let changed = output
            .text
            .lines()
            .filter(|line| strip_ansi(line).contains("short"))
            .collect::<Vec<_>>();
        assert_eq!(changed.len(), 2);
        assert!(changed.iter().all(|line| {
            line.rfind("\u{1b}[48;2;")
                .is_some_and(|index| line[index..].ends_with("m\u{1b}[K\u{1b}[0m"))
        }));
    }

    #[test]
    fn uses_custom_theme_text_color() {
        let theme = custom_theme(serde_json::json!({
            "id": "custom",
            "base": "github-dark-default",
            "text": "#123456"
        }));
        let output = render_static_diff_pager(
            patch(),
            &CommonOptions {
                theme: Some("custom".into()),
                ..CommonOptions::default()
            },
            &[theme],
            None,
        );
        assert!(strip_ansi(&output.text).contains("a.ts modified +1 -1"));
        assert!(output.text.contains("\u{1b}[38;2;18;52;86m"));
    }

    #[test]
    fn translates_deprecated_semantic_comment_colors() {
        let patch = "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1,2 @@\n+// visible comment\n const value = 1;\n";
        let theme = custom_theme(serde_json::json!({
            "id": "custom",
            "base": "nord",
            "syntax": {"comment": "#ff00ff"}
        }));
        let output = render_static_diff_pager(
            patch,
            &CommonOptions {
                theme: Some("custom".into()),
                ..CommonOptions::default()
            },
            &[theme],
            None,
        );
        assert!(strip_ansi(&output.text).contains("// visible comment"));
        assert!(
            output.text.contains("\u{1b}[38;2;255;0;255m"),
            "{:?}",
            output.text
        );
    }

    #[test]
    fn applies_raw_textmate_comment_scopes() {
        let patch = "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1,2 @@\n+// visible comment\n const value = 1;\n";
        let theme = custom_theme(serde_json::json!({
            "id": "custom",
            "base": "nord",
            "syntaxScopes": {
                "comment": "#ff00ff",
                "punctuation.definition.comment": "#ff00ff"
            }
        }));
        let output = render_static_diff_pager(
            patch,
            &CommonOptions {
                theme: Some("custom".into()),
                ..CommonOptions::default()
            },
            &[theme],
            None,
        );
        assert!(strip_ansi(&output.text).contains("// visible comment"));
        assert!(
            output.text.contains("\u{1b}[38;2;255;0;255m"),
            "{:?}",
            output.text
        );
    }

    #[test]
    fn transparent_mode_keeps_only_added_and_removed_backgrounds() {
        let patch = "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1,3 +1,3 @@\n const a = 1;\n-const value = 1;\n+const value = 2;\n const z = 3;\n";
        let output = render_static_diff_pager(
            patch,
            &CommonOptions {
                transparent_background: Some(true),
                ..CommonOptions::default()
            },
            &[],
            None,
        );
        let line_with = |needle: &str| {
            output
                .text
                .lines()
                .find(|line| strip_ansi(line).contains(needle))
                .unwrap_or_default()
        };
        assert!(output.text.contains("\u{1b}[38;2;"));
        assert!(!line_with("@@ -1,3 +1,3 @@").contains("\u{1b}[48;2;"));
        assert!(!line_with("const a = 1;").contains("\u{1b}[48;2;"));
        assert!(!line_with("const z = 3;").contains("\u{1b}[48;2;"));
        assert!(line_with("const value = 1;").contains("\u{1b}[48;2;"));
        assert!(line_with("const value = 2;").contains("\u{1b}[48;2;"));
    }

    #[test]
    fn shows_semantic_file_metadata_without_patch_transport_headers() {
        let patch = "diff --git a/new.txt b/new.txt\nnew file mode 100644\nindex 0000000..587be6b\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+hello\n";
        let plain =
            strip_ansi(&render_static_diff_pager(patch, &CommonOptions::default(), &[], None).text);
        assert!(plain.contains("new.txt new file 100644 +1 -0"), "{plain:?}");
        assert!(!plain.contains("diff --git"));
        assert!(!plain.contains("index 0000000"));
    }

    #[test]
    fn malformed_patch_falls_back_with_diagnostic() {
        let text = "diff --git incomplete\n";
        let output = render_static_diff_pager(text, &CommonOptions::default(), &[], None);
        assert_eq!(output.text, text);
        assert_eq!(output.fallback_reason.as_deref(), Some("no files rendered"));
    }

    #[test]
    fn malformed_fallback_strips_unsafe_terminal_controls() {
        let text = "diff --git incomplete\nclipboard \u{1b}]52;c;SGVsbG8=\u{7}\nclear-screen \u{1b}[2J\ndevice-control \u{1b}Pqpayload\u{1b}\\\nbell \u{7}\ncarriage\rspoof\nbackspace\u{8}spoof\nbare-escape \u{1b}\n";
        let output = render_static_diff_pager(text, &CommonOptions::default(), &[], None);
        assert_no_unsafe_terminal_controls(&strip_intentional_ansi(&output.text));
    }

    #[test]
    fn parsed_paths_and_hunk_headers_strip_unsafe_terminal_controls() {
        let payload = "\u{1b}]52;c;SGVsbG8=\u{7}\u{1b}[2J\u{1b}Pqpayload\u{1b}\\\u{1b}_payload\u{1b}\\\u{1b}^payload\u{1b}\\\u{1b}Xpayload\u{1b}\\\u{7}\rspoof\u{8}hidden\u{1b}";
        let patch = format!(
            "diff --git a/evil{payload}.ts b/evil{payload}.ts\n--- a/evil{payload}.ts\n+++ b/evil{payload}.ts\n@@ -1 +1 @@ {payload}\n-const value = 1;\n+const value = 2;\n"
        );
        let output = render_static_diff_pager(&patch, &CommonOptions::default(), &[], None);
        let unstyled = strip_intentional_ansi(&output.text);
        assert!(unstyled.contains("evil"));
        assert!(unstyled.contains("@@ -1 +1 @@"));
        assert_no_unsafe_terminal_controls(&unstyled);
    }

    #[test]
    fn parsed_diff_content_strips_unsafe_terminal_controls() {
        let patch = "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-safe\u{1b}]52;c;SGVsbG8=\u{7}\u{1b}[2J\u{1b}Pqpayload\u{1b}\\\u{7}\rspoof\u{8}hidden\u{1b}\n+const value = 2;\n";
        let output = render_static_diff_pager(patch, &CommonOptions::default(), &[], None);
        assert_no_unsafe_terminal_controls(&strip_intentional_ansi(&output.text));
    }
}
