use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use workdeck_diff::sanitize_terminal_line;
use workdeck_extension_api::{
    ExtensionFileSide, ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewRowComponent, ExtensionFileViewSelectionPrefix, ExtensionFileViewSourceRange,
    ExtensionFileViewSpan, ExtensionFileViewTone, ExtensionTextAttribute, ValidatedFileViewLayout,
    ViewNode, validate_view, view_contains_input,
};

pub const FILE_VIEW_MAX_ROWS: usize = 10_000;
pub const FILE_VIEW_MAX_SPANS: usize = 40_000;
pub const FILE_VIEW_MAX_TEXT_LENGTH: usize = 1_000_000;
pub const FILE_VIEW_MAX_COMPONENT_ROW_HEIGHT: usize = 256;
pub const FILE_VIEW_MAX_SOURCE_RANGES: usize = 40_000;
pub const FILE_VIEW_MAX_LAYOUT_HEIGHT: usize = 100_000;

/// Explain why an extension result cannot safely join the host-owned review stream.
pub fn validate_file_view_layout(
    value: &Value,
    hunk_count: usize,
    width: usize,
) -> Result<ValidatedFileViewLayout, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "layout is not an object".to_owned())?;
    let row_values = object
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| "layout must include rows and hunkRows arrays".to_owned())?;
    let hunk_values = object
        .get("hunkRows")
        .and_then(Value::as_array)
        .ok_or_else(|| "layout must include rows and hunkRows arrays".to_owned())?;
    if row_values.len() > FILE_VIEW_MAX_ROWS {
        return Err(format!("layout has more than {FILE_VIEW_MAX_ROWS} rows"));
    }

    let mut ids = BTreeSet::new();
    let mut span_count = 0_usize;
    let mut source_range_count = 0_usize;
    let mut text_length = 0_usize;
    let mut layout_height = 0_usize;
    let mut source_ranges_by_side =
        BTreeMap::<ExtensionFileSide, Vec<([usize; 2], usize)>>::from([
            (ExtensionFileSide::Old, Vec::new()),
            (ExtensionFileSide::New, Vec::new()),
        ]);
    let mut rows = Vec::with_capacity(row_values.len());
    let mut row_heights = Vec::with_capacity(row_values.len());
    let usable_width = width.max(1);

    for (index, row_value) in row_values.iter().enumerate() {
        let row = row_value
            .as_object()
            .ok_or_else(|| format!("rows[{index}] has no non-empty id"))?;
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| format!("rows[{index}] has no non-empty id"))?;
        if !ids.insert(id.to_owned()) {
            return Err(format!("rows[{index}] repeats id \"{id}\""));
        }

        let component = parse_component(row, index)?;
        let span_values = row
            .get("spans")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("rows[{index}].spans is not an array"))?;
        let mut row_text = String::new();
        let mut spans = Vec::with_capacity(span_values.len());
        for span_value in span_values {
            span_count = span_count.saturating_add(1);
            if span_count > FILE_VIEW_MAX_SPANS {
                return Err(format!("layout has more than {FILE_VIEW_MAX_SPANS} spans"));
            }
            let Some(span) = span_value.as_object() else {
                return Err(format!("rows[{index}] contains an invalid span"));
            };
            let Some(text) = span.get("text").and_then(Value::as_str) else {
                return Err(format!("rows[{index}] contains an invalid span"));
            };
            if text.contains('\n') {
                return Err(format!("rows[{index}] contains an invalid span"));
            }
            let tone = parse_tone(span, index)?;
            let attributes = parse_attributes(span, index)?;
            text_length = text_length.saturating_add(text.encode_utf16().count());
            if text_length > FILE_VIEW_MAX_TEXT_LENGTH {
                return Err(format!(
                    "layout text exceeds {FILE_VIEW_MAX_TEXT_LENGTH} characters"
                ));
            }
            row_text.push_str(text);
            spans.push(ExtensionFileViewSpan {
                text: text.to_owned(),
                tone,
                attributes,
            });
        }

        let source_ranges = parse_source_ranges(
            row,
            index,
            &mut source_range_count,
            &mut source_ranges_by_side,
        )?;
        let row_height = component.as_ref().map_or_else(
            || wrapped_line_count(&sanitize_terminal_line(&row_text), usable_width).max(1),
            |component| component.height,
        );
        layout_height = layout_height.saturating_add(row_height);
        if layout_height > FILE_VIEW_MAX_LAYOUT_HEIGHT {
            return Err(format!(
                "layout exceeds {FILE_VIEW_MAX_LAYOUT_HEIGHT} terminal rows"
            ));
        }
        row_heights.push(row_height);
        rows.push(ExtensionFileViewRow {
            id: id.to_owned(),
            spans,
            source_ranges,
            component,
        });
    }

    validate_source_range_overlap(&mut source_ranges_by_side)?;

    if hunk_values.len() != hunk_count {
        return Err(format!(
            "layout has {} hunk bounds for {hunk_count} hunks",
            hunk_values.len()
        ));
    }
    let mut hunk_rows = Vec::with_capacity(hunk_values.len());
    for (position, value) in hunk_values.iter().enumerate() {
        let valid = value.as_object().and_then(|hunk| {
            let start_row = exact_usize(hunk.get("startRow")?)?;
            let end_row = exact_usize(hunk.get("endRow")?)?;
            (start_row < rows.len() && end_row < rows.len() && start_row <= end_row)
                .then_some(ExtensionFileViewHunkRows { start_row, end_row })
        });
        let Some(valid) = valid else {
            return Err(format!(
                "hunkRows[{position}] is not an in-bounds row range"
            ));
        };
        hunk_rows.push(valid);
    }

    let mut owner_deltas = vec![0_i64; rows.len().saturating_add(1)];
    for hunk in &hunk_rows {
        owner_deltas[hunk.start_row] += 1;
        owner_deltas[hunk.end_row + 1] -= 1;
    }
    let mut owner_count = 0_i64;
    for (row_index, row) in rows.iter().enumerate() {
        owner_count += owner_deltas[row_index];
        if !row.source_ranges.is_empty() && owner_count != 1 {
            return Err(format!(
                "rows[{row_index}].sourceRanges must belong to exactly one hunkRows range"
            ));
        }
    }

    Ok(ValidatedFileViewLayout {
        layout: ExtensionFileViewLayout { rows, hunk_rows },
        row_heights,
    })
}

