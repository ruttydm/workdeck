//! Stable top-row capture and restoration across review relayouts.

use workdeck_core::DiffFile;

use crate::{
    DEFAULT_FILE_GAP, ViewportRowBounds, ViewportSectionGeometry, build_file_section_layouts,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportRowAnchor {
    pub file_id: String,
    pub row_key: String,
    pub stable_key: String,
    pub row_offset_within: usize,
}

fn binary_search_row_bounds(
    section_row_bounds: &[ViewportRowBounds],
    relative_top: usize,
) -> Option<&ViewportRowBounds> {
    let mut low = 0;
    let mut high = section_row_bounds.len();
    while low < high {
        let middle = low + (high - low) / 2;
        let bounds = &section_row_bounds[middle];
        if relative_top < bounds.top {
            high = middle;
        } else if relative_top >= bounds.top.saturating_add(bounds.height) {
            low = middle + 1;
        } else {
            return Some(bounds);
        }
    }
    None
}

fn section_body_heights(section_geometry: &[ViewportSectionGeometry]) -> Vec<i64> {
    section_geometry
        .iter()
        .map(|geometry| i64::try_from(geometry.body_height).unwrap_or(i64::MAX))
        .collect()
}

fn file_id(file: &DiffFile) -> &str {
    if file.runtime_id.is_empty() {
        &file.key
    } else {
        &file.runtime_id
    }
}

#[must_use]
pub fn find_viewport_row_anchor(
    files: &[DiffFile],
    section_geometry: &[ViewportSectionGeometry],
    scroll_top: i64,
    header_heights: &[i64],
    preferred_stable_key: Option<&str>,
) -> Option<ViewportRowAnchor> {
    find_viewport_row_anchor_with_gap(
        files,
        section_geometry,
        scroll_top,
        header_heights,
        preferred_stable_key,
        DEFAULT_FILE_GAP,
    )
}

#[must_use]
pub fn find_viewport_row_anchor_with_gap(
    files: &[DiffFile],
    section_geometry: &[ViewportSectionGeometry],
    scroll_top: i64,
    header_heights: &[i64],
    preferred_stable_key: Option<&str>,
    file_gap: i64,
) -> Option<ViewportRowAnchor> {
    let body_heights = section_body_heights(section_geometry);
    let layouts = build_file_section_layouts(files, &body_heights, Some(header_heights), file_gap);

    for (index, file) in files.iter().enumerate() {
        let body_top = layouts.get(index).map_or(0, |layout| layout.body_top);
        let Some(geometry) = section_geometry.get(index) else {
            continue;
        };
        let relative_top = scroll_top.saturating_sub(body_top);
        let body_height = i64::try_from(geometry.body_height).unwrap_or(i64::MAX);
        if relative_top < 0 || relative_top >= body_height {
            continue;
        }
        let relative_top = usize::try_from(relative_top).ok()?;
        let Some(bounds) = binary_search_row_bounds(&geometry.row_bounds, relative_top) else {
            continue;
        };
        let stable_key = preferred_stable_key
            .filter(|preferred| bounds.stable_keys.iter().any(|key| key == preferred))
            .unwrap_or(&bounds.stable_key);
        return Some(ViewportRowAnchor {
            file_id: file_id(file).to_owned(),
            row_key: bounds.key.clone(),
            stable_key: stable_key.to_owned(),
            row_offset_within: relative_top.saturating_sub(bounds.top),
        });
    }
    None
}

#[must_use]
pub fn resolve_viewport_row_anchor_top(
    files: &[DiffFile],
    section_geometry: &[ViewportSectionGeometry],
    anchor: &ViewportRowAnchor,
    header_heights: &[i64],
) -> i64 {
    resolve_viewport_row_anchor_top_with_gap(
        files,
        section_geometry,
        anchor,
        header_heights,
        DEFAULT_FILE_GAP,
    )
}

#[must_use]
pub fn resolve_viewport_row_anchor_top_with_gap(
    files: &[DiffFile],
    section_geometry: &[ViewportSectionGeometry],
    anchor: &ViewportRowAnchor,
    header_heights: &[i64],
    file_gap: i64,
) -> i64 {
    let body_heights = section_body_heights(section_geometry);
    let layouts = build_file_section_layouts(files, &body_heights, Some(header_heights), file_gap);

    for (index, file) in files.iter().enumerate() {
        let body_top = layouts.get(index).map_or(0, |layout| layout.body_top);
        let Some(geometry) = section_geometry.get(index) else {
            continue;
        };
        if file_id(file) != anchor.file_id {
            continue;
        }
        let bounds = geometry
            .bounds_for_stable_key(&anchor.stable_key)
            .or_else(|| geometry.bounds_for_key(&anchor.row_key));
        let Some(bounds) = bounds else {
            return body_top;
        };
        let row_offset = anchor
            .row_offset_within
            .min(bounds.height.saturating_sub(1));
        let row_top = bounds.top.saturating_add(row_offset);
        return body_top.saturating_add(i64::try_from(row_top).unwrap_or(i64::MAX));
    }
    0
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::Value;
    use workdeck_core::ChangesetSource;
    use workdeck_diff::parse_patch;

    use super::*;

    fn changed_file(id: &str) -> DiffFile {
        let mut file = parse_patch(
            "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-const alpha = 1;\n+const alpha = 2;\n",
            "viewport",
            "Viewport",
            ChangesetSource::Patch {
                label: "viewport".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        file.runtime_id = id.into();
        file
    }

    fn row(key: &str, stable_key: &str, aliases: &[&str], top: usize) -> ViewportRowBounds {
        let mut stable_keys = vec![stable_key.to_owned()];
        stable_keys.extend(aliases.iter().map(|key| (*key).to_owned()));
        ViewportRowBounds {
            key: key.into(),
            stable_key: stable_key.into(),
            stable_keys,
            expanded_gap_key: None,
            top,
            height: 1,
        }
    }

    fn stack_geometry() -> ViewportSectionGeometry {
        ViewportSectionGeometry::new(
            2,
            vec![
                row(
                    "diff-row:viewport-anchor:stack:0:deletion:0",
                    "line:0:old:1",
                    &[],
                    0,
                ),
                row(
                    "diff-row:viewport-anchor:stack:0:addition:0",
                    "line:0:new:1",
                    &[],
                    1,
                ),
            ],
            HashMap::new(),
        )
    }

    fn split_geometry() -> ViewportSectionGeometry {
        ViewportSectionGeometry::new(
            1,
            vec![row(
                "diff-row:viewport-anchor:split:0:change:0:0",
                "line:0:new:1",
                &["line:0:old:1"],
                0,
            )],
            HashMap::new(),
        )
    }

    fn oracle() -> Value {
        serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/viewport-navigation.json"
        ))
        .unwrap()
    }

    #[test]
    fn preferred_stable_key_follows_each_stacked_side_into_one_split_change_row() {
        let file = changed_file("viewport-anchor");
        let stack = stack_geometry();
        let split = split_geometry();
        let deletion = find_viewport_row_anchor(
            std::slice::from_ref(&file),
            std::slice::from_ref(&stack),
            0,
            &[0],
            None,
        )
        .expect("deletion row anchor");
        let addition = find_viewport_row_anchor(
            std::slice::from_ref(&file),
            std::slice::from_ref(&stack),
            1,
            &[0],
            None,
        )
        .expect("addition row anchor");
        let split_as_deletion = find_viewport_row_anchor(
            std::slice::from_ref(&file),
            std::slice::from_ref(&split),
            0,
            &[0],
            Some(&deletion.stable_key),
        )
        .expect("split deletion alias");
        let split_as_addition = find_viewport_row_anchor(
            std::slice::from_ref(&file),
            std::slice::from_ref(&split),
            0,
            &[0],
            Some(&addition.stable_key),
        )
        .expect("split addition alias");

        let expected = &oracle()["sharedProjection"]["anchor"];
        assert_eq!(deletion.file_id, expected["deletion"]["fileId"]);
        assert_eq!(deletion.row_key, expected["deletion"]["rowKey"]);
        assert_eq!(deletion.stable_key, expected["deletion"]["stableKey"]);
        assert_eq!(addition.stable_key, expected["addition"]["stableKey"]);
        assert_eq!(
            split_as_deletion.stable_key,
            expected["splitAsDeletion"]["stableKey"]
        );
        assert_eq!(
            split_as_addition.stable_key,
            expected["splitAsAddition"]["stableKey"]
        );
    }

    #[test]
    fn stacked_deletion_round_trips_through_split_without_moving_the_anchor() {
        let file = changed_file("viewport-anchor");
        let stack = stack_geometry();
        let split = split_geometry();
        let deletion = find_viewport_row_anchor(
            std::slice::from_ref(&file),
            std::slice::from_ref(&stack),
            0,
            &[0],
            None,
        )
        .unwrap();
        let split_top = resolve_viewport_row_anchor_top(
            std::slice::from_ref(&file),
            std::slice::from_ref(&split),
            &deletion,
            &[0],
        );
        let split_anchor = find_viewport_row_anchor(
            std::slice::from_ref(&file),
            std::slice::from_ref(&split),
            split_top,
            &[0],
            Some(&deletion.stable_key),
        )
        .unwrap();
        let round_trip = resolve_viewport_row_anchor_top(
            std::slice::from_ref(&file),
            std::slice::from_ref(&stack),
            &split_anchor,
            &[0],
        );

        let expected = &oracle()["sharedProjection"]["anchor"];
        assert_eq!(split_top, expected["splitResolved"]);
        assert_eq!(round_trip, expected["roundTrip"]);
    }

    #[test]
    fn resolve_clamps_offsets_falls_back_to_row_key_and_handles_retired_rows() {
        let file = changed_file("viewport-anchor");
        let geometry = ViewportSectionGeometry::new(
            3,
            vec![ViewportRowBounds {
                key: "fallback".into(),
                stable_key: "current".into(),
                stable_keys: vec!["current".into()],
                expanded_gap_key: None,
                top: 1,
                height: 2,
            }],
            HashMap::new(),
        );
        let fallback = ViewportRowAnchor {
            file_id: "viewport-anchor".into(),
            row_key: "fallback".into(),
            stable_key: "retired-stable-key".into(),
            row_offset_within: 99,
        };
        assert_eq!(
            resolve_viewport_row_anchor_top(
                std::slice::from_ref(&file),
                std::slice::from_ref(&geometry),
                &fallback,
                &[0]
            ),
            2
        );

        let retired = ViewportRowAnchor {
            row_key: "retired-row".into(),
            ..fallback.clone()
        };
        assert_eq!(
            resolve_viewport_row_anchor_top(
                std::slice::from_ref(&file),
                std::slice::from_ref(&geometry),
                &retired,
                &[0]
            ),
            0
        );
        let missing_file = ViewportRowAnchor {
            file_id: "missing".into(),
            ..fallback
        };
        assert_eq!(
            resolve_viewport_row_anchor_top(std::slice::from_ref(&file), &[], &missing_file, &[0]),
            0
        );
    }
}
