//! Sparse file-level mounting plans preserving exact review-stream extent.

use crate::FileSectionLayout;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRenderWindowItem {
    File {
        file_id: String,
        section_index: usize,
    },
    Spacer {
        key: String,
        height: i64,
        start_index: usize,
        end_index: usize,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileRenderWindowPlan {
    pub items: Vec<FileRenderWindowItem>,
    pub mounted_file_indices: Vec<usize>,
    pub visible_start_index: Option<usize>,
    pub visible_end_index: Option<usize>,
    pub top_spacer_height: i64,
    pub bottom_spacer_height: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct FileRenderWindowOptions<'a> {
    pub include_file_ids: &'a [&'a str],
    pub overscan_files: i64,
    pub scroll_top: i64,
    pub selected_file_id: Option<&'a str>,
    pub viewport_height: i64,
}

impl Default for FileRenderWindowOptions<'_> {
    fn default() -> Self {
        Self {
            include_file_ids: &[],
            overscan_files: 2,
            scroll_top: 0,
            selected_file_id: None,
            viewport_height: 0,
        }
    }
}

#[must_use]
pub fn build_file_section_index_by_id(layouts: &[FileSectionLayout]) -> BTreeMap<String, usize> {
    layouts
        .iter()
        .enumerate()
        .map(|(index, layout)| (layout.file_id.clone(), index))
        .collect()
}

