//! Provider-neutral highlighted diff-row construction.
//!
//! This is a native Rust reimplementation of Hunk's `src/ui/diff/diffRows.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, including the behavior also shipped by the
//! pinned `v0.20.1` tree. Highlight acquisition lives in `HighlightedDiffRuntime`; this module
//! owns the pure threshold, span, word-emphasis, stable-key, and split/stack row projections.

use std::ops::Range;

use unicode_width::UnicodeWidthStr;
use workdeck_core::{DiffFile, DiffLine, DiffLineKind, ReviewGapPosition, ReviewLineMoveKind};
use workdeck_diff::{
    DiffRow, HighlightedDiffCode, HighlightedLine, RenderSpan, SplitLineCell, SplitLineKind,
    StackLineCell, StackLineKind, expand_diff_tabs, plan_split_line_pairs, sanitize_terminal_line,
    word_diff_ranges,
};
use workdeck_review::{
    ReviewGapAddress, review_gap_source_for_file, review_leading_gap, review_trailing_gap,
};

use crate::{AppTheme, resolve_word_diff_highlight_bg};

/// First per-side line count that is worth moving to the native worker.
pub const HIGHLIGHT_WORKER_MIN_LINES: usize = 40;

/// Largest two-sided diff retained for syntax highlighting.
pub const MAX_HIGHLIGHTED_DIFF_LINES: usize = 10_000;

#[must_use]
pub fn review_file_id(file: &DiffFile) -> &str {
    if file.runtime_id.is_empty() {
        &file.key
    } else {
        &file.runtime_id
    }
}

/// Count the syntax lines retained across both sides of one diff.
#[must_use]
pub fn highlighted_diff_line_count(file: &DiffFile) -> usize {
    highlighted_diff_side_counts(file)
        .into_iter()
        .sum::<usize>()
}

#[must_use]
pub fn highlighted_diff_side_counts(file: &DiffFile) -> [usize; 2] {
    file.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .fold([0, 0], |[deletions, additions], line| {
            [
                deletions.saturating_add(usize::from(line.kind != DiffLineKind::Addition)),
                additions.saturating_add(usize::from(line.kind != DiffLineKind::Deletion)),
            ]
        })
}

#[must_use]
pub fn should_highlight_diff(file: &DiffFile) -> bool {
    highlighted_diff_line_count(file) <= MAX_HIGHLIGHTED_DIFF_LINES
}

/// Return whether an interactive render may use the bundled-theme worker path.
#[must_use]
pub fn should_offload_highlight(file: &DiffFile, theme: &AppTheme, requested: bool) -> bool {
    requested
        && theme.syntax_scope_overrides.is_empty()
        && should_highlight_diff(file)
        && highlighted_diff_side_counts(file)
            .into_iter()
            .max()
            .unwrap_or(0)
            >= HIGHLIGHT_WORKER_MIN_LINES
}

fn merge_span(target: &mut Vec<RenderSpan>, next: RenderSpan) {
    if next.text.is_empty() {
        return;
    }
    if let Some(previous) = target.last_mut()
        && previous.foreground == next.foreground
        && previous.background == next.background
        && previous.transform_foreground == next.transform_foreground
    {
        previous.text.push_str(&next.text);
    } else {
        target.push(next);
    }
}

pub(crate) fn plain_diff_spans(text: &str, tab_width: u16) -> Vec<RenderSpan> {
    let text = expand_diff_tabs(&sanitize_terminal_line(text), tab_width, 0);
    if text.is_empty() {
        Vec::new()
    } else {
        vec![RenderSpan {
            text,
            foreground: None,
            background: None,
            transform_foreground: None,
        }]
    }
}

