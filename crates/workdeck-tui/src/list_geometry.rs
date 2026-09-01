//! Fixed-row list and sidebar window planning shared by Ratatui surfaces.

use std::collections::BTreeSet;

pub const SIDEBAR_ROW_HEIGHT: usize = 1;

/// Center a selected fixed-height row when possible and pin the window at its ends.
#[must_use]
pub fn list_window_start(selected_index: usize, row_count: usize, visible_rows: usize) -> usize {
    if row_count <= visible_rows {
        return 0;
    }

    selected_index
        .saturating_sub(visible_rows / 2)
        .min(row_count.saturating_sub(visible_rows))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarWindowEntryKind {
    Group,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarWindowEntry {
    pub id: String,
    pub kind: SidebarWindowEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarRenderWindowItem {
    Entry {
        entry_index: usize,
    },
    Spacer {
        height: usize,
        start_index: usize,
        end_index: usize,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarRenderWindowPlan {
    pub items: Vec<SidebarRenderWindowItem>,
    pub mounted_entry_indices: Vec<usize>,
    pub visible_start_index: Option<usize>,
    pub visible_end_index: Option<usize>,
    pub top_spacer_height: usize,
    pub bottom_spacer_height: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct SidebarRenderWindowOptions<'a> {
    pub estimated_viewport_rows: i64,
    pub overscan_rows: i64,
    pub scroll_top: i64,
    pub selected_file_id: Option<&'a str>,
    pub viewport_height: i64,
}

impl Default for SidebarRenderWindowOptions<'_> {
    fn default() -> Self {
        Self {
            estimated_viewport_rows: 32,
            overscan_rows: 4,
            scroll_top: 0,
            selected_file_id: None,
            viewport_height: 0,
        }
    }
}

/// Build a sparse fixed-row sidebar plan while retaining its exact scroll extent.
#[must_use]
pub fn build_sidebar_render_window(
    entries: &[SidebarWindowEntry],
    options: SidebarRenderWindowOptions<'_>,
) -> SidebarRenderWindowPlan {
    let effective_viewport_height = if options.viewport_height > 0 {
        options.viewport_height
    } else {
        options.estimated_viewport_rows.max(0)
    };
    let visible_range =
        find_visible_entry_range(entries.len(), options.scroll_top, effective_viewport_height);
    let overscan = options.overscan_rows.max(0) as usize;
    let mut mounted = BTreeSet::new();

    if let Some((start, end)) = visible_range {
        add_entry_index_range(
            &mut mounted,
            start.saturating_sub(overscan),
            end.saturating_add(overscan),
            entries.len(),
        );
    }

    if let Some(selected_file_id) = options.selected_file_id
        && let Some(selected_index) = entries.iter().position(|entry| {
            entry.kind == SidebarWindowEntryKind::File && entry.id == selected_file_id
        })
    {
        mounted.insert(selected_index);
    }

    let mounted_entry_indices = mounted.into_iter().collect::<Vec<_>>();
    let mut items = Vec::new();
    let mut cursor = 0;
    let mut top_spacer_height = 0;
    let mut bottom_spacer_height = 0;

    for &index in &mounted_entry_indices {
        if index > cursor {
            push_spacer(
                &mut items,
                cursor,
                index - 1,
                entries.len(),
                &mut top_spacer_height,
                &mut bottom_spacer_height,
            );
        }
        items.push(SidebarRenderWindowItem::Entry { entry_index: index });
        cursor = index + 1;
    }

    if cursor < entries.len() {
        push_spacer(
            &mut items,
            cursor,
            entries.len() - 1,
            entries.len(),
            &mut top_spacer_height,
            &mut bottom_spacer_height,
        );
    }

    if mounted_entry_indices.is_empty() && !entries.is_empty() {
        let full_height = entry_range_height(0, entries.len() - 1);
        top_spacer_height = full_height;
        bottom_spacer_height = full_height;
    }

    SidebarRenderWindowPlan {
        items,
        mounted_entry_indices,
        visible_start_index: visible_range.map(|range| range.0),
        visible_end_index: visible_range.map(|range| range.1),
        top_spacer_height,
        bottom_spacer_height,
    }
}

fn find_visible_entry_range(
    entry_count: usize,
    scroll_top: i64,
    viewport_height: i64,
) -> Option<(usize, usize)> {
    if entry_count == 0 || viewport_height <= 0 {
        return None;
    }

    let min_y = scroll_top.max(0) as usize;
    let max_y = min_y.saturating_add(viewport_height.max(0) as usize);
    let total_height = entry_count.saturating_mul(SIDEBAR_ROW_HEIGHT);
    if min_y >= total_height || max_y == 0 {
        return None;
    }

    let start = (min_y / SIDEBAR_ROW_HEIGHT).min(entry_count - 1);
    let end = max_y
        .div_ceil(SIDEBAR_ROW_HEIGHT)
        .saturating_sub(1)
        .max(start)
        .min(entry_count - 1);
    Some((start, end))
}

fn add_entry_index_range(
    indices: &mut BTreeSet<usize>,
    start_index: usize,
    end_index: usize,
    count: usize,
) {
    let Some(end) = count.checked_sub(1).map(|last| end_index.min(last)) else {
        return;
    };
    for index in start_index..=end {
        indices.insert(index);
    }
}

const fn entry_range_height(start_index: usize, end_index: usize) -> usize {
    if start_index > end_index {
        0
    } else {
        (end_index - start_index + 1) * SIDEBAR_ROW_HEIGHT
    }
}

fn push_spacer(
    items: &mut Vec<SidebarRenderWindowItem>,
    start_index: usize,
    end_index: usize,
    entry_count: usize,
    top_spacer_height: &mut usize,
    bottom_spacer_height: &mut usize,
) {
    let height = entry_range_height(start_index, end_index);
    if height == 0 {
        return;
    }
    if start_index == 0 {
        *top_spacer_height += height;
    }
    if end_index == entry_count.saturating_sub(1) {
        *bottom_spacer_height += height;
    }
    items.push(SidebarRenderWindowItem::Spacer {
        height,
        start_index,
        end_index,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(ids: &[&str]) -> Vec<SidebarWindowEntry> {
        ids.iter()
            .map(|id| SidebarWindowEntry {
                id: (*id).into(),
                kind: if id.starts_with("group:") {
                    SidebarWindowEntryKind::Group
                } else {
                    SidebarWindowEntryKind::File
                },
            })
            .collect()
    }

    fn options(scroll_top: i64, viewport_height: i64) -> SidebarRenderWindowOptions<'static> {
        SidebarRenderWindowOptions {
            overscan_rows: 0,
            scroll_top,
            viewport_height,
            ..SidebarRenderWindowOptions::default()
        }
    }

    fn rendered_height(items: &[SidebarRenderWindowItem]) -> usize {
        items
            .iter()
            .map(|item| match item {
                SidebarRenderWindowItem::Entry { .. } => SIDEBAR_ROW_HEIGHT,
                SidebarRenderWindowItem::Spacer { height, .. } => *height,
            })
            .sum()
    }

    #[test]
    fn list_window_shows_the_whole_list_when_it_fits() {
        assert_eq!(list_window_start(0, 3, 5), 0);
        assert_eq!(list_window_start(2, 3, 5), 0);
    }

    #[test]
    fn list_window_centers_the_selection_once_it_scrolls() {
        assert_eq!(list_window_start(5, 20, 5), 3);
    }

    #[test]
    fn list_window_pins_at_both_ends() {
        assert_eq!(list_window_start(0, 20, 5), 0);
        assert_eq!(list_window_start(19, 20, 5), 15);
    }

    #[test]
    fn empty_sidebar_has_an_empty_plan() {
        assert_eq!(
            build_sidebar_render_window(&[], options(0, 10)),
            SidebarRenderWindowPlan::default()
        );
    }

    #[test]
    fn first_viewport_reserves_the_remaining_height() {
        let entries = entries(&["file-0", "file-1", "file-2", "file-3", "file-4"]);
        let plan = build_sidebar_render_window(&entries, options(0, 3));
        assert_eq!(plan.mounted_entry_indices, [0, 1, 2]);
        assert_eq!(
            (plan.visible_start_index, plan.visible_end_index),
            (Some(0), Some(2))
        );
        assert_eq!(plan.top_spacer_height, 0);
        assert_eq!(plan.bottom_spacer_height, 2);
        assert_eq!(
            plan.items,
            [
                SidebarRenderWindowItem::Entry { entry_index: 0 },
                SidebarRenderWindowItem::Entry { entry_index: 1 },
                SidebarRenderWindowItem::Entry { entry_index: 2 },
                SidebarRenderWindowItem::Spacer {
                    height: 2,
                    start_index: 3,
                    end_index: 4,
                },
            ]
        );
        assert_eq!(rendered_height(&plan.items), entries.len());
    }

    #[test]
    fn overscan_clamps_at_the_last_viewport() {
        let entries = entries(&["file-0", "file-1", "file-2", "file-3", "file-4"]);
        let mut last = options(3, 2);
        last.overscan_rows = 1;
        let plan = build_sidebar_render_window(&entries, last);
        assert_eq!(plan.mounted_entry_indices, [2, 3, 4]);
        assert_eq!(
            (plan.visible_start_index, plan.visible_end_index),
            (Some(3), Some(4))
        );
        assert_eq!(plan.top_spacer_height, 2);
        assert_eq!(plan.bottom_spacer_height, 0);
        assert_eq!(
            plan.items,
            [
                SidebarRenderWindowItem::Spacer {
                    height: 2,
                    start_index: 0,
                    end_index: 1,
                },
                SidebarRenderWindowItem::Entry { entry_index: 2 },
                SidebarRenderWindowItem::Entry { entry_index: 3 },
                SidebarRenderWindowItem::Entry { entry_index: 4 },
            ]
        );
        assert_eq!(rendered_height(&plan.items), entries.len());
    }

    #[test]
    fn adds_row_level_overscan_around_the_visible_range() {
        let entries = entries(&["file-0", "file-1", "file-2", "file-3", "file-4", "file-5"]);
        let mut middle = options(2, 1);
        middle.overscan_rows = 2;
        let plan = build_sidebar_render_window(&entries, middle);
        assert_eq!(plan.visible_start_index, Some(2));
        assert_eq!(plan.visible_end_index, Some(2));
        assert_eq!(plan.mounted_entry_indices, [0, 1, 2, 3, 4]);
        assert_eq!(plan.bottom_spacer_height, 1);
        assert_eq!(
            plan.items.last(),
            Some(&SidebarRenderWindowItem::Spacer {
                height: 1,
                start_index: 5,
                end_index: 5,
            })
        );
        assert_eq!(rendered_height(&plan.items), entries.len());
    }

    #[test]
    fn selected_file_outside_the_viewport_remains_a_sparse_island() {
        let entries = entries(&["file-0", "file-1", "file-2", "file-3", "file-4"]);
        let mut options = options(0, 2);
        options.selected_file_id = Some("file-4");
        let plan = build_sidebar_render_window(&entries, options);
        assert_eq!(plan.mounted_entry_indices, [0, 1, 4]);
        assert_eq!(
            plan.items,
            [
                SidebarRenderWindowItem::Entry { entry_index: 0 },
                SidebarRenderWindowItem::Entry { entry_index: 1 },
                SidebarRenderWindowItem::Spacer {
                    height: 2,
                    start_index: 2,
                    end_index: 3,
                },
                SidebarRenderWindowItem::Entry { entry_index: 4 },
            ]
        );
        assert_eq!(rendered_height(&plan.items), entries.len());
    }

    #[test]
    fn filtered_entry_list_ignores_a_selected_file_that_is_filtered_out() {
        let entries = entries(&["group:src", "file-1", "file-3"]);
        let mut options = options(0, 2);
        options.selected_file_id = Some("file-9");
        let plan = build_sidebar_render_window(&entries, options);
        assert_eq!(plan.mounted_entry_indices, [0, 1]);
        assert_eq!(
            plan.items,
            [
                SidebarRenderWindowItem::Entry { entry_index: 0 },
                SidebarRenderWindowItem::Entry { entry_index: 1 },
                SidebarRenderWindowItem::Spacer {
                    height: 1,
                    start_index: 2,
                    end_index: 2,
                },
            ]
        );
        assert_eq!(rendered_height(&plan.items), entries.len());
    }

    #[test]
    fn sparse_group_and_file_islands_preserve_spacers_and_total_height() {
        let entries = entries(&[
            "group:src",
            "file-0",
            "file-1",
            "group:test",
            "file-2",
            "file-3",
            "file-4",
        ]);
        let mut options = options(1, 1);
        options.selected_file_id = Some("file-4");
        let plan = build_sidebar_render_window(&entries, options);
        assert_eq!(plan.mounted_entry_indices, [1, 6]);
        assert_eq!(
            plan.items,
            [
                SidebarRenderWindowItem::Spacer {
                    height: 1,
                    start_index: 0,
                    end_index: 0,
                },
                SidebarRenderWindowItem::Entry { entry_index: 1 },
                SidebarRenderWindowItem::Spacer {
                    height: 4,
                    start_index: 2,
                    end_index: 5,
                },
                SidebarRenderWindowItem::Entry { entry_index: 6 },
            ]
        );
        assert_eq!(plan.top_spacer_height, 1);
        assert_eq!(rendered_height(&plan.items), entries.len());
    }

    #[test]
    fn viewport_beyond_rows_preserves_the_full_extent_as_one_spacer() {
        let entries = entries(&["file-0", "file-1"]);
        let plan = build_sidebar_render_window(&entries, options(99, 10));
        assert!(plan.mounted_entry_indices.is_empty());
        assert_eq!(
            (plan.visible_start_index, plan.visible_end_index),
            (None, None)
        );
        assert_eq!(
            plan.items,
            [SidebarRenderWindowItem::Spacer {
                height: 2,
                start_index: 0,
                end_index: 1,
            }]
        );
        assert_eq!(plan.top_spacer_height, 2);
        assert_eq!(plan.bottom_spacer_height, 2);
    }
}