fn visible_index_range(
    layouts: &[FileSectionLayout],
    scroll_top: i64,
    viewport_height: i64,
) -> Option<(usize, usize)> {
    if layouts.is_empty() {
        return None;
    }
    let min_y = scroll_top.max(0);
    let max_y = min_y.saturating_add(viewport_height.max(0));
    let mut low = 0;
    let mut high = layouts.len();
    while low < high {
        let mid = low + (high - low) / 2;
        if layouts[mid].section_bottom >= min_y {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    if low >= layouts.len() {
        return None;
    }
    let start = low;
    let mut end = start.checked_sub(1);
    for (index, layout) in layouts.iter().enumerate().skip(start) {
        if layout.section_top > max_y {
            break;
        }
        end = Some(index);
    }
    end.map(|end| (start, end))
}

fn add_index_range(indices: &mut BTreeSet<usize>, start: i64, end: i64, count: usize) {
    let start = start.max(0) as usize;
    let end = usize::try_from(end)
        .unwrap_or_default()
        .min(count.saturating_sub(1));
    if start <= end && start < count {
        indices.extend(start..=end);
    }
}

fn section_range_height(layouts: &[FileSectionLayout], start: usize, end: usize) -> i64 {
    if start > end {
        return 0;
    }
    let (Some(start), Some(end)) = (layouts.get(start), layouts.get(end)) else {
        return 0;
    };
    end.section_bottom.saturating_sub(start.section_top).max(0)
}

#[must_use]
pub fn build_file_render_window(
    layouts: &[FileSectionLayout],
    options: FileRenderWindowOptions<'_>,
) -> FileRenderWindowPlan {
    build_file_render_window_with_index(layouts, &build_file_section_index_by_id(layouts), options)
}

#[must_use]
pub fn build_file_render_window_with_index(
    layouts: &[FileSectionLayout],
    index_by_file_id: &BTreeMap<String, usize>,
    options: FileRenderWindowOptions<'_>,
) -> FileRenderWindowPlan {
    let visible = visible_index_range(layouts, options.scroll_top, options.viewport_height);
    let overscan = options.overscan_files.max(0);
    let mut mounted = BTreeSet::new();
    if let Some((start, end)) = visible {
        add_index_range(
            &mut mounted,
            start as i64 - overscan,
            end as i64 + overscan,
            layouts.len(),
        );
    }
    if let Some(selected) = options.selected_file_id
        && let Some(index) = index_by_file_id.get(selected)
    {
        mounted.insert(*index);
    }
    for file_id in options.include_file_ids {
        if let Some(index) = index_by_file_id.get(*file_id) {
            mounted.insert(*index);
        }
    }

    let mounted_file_indices = mounted.into_iter().collect::<Vec<_>>();
    let mut plan = FileRenderWindowPlan {
        mounted_file_indices: mounted_file_indices.clone(),
        visible_start_index: visible.map(|range| range.0),
        visible_end_index: visible.map(|range| range.1),
        ..FileRenderWindowPlan::default()
    };
    let mut cursor = 0;
    for &index in &mounted_file_indices {
        if index > cursor {
            push_spacer(&mut plan, layouts, cursor, index - 1);
        }
        let layout = &layouts[index];
        plan.items.push(FileRenderWindowItem::File {
            file_id: layout.file_id.clone(),
            section_index: index,
        });
        cursor = index + 1;
    }
    if cursor < layouts.len() {
        push_spacer(&mut plan, layouts, cursor, layouts.len() - 1);
    }
    if mounted_file_indices.is_empty() && !layouts.is_empty() {
        let full_height = section_range_height(layouts, 0, layouts.len() - 1);
        plan.top_spacer_height = full_height;
        plan.bottom_spacer_height = full_height;
    }
    plan
}

fn push_spacer(
    plan: &mut FileRenderWindowPlan,
    layouts: &[FileSectionLayout],
    start_index: usize,
    end_index: usize,
) {
    let height = section_range_height(layouts, start_index, end_index);
    if height <= 0 {
        return;
    }
    if start_index == 0 {
        plan.top_spacer_height += height;
    }
    if end_index == layouts.len().saturating_sub(1) {
        plan.bottom_spacer_height += height;
    }
    plan.items.push(FileRenderWindowItem::Spacer {
        key: format!("file-spacer:{start_index}:{end_index}"),
        height,
        start_index,
        end_index,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layouts(count: usize, body_height: i64) -> Vec<FileSectionLayout> {
        let mut result = Vec::new();
        let mut cursor = 0;
        for index in 0..count {
            let separator = i64::from(index > 0);
            let header = i64::from(index > 0);
            let section_top = cursor;
            let header_top = section_top + separator;
            let body_top = header_top + header;
            let section_bottom = body_top + body_height;
            result.push(FileSectionLayout {
                file_id: format!("file-{index}"),
                section_index: index,
                section_top,
                header_top,
                body_top,
                body_height,
                section_bottom,
            });
            cursor = section_bottom;
        }
        result
    }

    fn options(scroll_top: i64, viewport_height: i64) -> FileRenderWindowOptions<'static> {
        FileRenderWindowOptions {
            scroll_top,
            viewport_height,
            ..FileRenderWindowOptions::default()
        }
    }

    fn summary(items: &[FileRenderWindowItem]) -> Vec<(String, i64, usize, usize)> {
        items
            .iter()
            .map(|item| match item {
                FileRenderWindowItem::File { section_index, .. } => {
                    ("file".into(), 0, *section_index, *section_index)
                }
                FileRenderWindowItem::Spacer {
                    height,
                    start_index,
                    end_index,
                    ..
                } => ("spacer".into(), *height, *start_index, *end_index),
            })
            .collect()
    }

    fn rendered_height(plan: &FileRenderWindowPlan, layouts: &[FileSectionLayout]) -> i64 {
        plan.items
            .iter()
            .map(|item| match item {
                FileRenderWindowItem::File { section_index, .. } => {
                    layouts[*section_index].section_bottom - layouts[*section_index].section_top
                }
                FileRenderWindowItem::Spacer { height, .. } => *height,
            })
            .sum()
    }

    #[test]
    fn empty_layouts_produce_an_empty_plan() {
        assert_eq!(
            build_file_render_window(&[], options(0, 10)),
            FileRenderWindowPlan::default()
        );
    }

    #[test]
    fn zero_height_viewport_at_top_mounts_first_file_and_overscan() {
        let layouts = layouts(6, 8);
        let plan = build_file_render_window(
            &layouts,
            FileRenderWindowOptions {
                overscan_files: 1,
                ..options(0, 0)
            },
        );
        assert_eq!(plan.mounted_file_indices, [0, 1]);
        assert_eq!(plan.visible_start_index, Some(0));
        assert_eq!(plan.visible_end_index, Some(0));
    }

    #[test]
    fn first_visible_file_reserves_exact_bottom_extent() {
        let layouts = layouts(4, 10);
        let plan = build_file_render_window(
            &layouts,
            FileRenderWindowOptions {
                overscan_files: 0,
                ..options(0, 5)
            },
        );
        assert_eq!(plan.mounted_file_indices, [0]);
        assert_eq!(plan.visible_start_index, Some(0));
        assert_eq!(plan.visible_end_index, Some(0));
        assert_eq!(plan.top_spacer_height, 0);
        assert_eq!(plan.bottom_spacer_height, 36);
        assert_eq!(
            summary(&plan.items),
            [("file".into(), 0, 0, 0), ("spacer".into(), 36, 1, 3)]
        );
        assert_eq!(
            rendered_height(&plan, &layouts),
            layouts.last().unwrap().section_bottom
        );
    }

    #[test]
    fn overscan_keeps_exact_top_and_bottom_spacers() {
        let layouts = layouts(5, 10);
        let plan = build_file_render_window(
            &layouts,
            FileRenderWindowOptions {
                overscan_files: 1,
                ..options(layouts[2].body_top, 3)
            },
        );
        assert_eq!(plan.mounted_file_indices, [1, 2, 3]);
        assert_eq!(
            (plan.visible_start_index, plan.visible_end_index),
            (Some(2), Some(2))
        );
        assert_eq!(
            (plan.top_spacer_height, plan.bottom_spacer_height),
            (10, 12)
        );
        assert_eq!(
            summary(&plan.items),
            [
                ("spacer".into(), 10, 0, 0),
                ("file".into(), 0, 1, 1),
                ("file".into(), 0, 2, 2),
                ("file".into(), 0, 3, 3),
                ("spacer".into(), 12, 4, 4),
            ]
        );
        assert_eq!(
            rendered_height(&plan, &layouts),
            layouts.last().unwrap().section_bottom
        );
    }

    #[test]
    fn selected_file_outside_viewport_remains_a_sparse_island() {
        let layouts = layouts(5, 10);
        let plan = build_file_render_window(
            &layouts,
            FileRenderWindowOptions {
                overscan_files: 0,
                selected_file_id: Some("file-4"),
                ..options(0, 4)
            },
        );
        assert_eq!(plan.mounted_file_indices, [0, 4]);
        assert_eq!(
            summary(&plan.items),
            [
                ("file".into(), 0, 0, 0),
                ("spacer".into(), 36, 1, 3),
                ("file".into(), 0, 4, 4),
            ]
        );
        assert_eq!(
            rendered_height(&plan, &layouts),
            layouts.last().unwrap().section_bottom
        );
    }

    #[test]
    fn explicit_prefetch_files_remain_sparse_islands() {
        let layouts = layouts(6, 10);
        let plan = build_file_render_window(
            &layouts,
            FileRenderWindowOptions {
                include_file_ids: &["file-5"],
                overscan_files: 0,
                ..options(layouts[2].body_top, 2)
            },
        );
        assert_eq!(plan.mounted_file_indices, [2, 5]);
        assert_eq!(
            summary(&plan.items),
            [
                ("spacer".into(), 22, 0, 1),
                ("file".into(), 0, 2, 2),
                ("spacer".into(), 24, 3, 4),
                ("file".into(), 0, 5, 5),
            ]
        );
        assert_eq!(
            rendered_height(&plan, &layouts),
            layouts.last().unwrap().section_bottom
        );
    }

    #[test]
    fn overscan_clamps_at_last_file() {
        let layouts = layouts(3, 10);
        let plan = build_file_render_window(
            &layouts,
            FileRenderWindowOptions {
                overscan_files: 2,
                ..options(layouts[2].body_top, 3)
            },
        );
        assert_eq!(plan.mounted_file_indices, [0, 1, 2]);
        assert_eq!((plan.top_spacer_height, plan.bottom_spacer_height), (0, 0));
        assert_eq!(
            rendered_height(&plan, &layouts),
            layouts.last().unwrap().section_bottom
        );
    }

    #[test]
    fn viewport_beyond_stream_preserves_full_extent_as_one_spacer() {
        let layouts = layouts(2, 10);
        let plan = build_file_render_window(
            &layouts,
            FileRenderWindowOptions {
                overscan_files: 0,
                ..options(999, 10)
            },
        );
        assert!(plan.mounted_file_indices.is_empty());
        assert_eq!(
            (plan.visible_start_index, plan.visible_end_index),
            (None, None)
        );
        assert_eq!(summary(&plan.items), [("spacer".into(), 22, 0, 1)]);
        assert_eq!(
            (plan.top_spacer_height, plan.bottom_spacer_height),
            (22, 22)
        );
        assert_eq!(
            rendered_height(&plan, &layouts),
            layouts.last().unwrap().section_bottom
        );
    }

    #[test]
    fn supplied_index_uses_last_duplicate_and_ignores_unknown_ids() {
        let mut layouts = layouts(3, 10);
        layouts[2].file_id = "file-0".into();
        let index = build_file_section_index_by_id(&layouts);
        assert_eq!(index["file-0"], 2);
        let plan = build_file_render_window_with_index(
            &layouts,
            &index,
            FileRenderWindowOptions {
                include_file_ids: &["missing"],
                overscan_files: 0,
                selected_file_id: Some("file-0"),
                ..options(999, 1)
            },
        );
        assert_eq!(plan.mounted_file_indices, [2]);
    }

    #[test]
    fn native_sparse_plan_matches_both_executed_pinned_oracles() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/file-render-window.json"
        ))
        .unwrap();
        let layouts = layouts(6, 10);
        assert_eq!(
            serde_json::json!(
                layouts
                    .iter()
                    .map(|layout| layout.section_bottom)
                    .collect::<Vec<_>>()
            ),
            oracle["sharedProjection"]["sectionBottoms"]
        );
        let plan = build_file_render_window(
            &layouts,
            FileRenderWindowOptions {
                include_file_ids: &["file-5"],
                overscan_files: 0,
                selected_file_id: Some("file-0"),
                ..options(layouts[2].body_top, 2)
            },
        );
        let items = plan
            .items
            .iter()
            .map(|item| match item {
                FileRenderWindowItem::File {
                    file_id,
                    section_index,
                } => serde_json::json!({
                    "kind": "file",
                    "fileId": file_id,
                    "sectionIndex": section_index,
                }),
                FileRenderWindowItem::Spacer {
                    key,
                    height,
                    start_index,
                    end_index,
                } => serde_json::json!({
                    "kind": "spacer",
                    "key": key,
                    "height": height,
                    "startIndex": start_index,
                    "endIndex": end_index,
                }),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::Value::Array(items),
            oracle["sharedProjection"]["sparse"]["items"]
        );
        assert_eq!(
            serde_json::json!(plan.mounted_file_indices),
            oracle["sharedProjection"]["sparse"]["mountedFileIndices"]
        );
        assert_eq!(
            serde_json::json!(plan.visible_start_index),
            oracle["sharedProjection"]["sparse"]["visibleStartIndex"]
        );
        assert_eq!(
            serde_json::json!(plan.visible_end_index),
            oracle["sharedProjection"]["sparse"]["visibleEndIndex"]
        );
    }
}
