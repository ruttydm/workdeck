//! Renderer-neutral state for one live semantic review.

use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
use workdeck_core::{
    ReviewNoteSource, ReviewSide, SemanticReviewDocument, SemanticReviewLineAddress,
    SemanticReviewNote,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewNoteResolution {
    Active,
    Stale,
    Orphaned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStoredNote {
    pub note: SemanticReviewNote,
    pub resolution: ReviewNoteResolution,
}

pub fn is_renderable_stored_review_note(entry: &ReviewStoredNote) -> bool {
    entry.resolution != ReviewNoteResolution::Orphaned
}

pub fn review_note_visible_by_policy(note: &SemanticReviewNote, show_agent_notes: bool) -> bool {
    show_agent_notes || note.source == ReviewNoteSource::User
}

pub fn review_note_owner_hunk_index(note: &SemanticReviewNote) -> usize {
    note.anchor
        .owner_hunk_index
        .or_else(|| note.anchor.intersecting_hunk_indices.first().copied())
        .unwrap_or(0)
}

pub fn review_note_anchor_line(note: &SemanticReviewNote) -> SemanticReviewLineAddress {
    note.anchor.preferred.unwrap_or(SemanticReviewLineAddress {
        side: ReviewSide::New,
        line: 1,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticReviewSelection {
    pub file_key: Option<String>,
    pub hunk_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewRevealAnchor {
    Hunk,
    FileTop,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRevealRequest {
    pub anchor: ReviewRevealAnchor,
    pub scroll_to_note: bool,
}

pub const REVIEW_VIEWPORT_ANCHOR_REVEAL: ReviewRevealRequest = ReviewRevealRequest {
    anchor: ReviewRevealAnchor::None,
    scroll_to_note: false,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRevealIntent {
    pub file_top_token: u64,
    pub hunk_token: u64,
    pub scroll_to_note: bool,
}

#[must_use]
pub fn apply_review_reveal_request(
    current: ReviewRevealIntent,
    request: ReviewRevealRequest,
) -> ReviewRevealIntent {
    ReviewRevealIntent {
        file_top_token: current.file_top_token
            + u64::from(request.anchor == ReviewRevealAnchor::FileTop),
        hunk_token: current.hunk_token + u64::from(request.anchor == ReviewRevealAnchor::Hunk),
        scroll_to_note: request.scroll_to_note,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum ReviewDraftKind {
    Create,
    Edit { target_note_id: String },
    Reply { parent_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDraftNote {
    pub id: String,
    pub file_key: String,
    pub hunk_index: usize,
    pub side: ReviewSide,
    pub line: u32,
    pub body: String,
    #[serde(flatten)]
    pub kind: ReviewDraftKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewSourceErrorReason {
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ReviewSourceStatus {
    Loading,
    Loaded {
        text: String,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<ReviewSourceErrorReason>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewExpandedGapState {
    pub file_key: String,
    pub gap_id: String,
    pub expanded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticReviewState {
    pub document: Arc<SemanticReviewDocument>,
    pub state_revision: u64,
    pub selection: SemanticReviewSelection,
    pub reveal: ReviewRevealIntent,
    pub filter: String,
    pub show_agent_notes: bool,
    pub live_notes: Vec<ReviewStoredNote>,
    pub user_notes: Vec<ReviewStoredNote>,
    pub draft_note: Option<ReviewDraftNote>,
    pub expanded_gaps: Vec<ReviewExpandedGapState>,
    pub source_status_by_file_key: BTreeMap<String, ReviewSourceStatus>,
}

impl SemanticReviewState {
    #[must_use]
    pub fn new(document: Arc<SemanticReviewDocument>, show_agent_notes: bool) -> Self {
        Self {
            selection: SemanticReviewSelection {
                file_key: document.files.first().map(|file| file.key.clone()),
                hunk_index: 0,
            },
            document,
            state_revision: 0,
            reveal: ReviewRevealIntent::default(),
            filter: String::new(),
            show_agent_notes,
            live_notes: Vec::new(),
            user_notes: Vec::new(),
            draft_note: None,
            expanded_gaps: Vec::new(),
            source_status_by_file_key: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{ReviewFileChangeKind, SemanticReviewRangeAnchor};

    use crate::semantic_test_support::{document, note};

    #[test]
    fn initial_state_selects_first_file_and_honors_visibility_default() {
        let state = SemanticReviewState::new(document(&[("alpha", 1), ("beta", 2)]), true);
        assert_eq!(state.selection.file_key.as_deref(), Some("alpha"));
        assert_eq!(state.selection.hunk_index, 0);
        assert_eq!(state.reveal, ReviewRevealIntent::default());
        assert_eq!(state.state_revision, 0);
        assert!(state.show_agent_notes);
        assert!(
            SemanticReviewState::new(document(&[]), false)
                .selection
                .file_key
                .is_none()
        );
    }

    #[test]
    fn note_visibility_resolution_and_anchor_fallbacks_match_policy() {
        let user = note("user", "alpha", ReviewNoteSource::User);
        let agent = note("agent", "alpha", ReviewNoteSource::Agent);
        assert!(review_note_visible_by_policy(&user, false));
        assert!(!review_note_visible_by_policy(&agent, false));
        assert!(review_note_visible_by_policy(&agent, true));

        let mut stored = ReviewStoredNote {
            note: agent,
            resolution: ReviewNoteResolution::Stale,
        };
        assert!(is_renderable_stored_review_note(&stored));
        stored.resolution = ReviewNoteResolution::Orphaned;
        assert!(!is_renderable_stored_review_note(&stored));
        assert_eq!(review_note_owner_hunk_index(&stored.note), 0);
        assert_eq!(review_note_anchor_line(&stored.note).line, 1);

        stored.note.anchor = SemanticReviewRangeAnchor {
            old_range: None,
            new_range: None,
            preferred: Some(SemanticReviewLineAddress {
                side: ReviewSide::Old,
                line: 9,
            }),
            intersecting_hunk_indices: vec![3],
            owner_hunk_index: Some(4),
        };
        assert_eq!(review_note_owner_hunk_index(&stored.note), 4);
        assert_eq!(review_note_anchor_line(&stored.note).line, 9);
        let _ = ReviewFileChangeKind::Change;
    }

    #[test]
    fn reveal_requests_advance_only_the_named_counter() {
        let original = ReviewRevealIntent::default();
        let hunk = apply_review_reveal_request(
            original,
            ReviewRevealRequest {
                anchor: ReviewRevealAnchor::Hunk,
                scroll_to_note: true,
            },
        );
        assert_eq!(hunk.hunk_token, 1);
        assert_eq!(hunk.file_top_token, 0);
        assert!(hunk.scroll_to_note);
        let anchored = apply_review_reveal_request(hunk, REVIEW_VIEWPORT_ANCHOR_REVEAL);
        assert_eq!(anchored.hunk_token, 1);
        assert!(!anchored.scroll_to_note);
    }

    #[test]
    fn draft_and_source_status_use_the_canonical_wire_shape() {
        let draft = ReviewDraftNote {
            id: "draft".into(),
            file_key: "alpha".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 4,
            body: "body".into(),
            kind: ReviewDraftKind::Edit {
                target_note_id: "note".into(),
            },
        };
        let json = serde_json::to_value(&draft).unwrap();
        assert_eq!(json["kind"], "edit");
        assert_eq!(json["targetNoteId"], "note");
        assert_eq!(
            serde_json::to_value(ReviewSourceStatus::Error {
                reason: Some(ReviewSourceErrorReason::TooLarge)
            })
            .unwrap(),
            serde_json::json!({"kind":"error","reason":"too-large"})
        );
    }
}
