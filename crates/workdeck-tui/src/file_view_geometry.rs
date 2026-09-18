use std::collections::HashMap;

use workdeck_extension_api::ValidatedFileViewLayout;
use workdeck_review::{LayoutMode, PlannedFileViewRow};

use crate::measure_agent_inline_note_height;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileViewRowBounds {
    pub key: String,
    pub stable_key: String,
    pub stable_keys: Vec<String>,
    pub top: usize,
    pub height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFileViewHunkBounds {
    pub top: usize,
    pub height: usize,
    pub start_row_id: String,
    pub end_row_id: String,
}

/// Host-owned scrolling, note, and hunk geometry for one alternate presentation.
#[derive(Debug)]
pub struct FileViewGeometry<'a> {
    pub body_height: usize,
    pub hunk_anchor_rows: HashMap<usize, usize>,
    pub hunk_bounds: HashMap<usize, PlannedFileViewHunkBounds>,
    pub line_number_digits: usize,
    pub file_view_rows: &'a [PlannedFileViewRow],
    pub row_bounds: Vec<FileViewRowBounds>,
    row_bounds_by_key: HashMap<String, usize>,
    row_bounds_by_stable_key: HashMap<String, usize>,
}

impl FileViewGeometry<'_> {
    #[must_use]
    pub fn bounds_for_key(&self, key: &str) -> Option<&FileViewRowBounds> {
        self.row_bounds_by_key
            .get(key)
            .and_then(|index| self.row_bounds.get(*index))
    }

    #[must_use]
    pub fn bounds_for_stable_key(&self, key: &str) -> Option<&FileViewRowBounds> {
        self.row_bounds_by_stable_key
            .get(key)
            .and_then(|index| self.row_bounds.get(*index))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PlanExtent {
    anchor: Option<usize>,
    first: Option<usize>,
    last: Option<usize>,
}

/// Measure alternate presentation rows at the same width used by Ratatui painting.
#[must_use]
pub fn measure_file_view_geometry<'a>(
    resolved: &ValidatedFileViewLayout,
    planned_rows: &'a [PlannedFileViewRow],
    width: usize,
) -> FileViewGeometry<'a> {
    let mut row_bounds = Vec::with_capacity(planned_rows.len());
    let mut row_bounds_by_key = HashMap::with_capacity(planned_rows.len());
    let mut row_bounds_by_stable_key = HashMap::with_capacity(planned_rows.len());
    let mut body_height = 0_usize;

    for row in planned_rows {
        let (stable_keys, height) = match row {
            PlannedFileViewRow::FileViewRow {
                stable_key,
                stable_alias_keys,
                row_index,
                ..
            } => {
                let mut keys = Vec::with_capacity(stable_alias_keys.len().saturating_add(1));
                keys.push(stable_key.clone());
                keys.extend(stable_alias_keys.iter().cloned());
                let height = *resolved
                    .row_heights
                    .get(*row_index)
                    .expect("validated file-view row has a retained height");
                (keys, height)
            }
            PlannedFileViewRow::InlineNote {
                stable_key,
                annotation,
                anchor_side,
                note,
                ..
            } => (
                vec![stable_key.clone()],
                measure_agent_inline_note_height(
                    annotation,
                    Some(*anchor_side),
                    LayoutMode::Stack,
                    width,
                    note.thread_depth,
                ),
            ),
        };
        let bounds = FileViewRowBounds {
            key: row.key().to_owned(),
            stable_key: row.stable_key().to_owned(),
            stable_keys,
            top: body_height,
            height,
        };
        let bounds_index = row_bounds.len();
        row_bounds_by_key.insert(bounds.key.clone(), bounds_index);
        for stable_key in &bounds.stable_keys {
            row_bounds_by_stable_key
                .entry(stable_key.clone())
                .or_insert(bounds_index);
        }
        body_height = body_height.saturating_add(height);
        row_bounds.push(bounds);
    }

    let mut extents = vec![PlanExtent::default(); resolved.layout.rows.len()];
    for (plan_index, row) in planned_rows.iter().enumerate() {
        let (row_index, is_anchor) = match row {
            PlannedFileViewRow::FileViewRow { row_index, .. } => (*row_index, true),
            PlannedFileViewRow::InlineNote {
                anchor_row_index, ..
            } => (*anchor_row_index, false),
        };
        let extent = extents
            .get_mut(row_index)
            .expect("planned file-view row targets a validated layout row");
        extent.first.get_or_insert(plan_index);
        extent.last = Some(plan_index);
        if is_anchor {
            extent.anchor = Some(plan_index);
        }
    }

    let mut hunk_anchor_rows = HashMap::with_capacity(resolved.layout.hunk_rows.len());
    let mut hunk_bounds = HashMap::with_capacity(resolved.layout.hunk_rows.len());
    for (hunk_index, hunk) in resolved.layout.hunk_rows.iter().enumerate() {
        let start_extent = extents[hunk.start_row];
        let end_extent = extents[hunk.end_row];
        let (Some(anchor_index), Some(start_index), Some(end_index)) =
            (start_extent.anchor, start_extent.first, end_extent.last)
        else {
            continue;
        };
        let anchor = &row_bounds[anchor_index];
        let start = &row_bounds[start_index];
        let end = &row_bounds[end_index];
        hunk_anchor_rows.insert(hunk_index, anchor.top);
        hunk_bounds.insert(
            hunk_index,
            PlannedFileViewHunkBounds {
                top: start.top,
                height: end.top.saturating_add(end.height).saturating_sub(start.top),
                start_row_id: crate::review_row_id(&start.key),
                end_row_id: crate::review_row_id(&end.key),
            },
        );
    }

    FileViewGeometry {
        body_height,
        hunk_anchor_rows,
        hunk_bounds,
        line_number_digits: 1,
        file_view_rows: planned_rows,
        row_bounds,
        row_bounds_by_key,
        row_bounds_by_stable_key,
    }
}