fn highlighted_spans(
    tokens: Option<&HighlightedLine>,
    text: &str,
    tab_width: u16,
) -> Vec<RenderSpan> {
    let Some(tokens) = tokens.filter(|tokens| !tokens.is_empty()) else {
        return plain_diff_spans(text, tab_width);
    };
    let mut column = 0;
    let mut spans = Vec::new();
    for token in tokens {
        let text = expand_diff_tabs(&sanitize_terminal_line(&token.text), tab_width, column);
        column = column.saturating_add(text.width());
        merge_span(
            &mut spans,
            RenderSpan {
                text,
                foreground: Some(format!(
                    "#{:02x}{:02x}{:02x}",
                    token.foreground.red, token.foreground.green, token.foreground.blue
                )),
                background: None,
                transform_foreground: None,
            },
        );
    }
    if spans.is_empty() {
        plain_diff_spans(text, tab_width)
    } else {
        spans
    }
}

fn emphasize_spans(
    spans: Vec<RenderSpan>,
    ranges: &[Range<usize>],
    background: &str,
) -> Vec<RenderSpan> {
    if ranges.is_empty() {
        return spans;
    }
    let mut result = Vec::new();
    let mut offset = 0;
    for span in spans {
        for character in span.text.chars() {
            let emphasized = ranges
                .iter()
                .any(|range| range.start <= offset && offset < range.end);
            merge_span(
                &mut result,
                RenderSpan {
                    text: character.to_string(),
                    foreground: span.foreground.clone(),
                    background: if emphasized {
                        Some(background.into())
                    } else {
                        span.background.clone()
                    },
                    transform_foreground: span.transform_foreground.clone(),
                },
            );
            offset += character.len_utf8();
        }
    }
    result
}

fn word_emphasis_background(kind: DiffLineKind, theme: &AppTheme) -> &str {
    match kind {
        DiffLineKind::Deletion => &theme.removed_content_bg,
        DiffLineKind::Addition => &theme.added_content_bg,
        DiffLineKind::Context => &theme.context_content_bg,
    }
}

fn resolved_word_emphasis_background(kind: DiffLineKind, theme: &AppTheme) -> String {
    match kind {
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
        DiffLineKind::Context => word_emphasis_background(kind, theme).into(),
    }
}

/// Convert one highlighted full-source line into the spans used by expanded context rows.
#[must_use]
pub fn spans_for_highlighted_source_line(
    raw_line: Option<&str>,
    highlighted_line: Option<&HighlightedLine>,
    _theme: &AppTheme,
    tab_width: u16,
) -> Vec<RenderSpan> {
    highlighted_spans(highlighted_line, raw_line.unwrap_or_default(), tab_width)
}

fn split_cell(
    kind: SplitLineKind,
    line: Option<&DiffLine>,
    highlighted: Option<&HighlightedLine>,
    emphasis: &[Range<usize>],
    theme: &AppTheme,
    tab_width: u16,
) -> SplitLineCell {
    let sign = match kind {
        SplitLineKind::Addition => "+",
        SplitLineKind::Deletion => "-",
        SplitLineKind::Context | SplitLineKind::Empty => " ",
    };
    let spans = line.map_or_else(Vec::new, |line| {
        let spans = highlighted_spans(highlighted, &line.content, tab_width);
        let background = resolved_word_emphasis_background(line.kind, theme);
        emphasize_spans(
            spans,
            if highlighted.is_some() { emphasis } else { &[] },
            &background,
        )
    });
    SplitLineCell {
        kind,
        sign: sign.into(),
        line_number: line
            .and_then(|line| match kind {
                SplitLineKind::Addition => line.new_line,
                SplitLineKind::Deletion | SplitLineKind::Context => line.old_line,
                SplitLineKind::Empty => None,
            })
            .map(|line| line as usize),
        move_kind: line_move_kind(line),
        spans,
    }
}

