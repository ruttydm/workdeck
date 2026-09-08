//! Terminal column math translated from Hunk's `src/ui/diff/codeColumns.ts`.

use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;
use workdeck_core::{DiffFile, DiffLineKind};

pub const DEFAULT_TAB_WIDTH: u16 = 4;
pub const MIN_TAB_WIDTH: u16 = 1;
pub const MAX_TAB_WIDTH: u16 = 16;
pub const DIFF_RAIL_PREFIX_WIDTH: usize = 1;
pub const DIFF_SPLIT_SEPARATOR_WIDTH: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellGeometry {
    pub gutter_width: usize,
    pub content_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitPaneWidths {
    pub left_width: usize,
    pub right_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeLayout {
    Split,
    Stack,
}

/// The line-number-bearing parts of Hunk's four terminal diff-row variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffRowLineNumbers {
    Collapsed {
        old_range: (u32, u32),
        new_range: (u32, u32),
    },
    HunkHeader,
    SplitLine {
        left: Option<u32>,
        right: Option<u32>,
    },
    StackLine {
        old: Option<u32>,
        new: Option<u32>,
    },
}

/// Per-render cache equivalent to Hunk's metadata-keyed weak map.
#[derive(Debug, Default)]
pub struct MaxFileCodeLineWidthCache {
    widths: HashMap<(String, u16), usize>,
}

impl MaxFileCodeLineWidthCache {
    #[must_use]
    pub fn measure(&mut self, file: &DiffFile, tab_width: u16) -> usize {
        let key = (file_width_cache_identity(file), tab_width);
        if let Some(width) = self.widths.get(&key) {
            return *width;
        }
        let width = max_file_code_line_width(file, tab_width);
        self.widths.insert(key, width);
        width
    }
}

fn validated_tab_width(tab_width: u16) -> usize {
    assert!(
        (MIN_TAB_WIDTH..=MAX_TAB_WIDTH).contains(&tab_width),
        "Invalid tab width: {tab_width} (expected {MIN_TAB_WIDTH}-{MAX_TAB_WIDTH})"
    );
    usize::from(tab_width)
}

/// Expand tabs to the next source-column stop using terminal-cell widths.
#[must_use]
pub fn expand_diff_tabs(text: &str, tab_width: u16, initial_column: usize) -> String {
    if !text.contains('\t') {
        return text.to_owned();
    }

    let tab_width = validated_tab_width(tab_width);
    let mut column = initial_column;
    let mut expanded = String::with_capacity(text.len());
    let mut segments = text.split('\t').peekable();
    while let Some(segment) = segments.next() {
        expanded.push_str(segment);
        column = column.saturating_add(segment.width());
        if segments.peek().is_some() {
            let spaces = tab_width - (column % tab_width);
            expanded.push_str(&" ".repeat(spaces));
            column = column.saturating_add(spaces);
        }
    }
    expanded
}

/// Measure one rendered code line after tab expansion and one trailing newline trim.
#[must_use]
pub fn measure_rendered_code_line_width(line: Option<&str>, tab_width: u16) -> usize {
    let line = line.unwrap_or_default();
    expand_diff_tabs(line.strip_suffix('\n').unwrap_or(line), tab_width, 0).width()
}

/// Measure the widest rendered old- or new-side line for one file.
#[must_use]
pub fn max_file_code_line_width(file: &DiffFile, tab_width: u16) -> usize {
    let old_source = file
        .sources
        .old
        .as_ref()
        .map(|source| source.content.as_str());
    let new_source = file
        .sources
        .new
        .as_ref()
        .map(|source| source.content.as_str());
    let source_width = old_source
        .into_iter()
        .chain(new_source)
        .flat_map(|source| source.split_inclusive('\n'))
        .map(|line| measure_rendered_code_line_width(Some(line), tab_width))
        .max();
    source_width.unwrap_or_else(|| {
        file.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| {
                matches!(
                    line.kind,
                    DiffLineKind::Context | DiffLineKind::Addition | DiffLineKind::Deletion
                )
            })
            .map(|line| measure_rendered_code_line_width(Some(&line.content), tab_width))
            .max()
            .unwrap_or(0)
    })
}

fn file_width_cache_identity(file: &DiffFile) -> String {
    format!(
        "{}\0{}\0{}\0{}",
        file.content_identity,
        file.source_identity.as_deref().unwrap_or_default(),
        file.path,
        file.previous_path.as_deref().unwrap_or_default()
    )
}

