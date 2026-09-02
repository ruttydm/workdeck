//! Absolute multi-file review-stream geometry.

use std::collections::BTreeSet;
use workdeck_core::DiffFile;

pub const DEFAULT_FILE_GAP: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSectionLayout {
    pub file_id: String,
    pub section_index: usize,
    pub section_top: i64,
    pub header_top: i64,
    pub body_top: i64,
    pub body_height: i64,
    pub section_bottom: i64,
}

#[must_use]
pub const fn in_stream_file_header_height(section_index: usize) -> i64 {
    if section_index == 0 { 0 } else { 1 }
}

#[must_use]
pub const fn should_render_in_stream_file_header(section_index: usize) -> bool {
    in_stream_file_header_height(section_index) > 0
}

#[must_use]
pub fn build_in_stream_file_header_heights(files: &[DiffFile]) -> Vec<i64> {
    files
        .iter()
        .enumerate()
        .map(|(index, _)| in_stream_file_header_height(index))
        .collect()
}

/// Build absolute offsets from file order and measured row heights. Missing and negative heights
/// clamp to zero, matching the source projection's defensive layout policy.
#[must_use]
pub fn build_file_section_layouts(
    files: &[DiffFile],
    body_heights: &[i64],
    header_heights: Option<&[i64]>,
    file_gap: i64,
) -> Vec<FileSectionLayout> {
    let mut layouts = Vec::with_capacity(files.len());
    let mut cursor = 0_i64;
    for (index, file) in files.iter().enumerate() {
        let separator_height = if index > 0 { file_gap.max(0) } else { 0 };
        let header_height = header_heights
            .and_then(|heights| heights.get(index))
            .copied()
            .unwrap_or_else(|| in_stream_file_header_height(index))
            .max(0);
        let body_height = body_heights.get(index).copied().unwrap_or_default().max(0);
        let section_top = cursor;
        let header_top = section_top.saturating_add(separator_height);
        let body_top = header_top.saturating_add(header_height);
        let section_bottom = body_top.saturating_add(body_height);
        layouts.push(FileSectionLayout {
            file_id: review_file_id(file).to_owned(),
            section_index: index,
            section_top,
            header_top,
            body_top,
            body_height,
            section_bottom,
        });
        cursor = section_bottom;
    }
    layouts
}

#[must_use]
pub fn find_file_section_at_offset(
    layouts: &[FileSectionLayout],
    offset: i64,
) -> Option<&FileSectionLayout> {
    let first = layouts.first()?;
    let last = layouts.last()?;
    if offset <= first.section_top {
        return Some(first);
    }
    if offset >= last.section_bottom {
        return Some(last);
    }
    let mut low = 0;
    let mut high = layouts.len() - 1;
    while low <= high {
        let mid = low + (high - low) / 2;
        let layout = &layouts[mid];
        if offset < layout.section_top {
            let Some(next_high) = mid.checked_sub(1) else {
                break;
            };
            high = next_high;
        } else if offset >= layout.section_bottom {
            low = mid + 1;
        } else {
            return Some(layout);
        }
    }
    Some(last)
}

fn first_potentially_intersecting_index(layouts: &[FileSectionLayout], min_y: i64) -> usize {
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
    low
}

#[must_use]
pub fn collect_intersecting_file_section_ids(
    layouts: &[FileSectionLayout],
    min_y: i64,
    max_y: i64,
) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    if layouts.is_empty() || max_y < min_y {
        return result;
    }
    for layout in layouts
        .iter()
        .skip(first_potentially_intersecting_index(layouts, min_y))
    {
        if layout.section_top > max_y {
            break;
        }
        result.insert(layout.file_id.clone());
    }
    result
}