fn stack_cell(
    kind: StackLineKind,
    line: &DiffLine,
    highlighted: Option<&HighlightedLine>,
    emphasis: &[Range<usize>],
    theme: &AppTheme,
    tab_width: u16,
) -> StackLineCell {
    let sign = match kind {
        StackLineKind::Addition => "+",
        StackLineKind::Deletion => "-",
        StackLineKind::Context => " ",
    };
    let spans = highlighted_spans(highlighted, &line.content, tab_width);
    let background = resolved_word_emphasis_background(line.kind, theme);
    StackLineCell {
        kind,
        sign: sign.into(),
        old_line_number: line.old_line.map(|line| line as usize),
        new_line_number: line.new_line.map(|line| line as usize),
        move_kind: line_move_kind(Some(line)),
        spans: emphasize_spans(
            spans,
            if highlighted.is_some() { emphasis } else { &[] },
            &background,
        ),
    }
}

fn line_move_kind(line: Option<&DiffLine>) -> Option<ReviewLineMoveKind> {
    line.filter(|line| line.moved)
        .map(|_| ReviewLineMoveKind::Moved)
}

fn collapsed_gap_row(file: &DiffFile, address: ReviewGapAddress, stack: bool) -> DiffRow {
    let suffix = if address.position == ReviewGapPosition::Trailing {
        "trailing".into()
    } else {
        address.hunk_index.to_string()
    };
    let prefix = if stack {
        "stack:collapsed:"
    } else {
        "collapsed:"
    };
    let noun = if address.line_count == 1 {
        "line"
    } else {
        "lines"
    };
    DiffRow::Collapsed {
        key: format!("{}:{prefix}{suffix}", review_file_id(file)),
        file_id: review_file_id(file).into(),
        hunk_index: address.hunk_index,
        text: format!("{} unchanged {noun}", address.line_count),
        position: address.position,
        old_range: [
            address.old_range.start as usize,
            address.old_range.end as usize,
        ],
        new_range: [
            address.new_range.start as usize,
            address.new_range.end as usize,
        ],
    }
}

fn highlighted_line(
    highlighted: Option<&HighlightedDiffCode>,
    hunk_index: usize,
    line_index: usize,
) -> Option<&workdeck_diff::HighlightedDiffLine> {
    highlighted?.highlighted.get(hunk_index)?.get(line_index)
}

fn hunk_side_counts(lines: &[DiffLine]) -> [usize; 2] {
    lines.iter().fold([0, 0], |[deletions, additions], line| {
        [
            deletions + usize::from(line.kind != DiffLineKind::Addition),
            additions + usize::from(line.kind != DiffLineKind::Deletion),
        ]
    })
}

