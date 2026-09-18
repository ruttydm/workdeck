use std::collections::{BTreeMap, BTreeSet};

use workdeck_core::{AgentAnnotation, ReviewSide};
use workdeck_extension_api::{ExtensionFileSide, ExtensionFileViewLayout, ExtensionFileViewRow};

/// The host-owned note payload retained by an alternate file-view render plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleFileViewNote {
    pub id: String,
    pub annotation: AgentAnnotation,
    pub thread_depth: usize,
    pub has_actions: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlannedFileViewRow {
    FileViewRow {
        key: String,
        stable_key: String,
        stable_alias_keys: Vec<String>,
        row: Box<ExtensionFileViewRow>,
        row_index: usize,
    },
    InlineNote {
        key: String,
        stable_key: String,
        annotation: Box<AgentAnnotation>,
        anchor_row_index: usize,
        anchor_side: ReviewSide,
        hunk_index: usize,
        note: Box<VisibleFileViewNote>,
        note_count: usize,
        note_index: usize,
    },
}

impl PlannedFileViewRow {
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::FileViewRow { key, .. } | Self::InlineNote { key, .. } => key,
        }
    }

    #[must_use]
    pub fn stable_key(&self) -> &str {
        match self {
            Self::FileViewRow { stable_key, .. } | Self::InlineNote { stable_key, .. } => {
                stable_key
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileViewRenderPlan {
    pub rows: Vec<PlannedFileViewRow>,
    /// Notes without one exact bound anchor force the file back to raw diff.
    pub unresolved_note_ids: Vec<String>,
}

/// Insert host-owned notes into one immutable alternate-view row stream.
///
/// Placement is all-or-raw: a visible note without one exact bound anchor
/// inside a unique declared hunk is reported as unresolved instead of dropped
/// or placed heuristically.
#[must_use]
pub fn build_file_view_render_plan(
    layout: &ExtensionFileViewLayout,
    visible_notes: &[VisibleFileViewNote],
) -> FileViewRenderPlan {
    let owners = hunk_owners_by_row(layout);
    let mut notes_by_row = BTreeMap::<usize, Vec<(&VisibleFileViewNote, ReviewSide, usize)>>::new();
    let mut unresolved_note_ids = Vec::new();

    for note in visible_notes {
        let anchor = annotation_anchor(&note.annotation);
        let row_index = anchor
            .and_then(|(side, line)| bound_row_index(layout, side, line))
            .unwrap_or(usize::MAX);
        let hunk_index = owners.get(row_index).copied().flatten();
        let Some(((side, _), hunk_index)) = anchor.zip(hunk_index) else {
            unresolved_note_ids.push(note.id.clone());
            continue;
        };
        notes_by_row
            .entry(row_index)
            .or_default()
            .push((note, side, hunk_index));
    }

    let mut rows = Vec::new();
    let mut claimed_line_keys = BTreeSet::new();
    for (row_index, row) in layout.rows.iter().enumerate() {
        let key = format!("file-view:{}", row.id);
        let line_key =
            owners[row_index].and_then(|hunk_index| row_line_stable_key(row, hunk_index));
        let stable_alias_keys = line_key
            .filter(|line_key| claimed_line_keys.insert(line_key.clone()))
            .into_iter()
            .collect();
        rows.push(PlannedFileViewRow::FileViewRow {
            key: key.clone(),
            stable_key: key,
            stable_alias_keys,
            row: Box::new(row.clone()),
            row_index,
        });

        let anchored_notes = notes_by_row.get(&row_index).map_or(&[][..], Vec::as_slice);
        for (note_index, (note, anchor_side, hunk_index)) in anchored_notes.iter().enumerate() {
            rows.push(PlannedFileViewRow::InlineNote {
                key: format!("inline-note:{}:file-view:{}:{note_index}", note.id, row.id),
                stable_key: inline_note_stable_key(&note.id),
                annotation: Box::new(note.annotation.clone()),
                anchor_row_index: row_index,
                anchor_side: *anchor_side,
                hunk_index: *hunk_index,
                note: Box::new((*note).clone()),
                note_count: anchored_notes.len(),
                note_index,
            });
        }
    }

    FileViewRenderPlan {
        rows,
        unresolved_note_ids,
    }
}

fn hunk_owners_by_row(layout: &ExtensionFileViewLayout) -> Vec<Option<usize>> {
    let mut starts = vec![Vec::new(); layout.rows.len().saturating_add(1)];
    let mut ends = vec![Vec::new(); layout.rows.len().saturating_add(1)];
    for (hunk_index, hunk) in layout.hunk_rows.iter().enumerate() {
        if hunk.start_row < starts.len() {
            starts[hunk.start_row].push(hunk_index);
        }
        if hunk.end_row.saturating_add(1) < ends.len() {
            ends[hunk.end_row + 1].push(hunk_index);
        }
    }

    let mut active = BTreeSet::new();
    (0..layout.rows.len())
        .map(|row_index| {
            for hunk_index in &ends[row_index] {
                active.remove(hunk_index);
            }
            for hunk_index in &starts[row_index] {
                active.insert(*hunk_index);
            }
            (active.len() == 1).then(|| *active.first().expect("one active hunk"))
        })
        .collect()
}

fn row_line_stable_key(row: &ExtensionFileViewRow, hunk_index: usize) -> Option<String> {
    let range = row.source_ranges.first()?;
    Some(line_stable_key(hunk_index, range.side, range.range[0]))
}

fn bound_row_index(layout: &ExtensionFileViewLayout, side: ReviewSide, line: u32) -> Option<usize> {
    let side = match side {
        ReviewSide::Old => ExtensionFileSide::Old,
        ReviewSide::New => ExtensionFileSide::New,
    };
    let line = usize::try_from(line).ok()?;
    layout.rows.iter().position(|row| {
        row.source_ranges
            .iter()
            .any(|range| range.side == side && range.range[0] <= line && line <= range.range[1])
    })
}

fn annotation_anchor(annotation: &AgentAnnotation) -> Option<(ReviewSide, u32)> {
    annotation
        .new_range
        .map(|range| (ReviewSide::New, range.start))
        .or_else(|| {
            annotation
                .old_range
                .map(|range| (ReviewSide::Old, range.start))
        })
}

#[must_use]
pub fn line_stable_key(hunk_index: usize, side: ExtensionFileSide, line_number: usize) -> String {
    let side = match side {
        ExtensionFileSide::Old => "old",
        ExtensionFileSide::New => "new",
    };
    format!("line:{hunk_index}:{side}:{line_number}")
}

#[must_use]
pub fn inline_note_stable_key(note_id: &str) -> String {
    format!("inline-note:{note_id}")
}

#[cfg(test)]
mod tests {
    use workdeck_core::LineRange;
    use workdeck_extension_api::{
        ExtensionFileViewHunkRows, ExtensionFileViewSourceRange, ExtensionFileViewSpan,
    };

    use super::*;

    fn layout() -> ExtensionFileViewLayout {
        ExtensionFileViewLayout {
            rows: vec![
                ExtensionFileViewRow {
                    id: "old-summary".into(),
                    spans: vec![span("old")],
                    source_ranges: vec![source_range(ExtensionFileSide::Old, 2, 4)],
                    component: None,
                },
                ExtensionFileViewRow {
                    id: "new-summary".into(),
                    spans: vec![span("new")],
                    source_ranges: vec![source_range(ExtensionFileSide::New, 5, 8)],
                    component: None,
                },
            ],
            hunk_rows: vec![ExtensionFileViewHunkRows {
                start_row: 0,
                end_row: 1,
            }],
        }
    }

    fn span(text: &str) -> ExtensionFileViewSpan {
        ExtensionFileViewSpan {
            text: text.into(),
            tone: None,
            attributes: Vec::new(),
        }
    }

    fn source_range(
        side: ExtensionFileSide,
        start: usize,
        end: usize,
    ) -> ExtensionFileViewSourceRange {
        ExtensionFileViewSourceRange {
            side,
            range: [start, end],
        }
    }

    fn note(
        id: &str,
        old_range: Option<[u32; 2]>,
        new_range: Option<[u32; 2]>,
    ) -> VisibleFileViewNote {
        VisibleFileViewNote {
            id: id.into(),
            annotation: AgentAnnotation {
                extra: Default::default(),
                id: Some(id.into()),
                old_range: old_range.map(|range| LineRange {
                    start: range[0],
                    end: range[1],
                }),
                new_range: new_range.map(|range| LineRange {
                    start: range[0],
                    end: range[1],
                }),
                summary: id.into(),
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

    #[test]
    fn inserts_notes_after_the_uniquely_bound_preferred_side_row() {
        let plan =
            build_file_view_render_plan(&layout(), &[note("both", Some([3, 3]), Some([6, 7]))]);
        assert!(plan.unresolved_note_ids.is_empty());
        assert_eq!(
            plan.rows.iter().map(|row| row.key()).collect::<Vec<_>>(),
            [
                "file-view:old-summary",
                "file-view:new-summary",
                "inline-note:both:file-view:new-summary:0",
            ]
        );
        assert!(matches!(
            &plan.rows[2],
            PlannedFileViewRow::InlineNote {
                anchor_row_index: 1,
                anchor_side: ReviewSide::New,
                hunk_index: 0,
                ..
            }
        ));
    }

    #[test]
    fn anchors_each_row_on_the_source_line_the_raw_diff_addresses() {
        let plan = build_file_view_render_plan(&layout(), &[]);
        let aliases = plan
            .rows
            .iter()
            .map(|row| match row {
                PlannedFileViewRow::FileViewRow {
                    stable_alias_keys, ..
                } => stable_alias_keys.clone(),
                PlannedFileViewRow::InlineNote { .. } => Vec::new(),
            })
            .collect::<Vec<_>>();
        assert_eq!(aliases, [["line:0:old:2"], ["line:0:new:5"]]);
    }

    #[test]
    fn gives_one_source_line_to_the_first_row_that_presents_it() {
        let repeated = ExtensionFileViewLayout {
            rows: vec![
                ExtensionFileViewRow {
                    id: "first".into(),
                    spans: vec![span("a")],
                    source_ranges: vec![source_range(ExtensionFileSide::New, 5, 5)],
                    component: None,
                },
                ExtensionFileViewRow {
                    id: "second".into(),
                    spans: vec![span("b")],
                    source_ranges: vec![source_range(ExtensionFileSide::New, 5, 5)],
                    component: None,
                },
            ],
            hunk_rows: vec![ExtensionFileViewHunkRows {
                start_row: 0,
                end_row: 1,
            }],
        };
        let plan = build_file_view_render_plan(&repeated, &[]);
        assert!(matches!(
            &plan.rows[0],
            PlannedFileViewRow::FileViewRow { stable_alias_keys, .. }
                if stable_alias_keys == &["line:0:new:5"]
        ));
        assert!(matches!(
            &plan.rows[1],
            PlannedFileViewRow::FileViewRow { stable_alias_keys, .. }
                if stable_alias_keys.is_empty()
        ));
    }

    #[test]
    fn leaves_rows_outside_one_hunk_unaddressable_by_line_navigation() {
        let mut overlapping = layout();
        overlapping.hunk_rows.push(ExtensionFileViewHunkRows {
            start_row: 0,
            end_row: 1,
        });
        let plan = build_file_view_render_plan(&overlapping, &[]);
        assert!(plan.rows.iter().all(|row| matches!(
            row,
            PlannedFileViewRow::FileViewRow { stable_alias_keys, .. }
                if stable_alias_keys.is_empty()
        )));
    }

    #[test]
    fn groups_notes_at_one_anchor_in_stable_input_order() {
        let notes = [
            note("first", None, Some([5, 5])),
            note("second", None, Some([8, 8])),
        ];
        let plan = build_file_view_render_plan(&layout(), &notes);
        let planned_notes = plan
            .rows
            .iter()
            .filter_map(|row| match row {
                PlannedFileViewRow::InlineNote {
                    note,
                    note_index,
                    note_count,
                    ..
                } => Some((note.id.as_str(), *note_index, *note_count)),
                PlannedFileViewRow::FileViewRow { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(planned_notes, [("first", 0, 2), ("second", 1, 2)]);
    }

    #[test]
    fn reports_unbound_and_ambiguous_notes_instead_of_guessing() {
        let mut outside_hunk = layout();
        outside_hunk.hunk_rows[0].end_row = 0;
        let notes = [
            note("range-less", None, None),
            note("unbound", None, Some([20, 20])),
            note("outside-hunk", None, Some([6, 6])),
        ];
        let plan = build_file_view_render_plan(&outside_hunk, &notes);
        assert_eq!(
            plan.unresolved_note_ids,
            ["range-less", "unbound", "outside-hunk"]
        );
        assert!(
            plan.rows
                .iter()
                .all(|row| matches!(row, PlannedFileViewRow::FileViewRow { .. }))
        );

        let mut overlapping = layout();
        overlapping.hunk_rows.push(ExtensionFileViewHunkRows {
            start_row: 1,
            end_row: 1,
        });
        let plan = build_file_view_render_plan(
            &overlapping,
            &[note("ambiguous-hunk", None, Some([6, 6]))],
        );
        assert_eq!(plan.unresolved_note_ids, ["ambiguous-hunk"]);
    }
}