/// Return the section owning the sticky viewport header. Separator rows remain owned by the
/// preceding file until the next file header itself reaches the viewport.
#[must_use]
pub fn find_header_owning_file_section(
    layouts: &[FileSectionLayout],
    scroll_top: i64,
) -> Option<&FileSectionLayout> {
    layouts.first()?;
    let mut low = 0;
    let mut high = layouts.len();
    while low < high {
        let mid = low + (high - low) / 2;
        if layouts[mid].header_top <= scroll_top {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    layouts.get(low.saturating_sub(1))
}

fn review_file_id(file: &DiffFile) -> &str {
    if file.runtime_id.is_empty() {
        &file.key
    } else {
        &file.runtime_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::ChangesetSource;
    use workdeck_diff::parse_patch;

    fn file(id: &str) -> DiffFile {
        let mut file = parse_patch(
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n",
            "sections",
            "Sections",
            ChangesetSource::Patch {
                label: "sections".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        file.runtime_id = id.into();
        file
    }

    fn layouts() -> Vec<FileSectionLayout> {
        vec![
            FileSectionLayout {
                file_id: "alpha".into(),
                section_index: 0,
                section_top: 0,
                header_top: 0,
                body_top: 0,
                body_height: 5,
                section_bottom: 5,
            },
            FileSectionLayout {
                file_id: "beta".into(),
                section_index: 1,
                section_top: 5,
                header_top: 6,
                body_top: 7,
                body_height: 4,
                section_bottom: 11,
            },
            FileSectionLayout {
                file_id: "gamma".into(),
                section_index: 2,
                section_top: 11,
                header_top: 12,
                body_top: 13,
                body_height: 6,
                section_bottom: 19,
            },
        ]
    }

    #[test]
    fn offset_lookup_contains_and_clamps_past_both_ends() {
        let layouts = layouts();
        assert!(find_file_section_at_offset(&[], 3).is_none());
        for (offset, expected) in [
            (-5, "alpha"),
            (4, "alpha"),
            (5, "beta"),
            (10, "beta"),
            (11, "gamma"),
            (99, "gamma"),
        ] {
            assert_eq!(
                find_file_section_at_offset(&layouts, offset)
                    .unwrap()
                    .file_id,
                expected
            );
        }
    }

    #[test]
    fn intersection_collects_every_overlapping_section() {
        let layouts = layouts();
        let ids = |min_y, max_y| {
            collect_intersecting_file_section_ids(&layouts, min_y, max_y)
                .into_iter()
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(6, 10), ["beta"]);
        assert_eq!(ids(4, 12), ["alpha", "beta", "gamma"]);
        assert!(ids(20, 24).is_empty());
        assert!(ids(10, 6).is_empty());
    }

    #[test]
    fn intersection_search_handles_ten_thousand_ordered_layouts() {
        let layouts = (0..10_000)
            .map(|index| FileSectionLayout {
                file_id: format!("file:{index}"),
                section_index: index,
                section_top: index as i64 * 3,
                header_top: index as i64 * 3,
                body_top: index as i64 * 3,
                body_height: 2,
                section_bottom: index as i64 * 3 + 2,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            collect_intersecting_file_section_ids(&layouts, 15_000, 15_006)
                .into_iter()
                .collect::<Vec<_>>(),
            ["file:5000", "file:5001", "file:5002"]
        );
    }

    #[test]
    fn configurable_file_gaps_shift_later_offsets_and_keep_first_flush() {
        let files = [file("a"), file("b")];
        let gap0 = build_file_section_layouts(&files, &[5, 4], None, 0);
        assert_eq!(
            (gap0[0].section_top, gap0[0].header_top, gap0[0].body_top),
            (0, 0, 0)
        );
        assert_eq!(
            (gap0[1].section_top, gap0[1].header_top, gap0[1].body_top),
            (5, 5, 6)
        );
        let gap1 = build_file_section_layouts(&files, &[5, 4], None, 1);
        assert_eq!(
            (gap1[1].section_top, gap1[1].header_top, gap1[1].body_top),
            (5, 6, 7)
        );
        let gap3 = build_file_section_layouts(&files, &[5, 4], None, 3);
        assert_eq!(
            (
                gap3[1].section_top,
                gap3[1].header_top,
                gap3[1].body_top,
                gap3[1].section_bottom
            ),
            (5, 8, 9, 13)
        );
    }

    #[test]
    fn header_helpers_and_owner_switch_at_the_next_header_not_separator() {
        assert_eq!(in_stream_file_header_height(0), 0);
        assert_eq!(in_stream_file_header_height(1), 1);
        assert!(!should_render_in_stream_file_header(0));
        assert!(should_render_in_stream_file_header(1));
        assert_eq!(
            build_in_stream_file_header_heights(&[file("a"), file("b")]),
            [0, 1]
        );
        let layouts = layouts();
        assert_eq!(
            find_header_owning_file_section(&layouts, 5)
                .unwrap()
                .file_id,
            "alpha"
        );
        assert_eq!(
            find_header_owning_file_section(&layouts, 6)
                .unwrap()
                .file_id,
            "beta"
        );
        assert_eq!(
            find_header_owning_file_section(&layouts, 11)
                .unwrap()
                .file_id,
            "beta"
        );
        assert_eq!(
            find_header_owning_file_section(&layouts, 12)
                .unwrap()
                .file_id,
            "gamma"
        );
    }

    #[test]
    fn negative_or_missing_measurements_clamp_to_zero() {
        let files = [file("a"), file("b")];
        let layouts = build_file_section_layouts(&files, &[-3], Some(&[-2, -4]), -1);
        assert_eq!(layouts[0].section_bottom, 0);
        assert_eq!(layouts[1].section_top, 0);
        assert_eq!(layouts[1].section_bottom, 0);
    }

    #[test]
    fn configurable_gap_matches_main_oracle_and_records_stable_predecessor() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/file-render-window.json"
        ))
        .unwrap();
        let files = [file("a"), file("b")];
        let layouts = build_file_section_layouts(&files, &[5, 4], None, 3);
        assert_eq!(
            serde_json::json!({
                "sectionTop": layouts[1].section_top,
                "headerTop": layouts[1].header_top,
                "bodyTop": layouts[1].body_top,
                "sectionBottom": layouts[1].section_bottom,
            }),
            oracle["gap3Projection"]["baseline"]["second"]
        );
        assert_eq!(
            oracle["gap3Projection"]["stable"]["second"]["sectionBottom"],
            11
        );
    }
}