/// Expand one file into the flat split-view row stream consumed by planning and paint.
#[must_use]
pub fn build_split_rows(
    file: &DiffFile,
    highlighted: Option<&HighlightedDiffCode>,
    theme: &AppTheme,
    tab_width: u16,
) -> Vec<DiffRow> {
    let id = review_file_id(file);
    let gap_source = review_gap_source_for_file(file);
    let mut rows = Vec::new();
    for (hunk_index, hunk) in file.hunks.iter().enumerate() {
        if !hunk.lines.is_empty()
            && let Some(gap) = review_leading_gap(&gap_source, hunk_index)
        {
            rows.push(collapsed_gap_row(file, gap, false));
        }
        rows.push(DiffRow::HunkHeader {
            key: format!("{id}:header:{hunk_index}"),
            file_id: id.into(),
            hunk_index,
            text: hunk.formatted_header(),
        });

        let mut deletion_index = hunk.old_start.saturating_sub(1) as usize;
        let mut addition_index = hunk.new_start.saturating_sub(1) as usize;
        let mut cursor = 0;
        while cursor < hunk.lines.len() {
            if hunk.lines[cursor].kind == DiffLineKind::Context {
                let line = &hunk.lines[cursor];
                let highlight = highlighted_line(highlighted, hunk_index, cursor);
                rows.push(DiffRow::SplitLine {
                    key: format!(
                        "{id}:split:{hunk_index}:context:{deletion_index}:{addition_index}"
                    ),
                    file_id: id.into(),
                    hunk_index,
                    left: split_cell(
                        SplitLineKind::Context,
                        Some(line),
                        highlight.and_then(|line| line.deletion.as_ref()),
                        &[],
                        theme,
                        tab_width,
                    ),
                    right: {
                        let mut cell = split_cell(
                            SplitLineKind::Context,
                            Some(line),
                            highlight
                                .and_then(|line| line.addition.as_ref())
                                .or_else(|| highlight.and_then(|line| line.deletion.as_ref())),
                            &[],
                            theme,
                            tab_width,
                        );
                        cell.line_number = line.new_line.map(|line| line as usize);
                        cell
                    },
                    is_expansion_row: false,
                    expanded_gap_key: None,
                });
                cursor += 1;
                deletion_index += 1;
                addition_index += 1;
                continue;
            }

            let block_start = cursor;
            while cursor < hunk.lines.len() && hunk.lines[cursor].kind != DiffLineKind::Context {
                cursor += 1;
            }
            let block = &hunk.lines[block_start..cursor];
            let pairs = plan_split_line_pairs(block);
            for (offset, pair) in pairs.into_iter().enumerate() {
                let old = pair
                    .old_index
                    .map(|index| (block_start + index, &block[index]));
                let new = pair
                    .new_index
                    .map(|index| (block_start + index, &block[index]));
                let emphasis = old.zip(new).map(|((_, old), (_, new))| {
                    word_diff_ranges(
                        &expand_diff_tabs(&sanitize_terminal_line(&old.content), tab_width, 0),
                        &expand_diff_tabs(&sanitize_terminal_line(&new.content), tab_width, 0),
                    )
                });
                rows.push(DiffRow::SplitLine {
                    key: format!(
                        "{id}:split:{hunk_index}:change:{}:{}",
                        deletion_index + offset,
                        addition_index + offset
                    ),
                    file_id: id.into(),
                    hunk_index,
                    left: old.map_or_else(
                        || split_cell(SplitLineKind::Empty, None, None, &[], theme, tab_width),
                        |(line_index, line)| {
                            split_cell(
                                SplitLineKind::Deletion,
                                Some(line),
                                highlighted_line(highlighted, hunk_index, line_index)
                                    .and_then(|line| line.deletion.as_ref()),
                                emphasis.as_ref().map_or(&[], |ranges| &ranges.old),
                                theme,
                                tab_width,
                            )
                        },
                    ),
                    right: new.map_or_else(
                        || split_cell(SplitLineKind::Empty, None, None, &[], theme, tab_width),
                        |(line_index, line)| {
                            split_cell(
                                SplitLineKind::Addition,
                                Some(line),
                                highlighted_line(highlighted, hunk_index, line_index)
                                    .and_then(|line| line.addition.as_ref()),
                                emphasis.as_ref().map_or(&[], |ranges| &ranges.new),
                                theme,
                                tab_width,
                            )
                        },
                    ),
                    is_expansion_row: false,
                    expanded_gap_key: None,
                });
            }
            let [deletions, additions] = hunk_side_counts(block);
            deletion_index += deletions;
            addition_index += additions;
        }
    }
    if let Some(gap) = review_trailing_gap(&gap_source) {
        rows.push(collapsed_gap_row(file, gap, false));
    }
    rows
}