#[cfg(test)]
mod tests {
    use workdeck_core::{AgentAnnotation, LineRange};
    use workdeck_extension_api::{
        ExtensionFileSide, ExtensionFileViewHunkRows, ExtensionFileViewLayout,
        ExtensionFileViewRow, ExtensionFileViewSourceRange, ExtensionFileViewSpan,
    };
    use workdeck_review::{VisibleFileViewNote, build_file_view_render_plan};

    use super::*;

    #[test]
    fn uses_declared_heights_with_stable_ids_and_hunk_bounds() {
        let resolved = resolved(
            vec![
                row("intro", None),
                row("custom-a", None),
                row("custom-b", None),
            ],
            vec![(0, 1), (2, 2)],
            vec![1, 3, 2],
        );
        let plan = build_file_view_render_plan(&resolved.layout, &[]);
        let geometry = measure_file_view_geometry(&resolved, &plan.rows, 80);

        assert_eq!(
            geometry
                .row_bounds
                .iter()
                .map(|row| row.height)
                .collect::<Vec<_>>(),
            resolved.row_heights
        );
        assert_eq!(geometry.body_height, 6);
        assert_eq!(
            geometry
                .row_bounds
                .iter()
                .map(|row| (row.stable_key.as_str(), row.top, row.height))
                .collect::<Vec<_>>(),
            [
                ("file-view:intro", 0, 1),
                ("file-view:custom-a", 1, 3),
                ("file-view:custom-b", 4, 2),
            ]
        );
        assert_eq!(geometry.hunk_anchor_rows.get(&0), Some(&0));
        assert_eq!(geometry.hunk_anchor_rows.get(&1), Some(&4));
        assert_eq!(
            geometry
                .hunk_bounds
                .get(&0)
                .map(|bounds| (bounds.top, bounds.height)),
            Some((0, 4))
        );
        assert_eq!(
            geometry
                .hunk_bounds
                .get(&1)
                .map(|bounds| (bounds.top, bounds.height)),
            Some((4, 2))
        );
    }

