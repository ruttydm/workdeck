//! MIT translation of DiffPane.tsx's selected/adjacent/viewport highlight policy.
//! Runtime wiring is separate from this pure policy; see the port ledger for coverage.

use std::collections::BTreeSet;

use crate::{FileSectionLayout, collect_intersecting_file_section_ids};

pub fn adjacent_highlight_prefetch_ids(files: &[&str], selected: Option<&str>) -> BTreeSet<String> {
    let Some(selected) = selected.filter(|id| !id.is_empty()) else {
        return BTreeSet::new();
    };
    let Some(index) = files.iter().position(|id| *id == selected) else {
        return BTreeSet::new();
    };
    index
        .checked_sub(1)
        .and_then(|index| files.get(index))
        .into_iter()
        .chain(files.get(index + 1))
        .map(|id| (*id).to_owned())
        .collect()
}

pub fn highlight_prefetch_ids(
    adjacent: &BTreeSet<String>,
    layouts: &[FileSectionLayout],
    rapid_scroll_overscan_rows: usize,
    scroll_top: i64,
    viewport_height: i64,
    selected: Option<&str>,
) -> BTreeSet<String> {
    let mut next = adjacent.clone();
    if let Some(selected) = selected.filter(|id| !id.is_empty()) {
        next.insert(selected.into());
    }
    let halo = 24_i64
        .max(viewport_height.max(1).saturating_mul(3))
        .max(i64::try_from(rapid_scroll_overscan_rows).unwrap_or(i64::MAX));
    let min_y = scroll_top.saturating_sub(halo).max(0);
    let max_y = scroll_top
        .saturating_add(viewport_height)
        .saturating_add(halo);
    next.extend(collect_intersecting_file_section_ids(layouts, min_y, max_y));
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_pins_match_prefetch_policy_including_halo_and_selection_edges() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/highlight-prefetch-policy.json"
        ))
        .unwrap();
        let files = ["f0", "f1", "f2", "f3", "f4", "f5"];
        let layouts = files
            .iter()
            .enumerate()
            .map(|(index, id)| FileSectionLayout {
                file_id: (*id).into(),
                section_index: index,
                section_top: index as i64 * 100,
                header_top: index as i64 * 100,
                body_top: index as i64 * 100 + 1,
                body_height: 99,
                section_bottom: (index as i64 + 1) * 100,
            })
            .collect::<Vec<_>>();
        assert_eq!(fixture["runs"].as_array().unwrap().len(), 2);
        for run in fixture["runs"].as_array().unwrap() {
            assert_eq!(run["cases"].as_array().unwrap().len(), 8);
            for case in run["cases"].as_array().unwrap() {
                let selected = case["selected"].as_str();
                let adjacent = adjacent_highlight_prefetch_ids(&files, selected);
                let actual = highlight_prefetch_ids(
                    &adjacent,
                    &layouts,
                    case["rapid"].as_u64().unwrap() as usize,
                    case["top"].as_i64().unwrap(),
                    case["height"].as_i64().unwrap(),
                    selected,
                );
                let expected = case["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_str().unwrap().to_owned())
                    .collect::<BTreeSet<_>>();
                assert_eq!(actual, expected, "{case}");
                let expected_adjacent = case["adjacent"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_str().unwrap().to_owned())
                    .collect::<BTreeSet<_>>();
                assert_eq!(adjacent, expected_adjacent);
            }
        }
    }
}
