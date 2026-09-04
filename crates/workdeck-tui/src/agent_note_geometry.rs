//! Placement shared by agent-note rendering and markup width reporting.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use workdeck_core::{AgentAnnotation, ReviewSide};
use workdeck_diff::sanitize_terminal_line;
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

/// Measure one host-owned inline note against the exact content width used for painting.
#[must_use]
pub fn measure_agent_inline_note_height(
    annotation: &AgentAnnotation,
    anchor_side: Option<ReviewSide>,
    layout: LayoutMode,
    width: usize,
    thread_depth: usize,
) -> usize {
    let content_width = agent_note_markup_width(anchor_side, layout, width, thread_depth);
    if annotation.source.as_deref() == Some("user-draft") {
        return draft_visual_line_count(&annotation.summary, content_width).saturating_add(3);
    }

    let markup_lines = annotation.markup.as_deref().and_then(|markup| {
        let lines = workdeck_markup::render(markup, content_width).lines;
        (!lines.is_empty()).then_some(lines.len())
    });
    let body_lines = markup_lines.unwrap_or_else(|| {
        wrapped_note_line_count(&annotation.summary, content_width)
            + annotation.rationale.as_deref().map_or(0, |rationale| {
                wrapped_note_line_count(rationale, content_width)
            })
    });
    body_lines.saturating_add(3)
}

fn wrapped_note_line_count(text: &str, width: usize) -> usize {
    text.split('\n')
        .map(|line| wrapped_prose_line_count(&sanitize_terminal_line(line), width))
        .sum()
}

fn wrapped_prose_line_count(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return 1;
    }

    let mut rows = 0_usize;
    let mut used = 0_usize;
    for word in words {
        let word_width = UnicodeWidthStr::width(word);
        if word_width > width {
            rows = rows.saturating_add(usize::from(used > 0));
            used = 0;
            let chunks = grapheme_wrapped_line_count(word, width);
            rows = rows.saturating_add(chunks);
            continue;
        }
        let next_width = if used == 0 {
            word_width
        } else {
            used.saturating_add(1).saturating_add(word_width)
        };
        if next_width <= width {
            used = next_width;
        } else {
            rows = rows.saturating_add(1);
            used = word_width;
        }
    }
    rows.saturating_add(usize::from(used > 0)).max(1)
}

#[must_use]
pub fn draft_visual_line_count(text: &str, width: usize) -> usize {
    draft_editor_visual_lines(text, width).len().max(1)
}

fn grapheme_wrapped_line_count(text: &str, width: usize) -> usize {
    let mut rows = 0_usize;
    let mut used = 0_usize;
    for cluster in text.graphemes(true) {
        let cluster_width = UnicodeWidthStr::width(cluster);
        if used > 0 && used.saturating_add(cluster_width) > width {
            rows = rows.saturating_add(1);
            used = 0;
        }
        if cluster_width > width {
            rows = rows.saturating_add(1);
        } else {
            used = used.saturating_add(cluster_width);
        }
    }
    rows.saturating_add(usize::from(used > 0))
}

fn draft_editor_cluster_width(cluster: &str) -> usize {
    match cluster {
        // OpenTUI's native editor advances tabs by two cells and treats the
        // bare heart as emoji-width even without a variation selector.
        "\t" | "❤" => 2,
        _ => UnicodeWidthStr::width(cluster),
    }
}

pub(crate) fn draft_editor_visual_lines(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut output = Vec::new();
    for hard_line in text.split('\n') {
        let safe = sanitize_terminal_line(hard_line);
        let mut current = String::new();
        let mut used = 0_usize;
        for cluster in safe.graphemes(true) {
            let cluster_width = draft_editor_cluster_width(cluster);
            if used > 0 && used.saturating_add(cluster_width) > width {
                output.push(std::mem::take(&mut current));
                used = 0;
            }
            let displayed = if cluster == "\t" { "  " } else { cluster };
            if cluster_width > width {
                output.push(displayed.to_owned());
            } else {
                current.push_str(displayed);
                used = used.saturating_add(cluster_width);
            }
        }
        if !current.is_empty() || used > 0 {
            output.push(current);
        } else if safe.is_empty() {
            output.push(String::new());
        }
    }
    if output.is_empty() {
        output.push(String::new());
    }
    output
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

    #[test]
    fn note_height_tracks_plain_markup_and_draft_bodies() {
        let mut annotation = annotation("one two three");
        assert_eq!(
            measure_agent_inline_note_height(
                &annotation,
                Some(ReviewSide::New),
                LayoutMode::Stack,
                80,
                0,
            ),
            4
        );
        annotation.markup = Some("<p>first</p><p>second</p>".into());
        assert_eq!(
            measure_agent_inline_note_height(
                &annotation,
                Some(ReviewSide::New),
                LayoutMode::Stack,
                80,
                0,
            ),
            5
        );
        annotation.source = Some("user-draft".into());
        annotation.summary = "first\nsecond".into();
        assert_eq!(
            measure_agent_inline_note_height(
                &annotation,
                Some(ReviewSide::New),
                LayoutMode::Stack,
                80,
                0,
            ),
            5
        );
    }

    fn annotation(summary: &str) -> AgentAnnotation {
        AgentAnnotation {
            id: Some("note".into()),
            old_range: None,
            new_range: None,
            summary: summary.into(),
            rationale: None,
            markup: None,
            tags: Vec::new(),
            confidence: None,
            source: None,
            title: None,
            author: None,
            created_at: None,
            updated_at: None,
            editable: false,
        }
    }
}