fn parse_component(
    row: &Map<String, Value>,
    row_index: usize,
) -> Result<Option<ExtensionFileViewRowComponent>, String> {
    let Some(value) = row.get("component") else {
        return Ok(None);
    };
    let component = value
        .as_object()
        .ok_or_else(|| format!("rows[{row_index}].component is not an object"))?;
    let height = component
        .get("height")
        .and_then(exact_usize)
        .filter(|height| (1..=FILE_VIEW_MAX_COMPONENT_ROW_HEIGHT).contains(height))
        .ok_or_else(|| {
            format!(
                "rows[{row_index}].component.height must be an integer from 1 to {FILE_VIEW_MAX_COMPONENT_ROW_HEIGHT}"
            )
        })?;
    let content = component
        .get("content")
        .cloned()
        .ok_or_else(|| format!("rows[{row_index}].component.content is not a declarative view"))?;
    let content = serde_json::from_value::<ViewNode>(content)
        .map_err(|_| format!("rows[{row_index}].component.content is not a declarative view"))?;
    if view_contains_input(&content) {
        return Err(format!(
            "rows[{row_index}].component.content contains a pane-only input"
        ));
    }
    validate_view(&content)
        .map_err(|issue| format!("rows[{row_index}].component.content {issue}"))?;
    let selected_content = component
        .get("selectedContent")
        .cloned()
        .map(|content| {
            let content = serde_json::from_value::<ViewNode>(content).map_err(|_| {
                format!("rows[{row_index}].component.selectedContent is not a declarative view")
            })?;
            if view_contains_input(&content) {
                return Err(format!(
                    "rows[{row_index}].component.selectedContent contains a pane-only input"
                ));
            }
            validate_view(&content)
                .map_err(|issue| format!("rows[{row_index}].component.selectedContent {issue}"))?;
            Ok::<_, String>(content)
        })
        .transpose()?;
    let expanded_content = component
        .get("expandedContent")
        .cloned()
        .map(|content| {
            let content = serde_json::from_value::<ViewNode>(content).map_err(|_| {
                format!("rows[{row_index}].component.expandedContent is not a declarative view")
            })?;
            if view_contains_input(&content) {
                return Err(format!(
                    "rows[{row_index}].component.expandedContent contains a pane-only input"
                ));
            }
            validate_view(&content)
                .map_err(|issue| format!("rows[{row_index}].component.expandedContent {issue}"))?;
            Ok::<_, String>(content)
        })
        .transpose()?;
    let selected_expanded_content = component
        .get("selectedExpandedContent")
        .cloned()
        .map(|content| {
            let content = serde_json::from_value::<ViewNode>(content).map_err(|_| {
                format!(
                    "rows[{row_index}].component.selectedExpandedContent is not a declarative view"
                )
            })?;
            if view_contains_input(&content) {
                return Err(format!(
                    "rows[{row_index}].component.selectedExpandedContent contains a pane-only input"
                ));
            }
            validate_view(&content).map_err(|issue| {
                format!("rows[{row_index}].component.selectedExpandedContent {issue}")
            })?;
            Ok::<_, String>(content)
        })
        .transpose()?;
    let toggle_expanded_on_left_mouse_up =
        component
            .get("toggleExpandedOnLeftMouseUp")
            .map_or(Ok(false), |value| {
                value.as_bool().ok_or_else(|| {
                    format!(
                        "rows[{row_index}].component.toggleExpandedOnLeftMouseUp is not a boolean"
                    )
                })
            })?;
    if toggle_expanded_on_left_mouse_up && expanded_content.is_none() {
        return Err(format!(
            "rows[{row_index}].component toggles without expandedContent"
        ));
    }
    let selection_prefix = component
        .get("selectionPrefix")
        .cloned()
        .map(|prefix| {
            let prefix = serde_json::from_value::<ExtensionFileViewSelectionPrefix>(prefix)
                .map_err(|_| format!("rows[{row_index}].component.selectionPrefix is invalid"))?;
            if sanitize_terminal_line(&prefix.selected) != prefix.selected
                || sanitize_terminal_line(&prefix.unselected) != prefix.unselected
                || prefix.selected.encode_utf16().count() > 32
                || prefix.unselected.encode_utf16().count() > 32
            {
                return Err(format!(
                    "rows[{row_index}].component.selectionPrefix is invalid"
                ));
            }
            Ok::<_, String>(prefix)
        })
        .transpose()?;
    Ok(Some(ExtensionFileViewRowComponent {
        height,
        content,
        selected_content,
        expanded_content,
        selected_expanded_content,
        toggle_expanded_on_left_mouse_up,
        selection_prefix,
    }))
}