/// Find the widest line-number gutter needed for one parsed file.
#[must_use]
pub fn find_max_line_number(file: &DiffFile) -> u32 {
    file.hunks
        .iter()
        .flat_map(|hunk| {
            [
                hunk.old_start.saturating_add(hunk.old_count),
                hunk.new_start.saturating_add(hunk.new_count),
            ]
        })
        .max()
        .unwrap_or(0)
        .max(1)
}

/// Find the widest line-number gutter needed for an already-expanded row stream.
#[must_use]
pub fn find_max_line_number_in_rows(
    rows: impl IntoIterator<Item = DiffRowLineNumbers>,
    fallback: u32,
) -> u32 {
    rows.into_iter()
        .fold(fallback, |highest, row| {
            let row_highest = match row {
                DiffRowLineNumbers::Collapsed {
                    old_range,
                    new_range,
                } => old_range.1.max(new_range.1),
                DiffRowLineNumbers::HunkHeader => 0,
                DiffRowLineNumbers::SplitLine { left, right } => {
                    left.unwrap_or(0).max(right.unwrap_or(0))
                }
                DiffRowLineNumbers::StackLine { old, new } => {
                    old.unwrap_or(0).max(new.unwrap_or(0))
                }
            };
            highest.max(row_highest)
        })
        .max(1)
}

/// Split panes reserve one rail column on the left and one separator column in the middle.
#[must_use]
pub fn resolve_split_pane_widths(width: usize) -> SplitPaneWidths {
    let usable_width = width.saturating_sub(DIFF_RAIL_PREFIX_WIDTH + DIFF_SPLIT_SEPARATOR_WIDTH);
    let left_code_width = usable_width / 2;
    SplitPaneWidths {
        left_width: DIFF_RAIL_PREFIX_WIDTH.saturating_add(left_code_width),
        right_width: DIFF_SPLIT_SEPARATOR_WIDTH
            .saturating_add(usable_width.saturating_sub(left_code_width)),
    }
}

/// Resolve a split-cell gutter and code viewport after its rail prefix.
#[must_use]
pub fn resolve_split_cell_geometry(
    width: usize,
    line_number_digits: usize,
    show_line_numbers: bool,
    prefix_width: usize,
) -> CellGeometry {
    let available_width = width.saturating_sub(prefix_width);
    let desired_gutter = if show_line_numbers {
        line_number_digits.saturating_add(3)
    } else {
        2
    };
    let gutter_width = available_width.min(desired_gutter);
    CellGeometry {
        gutter_width,
        content_width: available_width.saturating_sub(gutter_width),
    }
}

/// Resolve a stack-cell gutter and code viewport after its left rail prefix.
#[must_use]
pub fn resolve_stack_cell_geometry(
    width: usize,
    line_number_digits: usize,
    show_line_numbers: bool,
    prefix_width: usize,
) -> CellGeometry {
    let available_width = width.saturating_sub(prefix_width);
    let desired_gutter = if show_line_numbers {
        line_number_digits.saturating_mul(2).saturating_add(5)
    } else {
        2
    };
    let gutter_width = available_width.min(desired_gutter);
    CellGeometry {
        gutter_width,
        content_width: available_width.saturating_sub(gutter_width),
    }
}

