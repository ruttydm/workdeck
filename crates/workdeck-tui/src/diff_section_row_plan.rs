//! File-level review-row planning.
//!
//! This is a Rust reimplementation of Hunk's `src/ui/diff/diffSectionRowPlan.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. It deliberately owns only the composition
//! boundary: provider-neutral row construction and expansion remain reusable inputs to the
//! canonical note/hunk planner.

use std::collections::HashSet;

use workdeck_core::{DiffFile, ReviewGapPosition};
use workdeck_diff::{
    DEFAULT_TAB_WIDTH, DiffRow, DiffRowLineNumbers, HighlightedDiffCode, RenderSpan, SplitLineCell,
    SplitLineKind, StackLineCell, StackLineKind, find_max_line_number,
    find_max_line_number_in_rows, sanitize_terminal_line,
};
use workdeck_review::{
    ExpandedGapState, ExpandedSourceStatus, LayoutMode, plan_expanded_gap, review_expansion_side,
    review_gap_id, review_gap_source_for_file, review_leading_gap, review_trailing_gap,
};

use crate::{
    AppTheme, DEFAULT_HUNK_GAP, PlannedReviewRow, ReviewRenderPlanOptions, VisibleAgentNote,
    build_review_render_plan, build_split_rows, build_stack_rows, plain_diff_spans,
};

pub type SourceLineSpans<'a> = dyn Fn(Option<&str>, usize) -> Vec<RenderSpan> + Send + Sync + 'a;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSectionRowPlan {
    pub line_number_digits: usize,
    pub planned_rows: Vec<PlannedReviewRow>,
}

pub struct BuildDiffSectionRowPlanOptions<'a> {
    pub expanded_keys: &'a HashSet<String>,
    pub file: Option<&'a DiffFile>,
    pub highlighted_diff: Option<&'a HighlightedDiffCode>,
    pub layout: LayoutMode,
    pub show_hunk_headers: bool,
    pub source_line_spans: Option<&'a SourceLineSpans<'a>>,
    pub source_status: ExpandedSourceStatus<'a>,
    pub tab_width: u16,
    pub hunk_gap: usize,
    pub theme: &'a AppTheme,
    pub visible_agent_notes: &'a [VisibleAgentNote],
}

impl<'a> BuildDiffSectionRowPlanOptions<'a> {
    #[must_use]
    pub fn new(file: Option<&'a DiffFile>, layout: LayoutMode, theme: &'a AppTheme) -> Self {
        Self {
            expanded_keys: empty_expanded_keys(),
            file,
            highlighted_diff: None,
            layout,
            show_hunk_headers: true,
            source_line_spans: None,
            source_status: ExpandedSourceStatus::Pending,
            tab_width: DEFAULT_TAB_WIDTH,
            hunk_gap: DEFAULT_HUNK_GAP,
            theme,
            visible_agent_notes: &[],
        }
    }
}

fn empty_expanded_keys() -> &'static HashSet<String> {
    static EMPTY: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(HashSet::new);
    &EMPTY
}

fn file_id(file: &DiffFile) -> &str {
    if file.runtime_id.is_empty() {
        &file.key
    } else {
        &file.runtime_id
    }
}

fn sanitized_source_spans(spans: Vec<RenderSpan>) -> Vec<RenderSpan> {
    spans
        .into_iter()
        .filter_map(|mut span| {
            span.text = sanitize_terminal_line(&span.text);
            (!span.text.is_empty()).then_some(span)
        })
        .collect()
}

