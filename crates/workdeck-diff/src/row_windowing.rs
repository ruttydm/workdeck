//! Visible-row windowing translated from Hunk's `src/ui/diff/rowWindowing.ts`.
//!
//! The implementation is renderer-neutral: callers provide measured row bounds and receive the
//! exact mounted slice plus spacer heights that preserve the full section geometry.

/// One visible slice within a measured body, in body-local terminal rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleBodyBounds {
    pub top: i64,
    pub height: i64,
}

/// Measured bounds for one logical row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasuredRowBounds {
    pub key: String,
    pub top: usize,
    pub height: usize,
}

/// Index-only visible slice shared by review plans and alternate file views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRowIndexWindow {
    pub bottom_spacer_height: usize,
    pub end_index: usize,
    pub start_index: usize,
    pub top_spacer_height: usize,
}

/// A borrowed visible slice whose spacer heights preserve the measured body's total height.
#[derive(Debug, PartialEq, Eq)]
pub struct VisibleRowWindow<'a, T> {
    pub bottom_spacer_height: usize,
    pub rows: &'a [T],
    pub top_spacer_height: usize,
}

trait RowBoundsAccess {
    fn len(&self) -> usize;
    fn get(&self, index: usize) -> &MeasuredRowBounds;
}

impl RowBoundsAccess for [MeasuredRowBounds] {
    fn len(&self) -> usize {
        <[MeasuredRowBounds]>::len(self)
    }

    fn get(&self, index: usize) -> &MeasuredRowBounds {
        &self[index]
    }
}