/// Clamp horizontal reveal against the narrowest code viewport in the active layout.
#[must_use]
pub fn resolve_code_viewport_width(
    layout: CodeLayout,
    width: usize,
    line_number_digits: usize,
    show_line_numbers: bool,
) -> usize {
    match layout {
        CodeLayout::Split => {
            let panes = resolve_split_pane_widths(width);
            resolve_split_cell_geometry(
                panes.left_width,
                line_number_digits,
                show_line_numbers,
                DIFF_RAIL_PREFIX_WIDTH,
            )
            .content_width
            .min(
                resolve_split_cell_geometry(
                    panes.right_width,
                    line_number_digits,
                    show_line_numbers,
                    DIFF_RAIL_PREFIX_WIDTH,
                )
                .content_width,
            )
        }
        CodeLayout::Stack => {
            resolve_stack_cell_geometry(
                width,
                line_number_digits,
                show_line_numbers,
                DIFF_RAIL_PREFIX_WIDTH,
            )
            .content_width
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{
        DiffHunk, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats, SourceOrigin,
        SourceSnapshot,
    };

    fn file_with_new_source(lines: usize, widest: &str) -> DiffFile {
        let mut content = String::new();
        for index in 0..lines {
            content.push_str(if index + 1 == lines { widest } else { "x" });
            content.push('\n');
        }
        let mut file = DiffFile {
            key: "large-untracked".into(),
            runtime_id: "large-untracked".into(),
            path: "large-untracked.txt".into(),
            previous_path: None,
            change_kind: FileChangeKind::Untracked,
            language: None,
            stats: FileStats {
                additions: lines,
                deletions: 0,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: Vec::new(),
            content_identity: "fixture".into(),
            sources: FileSourceSnapshots {
                old: None,
                new: Some(SourceSnapshot::new(
                    content,
                    SourceOrigin::File {
                        path: "large-untracked.txt".into(),
                    },
                    true,
                )),
            },
            source_identity: None,
            source_capability: None,
            source_attested: true,
            agent: None,
        };
        file.refresh_identity();
        file
    }

    #[test]
    fn expands_tabs_to_configurable_terminal_cell_stops() {
        assert_eq!(expand_diff_tabs("\tvalue", 4, 0), "    value");
        assert_eq!(expand_diff_tabs("a\tb", 4, 0), "a   b");
        assert_eq!(expand_diff_tabs("abc\td", 4, 0), "abc d");
        assert_eq!(expand_diff_tabs("日本\tx", 4, 0), "日本    x");
        assert_eq!(expand_diff_tabs("a\tb", 4, 2), "a b");
        assert_eq!(measure_rendered_code_line_width(Some("a\tb"), 8), 9);
    }

    #[test]
    fn caches_widest_lines_separately_for_each_tab_width() {
        let file = file_with_new_source(2, "\twide");
        let mut cache = MaxFileCodeLineWidthCache::default();
        assert_eq!(cache.measure(&file, 2), 6);
        assert_eq!(cache.measure(&file, 8), 12);
        assert_eq!(cache.widths.len(), 2);
    }

    #[test]
    fn measures_large_generated_fixtures_without_stack_overflow() {
        let file = file_with_new_source(100_000, "the widest generated line");
        assert_eq!(
            max_file_code_line_width(&file, DEFAULT_TAB_WIDTH),
            "the widest generated line".len()
        );
    }

    #[test]
    fn counts_wide_cjk_characters_by_terminal_cells() {
        let file = file_with_new_source(2, "日本語");
        assert_eq!(max_file_code_line_width(&file, DEFAULT_TAB_WIDTH), 6);
    }

    #[test]
    fn max_line_number_rows_include_collapsed_gap_ranges() {
        let rows = [
            DiffRowLineNumbers::SplitLine {
                left: Some(5),
                right: Some(5),
            },
            DiffRowLineNumbers::Collapsed {
                old_range: (6, 1_000),
                new_range: (6, 1_000),
            },
        ];
        assert_eq!(find_max_line_number_in_rows(rows, 1), 1_000);
    }

    #[test]
    fn max_line_number_rows_include_synthesized_stack_expansion_rows() {
        let rows = [DiffRowLineNumbers::StackLine {
            old: Some(998),
            new: Some(1_002),
        }];
        assert_eq!(find_max_line_number_in_rows(rows, 9), 1_002);
    }

    #[test]
    fn resolves_split_stack_and_narrow_viewport_geometry() {
        assert_eq!(
            resolve_split_pane_widths(20),
            SplitPaneWidths {
                left_width: 10,
                right_width: 10
            }
        );
        assert_eq!(
            resolve_split_pane_widths(21),
            SplitPaneWidths {
                left_width: 10,
                right_width: 11
            }
        );
        assert_eq!(
            resolve_split_cell_geometry(10, 3, true, 1),
            CellGeometry {
                gutter_width: 6,
                content_width: 3
            }
        );
        assert_eq!(
            resolve_stack_cell_geometry(20, 3, true, 1),
            CellGeometry {
                gutter_width: 11,
                content_width: 8
            }
        );
        assert_eq!(
            resolve_code_viewport_width(CodeLayout::Split, 20, 3, true),
            3
        );
        assert_eq!(
            resolve_code_viewport_width(CodeLayout::Stack, 20, 3, true),
            8
        );
        assert_eq!(
            resolve_split_cell_geometry(1, 8, true, 1),
            CellGeometry {
                gutter_width: 0,
                content_width: 0
            }
        );
    }

    #[test]
    fn finds_file_gutter_width_from_hunk_endpoints() {
        let mut file = file_with_new_source(1, "x");
        file.hunks.push(DiffHunk {
            index: 0,
            header: String::new(),
            context: None,
            old_start: 98,
            old_count: 2,
            new_start: 999,
            new_count: 1,
            split_row_start: 0,
            split_row_count: 0,
            stack_row_start: 0,
            stack_row_count: 0,
            lines: Vec::new(),
        });
        assert_eq!(find_max_line_number(&file), 1_000);
    }

    #[test]
    #[should_panic(expected = "Invalid tab width")]
    fn rejects_out_of_range_tab_widths() {
        let _ = expand_diff_tabs("\t", 0, 0);
    }
}