/// Expand one file into the flat stack-view row stream consumed by planning and paint.
#[must_use]
pub fn build_stack_rows(
    file: &DiffFile,
    highlighted: Option<&HighlightedDiffCode>,
    theme: &AppTheme,
    tab_width: u16,
) -> Vec<DiffRow> {
    let id = review_file_id(file);
    let gap_source = review_gap_source_for_file(file);
    let mut rows = Vec::new();
    for (hunk_index, hunk) in file.hunks.iter().enumerate() {
        if !hunk.lines.is_empty()
            && let Some(gap) = review_leading_gap(&gap_source, hunk_index)
        {
            rows.push(collapsed_gap_row(file, gap, true));
        }
        rows.push(DiffRow::HunkHeader {
            key: format!("{id}:stack:header:{hunk_index}"),
            file_id: id.into(),
            hunk_index,
            text: hunk.formatted_header(),
        });
        let mut emphasis = vec![Vec::new(); hunk.lines.len()];
        for pair in plan_split_line_pairs(&hunk.lines) {
            let (Some(old_index), Some(new_index)) = (pair.old_index, pair.new_index) else {
                continue;
            };
            if old_index == new_index {
                continue;
            }
            let ranges = word_diff_ranges(
                &expand_diff_tabs(
                    &sanitize_terminal_line(&hunk.lines[old_index].content),
                    tab_width,
                    0,
                ),
                &expand_diff_tabs(
                    &sanitize_terminal_line(&hunk.lines[new_index].content),
                    tab_width,
                    0,
                ),
            );
            emphasis[old_index] = ranges.old;
            emphasis[new_index] = ranges.new;
        }
        let mut deletion_index = hunk.old_start.saturating_sub(1) as usize;
        let mut addition_index = hunk.new_start.saturating_sub(1) as usize;
        for (hunk_line_index, line) in hunk.lines.iter().enumerate() {
            let highlight = highlighted_line(highlighted, hunk_index, hunk_line_index);
            let (label, index, kind, tokens) = match line.kind {
                DiffLineKind::Context => (
                    "context",
                    deletion_index,
                    StackLineKind::Context,
                    highlight.and_then(|line| line.addition.as_ref().or(line.deletion.as_ref())),
                ),
                DiffLineKind::Deletion => (
                    "deletion",
                    deletion_index,
                    StackLineKind::Deletion,
                    highlight.and_then(|line| line.deletion.as_ref()),
                ),
                DiffLineKind::Addition => (
                    "addition",
                    addition_index,
                    StackLineKind::Addition,
                    highlight.and_then(|line| line.addition.as_ref()),
                ),
            };
            let key = if line.kind == DiffLineKind::Context {
                format!("{id}:stack:{hunk_index}:{label}:{deletion_index}:{addition_index}")
            } else {
                format!("{id}:stack:{hunk_index}:{label}:{index}")
            };
            rows.push(DiffRow::StackLine {
                key,
                file_id: id.into(),
                hunk_index,
                cell: stack_cell(
                    kind,
                    line,
                    tokens,
                    &emphasis[hunk_line_index],
                    theme,
                    tab_width,
                ),
                is_expansion_row: false,
                expanded_gap_key: None,
            });
            deletion_index += usize::from(line.kind != DiffLineKind::Addition);
            addition_index += usize::from(line.kind != DiffLineKind::Deletion);
        }
    }
    if let Some(gap) = review_trailing_gap(&gap_source) {
        rows.push(collapsed_gap_row(file, gap, true));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{ChangesetSource, FileSourceSnapshots, SourceOrigin, SourceSnapshot};
    use workdeck_diff::{
        DEFAULT_TAB_WIDTH, FileComparisonOptions, FileSnapshot, HighlightAppearance,
        HighlightCache, HighlightedDiffLine, SyntaxColor, SyntaxToken, diff_from_file_snapshots,
        parse_patch,
    };

    use crate::resolve_theme;

    fn file(before: &str, after: &str, id: &str, context_radius: usize) -> DiffFile {
        let mut file = diff_from_file_snapshots(
            FileSnapshot {
                cache_key: "before",
                contents: before,
                name: "fixture.ts",
            },
            FileSnapshot {
                cache_key: "after",
                contents: after,
                name: "fixture.ts",
            },
            FileComparisonOptions { context_radius },
        )
        .unwrap();
        file.runtime_id = id.into();
        file.flags.partial = false;
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                before.into(),
                SourceOrigin::File {
                    path: "fixture.ts".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                after.into(),
                SourceOrigin::File {
                    path: "fixture.ts".into(),
                },
                true,
            )),
        });
        file
    }

    fn token(text: &str, red: u8) -> SyntaxToken {
        SyntaxToken {
            text: text.into(),
            foreground: SyntaxColor {
                red,
                green: 2,
                blue: 3,
            },
            bold: false,
            italic: false,
            underline: false,
        }
    }

    fn highlighted(file: &DiffFile) -> HighlightedDiffCode {
        let hunks = file
            .hunks
            .iter()
            .map(|hunk| {
                hunk.lines
                    .iter()
                    .map(|line| HighlightedDiffLine {
                        deletion: (line.kind != DiffLineKind::Addition)
                            .then(|| vec![token(&line.content, 10)]),
                        addition: (line.kind != DiffLineKind::Deletion)
                            .then(|| vec![token(&line.content, 20)]),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let [deletions, additions] = highlighted_diff_side_counts(file);
        HighlightedDiffCode::new(hunks, deletions, additions)
    }

    #[test]
    fn highlight_thresholds_match_generated_and_worker_boundaries() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let below = (0..39)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let at = (0..40)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let below = file("", &format!("{below}\n"), "below", 0);
        let at = file("", &format!("{at}\n"), "at", 0);
        assert!(!should_offload_highlight(&below, &theme, true));
        assert!(should_offload_highlight(&at, &theme, true));
        assert!(!should_offload_highlight(&at, &theme, false));
        let mut custom = theme.clone();
        custom
            .syntax_scope_overrides
            .push(("keyword".into(), "#abcdef".into()));
        assert!(!should_offload_highlight(&at, &custom, true));

        let too_large_text = "generated\n".repeat(MAX_HIGHLIGHTED_DIFF_LINES + 1);
        let too_large = file("", &too_large_text, "large", 0);
        assert!(!should_highlight_diff(&too_large));
        assert!(!should_offload_highlight(&too_large, &theme, true));
    }

    #[test]
    fn oversized_source_plan_falls_back_to_the_visible_patch() {
        let mut file = parse_patch(
            concat!(
                "diff --git a/large.ts b/large.ts\n",
                "--- a/large.ts\n",
                "+++ b/large.ts\n",
                "@@ -10001 +10001 @@\n",
                "-const answer = 41;\n",
                "+const answer = 42;\n"
            ),
            "large-source",
            "large-source",
            ChangesetSource::Patch {
                label: "large-source".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        let prefix = "// prefix\n".repeat(MAX_HIGHLIGHTED_DIFF_LINES);
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                format!("{prefix}const answer = 41;\n"),
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                format!("{prefix}const answer = 42;\n"),
                SourceOrigin::WorkingTree,
                true,
            )),
        });
        let highlighted = HighlightCache::default().highlight(&file, "github-dark-default");
        assert_eq!(highlighted.len(), 1);
        assert_eq!(highlighted[0].len(), 2);
        assert_eq!(
            highlighted[0][0]
                .deletion
                .as_ref()
                .unwrap()
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "const answer = 41;"
        );
        assert_eq!(
            highlighted[0][1]
                .addition
                .as_ref()
                .unwrap()
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "const answer = 42;"
        );
    }

    #[test]
    fn split_rows_pair_changes_preserve_keys_tabs_emphasis_and_empty_cells() {
        let file = file(
            "keep\nold\tvalue\n",
            "keep\nnew\tvalue\nextra\n",
            "split",
            3,
        );
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let highlighted = highlighted(&file);
        let rows = build_split_rows(&file, Some(&highlighted), &theme, 4);
        let changes = rows
            .iter()
            .filter_map(|row| match row {
                DiffRow::SplitLine {
                    key, left, right, ..
                } if left.kind != SplitLineKind::Context => Some((key, left, right)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(changes.len(), 2);
        assert!(changes[0].0.ends_with(":change:1:1"));
        assert!(
            changes[0]
                .1
                .spans
                .iter()
                .any(|span| span.background.is_some())
        );
        assert!(
            changes[0]
                .2
                .spans
                .iter()
                .any(|span| span.background.is_some())
        );
        assert_eq!(
            changes[0]
                .1
                .spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "old value"
        );
        assert_eq!(changes[1].1.kind, SplitLineKind::Empty);
        assert_eq!(changes[1].2.kind, SplitLineKind::Addition);
    }

    #[test]
    fn stack_rows_separate_changes_carry_moves_and_strip_highlight_newlines() {
        let mut file = file("old\n", "new\n", "stack", 3);
        file.hunks[0].lines[0].moved = true;
        file.hunks[0].lines[1].moved = true;
        let highlighted = HighlightedDiffCode::new(
            vec![vec![
                HighlightedDiffLine {
                    deletion: Some(vec![token("old\n", 1)]),
                    addition: None,
                },
                HighlightedDiffLine {
                    deletion: None,
                    addition: Some(vec![token("new\n", 4)]),
                },
            ]],
            1,
            1,
        );
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let rows = build_stack_rows(&file, Some(&highlighted), &theme, DEFAULT_TAB_WIDTH);
        let cells = rows
            .iter()
            .filter_map(|row| match row {
                DiffRow::StackLine { cell, .. } => Some(cell),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].kind, StackLineKind::Deletion);
        assert_eq!(cells[1].kind, StackLineKind::Addition);
        assert_eq!(cells[0].move_kind, Some(ReviewLineMoveKind::Moved));
        assert!(
            cells
                .iter()
                .flat_map(|cell| &cell.spans)
                .all(|span| !span.text.contains('\n'))
        );
    }

    #[test]
    fn highlighted_source_spans_use_syntax_and_plain_fallbacks() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let highlighted = vec![token("export\t", 10), token("value", 20)];
        let spans =
            spans_for_highlighted_source_line(Some("ignored"), Some(&highlighted), &theme, 4);
        assert_eq!(
            spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "export  value"
        );
        assert!(spans.iter().all(|span| span.foreground.is_some()));
        assert_eq!(
            spans_for_highlighted_source_line(Some("plain\tline\n"), None, &theme, 4)[0].text,
            "plain   line"
        );
    }

    #[test]
    fn collapsed_rows_keep_exact_ranges_and_layout_specific_keys() {
        let before = (1..=8)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let after = before.replace("line 4", "changed 4");
        let file = file(&before, &after, "gaps", 0);
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let split = build_split_rows(&file, None, &theme, DEFAULT_TAB_WIDTH);
        let stack = build_stack_rows(&file, None, &theme, DEFAULT_TAB_WIDTH);
        let split_gaps = split
            .iter()
            .filter(|row| matches!(row, DiffRow::Collapsed { .. }))
            .count();
        let stack_keys = stack
            .iter()
            .filter_map(|row| match row {
                DiffRow::Collapsed {
                    key,
                    old_range,
                    new_range,
                    ..
                } => Some((key, old_range, new_range)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(split_gaps, 2);
        assert_eq!(stack_keys.len(), 2);
        assert!(
            stack_keys
                .iter()
                .all(|(key, _, _)| key.contains(":stack:collapsed:"))
        );
        assert!(
            stack_keys
                .iter()
                .all(|(_, old, new)| old[1] >= old[0] && new[1] >= new[0])
        );
    }

    #[test]
    fn transparent_word_emphasis_stays_transparent() {
        let mut theme = resolve_theme(Some("github-dark-default"), None, &[]);
        theme.added_content_bg = "transparent".into();
        theme.removed_content_bg = "transparent".into();
        let file = file("old\n", "new\n", "transparent", 3);
        let highlighted = highlighted(&file);
        let rows = build_split_rows(&file, Some(&highlighted), &theme, DEFAULT_TAB_WIDTH);
        let backgrounds = rows
            .iter()
            .flat_map(|row| match row {
                DiffRow::SplitLine { left, right, .. } => left
                    .spans
                    .iter()
                    .chain(right.spans.iter())
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .filter_map(|span| span.background.as_deref())
            .collect::<Vec<_>>();
        assert!(!backgrounds.is_empty());
        assert!(
            backgrounds
                .iter()
                .all(|background| *background == "transparent")
        );
    }

    #[test]
    fn native_rows_match_the_frozen_baseline_highlight_vector() {
        let file = file(
            "export const answer = 41;\nexport const stable = true;\n",
            concat!(
                "export const answer = 42;\n",
                "export const stable = true;\n",
                "export const added = true;\n"
            ),
            "example",
            3,
        );
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut cache = HighlightCache::default();
        let highlighted = cache.highlight_with_syntax_theme(
            &file,
            HighlightAppearance::Dark,
            theme.syntax_theme.as_deref(),
            &theme.syntax_scope_overrides,
        );
        let [deletions, additions] = highlighted_diff_side_counts(&file);
        let highlighted = HighlightedDiffCode::new(highlighted, deletions, additions);
        let split = build_split_rows(&file, Some(&highlighted), &theme, 4);
        assert_eq!(
            split
                .iter()
                .map(|row| match row {
                    DiffRow::Collapsed { key, .. }
                    | DiffRow::HunkHeader { key, .. }
                    | DiffRow::SplitLine { key, .. }
                    | DiffRow::StackLine { key, .. } => key.as_str(),
                })
                .collect::<Vec<_>>(),
            [
                "example:header:0",
                "example:split:0:change:0:0",
                "example:split:0:context:1:1",
                "example:split:0:change:2:2",
            ]
        );
        let DiffRow::SplitLine { left, right, .. } = &split[1] else {
            panic!("expected the paired changed row");
        };
        assert_eq!(
            left.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "export const answer = 41;"
        );
        assert_eq!(
            right
                .spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "export const answer = 42;"
        );
        assert_eq!(
            left.spans
                .iter()
                .find(|span| span.text == "41")
                .and_then(|span| span.background.as_deref()),
            Some("#4f2325")
        );
        assert_eq!(
            right
                .spans
                .iter()
                .find(|span| span.text == "42")
                .and_then(|span| span.background.as_deref()),
            Some("#163923")
        );
        assert_eq!(left.spans[0].foreground.as_deref(), Some("#ff7b72"));

        let stack = build_stack_rows(&file, Some(&highlighted), &theme, 4);
        assert_eq!(
            stack
                .iter()
                .map(|row| match row {
                    DiffRow::Collapsed { key, .. }
                    | DiffRow::HunkHeader { key, .. }
                    | DiffRow::SplitLine { key, .. }
                    | DiffRow::StackLine { key, .. } => key.as_str(),
                })
                .collect::<Vec<_>>(),
            [
                "example:stack:header:0",
                "example:stack:0:deletion:0",
                "example:stack:0:addition:0",
                "example:stack:0:context:1:1",
                "example:stack:0:addition:2",
            ]
        );
    }

    #[test]
    fn frozen_diff_rows_oracle_maps_both_pins_and_every_source_test() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/diff-rows.json"))
                .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineOracle"]["passed"], 29);
        assert_eq!(oracle["stableOracle"]["passed"], 29);
        assert_eq!(oracle["baselineOracle"]["expectations"], 134);
        let mappings = oracle["testMapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 29);
        let mut next_byte = 0;
        for mapping in mappings {
            assert_eq!(mapping["bytes"][0].as_u64().unwrap(), next_byte);
            next_byte = mapping["bytes"][1].as_u64().unwrap();
            assert!(!mapping["rustTests"].as_array().unwrap().is_empty());
        }
        assert_eq!(next_byte, 45_416);
        assert_eq!(
            oracle["projectionVectors"]["shared"]["splitKeys"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            oracle["projectionVectors"]["baselineWordBackgrounds"]["addition"],
            "#163923"
        );
        assert_eq!(
            oracle["projectionVectors"]["stableWordBackgrounds"]["addition"],
            "#1c4428"
        );
    }
}
