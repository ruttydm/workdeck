//! Canonical planned-row ownership shared by geometry, text, and review-stream projection.

use workdeck_core::{AgentAnnotation, ReviewSide};
use workdeck_diff::DiffRow;

use crate::VisibleAgentNote;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedReviewRow {
    DiffRow {
        key: String,
        stable_key: String,
        stable_alias_keys: Vec<String>,
        file_id: String,
        hunk_index: usize,
        row: DiffRow,
        anchor_id: Option<String>,
        note_guide_side: Option<ReviewSide>,
    },
    InlineNote {
        key: String,
        stable_key: String,
        file_id: String,
        hunk_index: usize,
        annotation_id: String,
        annotation: AgentAnnotation,
        note: Box<VisibleAgentNote>,
        anchor_side: Option<ReviewSide>,
        note_count: usize,
        note_index: usize,
    },
    HunkGap {
        key: String,
        stable_key: String,
        file_id: String,
        hunk_index: usize,
        height: usize,
    },
}

impl PlannedReviewRow {
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::DiffRow { key, .. }
            | Self::InlineNote { key, .. }
            | Self::HunkGap { key, .. } => key,
        }
    }

    #[must_use]
    pub const fn hunk_index(&self) -> usize {
        match self {
            Self::DiffRow { hunk_index, .. }
            | Self::InlineNote { hunk_index, .. }
            | Self::HunkGap { hunk_index, .. } => *hunk_index,
        }
    }

    #[must_use]
    pub fn diff_row(&self) -> Option<&DiffRow> {
        match self {
            Self::DiffRow { row, .. } => Some(row),
            Self::InlineNote { .. } | Self::HunkGap { .. } => None,
        }
    }

    #[must_use]
    pub const fn note_guide_side(&self) -> Option<ReviewSide> {
        match self {
            Self::DiffRow {
                note_guide_side, ..
            } => *note_guide_side,
            Self::InlineNote { .. } | Self::HunkGap { .. } => None,
        }
    }
}
