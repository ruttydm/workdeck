//! Placement shared by agent-note rendering and markup width reporting.

use workdeck_core::ReviewSide;
use workdeck_review::LayoutMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentNoteBoxLayout {
    pub box_width: usize,
    pub box_left: usize,
    pub content_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitPaneWidths {
    pub left_width: usize,
    pub right_width: usize,
}

/// Split panes reserve one rail cell on the left and one middle separator.
#[must_use]
pub fn resolve_split_pane_widths(width: usize) -> SplitPaneWidths {
    let usable_width = width.saturating_sub(2);
    let left_half = usable_width / 2;
    SplitPaneWidths {
        left_width: 1 + left_half,
        right_width: 1 + usable_width.saturating_sub(left_half),
    }
}

/// Resolve one note card against the same split widths used by code rows.
#[must_use]
pub fn agent_note_box_layout(
    anchor_side: Option<ReviewSide>,
    layout: LayoutMode,
    width: usize,
    thread_depth: usize,
) -> AgentNoteBoxLayout {
    let split = resolve_split_pane_widths(width);
    let can_dock_right =
        layout == LayoutMode::Split && anchor_side == Some(ReviewSide::New) && width >= 84;
    let can_dock_left =
        layout == LayoutMode::Split && anchor_side == Some(ReviewSide::Old) && width >= 84;
    let preferred_dock_width = if can_dock_right {
        split.right_width
    } else if can_dock_left {
        split.left_width
    } else {
        34.max(width.saturating_sub(4))
    };
    let thread_indent = thread_depth.min(3) * 2;
    let maximum = 28.max(width.saturating_sub(4).saturating_sub(thread_indent));
    let box_width = preferred_dock_width
        .saturating_sub(thread_indent)
        .clamp(28, maximum);
    let box_left = if can_dock_right {
        width.saturating_sub(box_width)
    } else if can_dock_left {
        thread_indent
    } else {
        (4 + thread_indent).min(width.saturating_sub(box_width))
    };
    let inner_width = box_width.saturating_sub(2).max(1);
    let content_width = inner_width.saturating_sub(2).max(1);
    AgentNoteBoxLayout {
        box_width,
        box_left,
        content_width,
    }
}

#[must_use]
pub fn agent_note_markup_width(
    anchor_side: Option<ReviewSide>,
    layout: LayoutMode,
    width: usize,
    thread_depth: usize,
) -> usize {
    agent_note_box_layout(anchor_side, layout, width, thread_depth).content_width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_notes_receive_nearly_the_full_pane() {
        let geometry = agent_note_box_layout(Some(ReviewSide::New), LayoutMode::Stack, 120, 0);
        assert_eq!(geometry.box_width, 116);
        assert_eq!(geometry.content_width, 112);
    }

    #[test]
    fn split_new_side_notes_dock_to_roughly_half_the_pane() {
        let geometry = agent_note_box_layout(Some(ReviewSide::New), LayoutMode::Split, 120, 0);
        assert!(geometry.box_width < 70);
        assert_eq!(geometry.box_left, 120 - geometry.box_width);
    }

    #[test]
    fn split_old_side_notes_dock_left() {
        assert_eq!(
            agent_note_box_layout(Some(ReviewSide::Old), LayoutMode::Split, 120, 0).box_left,
            0
        );
    }

    #[test]
    fn narrow_split_panes_fall_back_to_full_width() {
        assert_eq!(
            agent_note_box_layout(Some(ReviewSide::New), LayoutMode::Split, 83, 0).box_width,
            79
        );
    }

    #[test]
    fn notes_never_collapse_below_the_minimum_card_width() {
        let geometry = agent_note_box_layout(Some(ReviewSide::New), LayoutMode::Stack, 20, 0);
        assert!(geometry.box_width >= 16);
        assert!(geometry.content_width >= 1);
    }

    #[test]
    fn huge_terminals_grow_markup_with_the_active_pane() {
        assert_eq!(
            agent_note_markup_width(Some(ReviewSide::New), LayoutMode::Stack, 220, 0),
            212
        );
        assert!(agent_note_markup_width(Some(ReviewSide::New), LayoutMode::Split, 220, 0) > 100);
    }
}