fn first_row_with_bottom_after<A: RowBoundsAccess + ?Sized>(row_bounds: &A, top: usize) -> usize {
    let mut low = 0;
    let mut high = row_bounds.len();
    while low < high {
        let middle = low + (high - low) / 2;
        let bounds = row_bounds.get(middle);
        if bounds.top.saturating_add(bounds.height) > top {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

fn last_row_with_top_before<A: RowBoundsAccess + ?Sized>(
    row_bounds: &A,
    bottom: usize,
) -> Option<usize> {
    let mut low = 0;
    let mut high = row_bounds.len();
    while low < high {
        let middle = low + (high - low) / 2;
        if row_bounds.get(middle).top < bottom {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low.checked_sub(1)
}

fn row_overlaps_visible_range(
    bounds: &MeasuredRowBounds,
    min_visible_top: usize,
    max_visible_bottom: usize,
) -> bool {
    bounds.height > 0
        && bounds.top.saturating_add(bounds.height) > min_visible_top
        && bounds.top < max_visible_bottom
}

fn nonnegative_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

/// Resolve a measured visible slice in logarithmic time plus adjacent structural rows.
#[must_use]
pub fn resolve_visible_row_index_window(
    body_height: usize,
    row_bounds: &[MeasuredRowBounds],
    visible_body_bounds: VisibleBodyBounds,
) -> VisibleRowIndexWindow {
    resolve_visible_row_index_window_from_access(body_height, row_bounds, visible_body_bounds)
}

fn resolve_visible_row_index_window_from_access<A: RowBoundsAccess + ?Sized>(
    body_height: usize,
    row_bounds: &A,
    visible_body_bounds: VisibleBodyBounds,
) -> VisibleRowIndexWindow {
    let min_visible_top = nonnegative_usize(visible_body_bounds.top);
    let visible_height = nonnegative_usize(visible_body_bounds.height);
    let max_visible_bottom = body_height.min(min_visible_top.saturating_add(visible_height));

    let mut first_visible_index = first_row_with_bottom_after(row_bounds, min_visible_top);
    while first_visible_index < row_bounds.len()
        && !row_overlaps_visible_range(
            row_bounds.get(first_visible_index),
            min_visible_top,
            max_visible_bottom,
        )
    {
        first_visible_index += 1;
    }

    let mut last_visible_index = last_row_with_top_before(row_bounds, max_visible_bottom);
    while let Some(index) = last_visible_index {
        if row_overlaps_visible_range(row_bounds.get(index), min_visible_top, max_visible_bottom) {
            break;
        }
        last_visible_index = index.checked_sub(1);
    }

    let Some(last_visible_index) = last_visible_index else {
        let top_spacer_height = body_height.min(min_visible_top);
        return VisibleRowIndexWindow {
            bottom_spacer_height: body_height.saturating_sub(top_spacer_height),
            end_index: 0,
            start_index: 0,
            top_spacer_height,
        };
    };
    if first_visible_index >= row_bounds.len() || first_visible_index > last_visible_index {
        let top_spacer_height = body_height.min(min_visible_top);
        return VisibleRowIndexWindow {
            bottom_spacer_height: body_height.saturating_sub(top_spacer_height),
            end_index: 0,
            start_index: 0,
            top_spacer_height,
        };
    }

    let mut start_index = first_visible_index;
    while start_index > 0 && row_bounds.get(start_index - 1).height == 0 {
        start_index -= 1;
    }

    let mut end_index = last_visible_index + 1;
    while end_index < row_bounds.len() && row_bounds.get(end_index).height == 0 {
        end_index += 1;
    }

    let start_bounds = row_bounds.get(start_index);
    let end_bounds = row_bounds.get(end_index - 1);
    VisibleRowIndexWindow {
        top_spacer_height: start_bounds.top,
        start_index,
        end_index,
        bottom_spacer_height: body_height
            .saturating_sub(end_bounds.top.saturating_add(end_bounds.height)),
    }
}

/// Slice logical rows to the visible body range while preserving total section height.
#[must_use]
pub fn resolve_visible_row_window<'a, T>(
    rows: &'a [T],
    body_height: usize,
    row_bounds: &[MeasuredRowBounds],
    visible_body_bounds: VisibleBodyBounds,
) -> VisibleRowWindow<'a, T> {
    if rows.is_empty() || row_bounds.len() != rows.len() {
        return VisibleRowWindow {
            bottom_spacer_height: 0,
            rows,
            top_spacer_height: 0,
        };
    }
    let window = resolve_visible_row_index_window(body_height, row_bounds, visible_body_bounds);
    VisibleRowWindow {
        bottom_spacer_height: window.bottom_spacer_height,
        rows: &rows[window.start_index..window.end_index],
        top_spacer_height: window.top_spacer_height,
    }
}

/// Build measured bounds for the common case where every logical row occupies one terminal row.
#[must_use]
pub fn unit_row_bounds(row_count: usize) -> Vec<MeasuredRowBounds> {
    (0..row_count)
        .map(|index| MeasuredRowBounds {
            key: format!("row:{index}"),
            top: index,
            height: 1,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn bounds(entries: &[(&str, usize, usize)]) -> Vec<MeasuredRowBounds> {
        entries
            .iter()
            .map(|(key, top, height)| MeasuredRowBounds {
                key: (*key).into(),
                top: *top,
                height: *height,
            })
            .collect()
    }

    #[test]
    fn returns_only_rows_intersecting_the_visible_body_range() {
        let rows = ["row:0", "row:1", "row:2", "row:3"];
        let row_bounds = bounds(&[
            ("row:0", 0, 1),
            ("row:1", 1, 2),
            ("row:2", 3, 1),
            ("row:3", 4, 1),
        ]);
        let window = resolve_visible_row_window(
            &rows,
            5,
            &row_bounds,
            VisibleBodyBounds { top: 1, height: 3 },
        );
        assert_eq!(window.top_spacer_height, 1);
        assert_eq!(window.bottom_spacer_height, 1);
        assert_eq!(window.rows, &["row:1", "row:2"]);
    }

    #[test]
    fn keeps_adjacent_zero_height_rows_attached_to_the_visible_slice() {
        let rows = ["header:hidden", "code:1", "header:hidden:after", "code:2"];
        let row_bounds = bounds(&[
            ("header:hidden", 0, 0),
            ("code:1", 0, 1),
            ("header:hidden:after", 1, 0),
            ("code:2", 1, 1),
        ]);
        let window = resolve_visible_row_window(
            &rows,
            2,
            &row_bounds,
            VisibleBodyBounds { top: 0, height: 1 },
        );
        assert_eq!(window.top_spacer_height, 0);
        assert_eq!(window.bottom_spacer_height, 1);
        assert_eq!(
            window.rows,
            &["header:hidden", "code:1", "header:hidden:after"]
        );
    }

    #[test]
    fn collapses_a_fully_offscreen_body_above_the_viewport_into_top_spacer_height() {
        let rows = ["row:0", "row:1"];
        let row_bounds = bounds(&[("row:0", 0, 2), ("row:1", 2, 2)]);
        let window = resolve_visible_row_window(
            &rows,
            4,
            &row_bounds,
            VisibleBodyBounds { top: 10, height: 2 },
        );
        assert_eq!(window.top_spacer_height, 4);
        assert_eq!(window.bottom_spacer_height, 0);
        assert!(window.rows.is_empty());
    }

    #[test]
    fn collapses_a_fully_offscreen_body_below_the_viewport_into_bottom_spacer_height() {
        let rows = ["row:0", "row:1"];
        let row_bounds = bounds(&[("row:0", 0, 2), ("row:1", 2, 2)]);
        let window = resolve_visible_row_window(
            &rows,
            4,
            &row_bounds,
            VisibleBodyBounds { top: 0, height: 0 },
        );
        assert_eq!(window.top_spacer_height, 0);
        assert_eq!(window.bottom_spacer_height, 4);
        assert!(window.rows.is_empty());
    }

    #[test]
    fn indexes_only_logarithmically_into_a_ten_thousand_row_layout() {
        struct CountingBounds {
            entries: Vec<MeasuredRowBounds>,
            accesses: Cell<usize>,
        }

        impl RowBoundsAccess for CountingBounds {
            fn len(&self) -> usize {
                self.entries.len()
            }

            fn get(&self, index: usize) -> &MeasuredRowBounds {
                self.accesses.set(self.accesses.get() + 1);
                &self.entries[index]
            }
        }

        let row_bounds = CountingBounds {
            entries: (0..10_000)
                .map(|index| MeasuredRowBounds {
                    key: format!("row:{index}"),
                    top: index * 2,
                    height: 2,
                })
                .collect(),
            accesses: Cell::new(0),
        };
        let window = resolve_visible_row_index_window_from_access(
            20_000,
            &row_bounds,
            VisibleBodyBounds {
                top: 12_000,
                height: 10,
            },
        );
        assert_eq!(window.end_index - window.start_index, 5);
        assert!(row_bounds.accesses.get() < 80);
        let mounted_height = row_bounds.entries[window.start_index..window.end_index]
            .iter()
            .map(|row| row.height)
            .sum::<usize>();
        assert_eq!(
            window.top_spacer_height + mounted_height + window.bottom_spacer_height,
            20_000
        );
    }

    #[test]
    fn finds_visible_rows_in_a_very_large_row_bound_array() {
        let rows = (0..50_000)
            .map(|index| format!("row:{index}"))
            .collect::<Vec<_>>();
        let row_bounds = unit_row_bounds(rows.len());
        let window = resolve_visible_row_window(
            &rows,
            rows.len(),
            &row_bounds,
            VisibleBodyBounds {
                top: 30_000,
                height: 5,
            },
        );
        assert_eq!(window.top_spacer_height, 30_000);
        assert_eq!(window.bottom_spacer_height, 19_995);
        assert_eq!(
            window.rows,
            [
                "row:30000",
                "row:30001",
                "row:30002",
                "row:30003",
                "row:30004"
            ]
        );
    }

    #[test]
    fn mismatched_geometry_returns_the_original_rows_without_spacers() {
        let rows = ["row:0", "row:1"];
        let window = resolve_visible_row_window(
            &rows,
            2,
            &unit_row_bounds(1),
            VisibleBodyBounds { top: 1, height: 1 },
        );
        assert_eq!(window.rows, &rows);
        assert_eq!(window.top_spacer_height, 0);
        assert_eq!(window.bottom_spacer_height, 0);
    }

    #[test]
    fn clamps_negative_visible_bounds_like_the_source_implementation() {
        let rows = ["row:0", "row:1"];
        let window = resolve_visible_row_window(
            &rows,
            2,
            &unit_row_bounds(2),
            VisibleBodyBounds { top: -3, height: 1 },
        );
        assert_eq!(window.rows, &["row:0"]);
        assert_eq!(window.top_spacer_height, 0);
        assert_eq!(window.bottom_spacer_height, 1);
    }
}
