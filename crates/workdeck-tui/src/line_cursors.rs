//! Measured line-granular navigation for the continuous review stream.

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use workdeck_core::ReviewSide;

use crate::{VerticalBounds, ViewportRowBounds, ViewportSectionGeometry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineCursorFile {
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCursorTarget {
    pub side: ReviewSide,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineCursor {
    pub file_id: String,
    pub hunk_index: usize,
    /// Render-plan anchor shared with reveal, highlight, and viewport lookups.
    pub stable_key: String,
    pub target: LineCursorTarget,
    /// Exact collapsed gap that produced this cursor, when source expansion revealed it.
    pub expanded_gap_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LineCursorList {
    cursors: Arc<[LineCursor]>,
    indexes: Arc<HashMap<(String, String), usize>>,
}

impl Default for LineCursorList {
    fn default() -> Self {
        Self::from(Vec::new())
    }
}

impl From<Vec<LineCursor>> for LineCursorList {
    fn from(cursors: Vec<LineCursor>) -> Self {
        let indexes = cursors
            .iter()
            .enumerate()
            .map(|(index, cursor)| ((cursor.file_id.clone(), cursor.stable_key.clone()), index))
            .collect();
        Self {
            cursors: cursors.into(),
            indexes: Arc::new(indexes),
        }
    }
}

impl Deref for LineCursorList {
    type Target = [LineCursor];

    fn deref(&self) -> &Self::Target {
        &self.cursors
    }
}

impl PartialEq for LineCursorList {
    fn eq(&self, other: &Self) -> bool {
        self.cursors == other.cursors
    }
}

impl Eq for LineCursorList {}

impl LineCursorList {
    #[must_use]
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cursors, &other.cursors)
    }

    fn index_of(&self, cursor: &LineCursor) -> Option<usize> {
        self.indexes
            .get(&(cursor.file_id.clone(), cursor.stable_key.clone()))
            .copied()
    }
}

fn parse_number(value: Option<&str>) -> Option<usize> {
    value?.parse().ok()
}

fn parse_line(value: Option<&str>) -> Option<u32> {
    value?.parse().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextStableKey {
    hunk_index: usize,
    old_line: u32,
    new_line: u32,
}

fn context_line_stable_key_sides(stable_key: &str) -> Option<ContextStableKey> {
    let mut parts = stable_key.split(':');
    if parts.next()? != "line" {
        return None;
    }
    let hunk_index = parse_number(parts.next())?;
    if parts.next()? != "context" {
        return None;
    }
    let old_line = parse_line(parts.next())?;
    let new_line = parse_line(parts.next())?;
    parts.next().is_none().then_some(ContextStableKey {
        hunk_index,
        old_line,
        new_line,
    })
}

fn line_stable_key_target(stable_key: &str) -> Option<(usize, LineCursorTarget)> {
    let mut parts = stable_key.split(':');
    if parts.next()? != "line" {
        return None;
    }
    let hunk_index = parse_number(parts.next())?;
    let side = match parts.next()? {
        "old" => ReviewSide::Old,
        "new" => ReviewSide::New,
        _ => return None,
    };
    let line = parse_line(parts.next())?;
    parts
        .next()
        .is_none()
        .then_some((hunk_index, LineCursorTarget { side, line }))
}

fn row_line_cursors(file_id: &str, bounds: &ViewportRowBounds) -> Vec<LineCursor> {
    if let Some(context) = context_line_stable_key_sides(&bounds.stable_key) {
        return vec![LineCursor {
            file_id: file_id.into(),
            hunk_index: context.hunk_index,
            stable_key: bounds.stable_key.clone(),
            target: LineCursorTarget {
                side: ReviewSide::New,
                line: context.new_line,
            },
            expanded_gap_key: bounds.expanded_gap_key.clone(),
        }];
    }

    bounds
        .stable_keys
        .iter()
        .filter_map(|stable_key| {
            let (hunk_index, target) = line_stable_key_target(stable_key)?;
            Some(LineCursor {
                file_id: file_id.into(),
                hunk_index,
                stable_key: stable_key.clone(),
                target,
                expanded_gap_key: bounds.expanded_gap_key.clone(),
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
struct CachedGeometryCursors {
    geometry: ViewportSectionGeometry,
    cursors: Arc<[LineCursor]>,
}

/// Review-lifetime cache corresponding to Hunk's weak map keyed by measured section identity.
#[derive(Debug, Default)]
pub struct LineCursorBuilder {
    by_geometry: HashMap<usize, CachedGeometryCursors>,
}

impl LineCursorBuilder {
    fn file_line_cursors(
        &mut self,
        file_id: &str,
        geometry: &ViewportSectionGeometry,
    ) -> Arc<[LineCursor]> {
        let identity = std::ptr::from_ref(geometry) as usize;
        if let Some(cached) = self.by_geometry.get(&identity)
            && cached.geometry == *geometry
        {
            return Arc::clone(&cached.cursors);
        }
        let cursors = geometry
            .row_bounds
            .iter()
            .flat_map(|bounds| row_line_cursors(file_id, bounds))
            .collect::<Vec<_>>()
            .into();
        self.by_geometry.insert(
            identity,
            CachedGeometryCursors {
                geometry: geometry.clone(),
                cursors: Arc::clone(&cursors),
            },
        );
        cursors
    }

    /// Flatten files through cached measured sections into one ordered cursor list.
    #[must_use]
    pub fn build(
        &mut self,
        files: &[LineCursorFile],
        section_geometry: &[ViewportSectionGeometry],
    ) -> LineCursorList {
        let mut cursors = Vec::new();
        for (index, file) in files.iter().enumerate() {
            if let Some(geometry) = section_geometry.get(index) {
                cursors.extend(self.file_line_cursors(&file.id, geometry).iter().cloned());
            }
        }
        cursors.into()
    }

    #[cfg(test)]
    fn cached_geometry_count(&self) -> usize {
        self.by_geometry.len()
    }
}

/// Flatten measured file-section rows into the exact navigable review-stream order.
#[must_use]
pub fn build_line_cursors(
    files: &[LineCursorFile],
    section_geometry: &[ViewportSectionGeometry],
) -> LineCursorList {
    LineCursorBuilder::default().build(files, section_geometry)
}

/// Reuse list/index storage when remeasurement preserved every cursor value.
#[must_use]
pub fn reuse_equivalent_line_cursors(
    previous: &LineCursorList,
    next: LineCursorList,
) -> LineCursorList {
    if previous == &next {
        previous.clone()
    } else {
        next
    }
}

/// Compare only newly measured lists, preserving stable cursor identity across unrelated paints.
#[derive(Debug, Default)]
pub struct LineCursorStabilizer {
    measured: LineCursorList,
    stable: LineCursorList,
}

impl LineCursorStabilizer {
    #[must_use]
    pub fn stabilize(&mut self, next: LineCursorList) -> LineCursorList {
        if !self.measured.shares_storage_with(&next) {
            self.measured = next.clone();
            self.stable = reuse_equivalent_line_cursors(&self.stable, next);
        }
        self.stable.clone()
    }
}

fn nearest_cursor_in_file<'a>(
    cursors: &'a LineCursorList,
    file_id: &str,
    hunk_index: usize,
) -> Option<&'a LineCursor> {
    cursors
        .iter()
        .find(|cursor| cursor.file_id == file_id && cursor.hunk_index == hunk_index)
        .or_else(|| cursors.iter().find(|cursor| cursor.file_id == file_id))
}

/// Find the first cursor in the requested hunk, then anywhere in the same file.
#[must_use]
pub fn first_line_cursor_in_hunk(
    cursors: &LineCursorList,
    file_id: Option<&str>,
    hunk_index: usize,
) -> Option<LineCursor> {
    file_id.map_or_else(
        || cursors.first().cloned(),
        |file_id| nearest_cursor_in_file(cursors, file_id, hunk_index).cloned(),
    )
}

/// Step through the measured cursor list, clamping rather than wrapping at both ends.
#[must_use]
pub fn find_next_line_cursor(
    cursors: &LineCursorList,
    current: Option<&LineCursor>,
    delta: isize,
) -> Option<LineCursor> {
    let current_index = current.and_then(|cursor| cursors.index_of(cursor));
    let Some(current_index) = current_index else {
        return cursors.first().cloned();
    };
    let last = cursors.len().saturating_sub(1);
    let next = current_index.saturating_add_signed(delta).min(last);
    cursors.get(next).cloned()
}

#[must_use]
pub fn has_line_cursor(cursors: &LineCursorList, cursor: Option<&LineCursor>) -> bool {
    cursor.is_some_and(|cursor| cursors.index_of(cursor).is_some())
}

/// Keep a cursor on a real line after filtering or reload retires its exact row.
#[must_use]
pub fn resolve_line_cursor(
    cursors: &LineCursorList,
    current: Option<&LineCursor>,
) -> Option<LineCursor> {
    let current = current?;
    if has_line_cursor(cursors, Some(current)) {
        return Some(current.clone());
    }
    nearest_cursor_in_file(cursors, &current.file_id, current.hunk_index).cloned()
}

fn line_stable_key(hunk_index: usize, target: LineCursorTarget) -> String {
    let side = match target.side {
        ReviewSide::Old => "old",
        ReviewSide::New => "new",
    };
    format!("line:{hunk_index}:{side}:{}", target.line)
}

/// Resolve a navigable stop or synthesize the same stable anchor reveal would use.
#[must_use]
pub fn line_cursor_at(
    cursors: &LineCursorList,
    file_id: &str,
    hunk_index: usize,
    target: LineCursorTarget,
) -> LineCursor {
    cursors
        .iter()
        .find(|cursor| {
            cursor.file_id == file_id && cursor.hunk_index == hunk_index && cursor.target == target
        })
        .cloned()
        .unwrap_or_else(|| LineCursor {
            file_id: file_id.into(),
            hunk_index,
            stable_key: line_stable_key(hunk_index, target),
            target,
            expanded_gap_key: None,
        })
}

fn line_cursor_addresses(cursor: &LineCursor, side: ReviewSide, line: u32) -> bool {
    if cursor.target == (LineCursorTarget { side, line }) {
        return true;
    }
    context_line_stable_key_sides(&cursor.stable_key).is_some_and(|context| match side {
        ReviewSide::Old => context.old_line == line,
        ReviewSide::New => context.new_line == line,
    })
}

/// Find a row the measured stream actually draws, including either address of a context row.
#[must_use]
pub fn find_line_cursor_at(
    cursors: &LineCursorList,
    file_id: &str,
    side: ReviewSide,
    line: u32,
) -> Option<LineCursor> {
    cursors
        .iter()
        .find(|cursor| cursor.file_id == file_id && line_cursor_addresses(cursor, side, line))
        .cloned()
}

fn first_cursor_index_where(
    cursors: &LineCursorList,
    bounds_of: &impl Fn(&LineCursor) -> Option<VerticalBounds>,
    reached: impl Fn(VerticalBounds) -> bool,
) -> usize {
    let mut low = 0;
    let mut high = cursors.len();
    while low < high {
        let middle = (low + high) / 2;
        if bounds_of(&cursors[middle]).is_some_and(&reached) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

/// Snap the current line to the nearest fully visible measured row after viewport scrolling.
#[must_use]
pub fn clamp_line_cursor_to_viewport(
    cursors: &LineCursorList,
    current: Option<&LineCursor>,
    scroll_top: usize,
    viewport_height: usize,
    bounds_of: impl Fn(&LineCursor) -> Option<VerticalBounds>,
) -> Option<LineCursor> {
    if cursors.is_empty() || viewport_height == 0 {
        return current.cloned();
    }

    let viewport_bottom = scroll_top.saturating_add(viewport_height);
    let current_bounds = current.and_then(&bounds_of);
    if current_bounds.is_some_and(|bounds| {
        bounds.top >= scroll_top && bounds.top.saturating_add(bounds.height) <= viewport_bottom
    }) {
        return current.cloned();
    }

    let scrolled_past_above = current_bounds.is_none_or(|bounds| bounds.top < scroll_top);
    let index = if scrolled_past_above {
        first_cursor_index_where(cursors, &bounds_of, |bounds| bounds.top >= scroll_top)
    } else {
        first_cursor_index_where(cursors, &bounds_of, |bounds| {
            bounds.top.saturating_add(bounds.height) > viewport_bottom
        })
        .saturating_sub(1)
    };
    cursors.get(index.min(cursors.len() - 1)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn row(stable_key: &str, aliases: &[&str], top: usize) -> ViewportRowBounds {
        let mut stable_keys = vec![stable_key.into()];
        stable_keys.extend(aliases.iter().map(|key| (*key).into()));
        ViewportRowBounds {
            key: format!("row:{top}"),
            stable_key: stable_key.into(),
            stable_keys,
            expanded_gap_key: None,
            top,
            height: 1,
        }
    }

    fn geometry(rows: Vec<ViewportRowBounds>) -> ViewportSectionGeometry {
        ViewportSectionGeometry::new(rows.len(), rows, HashMap::new())
    }

    fn files(ids: &[&str]) -> Vec<LineCursorFile> {
        ids.iter()
            .map(|id| LineCursorFile { id: (*id).into() })
            .collect()
    }

    fn two_hunk_geometry() -> ViewportSectionGeometry {
        geometry(vec![
            row("line:0:old:1", &[], 0),
            row("line:0:new:1", &[], 1),
            row("line:1:old:10", &[], 2),
            row("line:1:new:10", &[], 3),
        ])
    }

    fn two_file_cursors() -> LineCursorList {
        build_line_cursors(
            &files(&["alpha", "beta"]),
            &[two_hunk_geometry(), two_hunk_geometry()],
        )
    }

    fn target(side: ReviewSide, line: u32) -> LineCursorTarget {
        LineCursorTarget { side, line }
    }

    #[test]
    fn frozen_hunk_line_cursor_oracles_record_main_and_older_stable_shapes() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/line-cursors.json"))
                .unwrap();
        assert_eq!(oracle["baselineOracle"]["passed"], 37);
        assert_eq!(oracle["baselineOracle"]["expectations"], 54);
        assert_eq!(oracle["stableOracle"]["passed"], 33);
        assert_eq!(oracle["stableDifference"]["authoritative"], "baseline");
    }

    #[test]
    fn measured_rows_flatten_context_changes_hunks_files_and_expanded_gaps() {
        let stack = geometry(vec![
            row("line:0:context:1:1", &[], 0),
            row("line:0:old:2", &[], 1),
            row("line:0:new:2", &[], 2),
            row("line:0:context:3:3", &[], 3),
        ]);
        let cursors = build_line_cursors(&files(&["alpha"]), &[stack]);
        assert_eq!(
            cursors
                .iter()
                .map(|cursor| cursor.target)
                .collect::<Vec<_>>(),
            [
                target(ReviewSide::New, 1),
                target(ReviewSide::Old, 2),
                target(ReviewSide::New, 2),
                target(ReviewSide::New, 3),
            ]
        );

        let split = geometry(vec![
            row("line:0:old:1", &["line:0:new:1"], 0),
            row("line:0:old:2", &["line:0:new:2"], 1),
            row("line:0:old:3", &["line:0:new:3"], 2),
        ]);
        let split = build_line_cursors(&files(&["alpha"]), &[split]);
        assert_eq!(
            split.iter().map(|cursor| cursor.target).collect::<Vec<_>>(),
            [
                target(ReviewSide::Old, 1),
                target(ReviewSide::New, 1),
                target(ReviewSide::Old, 2),
                target(ReviewSide::New, 2),
                target(ReviewSide::Old, 3),
                target(ReviewSide::New, 3),
            ]
        );

        let mut expanded = row("line:0:context:1:1", &[], 0);
        expanded.expanded_gap_key = Some("before:0".into());
        let expanded = build_line_cursors(&files(&["alpha"]), &[geometry(vec![expanded])]);
        assert_eq!(expanded[0].expanded_gap_key.as_deref(), Some("before:0"));

        let all = two_file_cursors();
        assert_eq!(all.len(), 8);
        assert!(all[..4].iter().all(|cursor| cursor.file_id == "alpha"));
        assert!(all[4..].iter().all(|cursor| cursor.file_id == "beta"));
        assert!(build_line_cursors(&[], &[]).is_empty());
        assert!(build_line_cursors(&files(&["empty"]), &[geometry(Vec::new())]).is_empty());
    }

    #[test]
    fn hunk_numbers_and_stack_column_order_remain_independent() {
        let cursors = build_line_cursors(&files(&["alpha"]), &[two_hunk_geometry()]);
        assert_eq!(
            cursors
                .iter()
                .map(|cursor| (cursor.hunk_index, cursor.target))
                .collect::<Vec<_>>(),
            [
                (0, target(ReviewSide::Old, 1)),
                (0, target(ReviewSide::New, 1)),
                (1, target(ReviewSide::Old, 10)),
                (1, target(ReviewSide::New, 10)),
            ]
        );
    }

    #[test]
    fn equivalent_remeasurement_reuses_storage_and_stabilizer_skips_same_measurement() {
        let previous = build_line_cursors(&files(&["alpha"]), &[two_hunk_geometry()]);
        let equivalent = LineCursorList::from(previous.iter().cloned().collect::<Vec<_>>());
        let stable = reuse_equivalent_line_cursors(&previous, equivalent);
        assert!(stable.shares_storage_with(&previous));

        let mut changed_values = previous.iter().cloned().collect::<Vec<_>>();
        changed_values[0].target.line += 1;
        let changed = LineCursorList::from(changed_values);
        assert!(
            reuse_equivalent_line_cursors(&previous, changed.clone()).shares_storage_with(&changed)
        );

        let mut stabilizer = LineCursorStabilizer::default();
        let first = stabilizer.stabilize(previous.clone());
        let same_measurement = stabilizer.stabilize(previous.clone());
        assert!(first.shares_storage_with(&same_measurement));
        let remeasured = LineCursorList::from(previous.iter().cloned().collect::<Vec<_>>());
        let equivalent_stable = stabilizer.stabilize(remeasured);
        assert!(first.shares_storage_with(&equivalent_stable));
    }

    #[test]
    fn builder_reuses_a_measured_section_until_its_value_changes() {
        let mut builder = LineCursorBuilder::default();
        let files = files(&["alpha"]);
        let mut geometry = two_hunk_geometry();
        let first = builder.build(&files, std::slice::from_ref(&geometry));
        let second = builder.build(&files, std::slice::from_ref(&geometry));
        assert_eq!(first, second);
        assert_eq!(builder.cached_geometry_count(), 1);

        geometry.row_bounds[0].stable_key = "line:0:old:2".into();
        geometry.row_bounds[0].stable_keys = vec!["line:0:old:2".into()];
        let changed = builder.build(&files, std::slice::from_ref(&geometry));
        assert_ne!(first, changed);
        assert_eq!(builder.cached_geometry_count(), 1);
    }

    #[test]
    fn side_lookup_handles_changed_context_hidden_and_file_scoped_lines() {
        let geometry = geometry(vec![
            row("line:0:new:2", &[], 0),
            row("line:0:context:3:4", &[], 1),
        ]);
        let cursors = build_line_cursors(&files(&["alpha", "beta"]), &[geometry.clone(), geometry]);
        assert_eq!(
            find_line_cursor_at(&cursors, "alpha", ReviewSide::New, 2)
                .unwrap()
                .target,
            target(ReviewSide::New, 2)
        );
        let by_new = find_line_cursor_at(&cursors, "alpha", ReviewSide::New, 4).unwrap();
        let by_old = find_line_cursor_at(&cursors, "alpha", ReviewSide::Old, 3).unwrap();
        assert_eq!(by_new, by_old);
        assert_eq!(
            find_line_cursor_at(&cursors, "beta", ReviewSide::New, 2)
                .unwrap()
                .file_id,
            "beta"
        );
        assert!(find_line_cursor_at(&cursors, "alpha", ReviewSide::New, 3).is_none());
        assert!(find_line_cursor_at(&cursors, "missing", ReviewSide::New, 2).is_none());
    }

    #[test]
    fn stepping_crosses_hunks_and_files_and_clamps_or_recovers_at_edges() {
        let cursors = two_file_cursors();
        assert_eq!(
            find_next_line_cursor(&cursors, Some(&cursors[0]), 1),
            Some(cursors[1].clone())
        );
        assert_eq!(
            find_next_line_cursor(&cursors, Some(&cursors[1]), -1),
            Some(cursors[0].clone())
        );
        assert_eq!(
            find_next_line_cursor(&cursors, Some(&cursors[3]), 1),
            Some(cursors[4].clone())
        );
        assert_eq!(
            find_next_line_cursor(&cursors, Some(&cursors[0]), -1),
            Some(cursors[0].clone())
        );
        assert_eq!(
            find_next_line_cursor(&cursors, Some(&cursors[7]), 1),
            Some(cursors[7].clone())
        );
        assert_eq!(
            find_next_line_cursor(&cursors, None, -1),
            Some(cursors[0].clone())
        );
        let retired = LineCursor {
            file_id: "gamma".into(),
            hunk_index: 4,
            stable_key: "line:4:new:99".into(),
            target: target(ReviewSide::New, 99),
            expanded_gap_key: None,
        };
        assert_eq!(
            find_next_line_cursor(&cursors, Some(&retired), 1),
            Some(cursors[0].clone())
        );
        assert_eq!(
            find_next_line_cursor(&LineCursorList::default(), None, 1),
            None
        );
    }

    #[test]
    fn hunk_seeding_and_reload_resolution_never_escape_the_requested_file() {
        let cursors = two_file_cursors();
        assert_eq!(
            first_line_cursor_in_hunk(&cursors, Some("beta"), 1),
            Some(cursors[6].clone())
        );
        assert_eq!(
            first_line_cursor_in_hunk(&cursors, Some("beta"), 7)
                .unwrap()
                .file_id,
            "beta"
        );
        assert_eq!(
            first_line_cursor_in_hunk(&cursors, None, 0),
            Some(cursors[0].clone())
        );
        assert_eq!(first_line_cursor_in_hunk(&cursors, Some("gamma"), 0), None);
        assert_eq!(
            first_line_cursor_in_hunk(&LineCursorList::default(), Some("alpha"), 0),
            None
        );

        assert_eq!(
            resolve_line_cursor(&cursors, Some(&cursors[2])),
            Some(cursors[2].clone())
        );
        let moved = LineCursor {
            file_id: "alpha".into(),
            hunk_index: 1,
            stable_key: "line:1:new:42".into(),
            target: target(ReviewSide::New, 42),
            expanded_gap_key: None,
        };
        assert_eq!(
            resolve_line_cursor(&cursors, Some(&moved)),
            Some(cursors[2].clone())
        );
        let retired_hunk = LineCursor {
            hunk_index: 9,
            ..moved.clone()
        };
        assert_eq!(
            resolve_line_cursor(&cursors, Some(&retired_hunk))
                .unwrap()
                .file_id,
            "alpha"
        );
        let retired_file = LineCursor {
            file_id: "gamma".into(),
            ..moved
        };
        assert_eq!(resolve_line_cursor(&cursors, Some(&retired_file)), None);
        assert_eq!(resolve_line_cursor(&cursors, None), None);
    }

    #[test]
    fn synthetic_note_cursor_uses_the_canonical_side_anchor() {
        let cursors = two_file_cursors();
        let existing = line_cursor_at(&cursors, "alpha", 1, target(ReviewSide::New, 10));
        assert_eq!(existing, cursors[3]);
        let synthetic = line_cursor_at(&cursors, "alpha", 1, target(ReviewSide::New, 42));
        assert_eq!(synthetic.stable_key, "line:1:new:42");
    }

    #[test]
    fn viewport_clamping_keeps_visible_rows_and_snaps_to_nearest_full_stop() {
        let cursors = build_line_cursors(&files(&["alpha"]), &[two_hunk_geometry()]);
        let bounds = |cursor: &LineCursor| {
            cursors
                .iter()
                .position(|candidate| candidate.stable_key == cursor.stable_key)
                .map(|top| VerticalBounds { top, height: 1 })
        };
        assert_eq!(
            clamp_line_cursor_to_viewport(&cursors, Some(&cursors[1]), 0, 3, bounds),
            Some(cursors[1].clone())
        );
        assert_eq!(
            clamp_line_cursor_to_viewport(&cursors, Some(&cursors[0]), 2, 2, bounds),
            Some(cursors[2].clone())
        );
        assert_eq!(
            clamp_line_cursor_to_viewport(&cursors, Some(&cursors[3]), 0, 2, bounds),
            Some(cursors[1].clone())
        );
        let retired = LineCursor {
            file_id: "alpha".into(),
            hunk_index: 9,
            stable_key: "line:9:new:1".into(),
            target: target(ReviewSide::New, 1),
            expanded_gap_key: None,
        };
        assert_eq!(
            clamp_line_cursor_to_viewport(&cursors, Some(&retired), 1, 2, bounds),
            Some(cursors[1].clone())
        );
        assert_eq!(
            clamp_line_cursor_to_viewport(
                &LineCursorList::default(),
                Some(&retired),
                40,
                4,
                |_| None
            ),
            Some(retired.clone())
        );
        assert_eq!(
            clamp_line_cursor_to_viewport(&cursors, Some(&cursors[0]), 40, 0, bounds),
            Some(cursors[0].clone())
        );
    }
}
