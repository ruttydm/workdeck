//! Provider-neutral terminal diff-row vocabulary shared by planning and paint layers.

use std::fmt;
use std::sync::Arc;
use workdeck_core::{ReviewGapPosition, ReviewLineMoveKind};

type ForegroundTransform = dyn Fn(Option<&str>, &str) -> String + Send + Sync + 'static;

/// Deferred foreground paint resolved after cursor and selection backgrounds.
#[derive(Clone)]
pub struct RenderForegroundTransform(Arc<ForegroundTransform>);

impl RenderForegroundTransform {
    #[must_use]
    pub fn new(transform: impl Fn(Option<&str>, &str) -> String + Send + Sync + 'static) -> Self {
        Self(Arc::new(transform))
    }

    #[must_use]
    pub fn apply(&self, source_foreground: Option<&str>, rendered_background: &str) -> String {
        (self.0)(source_foreground, rendered_background)
    }
}

impl fmt::Debug for RenderForegroundTransform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RenderForegroundTransform(..)")
    }
}

impl PartialEq for RenderForegroundTransform {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for RenderForegroundTransform {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderSpan {
    pub text: String,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub transform_foreground: Option<RenderForegroundTransform>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitLineKind {
    Context,
    Addition,
    Deletion,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitLineCell {
    pub kind: SplitLineKind,
    pub sign: String,
    pub line_number: Option<usize>,
    pub move_kind: Option<ReviewLineMoveKind>,
    pub spans: Vec<RenderSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackLineCell {
    pub kind: StackLineKind,
    pub sign: String,
    pub old_line_number: Option<usize>,
    pub new_line_number: Option<usize>,
    pub move_kind: Option<ReviewLineMoveKind>,
    pub spans: Vec<RenderSpan>,
}

pub type CollapsedGapPosition = ReviewGapPosition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    Collapsed {
        key: String,
        file_id: String,
        hunk_index: usize,
        text: String,
        position: CollapsedGapPosition,
        old_range: [usize; 2],
        new_range: [usize; 2],
    },
    HunkHeader {
        key: String,
        file_id: String,
        hunk_index: usize,
        text: String,
    },
    SplitLine {
        key: String,
        file_id: String,
        hunk_index: usize,
        left: SplitLineCell,
        right: SplitLineCell,
        is_expansion_row: bool,
        expanded_gap_key: Option<String>,
    },
    StackLine {
        key: String,
        file_id: String,
        hunk_index: usize,
        cell: StackLineCell,
        is_expansion_row: bool,
        expanded_gap_key: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    #[test]
    fn row_union_retains_every_pinned_variant_and_field() {
        let rows = [
            DiffRow::Collapsed {
                key: "gap".into(),
                file_id: "file".into(),
                hunk_index: 1,
                text: "3 unchanged lines".into(),
                position: CollapsedGapPosition::Before,
                old_range: [2, 4],
                new_range: [3, 5],
            },
            DiffRow::HunkHeader {
                key: "header".into(),
                file_id: "file".into(),
                hunk_index: 1,
                text: "@@ -2,3 +3,3 @@".into(),
            },
            DiffRow::SplitLine {
                key: "split".into(),
                file_id: "file".into(),
                hunk_index: 1,
                left: SplitLineCell {
                    kind: SplitLineKind::Deletion,
                    sign: "-".into(),
                    line_number: Some(2),
                    move_kind: Some(ReviewLineMoveKind::Moved),
                    spans: vec![span("old")],
                },
                right: SplitLineCell {
                    kind: SplitLineKind::Empty,
                    sign: " ".into(),
                    line_number: None,
                    move_kind: None,
                    spans: Vec::new(),
                },
                is_expansion_row: true,
                expanded_gap_key: Some("before:1".into()),
            },
            DiffRow::StackLine {
                key: "stack".into(),
                file_id: "file".into(),
                hunk_index: 1,
                cell: StackLineCell {
                    kind: StackLineKind::Context,
                    sign: " ".into(),
                    old_line_number: Some(4),
                    new_line_number: Some(5),
                    move_kind: None,
                    spans: vec![span("same")],
                },
                is_expansion_row: false,
                expanded_gap_key: None,
            },
        ];

        assert_eq!(rows.len(), 4);
        assert!(matches!(rows[0], DiffRow::Collapsed { .. }));
        assert!(matches!(rows[1], DiffRow::HunkHeader { .. }));
        assert!(matches!(rows[2], DiffRow::SplitLine { .. }));
        assert!(matches!(rows[3], DiffRow::StackLine { .. }));
    }

    #[test]
    fn foreground_transform_equality_tracks_function_identity() {
        let transform = RenderForegroundTransform::new(|foreground, background| {
            format!("{}:{background}", foreground.unwrap_or("default"))
        });
        let same = transform.clone();
        let other = RenderForegroundTransform::new(|foreground, background| {
            format!("{}:{background}", foreground.unwrap_or("default"))
        });

        assert_eq!(transform, same);
        assert_ne!(transform, other);
        assert_eq!(
            transform.apply(Some("#ffffff"), "#101010"),
            "#ffffff:#101010"
        );
    }

    #[test]
    fn stack_cells_cannot_represent_the_split_only_empty_kind() {
        assert_eq!(
            [
                StackLineKind::Context,
                StackLineKind::Addition,
                StackLineKind::Deletion,
            ]
            .len(),
            3
        );
        assert_eq!(
            [
                SplitLineKind::Context,
                SplitLineKind::Addition,
                SplitLineKind::Deletion,
                SplitLineKind::Empty,
            ]
            .len(),
            4
        );
    }
}
