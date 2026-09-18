//! File and hunk ownership at the center of the review viewport.

use workdeck_core::DiffFile;

use crate::{FileSectionLayout, ViewportSectionGeometry, find_file_section_at_offset};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportCenteredHunkTarget {
    pub file_id: String,
    pub hunk_index: usize,
}

fn nearest_hunk_index_at_body_offset(
    section_geometry: Option<&ViewportSectionGeometry>,
    body_offset: usize,
    hunk_count: usize,
) -> usize {
    let Some(section_geometry) = section_geometry else {
        return 0;
    };
    if hunk_count <= 1 || section_geometry.hunk_bounds.is_empty() {
        return 0;
    }

    let mut nearest_hunk_index = 0;
    let mut nearest_distance = usize::MAX;
    for hunk_index in 0..hunk_count {
        let Some(bounds) = section_geometry.hunk_bounds.get(&hunk_index) else {
            continue;
        };
        let hunk_bottom = bounds.top.saturating_add(bounds.height).saturating_sub(1);
        if body_offset >= bounds.top && body_offset <= hunk_bottom {
            return hunk_index;
        }
        let distance = if body_offset < bounds.top {
            bounds.top - body_offset
        } else {
            body_offset - hunk_bottom
        };
        if distance < nearest_distance
            || (distance == nearest_distance && hunk_index > nearest_hunk_index)
        {
            nearest_distance = distance;
            nearest_hunk_index = hunk_index;
        }
    }
    nearest_hunk_index
}

#[must_use]
pub fn find_viewport_centered_hunk_target(
    files: &[DiffFile],
    file_section_layouts: &[FileSectionLayout],
    section_geometry: &[ViewportSectionGeometry],
    scroll_top: i64,
    viewport_height: i64,
) -> Option<ViewportCenteredHunkTarget> {
    if files.is_empty() || file_section_layouts.is_empty() {
        return None;
    }
    let center_delta = viewport_height.saturating_sub(1).div_euclid(2).max(0);
    let center_offset = scroll_top.saturating_add(center_delta).max(0);
    let centered_section = find_file_section_at_offset(file_section_layouts, center_offset)?;
    let centered_file = files.get(centered_section.section_index)?;
    let body_offset = usize::try_from(center_offset.saturating_sub(centered_section.body_top))
        .unwrap_or_default();
    let file_id = if centered_file.runtime_id.is_empty() {
        centered_file.key.clone()
    } else {
        centered_file.runtime_id.clone()
    };
    Some(ViewportCenteredHunkTarget {
        file_id,
        hunk_index: nearest_hunk_index_at_body_offset(
            section_geometry.get(centered_section.section_index),
            body_offset,
            centered_file.hunks.len(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::Value;
    use workdeck_core::{ChangesetSource, DiffFile};
    use workdeck_diff::parse_patch;

    use crate::{VerticalBounds, build_file_section_layouts};

    use super::*;

    fn file(id: &str, two_hunks: bool) -> DiffFile {
        let patch = if two_hunks {
            "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-old\n+new\n@@ -60 +60 @@\n-old-60\n+new-60\n"
        } else {
            "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-old\n+new\n"
        };
        let mut file = parse_patch(
            patch,
            "viewport-selection",
            "Viewport selection",
            ChangesetSource::Patch {
                label: "viewport-selection".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        file.runtime_id = id.into();
        file
    }

    fn geometry(body_height: usize, bounds: &[(usize, usize)]) -> ViewportSectionGeometry {
        ViewportSectionGeometry::new(
            body_height,
            Vec::new(),
            bounds
                .iter()
                .enumerate()
                .map(|(index, (top, height))| {
                    (
                        index,
                        VerticalBounds {
                            top: *top,
                            height: *height,
                        },
                    )
                })
                .collect::<HashMap<_, _>>(),
        )
    }

    fn oracle() -> Value {
        serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/viewport-navigation.json"
        ))
        .unwrap()
    }

    #[test]
    fn viewport_center_entering_a_later_file_selects_its_first_hunk() {
        let files = [file("first", false), file("second", true)];
        let geometries = [geometry(3, &[(0, 3)]), geometry(15, &[(0, 5), (6, 8)])];
        let layouts = build_file_section_layouts(&files, &[3, 15], Some(&[0, 1]), 1);
        let second_hunk_top = layouts[1].body_top + geometries[1].hunk_bounds[&0].top as i64;
        let viewport_height = 7;
        let scroll_top = (second_hunk_top - (viewport_height - 1) / 2).max(0);
        let target = find_viewport_centered_hunk_target(
            &files,
            &layouts,
            &geometries,
            scroll_top,
            viewport_height,
        )
        .unwrap();

        let expected = &oracle()["sharedProjection"]["laterFileTarget"];
        assert_eq!(target.file_id, expected["fileId"]);
        assert_eq!(target.hunk_index, expected["hunkIndex"]);
    }

    #[test]
    fn exact_gap_tie_favors_the_later_hunk() {
        let files = [file("gap", true)];
        let geometries = [geometry(15, &[(0, 5), (6, 8)])];
        let layouts = build_file_section_layouts(&files, &[15], Some(&[0]), 1);
        let expected = &oracle()["sharedProjection"]["gap"];
        let target = find_viewport_centered_hunk_target(
            &files,
            &layouts,
            &geometries,
            expected["scrollTop"].as_i64().unwrap(),
            7,
        )
        .unwrap();

        assert_eq!(target.file_id, expected["target"]["fileId"]);
        assert_eq!(target.hunk_index, expected["target"]["hunkIndex"]);
    }

    #[test]
    fn empty_inputs_missing_geometry_and_contained_offsets_match_defaults() {
        assert!(find_viewport_centered_hunk_target(&[], &[], &[], 0, 7).is_none());

        let files = [file("gap", true)];
        let layouts = build_file_section_layouts(&files, &[15], Some(&[0]), 1);
        assert_eq!(
            find_viewport_centered_hunk_target(&files, &layouts, &[], 7, 1)
                .unwrap()
                .hunk_index,
            0
        );
        let geometries = [geometry(15, &[(0, 5), (6, 8)])];
        assert_eq!(
            find_viewport_centered_hunk_target(&files, &layouts, &geometries, 8, 1)
                .unwrap()
                .hunk_index,
            1
        );
    }
}