fn expanded_context_row(
    row: &DiffRow,
    line: &workdeck_review::ExpandedGapLine,
    layout: LayoutMode,
    spans: Vec<RenderSpan>,
) -> DiffRow {
    let (file_id, hunk_index, position) = match row {
        DiffRow::Collapsed {
            file_id,
            hunk_index,
            position,
            ..
        } => (file_id.clone(), *hunk_index, *position),
        _ => unreachable!("only collapsed rows expand"),
    };
    match layout {
        LayoutMode::Split => {
            let cell = |line_number| SplitLineCell {
                kind: SplitLineKind::Context,
                sign: " ".into(),
                line_number: Some(line_number),
                move_kind: None,
                spans: spans.clone(),
            };
            DiffRow::SplitLine {
                key: line.key.clone(),
                file_id,
                hunk_index,
                left: cell(line.old_line as usize),
                right: cell(line.new_line as usize),
                is_expansion_row: true,
                expanded_gap_key: Some(review_gap_id(position, hunk_index)),
            }
        }
        LayoutMode::Stack => DiffRow::StackLine {
            key: line.key.clone(),
            file_id,
            hunk_index,
            cell: StackLineCell {
                kind: StackLineKind::Context,
                sign: " ".into(),
                old_line_number: Some(line.old_line as usize),
                new_line_number: Some(line.new_line as usize),
                move_kind: None,
                spans,
            },
            is_expansion_row: true,
            expanded_gap_key: Some(review_gap_id(position, hunk_index)),
        },
        LayoutMode::Auto => unreachable!("section row planning requires a resolved layout"),
    }
}

fn expand_collapsed_rows(
    rows: Vec<DiffRow>,
    file: &DiffFile,
    options: &BuildDiffSectionRowPlanOptions<'_>,
) -> Vec<DiffRow> {
    if options.expanded_keys.is_empty() {
        return rows;
    }
    let gap_source = review_gap_source_for_file(file);
    let side = review_expansion_side(file.change_kind);
    let mut result = Vec::new();
    for row in rows {
        let DiffRow::Collapsed {
            position,
            hunk_index,
            ..
        } = &row
        else {
            result.push(row);
            continue;
        };
        let gap_key = review_gap_id(*position, *hunk_index);
        if !options.expanded_keys.contains(&gap_key) {
            result.push(row);
            continue;
        }
        let address = match position {
            ReviewGapPosition::Before => review_leading_gap(&gap_source, *hunk_index),
            ReviewGapPosition::Trailing => review_trailing_gap(&gap_source),
        };
        let Some(address) = address else {
            result.push(row);
            continue;
        };
        let plan = plan_expanded_gap(file_id(file), address, true, options.source_status, side);
        let mut state_row = row.clone();
        if let DiffRow::Collapsed { text, .. } = &mut state_row {
            *text = plan.label;
        }
        result.push(state_row);
        if plan.state != ExpandedGapState::Expanded {
            continue;
        }
        for line in plan.lines {
            let spans = options.source_line_spans.map_or_else(
                || plain_diff_spans(&line.text, options.tab_width),
                |resolve| sanitized_source_spans(resolve(Some(&line.text), line.source_line_index)),
            );
            result.push(expanded_context_row(&row, &line, options.layout, spans));
        }
    }
    result
}

fn row_line_numbers(row: &DiffRow) -> DiffRowLineNumbers {
    match row {
        DiffRow::Collapsed {
            old_range,
            new_range,
            ..
        } => DiffRowLineNumbers::Collapsed {
            old_range: (old_range[0] as u32, old_range[1] as u32),
            new_range: (new_range[0] as u32, new_range[1] as u32),
        },
        DiffRow::HunkHeader { .. } => DiffRowLineNumbers::HunkHeader,
        DiffRow::SplitLine { left, right, .. } => DiffRowLineNumbers::SplitLine {
            left: left.line_number.map(|line| line as u32),
            right: right.line_number.map(|line| line as u32),
        },
        DiffRow::StackLine { cell, .. } => DiffRowLineNumbers::StackLine {
            old: cell.old_line_number.map(|line| line as u32),
            new: cell.new_line_number.map(|line| line as u32),
        },
    }
}