    #[test]
    fn measures_ten_thousand_rows_and_hunks_through_retained_extents() {
        let rows = (0..10_000)
            .map(|index| row(&format!("row-{index}"), None))
            .collect();
        let hunks = (0..10_000).map(|index| (index, index)).collect();
        let resolved = resolved(rows, hunks, vec![1; 10_000]);
        let plan = build_file_view_render_plan(&resolved.layout, &[]);
        let geometry = measure_file_view_geometry(&resolved, &plan.rows, 80);
        assert_eq!(geometry.body_height, 10_000);
        assert_eq!(geometry.hunk_anchor_rows.get(&9_999), Some(&9_999));
        assert_eq!(
            geometry
                .hunk_bounds
                .get(&9_999)
                .map(|bounds| (bounds.top, bounds.height)),
            Some((9_999, 1))
        );
    }

    #[test]
    fn measures_host_notes_and_rows_from_one_planned_stream() {
        let resolved = resolved(
            vec![row(
                "summary",
                Some(ExtensionFileViewSourceRange {
                    side: ExtensionFileSide::New,
                    range: [1, 2],
                }),
            )],
            vec![(0, 0)],
            vec![2],
        );
        let note = note("note", 1);
        let plan = build_file_view_render_plan(&resolved.layout, std::slice::from_ref(&note));
        let note_height = measure_agent_inline_note_height(
            &note.annotation,
            Some(workdeck_core::ReviewSide::New),
            LayoutMode::Stack,
            80,
            0,
        );
        let geometry = measure_file_view_geometry(&resolved, &plan.rows, 80);
        assert!(std::ptr::eq(geometry.file_view_rows, plan.rows.as_slice()));
        assert_eq!(
            geometry
                .row_bounds
                .iter()
                .map(|row| row.height)
                .collect::<Vec<_>>(),
            [2, note_height]
        );
        assert_eq!(geometry.body_height, note_height + 2);
        assert_eq!(geometry.hunk_anchor_rows.get(&0), Some(&0));
        assert_eq!(
            geometry
                .hunk_bounds
                .get(&0)
                .map(|bounds| (bounds.top, bounds.height)),
            Some((0, note_height + 2))
        );
    }

    #[test]
    fn indexes_each_navigable_row_under_its_revealed_source_line() {
        let resolved = resolved(
            vec![
                row(
                    "summary",
                    Some(ExtensionFileViewSourceRange {
                        side: ExtensionFileSide::New,
                        range: [1, 1],
                    }),
                ),
                row("detail", None),
            ],
            vec![(0, 1)],
            vec![1, 1],
        );
        let plan = build_file_view_render_plan(&resolved.layout, &[]);
        let geometry = measure_file_view_geometry(&resolved, &plan.rows, 80);
        assert_eq!(
            geometry
                .bounds_for_stable_key("line:0:new:1")
                .map(|bounds| bounds.top),
            Some(0)
        );
        assert_eq!(
            geometry
                .bounds_for_stable_key("file-view:summary")
                .map(|bounds| bounds.top),
            Some(0)
        );
    }

    fn resolved(
        rows: Vec<ExtensionFileViewRow>,
        hunks: Vec<(usize, usize)>,
        row_heights: Vec<usize>,
    ) -> ValidatedFileViewLayout {
        ValidatedFileViewLayout {
            layout: ExtensionFileViewLayout {
                rows,
                hunk_rows: hunks
                    .into_iter()
                    .map(|(start_row, end_row)| ExtensionFileViewHunkRows { start_row, end_row })
                    .collect(),
            },
            row_heights,
        }
    }

    fn row(id: &str, source_range: Option<ExtensionFileViewSourceRange>) -> ExtensionFileViewRow {
        ExtensionFileViewRow {
            id: id.into(),
            spans: vec![ExtensionFileViewSpan {
                text: id.into(),
                tone: None,
                attributes: Vec::new(),
            }],
            source_ranges: source_range.into_iter().collect(),
            component: None,
        }
    }

    fn note(id: &str, line: u32) -> VisibleFileViewNote {
        VisibleFileViewNote {
            id: id.into(),
            annotation: AgentAnnotation {
                extra: Default::default(),
                id: Some(id.into()),
                old_range: None,
                new_range: Some(LineRange {
                    start: line,
                    end: line,
                }),
                summary: "Review this range".into(),
                rationale: None,
                markup: None,
                tags: Vec::new(),
                confidence: None,
                source: None,
                title: None,
                author: None,
                created_at: None,
                updated_at: None,
                editable: false,
            },
            thread_depth: 0,
            has_actions: false,
        }
    }
}
