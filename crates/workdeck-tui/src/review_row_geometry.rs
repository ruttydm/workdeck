//! Pre-mount height and hunk-bound measurement for planned review rows.
//!
//! This is a Rust reimplementation of Hunk's `src/ui/diff/reviewRowGeometry.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.

use std::collections::HashMap;

use workdeck_review::LayoutMode;

use crate::{PlannedReviewRow, SectionGeometry, measure_agent_inline_note_height, review_row_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedReviewRowLayoutOptions {
    pub show_hunk_headers: bool,
    pub layout: LayoutMode,
    pub width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedHunkBounds {
    pub top: usize,
    pub height: usize,
    pub start_row_id: String,
    pub end_row_id: String,
}

pub type PlannedSectionGeometry = SectionGeometry<PlannedHunkBounds>;

#[must_use]
pub fn planned_review_row_contributes_to_hunk_bounds(row: &PlannedReviewRow) -> bool {
    match row {
        PlannedReviewRow::HunkGap { .. } => false,
        PlannedReviewRow::InlineNote { .. } => true,
        PlannedReviewRow::DiffRow { row, .. } => match row {
            workdeck_diff::DiffRow::Collapsed { .. } => false,
            workdeck_diff::DiffRow::SplitLine {
                is_expansion_row, ..
            }
            | workdeck_diff::DiffRow::StackLine {
                is_expansion_row, ..
            } => !is_expansion_row,
            workdeck_diff::DiffRow::HunkHeader { .. } => true,
        },
    }
}

#[must_use]
pub fn planned_review_row_height(
    row: &PlannedReviewRow,
    options: PlannedReviewRowLayoutOptions,
) -> usize {
    match row {
        PlannedReviewRow::InlineNote {
            annotation,
            anchor_side,
            note,
            ..
        } => measure_agent_inline_note_height(
            annotation,
            *anchor_side,
            options.layout,
            options.width,
            note.thread.as_ref().map_or(0, |thread| thread.depth),
        ),
        PlannedReviewRow::HunkGap { height, .. } => *height,
        PlannedReviewRow::DiffRow {
            row: workdeck_diff::DiffRow::HunkHeader { .. },
            ..
        } => usize::from(options.show_hunk_headers),
        PlannedReviewRow::DiffRow { .. } => 1,
    }
}

#[must_use]
pub fn planned_review_row_visible(
    row: &PlannedReviewRow,
    options: PlannedReviewRowLayoutOptions,
) -> bool {
    planned_review_row_height(row, options) > 0
}

#[must_use]
pub fn measure_planned_section_geometry(
    planned_rows: &[PlannedReviewRow],
    options: PlannedReviewRowLayoutOptions,
) -> PlannedSectionGeometry {
    let mut hunk_anchor_rows = HashMap::new();
    let mut hunk_bounds = HashMap::<usize, PlannedHunkBounds>::new();
    let mut body_height = 0_usize;

    for row in planned_rows {
        if let PlannedReviewRow::DiffRow {
            anchor_id: Some(_),
            hunk_index,
            ..
        } = row
        {
            hunk_anchor_rows.entry(*hunk_index).or_insert(body_height);
        }

        let row_height = planned_review_row_height(row, options);
        if row_height > 0 && planned_review_row_contributes_to_hunk_bounds(row) {
            let row_id = review_row_id(row.key());
            if let Some(bounds) = hunk_bounds.get_mut(&row.hunk_index()) {
                bounds.end_row_id = row_id;
                bounds.height = bounds.height.saturating_add(row_height);
            } else {
                hunk_bounds.insert(
                    row.hunk_index(),
                    PlannedHunkBounds {
                        top: body_height,
                        height: row_height,
                        start_row_id: row_id.clone(),
                        end_row_id: row_id,
                    },
                );
            }
        }
        body_height = body_height.saturating_add(row_height);
    }

    SectionGeometry {
        body_height,
        hunk_anchor_rows,
        hunk_bounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{AgentAnnotation, LineRange, ReviewGapPosition, ReviewSide};
    use workdeck_diff::{DiffRow, RenderSpan, SplitLineCell, SplitLineKind};
    use workdeck_review::ResolvedReviewNoteAnchor;

    use crate::VisibleAgentNote;

    const BASE_OPTIONS: PlannedReviewRowLayoutOptions = PlannedReviewRowLayoutOptions {
        show_hunk_headers: true,
        layout: LayoutMode::Split,
        width: 100,
    };

    fn planned(row: DiffRow, anchor_id: Option<&str>) -> PlannedReviewRow {
        let (key, file_id, hunk_index) = match &row {
            DiffRow::Collapsed {
                key,
                file_id,
                hunk_index,
                ..
            }
            | DiffRow::HunkHeader {
                key,
                file_id,
                hunk_index,
                ..
            }
            | DiffRow::SplitLine {
                key,
                file_id,
                hunk_index,
                ..
            }
            | DiffRow::StackLine {
                key,
                file_id,
                hunk_index,
                ..
            } => (key.clone(), file_id.clone(), *hunk_index),
        };
        PlannedReviewRow::DiffRow {
            stable_key: key.clone(),
            key,
            stable_alias_keys: Vec::new(),
            file_id,
            hunk_index,
            row,
            anchor_id: anchor_id.map(str::to_owned),
            note_guide_side: None,
        }
    }

    fn hunk_header(key: &str, hunk_index: usize, anchor_id: Option<&str>) -> PlannedReviewRow {
        planned(
            DiffRow::HunkHeader {
                key: key.into(),
                file_id: "file-1".into(),
                hunk_index,
                text: "@@ -1,1 +1,1 @@".into(),
            },
            anchor_id,
        )
    }

    fn collapsed_row(key: &str, hunk_index: usize) -> PlannedReviewRow {
        planned(
            DiffRow::Collapsed {
                key: key.into(),
                file_id: "file-1".into(),
                hunk_index,
                text: "⋯".into(),
                position: ReviewGapPosition::Before,
                old_range: [1, 1],
                new_range: [1, 1],
            },
            None,
        )
    }

    fn split_line(key: &str, hunk_index: usize, anchor_id: Option<&str>) -> PlannedReviewRow {
        let cell = |kind, sign: &str, text: &str| SplitLineCell {
            kind,
            sign: sign.into(),
            line_number: Some(1),
            move_kind: None,
            spans: vec![RenderSpan {
                text: text.into(),
                foreground: None,
                background: None,
                transform_foreground: None,
            }],
        };
        planned(
            DiffRow::SplitLine {
                key: key.into(),
                file_id: "file-1".into(),
                hunk_index,
                left: cell(SplitLineKind::Deletion, "-", "old"),
                right: cell(SplitLineKind::Addition, "+", "new"),
                is_expansion_row: false,
                expanded_gap_key: None,
            },
            anchor_id,
        )
    }

    fn inline_note(key: &str, hunk_index: usize) -> PlannedReviewRow {
        let annotation = AgentAnnotation {
            id: Some("note-1".into()),
            old_range: None,
            new_range: Some(LineRange { start: 1, end: 1 }),
            summary: "Explain why this branch changed.".into(),
            rationale: Some("The note should reserve space in the hunk bounds.".into()),
            markup: None,
            tags: Vec::new(),
            confidence: None,
            source: Some("agent".into()),
            title: None,
            author: None,
            created_at: None,
            updated_at: None,
            editable: false,
        };
        let note = VisibleAgentNote {
            id: "note-1".into(),
            annotation: annotation.clone(),
            anchor: ResolvedReviewNoteAnchor {
                old_range: None,
                new_range: annotation.new_range,
                preferred: None,
                intersecting_hunk_indices: Vec::new(),
                owner_hunk_index: None,
            },
            source: None,
            editable: false,
            thread: None,
            actions: None,
            draft: None,
        };
        PlannedReviewRow::InlineNote {
            key: key.into(),
            stable_key: key.into(),
            file_id: "file-1".into(),
            hunk_index,
            annotation_id: "note-1".into(),
            annotation,
            note: Box::new(note),
            anchor_side: Some(ReviewSide::New),
            note_count: 1,
            note_index: 0,
        }
    }

    fn hunk_gap(key: &str, hunk_index: usize, height: usize) -> PlannedReviewRow {
        PlannedReviewRow::HunkGap {
            key: key.into(),
            stable_key: key.into(),
            file_id: "file-1".into(),
            hunk_index,
            height,
        }
    }

    fn guided_line(key: &str, hunk_index: usize) -> PlannedReviewRow {
        let mut row = split_line(key, hunk_index, None);
        if let PlannedReviewRow::DiffRow {
            note_guide_side, ..
        } = &mut row
        {
            *note_guide_side = Some(ReviewSide::New);
        }
        row
    }

    #[test]
    fn row_height_and_visibility_match_rendered_terminal_rows() {
        assert_eq!(
            planned_review_row_height(&hunk_header("header", 0, None), BASE_OPTIONS),
            1
        );
        let hidden = PlannedReviewRowLayoutOptions {
            show_hunk_headers: false,
            ..BASE_OPTIONS
        };
        assert_eq!(
            planned_review_row_height(&hunk_header("header", 0, None), hidden),
            0
        );
        assert!(!planned_review_row_visible(
            &hunk_header("header", 0, None),
            hidden
        ));
        assert_eq!(
            planned_review_row_height(&split_line("line", 0, None), BASE_OPTIONS),
            1
        );
        assert_eq!(
            planned_review_row_height(&guided_line("guide", 0), BASE_OPTIONS),
            1
        );
        assert_eq!(
            planned_review_row_height(&hunk_gap("gap", 1, 2), BASE_OPTIONS),
            2
        );
        assert!(planned_review_row_height(&inline_note("note", 0), BASE_OPTIONS) > 3);
    }

    #[test]
    fn bounds_ignore_collapsed_gaps_but_include_notes_and_guides() {
        let rows = vec![
            hunk_header("h0", 0, Some("hunk-0")),
            split_line("line-0", 0, None),
            collapsed_row("gap", 0),
            inline_note("note", 0),
            guided_line("guide", 0),
            hunk_header("h1", 1, Some("hunk-1")),
            split_line("line-1", 1, None),
        ];
        let measured = measure_planned_section_geometry(&rows, BASE_OPTIONS);
        let note_height = planned_review_row_height(&rows[3], BASE_OPTIONS);
        assert_eq!(measured.body_height, 6 + note_height);
        assert_eq!(measured.hunk_anchor_rows.get(&0), Some(&0));
        assert_eq!(measured.hunk_anchor_rows.get(&1), Some(&(4 + note_height)));
        assert_eq!(
            measured.hunk_bounds.get(&0),
            Some(&PlannedHunkBounds {
                top: 0,
                height: 3 + note_height,
                start_row_id: review_row_id("h0"),
                end_row_id: review_row_id("guide"),
            })
        );
        assert_eq!(
            measured.hunk_bounds.get(&1),
            Some(&PlannedHunkBounds {
                top: 4 + note_height,
                height: 2,
                start_row_id: review_row_id("h1"),
                end_row_id: review_row_id("line-1"),
            })
        );
    }

    #[test]
    fn decorative_hunk_gaps_occupy_body_but_not_hunk_bounds() {
        let rows = vec![
            hunk_header("h0", 0, Some("hunk-0")),
            split_line("line-0", 0, None),
            hunk_gap("spacer", 1, 2),
            hunk_header("h1", 1, Some("hunk-1")),
            split_line("line-1", 1, None),
        ];
        let measured = measure_planned_section_geometry(&rows, BASE_OPTIONS);
        assert_eq!(measured.body_height, 6);
        assert_eq!(measured.hunk_anchor_rows.get(&1), Some(&4));
        assert_eq!(
            measured.hunk_bounds.get(&0),
            Some(&PlannedHunkBounds {
                top: 0,
                height: 2,
                start_row_id: review_row_id("h0"),
                end_row_id: review_row_id("line-0"),
            })
        );
        assert_eq!(
            measured.hunk_bounds.get(&1),
            Some(&PlannedHunkBounds {
                top: 4,
                height: 2,
                start_row_id: review_row_id("h1"),
                end_row_id: review_row_id("line-1"),
            })
        );
    }

    #[test]
    fn hunk_gap_height_survives_hidden_headers() {
        let rows = vec![
            hunk_header("h0", 0, Some("hunk-0")),
            split_line("line-0", 0, None),
            hunk_gap("spacer", 1, 2),
            hunk_header("h1", 1, Some("hunk-1")),
            split_line("line-1", 1, None),
        ];
        let measured = measure_planned_section_geometry(
            &rows,
            PlannedReviewRowLayoutOptions {
                show_hunk_headers: false,
                ..BASE_OPTIONS
            },
        );
        assert_eq!(measured.body_height, 4);
        assert_eq!(
            measured.hunk_bounds.get(&1),
            Some(&PlannedHunkBounds {
                top: 3,
                height: 1,
                start_row_id: review_row_id("line-1"),
                end_row_id: review_row_id("line-1"),
            })
        );
    }

    #[test]
    fn hidden_header_anchors_navigation_without_widening_bounds() {
        let rows = vec![
            hunk_header("h0", 0, Some("hunk-0")),
            split_line("line-0", 0, None),
        ];
        let measured = measure_planned_section_geometry(
            &rows,
            PlannedReviewRowLayoutOptions {
                show_hunk_headers: false,
                ..BASE_OPTIONS
            },
        );
        assert_eq!(measured.body_height, 1);
        assert_eq!(measured.hunk_anchor_rows.get(&0), Some(&0));
        assert_eq!(
            measured.hunk_bounds.get(&0),
            Some(&PlannedHunkBounds {
                top: 0,
                height: 1,
                start_row_id: review_row_id("line-0"),
                end_row_id: review_row_id("line-0"),
            })
        );
    }

    #[test]
    fn collapsed_expansion_and_gap_rows_have_exact_bound_contribution_policy() {
        assert!(!planned_review_row_contributes_to_hunk_bounds(
            &collapsed_row("collapsed", 0)
        ));
        assert!(!planned_review_row_contributes_to_hunk_bounds(&hunk_gap(
            "gap", 0, 1
        )));
        assert!(planned_review_row_contributes_to_hunk_bounds(&inline_note(
            "note", 0
        )));
        let mut expansion = split_line("expansion", 0, None);
        if let PlannedReviewRow::DiffRow {
            row: DiffRow::SplitLine {
                is_expansion_row, ..
            },
            ..
        } = &mut expansion
        {
            *is_expansion_row = true;
        }
        assert!(!planned_review_row_contributes_to_hunk_bounds(&expansion));
    }

    fn geometry_json(geometry: &PlannedSectionGeometry) -> serde_json::Value {
        let mut anchors = geometry.hunk_anchor_rows.iter().collect::<Vec<_>>();
        anchors.sort_by_key(|(index, _)| **index);
        let mut bounds = geometry.hunk_bounds.iter().collect::<Vec<_>>();
        bounds.sort_by_key(|(index, _)| **index);
        serde_json::json!({
            "bodyHeight": geometry.body_height,
            "hunkAnchorRows": anchors.into_iter().map(|(index, top)| [*index, *top]).collect::<Vec<_>>(),
            "hunkBounds": bounds.into_iter().map(|(index, bounds)| serde_json::json!([
                index,
                {
                    "top": bounds.top,
                    "height": bounds.height,
                    "startRowId": bounds.start_row_id,
                    "endRowId": bounds.end_row_id,
                }
            ])).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn frozen_hunk_review_row_geometry_vectors_match_native() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/review-row-geometry.json"
        ))
        .expect("valid frozen Hunk row-geometry oracle");
        assert_eq!(oracle["baselineOracle"]["passed"], 5);
        assert_eq!(oracle["stableOracle"]["passed"], 3);
        let vectors = &oracle["projectionVectors"];

        let rows = vec![
            hunk_header("h0", 0, Some("hunk-0")),
            split_line("line-0", 0, None),
            collapsed_row("gap", 0),
            inline_note("note", 0),
            guided_line("guide", 0),
            hunk_header("h1", 1, Some("hunk-1")),
            split_line("line-1", 1, None),
        ];
        assert_eq!(
            planned_review_row_height(&rows[3], BASE_OPTIONS),
            vectors["noteHeight"].as_u64().expect("numeric height") as usize
        );
        let geometry = geometry_json(&measure_planned_section_geometry(&rows, BASE_OPTIONS));
        assert_eq!(geometry["bodyHeight"], vectors["bodyHeight"]);
        assert_eq!(geometry["hunkAnchorRows"], vectors["hunkAnchorRows"]);
        assert_eq!(geometry["hunkBounds"], vectors["hunkBounds"]);

        let hidden_rows = vec![
            hunk_header("h0", 0, Some("hunk-0")),
            split_line("line-0", 0, None),
            hunk_gap("spacer", 1, 2),
            hunk_header("h1", 1, Some("hunk-1")),
            split_line("line-1", 1, None),
        ];
        assert_eq!(
            geometry_json(&measure_planned_section_geometry(
                &hidden_rows,
                PlannedReviewRowLayoutOptions {
                    show_hunk_headers: false,
                    ..BASE_OPTIONS
                },
            )),
            vectors["hiddenGap"]
        );
    }
}