fn parse_tone(
    span: &Map<String, Value>,
    row_index: usize,
) -> Result<Option<ExtensionFileViewTone>, String> {
    let Some(value) = span.get("tone") else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|_| format!("rows[{row_index}] contains an invalid span tone"))
}

fn parse_attributes(
    span: &Map<String, Value>,
    row_index: usize,
) -> Result<Vec<ExtensionTextAttribute>, String> {
    let Some(value) = span.get("attributes") else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value.clone())
        .map_err(|_| format!("rows[{row_index}] contains invalid span attributes"))
}

fn parse_source_ranges(
    row: &Map<String, Value>,
    row_index: usize,
    source_range_count: &mut usize,
    by_side: &mut BTreeMap<ExtensionFileSide, Vec<([usize; 2], usize)>>,
) -> Result<Vec<ExtensionFileViewSourceRange>, String> {
    let Some(value) = row.get("sourceRanges") else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("rows[{row_index}].sourceRanges is not an array"))?;
    let mut ranges = Vec::with_capacity(values.len());
    for (range_index, value) in values.iter().enumerate() {
        *source_range_count = source_range_count.saturating_add(1);
        if *source_range_count > FILE_VIEW_MAX_SOURCE_RANGES {
            return Err(format!(
                "layout has more than {FILE_VIEW_MAX_SOURCE_RANGES} source ranges"
            ));
        }
        let parsed = value.as_object().and_then(|range_value| {
            let side = serde_json::from_value(range_value.get("side")?.clone()).ok()?;
            let bounds = range_value.get("range")?.as_array()?;
            if bounds.len() != 2 {
                return None;
            }
            let start = exact_usize(&bounds[0])?;
            let end = exact_usize(&bounds[1])?;
            (start >= 1 && start <= end).then_some(ExtensionFileViewSourceRange {
                side,
                range: [start, end],
            })
        });
        let Some(parsed) = parsed else {
            return Err(format!(
                "rows[{row_index}].sourceRanges[{range_index}] is not a valid one-based source range"
            ));
        };
        by_side
            .get_mut(&parsed.side)
            .expect("both source sides are initialized")
            .push((parsed.range, row_index));
        ranges.push(parsed);
    }
    Ok(ranges)
}

