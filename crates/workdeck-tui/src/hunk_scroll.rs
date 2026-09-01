//! Semantic reveal targets for hunks and current rendered lines.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentLineAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRevealPlacement {
    Nearest,
    Reveal,
}

/// Keep a fitting hunk wholly visible; otherwise bias toward its padded top.
#[must_use]
pub fn compute_hunk_reveal_scroll_top(
    hunk_top: i64,
    hunk_height: i64,
    preferred_top_padding: i64,
    viewport_height: i64,
) -> i64 {
    let top = hunk_top.max(0);
    let height = hunk_height.max(0);
    let viewport = viewport_height.max(0);
    let desired_top = (top - preferred_top_padding.max(0)).max(0);

    if viewport == 0 {
        return desired_top;
    }
    if height <= viewport {
        let minimum_top_for_full_hunk = (top + height - viewport).max(0);
        return desired_top.max(minimum_top_for_full_hunk);
    }
    desired_top
}

/// Place the current rendered line at a semantic viewport edge or center.
#[must_use]
pub fn compute_line_alignment_scroll_top(
    alignment: CurrentLineAlignment,
    line_top: i64,
    line_height: i64,
    viewport_height: i64,
) -> i64 {
    let top = line_top.max(0);
    let height = line_height.max(1);
    let viewport = viewport_height.max(1);
    match alignment {
        CurrentLineAlignment::Top => top,
        CurrentLineAlignment::Bottom => (top + height - viewport).max(0),
        CurrentLineAlignment::Center => (top - (viewport - height) / 2).max(0),
    }
}

/// Move only far enough to bring the current rendered line into view.
#[must_use]
pub fn compute_line_reveal_scroll_top(
    line_top: i64,
    line_height: i64,
    scroll_top: i64,
    viewport_height: i64,
) -> i64 {
    let top = line_top.max(0);
    let height = line_height.max(1);
    let viewport = viewport_height.max(0);
    if top < scroll_top {
        return top;
    }
    let line_bottom = top + height;
    let viewport_bottom = scroll_top + viewport;
    if line_bottom > viewport_bottom {
        return (line_bottom - viewport).max(0);
    }
    scroll_top
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_hunks_remain_wholly_visible_with_as_much_padding_as_possible() {
        assert_eq!(compute_hunk_reveal_scroll_top(20, 10, 4, 12), 18);
        assert_eq!(compute_hunk_reveal_scroll_top(20, 10, 4, 16), 16);
        assert_eq!(compute_hunk_reveal_scroll_top(3, 40, 4, 12), 0);
        assert_eq!(compute_hunk_reveal_scroll_top(20, 10, 4, 0), 16);
        assert_eq!(compute_hunk_reveal_scroll_top(-4, -1, -2, -1), 0);
    }

    #[test]
    fn line_alignment_uses_clamped_rendered_geometry() {
        assert_eq!(
            compute_line_alignment_scroll_top(CurrentLineAlignment::Top, 20, 2, 10),
            20
        );
        assert_eq!(
            compute_line_alignment_scroll_top(CurrentLineAlignment::Center, 20, 2, 10),
            16
        );
        assert_eq!(
            compute_line_alignment_scroll_top(CurrentLineAlignment::Bottom, 20, 2, 10),
            12
        );
        assert_eq!(
            compute_line_alignment_scroll_top(CurrentLineAlignment::Center, -1, 0, 0),
            0
        );
    }

    #[test]
    fn nearest_line_reveal_stays_put_until_an_edge_is_crossed() {
        assert_eq!(compute_line_reveal_scroll_top(15, 1, 10, 10), 10);
        assert_eq!(compute_line_reveal_scroll_top(8, 1, 10, 10), 8);
        assert_eq!(compute_line_reveal_scroll_top(20, 2, 10, 10), 12);
        assert_eq!(compute_line_reveal_scroll_top(-2, 0, 5, -1), 0);
    }
}