/// Build the shared file-level diff plan consumed by rendering and geometry measurement.
#[must_use]
pub fn build_diff_section_row_plan(
    options: BuildDiffSectionRowPlanOptions<'_>,
) -> DiffSectionRowPlan {
    let Some(file) = options.file else {
        return DiffSectionRowPlan {
            line_number_digits: 1,
            planned_rows: Vec::new(),
        };
    };
    assert_ne!(
        options.layout,
        LayoutMode::Auto,
        "section row planning requires a resolved layout"
    );
    let base_rows = match options.layout {
        LayoutMode::Split => build_split_rows(
            file,
            options.highlighted_diff,
            options.theme,
            options.tab_width,
        ),
        LayoutMode::Stack => build_stack_rows(
            file,
            options.highlighted_diff,
            options.theme,
            options.tab_width,
        ),
        LayoutMode::Auto => unreachable!(),
    };
    let rows = expand_collapsed_rows(base_rows, file, &options);
    let highest = find_max_line_number_in_rows(
        rows.iter().map(row_line_numbers),
        find_max_line_number(file),
    );
    DiffSectionRowPlan {
        line_number_digits: highest.to_string().len(),
        planned_rows: build_review_render_plan(ReviewRenderPlanOptions {
            file_id: file_id(file),
            rows: &rows,
            show_hunk_headers: options.show_hunk_headers,
            visible_agent_notes: options.visible_agent_notes,
            selected_hunk_index: None,
            hunk_gap: options.hunk_gap,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{FileSourceSnapshots, SourceOrigin, SourceSnapshot};
    use workdeck_diff::{
        FileComparisonOptions, FileSnapshot, HighlightedDiffLine, SyntaxColor, SyntaxToken,
        diff_from_file_snapshots,
    };

    use crate::resolve_theme;

    fn file(before: &str, after: &str, id: &str) -> DiffFile {
        let mut file = diff_from_file_snapshots(
            FileSnapshot {
                cache_key: "before",
                contents: before,
                name: "fixture.txt",
            },
            FileSnapshot {
                cache_key: "after",
                contents: after,
                name: "fixture.txt",
            },
            FileComparisonOptions { context_radius: 0 },
        )
        .unwrap();
        file.runtime_id = id.into();
        file.flags.partial = false;
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                before.into(),
                SourceOrigin::File {
                    path: "fixture.txt".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                after.into(),
                SourceOrigin::File {
                    path: "fixture.txt".into(),
                },
                true,
            )),
        });
        file
    }

    fn plan<'a>(
        file: Option<&'a DiffFile>,
        layout: LayoutMode,
        theme: &'a AppTheme,
    ) -> BuildDiffSectionRowPlanOptions<'a> {
        BuildDiffSectionRowPlanOptions::new(file, layout, theme)
    }

    #[test]
    fn missing_files_return_the_one_digit_empty_plan() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        assert_eq!(
            build_diff_section_row_plan(plan(None, LayoutMode::Split, &theme)),
            DiffSectionRowPlan {
                line_number_digits: 1,
                planned_rows: Vec::new(),
            }
        );
    }

    #[test]
    fn composes_exact_split_stack_keys_line_digits_and_hunk_gaps() {
        let before = concat!(
            "const alpha = 1;\n",
            "const beta = 2;\n",
            "const gamma = 3;\n",
            "const stable = true;\n"
        );
        let after = concat!(
            "const alpha = 10;\n",
            "const beta = 2;\n",
            "const gamma = 30;\n",
            "const stable = true;\n"
        );
        let file = file(before, after, "example");
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let split = build_diff_section_row_plan(plan(Some(&file), LayoutMode::Split, &theme));
        assert_eq!(split.line_number_digits, 1);
        assert_eq!(
            split
                .planned_rows
                .iter()
                .map(PlannedReviewRow::key)
                .collect::<Vec<_>>(),
            [
                "diff-row:example:header:0",
                "diff-row:example:split:0:change:0:0",
                "diff-row:example:collapsed:1",
                "diff-row:example:header:1",
                "diff-row:example:split:1:change:2:2",
                "diff-row:example:collapsed:trailing",
            ]
        );
        let mut stack_options = plan(Some(&file), LayoutMode::Stack, &theme);
        stack_options.hunk_gap = 2;
        let stack = build_diff_section_row_plan(stack_options);
        assert_eq!(stack.planned_rows.len(), 9);
        assert!(matches!(
            &stack.planned_rows[4],
            PlannedReviewRow::HunkGap {
                key,
                height: 2,
                ..
            } if key == "hunk-gap:example:1"
        ));
    }

    #[test]
    fn preserves_highlighted_spans_and_falls_back_to_sanitized_tabbed_text() {
        let file = file("old\n", "new\n", "highlighted");
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let token = |text: &str, red| SyntaxToken {
            text: text.into(),
            foreground: SyntaxColor {
                red,
                green: 2,
                blue: 3,
            },
            bold: false,
            italic: false,
            underline: false,
        };
        let highlighted = HighlightedDiffCode::new(
            vec![vec![
                HighlightedDiffLine {
                    deletion: Some(vec![token("old", 1)]),
                    addition: None,
                },
                HighlightedDiffLine {
                    deletion: None,
                    addition: Some(vec![token("n\tew", 4)]),
                },
            ]],
            1,
            1,
        );
        let mut options = plan(Some(&file), LayoutMode::Stack, &theme);
        options.highlighted_diff = Some(&highlighted);
        let rows = build_diff_section_row_plan(options).planned_rows;
        let spans = rows
            .iter()
            .filter_map(PlannedReviewRow::diff_row)
            .filter_map(|row| match row {
                DiffRow::StackLine { cell, .. } => Some(&cell.spans),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(spans[0][0].foreground.as_deref(), Some("#010203"));
        assert!(
            spans[1]
                .iter()
                .all(|span| span.foreground.as_deref() == Some("#040203"))
        );
        assert_eq!(
            spans[1]
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "n   ew"
        );
    }

    #[test]
    fn expansion_statuses_and_source_span_callback_flow_into_the_shared_plan() {
        let before = (1..=12)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before.replace("line 5\n", "line 5 modified\n");
        let file = file(&before, &after, "expand");
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let keys = HashSet::from(["trailing:0".into()]);
        let collapsed_text = |rows: &[PlannedReviewRow]| {
            rows.iter().find_map(|row| match row.diff_row()? {
                DiffRow::Collapsed { text, .. } => Some(text.clone()),
                _ => None,
            })
        };

        let mut loading_options = plan(Some(&file), LayoutMode::Split, &theme);
        loading_options.expanded_keys = &keys;
        loading_options.source_status = ExpandedSourceStatus::Loading;
        let loading = build_diff_section_row_plan(loading_options);
        assert_eq!(
            collapsed_text(&loading.planned_rows),
            Some("4 unchanged lines".into())
        );
        assert!(loading.planned_rows.iter().any(|row| {
            matches!(
                row.diff_row(),
                Some(DiffRow::Collapsed { text, .. }) if text == "Loading 7 unchanged lines…"
            )
        }));

        let mut error_options = plan(Some(&file), LayoutMode::Split, &theme);
        error_options.expanded_keys = &keys;
        error_options.source_status =
            ExpandedSourceStatus::Error(workdeck_review::ExpandedSourceError::TooLarge);
        let error = build_diff_section_row_plan(error_options);
        assert!(error.planned_rows.iter().any(|row| {
            matches!(
                row.diff_row(),
                Some(DiffRow::Collapsed { text, .. })
                    if text == "Source too large to expand 7 unchanged lines"
            )
        }));

        let resolve = |line: Option<&str>, _source_line: usize| {
            vec![RenderSpan {
                text: format!("\u{1b}]52;c;bad\u{7}{}", line.unwrap_or_default()),
                foreground: Some("#abcdef".into()),
                background: None,
                transform_foreground: None,
            }]
        };
        let mut loaded_options = plan(Some(&file), LayoutMode::Split, &theme);
        loaded_options.expanded_keys = &keys;
        loaded_options.source_status = ExpandedSourceStatus::Loaded(&after);
        loaded_options.source_line_spans = Some(&resolve);
        let loaded = build_diff_section_row_plan(loaded_options);
        assert_eq!(loaded.line_number_digits, 2);
        let expanded = loaded
            .planned_rows
            .iter()
            .find_map(|row| match row.diff_row()? {
                DiffRow::SplitLine {
                    left,
                    is_expansion_row: true,
                    expanded_gap_key,
                    ..
                } => Some((left, expanded_gap_key)),
                _ => None,
            })
            .expect("loaded source produces synthesized rows");
        assert_eq!(expanded.0.spans[0].text, "line 6");
        assert_eq!(expanded.0.spans[0].foreground.as_deref(), Some("#abcdef"));
        assert_eq!(expanded.1.as_deref(), Some("trailing:0"));
    }

    #[test]
    #[should_panic(expected = "section row planning requires a resolved layout")]
    fn auto_layout_must_be_resolved_before_section_planning() {
        let file = file("old\n", "new\n", "auto");
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let _ = build_diff_section_row_plan(plan(Some(&file), LayoutMode::Auto, &theme));
    }
}