fn validate_source_range_overlap(
    by_side: &mut BTreeMap<ExtensionFileSide, Vec<([usize; 2], usize)>>,
) -> Result<(), String> {
    for side in [ExtensionFileSide::Old, ExtensionFileSide::New] {
        let ranges = by_side
            .get_mut(&side)
            .expect("both source sides are initialized");
        ranges.sort_unstable_by_key(|(range, _)| (range[0], range[1]));
        let Some(mut furthest) = ranges.first().copied() else {
            continue;
        };
        for current in ranges.iter().copied().skip(1) {
            if current.0[0] <= furthest.0[1] && current.1 != furthest.1 {
                let label = match side {
                    ExtensionFileSide::Old => "old",
                    ExtensionFileSide::New => "new",
                };
                return Err(format!(
                    "{label}-side source ranges overlap between rows[{}] and rows[{}]",
                    furthest.1, current.1
                ));
            }
            if current.0[1] > furthest.0[1] {
                furthest = current;
            }
        }
    }
    Ok(())
}

fn exact_usize(value: &Value) -> Option<usize> {
    value.as_u64().and_then(|value| usize::try_from(value).ok())
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    if text.is_empty() || width == 0 {
        return 0;
    }
    let mut rows = 0_usize;
    let mut remaining = width;
    let mut has_content = false;
    for cluster in text.graphemes(true) {
        let cluster_width = UnicodeWidthStr::width(cluster);
        if cluster_width > remaining {
            let row_started = remaining < width || has_content;
            if has_content {
                rows = rows.saturating_add(1);
            }
            remaining = width;
            has_content = false;
            if cluster_width > width {
                if row_started {
                    rows = rows.saturating_add(1);
                }
                continue;
            }
        }
        remaining = remaining.saturating_sub(cluster_width);
        has_content = true;
    }
    rows.saturating_add(usize::from(has_content))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileViewSourceBindingIssueKind {
    UnavailableSource,
    OutOfBounds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileViewSourceBindingIssue {
    pub kind: FileViewSourceBindingIssueKind,
    pub detail: String,
}

/// Validate accepted row bindings against exact source documents the host can read.
#[must_use]
pub fn validate_file_view_source_ranges(
    layout: &ExtensionFileViewLayout,
    documents: &BTreeMap<ExtensionFileSide, Option<String>>,
) -> Option<FileViewSourceBindingIssue> {
    let line_counts = [ExtensionFileSide::Old, ExtensionFileSide::New]
        .into_iter()
        .map(|side| {
            let count = documents.get(&side).and_then(|source| {
                source
                    .as_deref()
                    .map(|source| source.strip_suffix('\n').unwrap_or(source))
                    .map(|source| {
                        if source.is_empty() {
                            0
                        } else {
                            source.split('\n').count()
                        }
                    })
            });
            (side, count)
        })
        .collect::<BTreeMap<_, _>>();

    for (row_index, row) in layout.rows.iter().enumerate() {
        for (range_index, source_range) in row.source_ranges.iter().enumerate() {
            let side = match source_range.side {
                ExtensionFileSide::Old => "old",
                ExtensionFileSide::New => "new",
            };
            let Some(line_count) = line_counts.get(&source_range.side).copied().flatten() else {
                return Some(FileViewSourceBindingIssue {
                    kind: FileViewSourceBindingIssueKind::UnavailableSource,
                    detail: format!(
                        "rows[{row_index}].sourceRanges[{range_index}] targets unavailable {side} source"
                    ),
                });
            };
            if source_range.range[1] > line_count {
                return Some(FileViewSourceBindingIssue {
                    kind: FileViewSourceBindingIssueKind::OutOfBounds,
                    detail: format!(
                        "rows[{row_index}].sourceRanges[{range_index}] exceeds the {side} source bounds"
                    ),
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn accepts_symbolic_rows_and_measures_terminal_width_wrapping() {
        let value = json!({
            "rows": [
                {"id": "heading", "spans": [{"text": "# title", "tone": "accent", "attributes": ["bold"]}]},
                {"id": "wide", "spans": [{"text": "界界"}]}
            ],
            "hunkRows": [{"startRow": 0, "endRow": 1}]
        });
        let validated = validate_file_view_layout(&value, 1, 3).unwrap();
        assert_eq!(validated.row_heights, [3, 2]);
    }

    #[test]
    fn returns_an_owned_snapshot_detached_from_extension_mutation() {
        let mut value = json!({
            "rows": [{
                "id": "original-row",
                "spans": [{"text": "original", "tone": "accent", "attributes": ["bold"]}],
                "sourceRanges": [{"side": "new", "range": [1, 2]}],
                "component": {"height": 2, "content": {"type": "empty"}}
            }],
            "hunkRows": [{"startRow": 0, "endRow": 0}]
        });
        let validated = validate_file_view_layout(&value, 1, 80).unwrap();
        value["rows"] = json!([]);
        value["hunkRows"] = json!([]);
        assert_eq!(validated.layout.rows[0].id, "original-row");
        assert_eq!(validated.layout.rows[0].spans[0].text, "original");
        assert_eq!(validated.layout.rows[0].source_ranges[0].range, [1, 2]);
        assert_eq!(validated.row_heights, [2]);
    }

    #[test]
    fn validates_exact_source_bindings_and_rejects_ambiguous_mappings() {
        let valid = validate_file_view_layout(
            &json!({
                "rows": [
                    {"id": "old", "spans": [], "sourceRanges": [{"side": "old", "range": [1, 2]}]},
                    {"id": "new", "spans": [], "sourceRanges": [{"side": "new", "range": [2, 3]}]}
                ],
                "hunkRows": [{"startRow": 0, "endRow": 1}]
            }),
            1,
            80,
        )
        .unwrap();
        let sources = BTreeMap::from([
            (ExtensionFileSide::Old, Some("a\nb\n".to_owned())),
            (ExtensionFileSide::New, Some("a\nb\nc".to_owned())),
        ]);
        assert_eq!(
            validate_file_view_source_ranges(&valid.layout, &sources),
            None
        );

        let unavailable = BTreeMap::from([
            (ExtensionFileSide::Old, Some("a\nb\n".to_owned())),
            (ExtensionFileSide::New, None),
        ]);
        assert_eq!(
            validate_file_view_source_ranges(&valid.layout, &unavailable),
            Some(FileViewSourceBindingIssue {
                kind: FileViewSourceBindingIssueKind::UnavailableSource,
                detail: "rows[1].sourceRanges[0] targets unavailable new source".into(),
            })
        );

        let out_of_bounds = BTreeMap::from([
            (ExtensionFileSide::Old, Some("a\n".to_owned())),
            (ExtensionFileSide::New, Some("a\nb\nc".to_owned())),
        ]);
        assert_eq!(
            validate_file_view_source_ranges(&valid.layout, &out_of_bounds),
            Some(FileViewSourceBindingIssue {
                kind: FileViewSourceBindingIssueKind::OutOfBounds,
                detail: "rows[0].sourceRanges[0] exceeds the old source bounds".into(),
            })
        );

        let aggregate = json!({
            "rows": [{"id": "aggregate", "spans": [], "sourceRanges": [
                {"side": "new", "range": [1, 2]}, {"side": "new", "range": [2, 3]}
            ]}],
            "hunkRows": [{"startRow": 0, "endRow": 0}]
        });
        assert!(validate_file_view_layout(&aggregate, 1, 80).is_ok());

        let overlap = json!({
            "rows": [
                {"id": "one", "spans": [], "sourceRanges": [{"side": "new", "range": [1, 3]}]},
                {"id": "two", "spans": [], "sourceRanges": [{"side": "new", "range": [3, 4]}]}
            ],
            "hunkRows": []
        });
        assert_eq!(
            validate_file_view_layout(&overlap, 0, 80).unwrap_err(),
            "new-side source ranges overlap between rows[0] and rows[1]"
        );

        let shared = json!({
            "rows": [{"id": "shared", "spans": [], "sourceRanges": [{"side": "new", "range": [1, 1]}]}],
            "hunkRows": [{"startRow": 0, "endRow": 0}, {"startRow": 0, "endRow": 0}]
        });
        assert_eq!(
            validate_file_view_layout(&shared, 2, 80).unwrap_err(),
            "rows[0].sourceRanges must belong to exactly one hunkRows range"
        );

        let zero_based = json!({
            "rows": [{"id": "bad", "spans": [], "sourceRanges": [{"side": "new", "range": [0, 1]}]}],
            "hunkRows": []
        });
        assert_eq!(
            validate_file_view_layout(&zero_based, 0, 80).unwrap_err(),
            "rows[0].sourceRanges[0] is not a valid one-based source range"
        );
    }

    #[test]
    fn validates_maximum_source_range_count_against_one_document() {
        let ranges = (0..FILE_VIEW_MAX_SOURCE_RANGES)
            .map(|_| json!({"side": "new", "range": [1, 1]}))
            .collect::<Vec<_>>();
        let value = json!({
            "rows": [{"id": "aggregate", "spans": [], "sourceRanges": ranges}],
            "hunkRows": [{"startRow": 0, "endRow": 0}]
        });
        let validated = validate_file_view_layout(&value, 1, 80).unwrap();
        let documents = BTreeMap::from([(ExtensionFileSide::New, Some("line\n".to_owned()))]);
        assert_eq!(
            validate_file_view_source_ranges(&validated.layout, &documents),
            None
        );
    }

    #[test]
    fn accepts_bounded_declarative_custom_row_painters() {
        let value = json!({
            "rows": [{
                "id": "custom", "spans": [{"text": "fallback"}],
                "component": {
                    "height": 4,
                    "content": {"type": "text", "text": "custom"},
                    "expandedContent": {"type": "text", "text": "expanded"},
                    "toggleExpandedOnLeftMouseUp": true,
                    "selectionPrefix": {"selected": "▶ ", "unselected": "  "}
                }
            }],
            "hunkRows": [{"startRow": 0, "endRow": 0}]
        });
        let validated = validate_file_view_layout(&value, 1, 80).unwrap();
        assert_eq!(validated.row_heights, [4]);
        let component = validated.layout.rows[0].component.as_ref().unwrap();
        assert!(component.expanded_content.is_some());
        assert!(component.toggle_expanded_on_left_mouse_up);
        assert_eq!(component.selection_prefix.as_ref().unwrap().selected, "▶ ");
    }

    #[test]
    fn rejects_pane_only_inputs_from_every_file_view_component_surface() {
        for field in [
            "content",
            "selectedContent",
            "expandedContent",
            "selectedExpandedContent",
        ] {
            let mut component = serde_json::Map::from_iter([
                ("height".into(), json!(2)),
                ("content".into(), json!({"type": "empty"})),
                ("expandedContent".into(), json!({"type": "empty"})),
            ]);
            component.insert(
                field.into(),
                json!({
                    "type": "input",
                    "id": "prompt",
                    "value": "",
                    "focused": true
                }),
            );
            let error = validate_file_view_layout(
                &json!({
                    "rows": [{"id": "custom", "spans": [], "component": component}],
                    "hunkRows": []
                }),
                0,
                80,
            )
            .unwrap_err();
            assert_eq!(
                error,
                format!("rows[0].component.{field} contains a pane-only input")
            );
        }
    }

    #[test]
    fn rejects_invalid_and_resource_heavy_custom_rows() {
        let invalid =
            json!({"rows": [{"id": "invalid", "spans": [], "component": "bad"}], "hunkRows": []});
        assert_eq!(
            validate_file_view_layout(&invalid, 0, 80).unwrap_err(),
            "rows[0].component is not an object"
        );
        let missing_content = json!({
            "rows": [{"id": "invalid", "spans": [], "component": {"height": 2}}],
            "hunkRows": []
        });
        assert_eq!(
            validate_file_view_layout(&missing_content, 0, 80).unwrap_err(),
            "rows[0].component.content is not a declarative view"
        );
        let missing_expanded = json!({
            "rows": [{"id": "invalid", "spans": [], "component": {
                "height": 2,
                "content": {"type": "empty"},
                "toggleExpandedOnLeftMouseUp": true
            }}],
            "hunkRows": []
        });
        assert_eq!(
            validate_file_view_layout(&missing_expanded, 0, 80).unwrap_err(),
            "rows[0].component toggles without expandedContent"
        );
        let tall = json!({
            "rows": [{"id": "tall", "spans": [], "component": {"height": 257, "content": {"type": "empty"}}}],
            "hunkRows": []
        });
        assert_eq!(
            validate_file_view_layout(&tall, 0, 80).unwrap_err(),
            "rows[0].component.height must be an integer from 1 to 256"
        );

        let rows = (0..391)
            .map(|index| {
                json!({
                    "id": format!("row-{index}"), "spans": [],
                    "component": {"height": 256, "content": {"type": "empty"}}
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            validate_file_view_layout(&json!({"rows": rows, "hunkRows": []}), 0, 80).unwrap_err(),
            "layout exceeds 100000 terminal rows"
        );

        let symbolic_rows = (0..FILE_VIEW_MAX_ROWS)
            .map(|index| {
                json!({"id": format!("symbolic-{index}"), "spans": [{"text": "xxxxxxxxxxx"}]})
            })
            .collect::<Vec<_>>();
        assert_eq!(
            validate_file_view_layout(&json!({"rows": symbolic_rows, "hunkRows": []}), 0, 1,)
                .unwrap_err(),
            "layout exceeds 100000 terminal rows"
        );
    }

    #[test]
    fn rejects_layouts_without_positional_hunk_geometry() {
        let value = json!({
            "rows": [{"id": "one", "spans": [{"text": "one"}]}],
            "hunkRows": [{"startRow": 0, "endRow": 0}]
        });
        assert_eq!(
            validate_file_view_layout(&value, 2, 80).unwrap_err(),
            "layout has 1 hunk bounds for 2 hunks"
        );
    }

    #[test]
    fn rejects_duplicate_ids_and_non_generic_tones() {
        let duplicate = json!({
            "rows": [
                {"id": "same", "spans": [{"text": "one"}]},
                {"id": "same", "spans": [{"text": "two"}]}
            ],
            "hunkRows": []
        });
        assert_eq!(
            validate_file_view_layout(&duplicate, 0, 80).unwrap_err(),
            "rows[1] repeats id \"same\""
        );
        for tone in ["heading", "text"] {
            let value = json!({"rows": [{"id": "one", "spans": [{"text": "one", "tone": tone}]}], "hunkRows": []});
            assert_eq!(
                validate_file_view_layout(&value, 0, 80).unwrap_err(),
                "rows[0] contains an invalid span tone"
            );
        }
    }
}
