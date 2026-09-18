//! Decided state transitions accepted by the semantic review reducer.

use std::sync::Arc;

use workdeck_core::SemanticReviewDocument;

use crate::{ReviewDraftNote, ReviewRevealRequest, ReviewSourceStatus, ReviewStoredNote};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticReviewAction {
    ReconcileDocument {
        document: Arc<SemanticReviewDocument>,
    },
    Select {
        file_key: String,
        hunk_index: isize,
        reveal: Option<ReviewRevealRequest>,
    },
    SetFilter(String),
    SetNoteVisibility(bool),
    AddLiveNotes(Vec<ReviewStoredNote>),
    RemoveLiveNote(String),
    ClearNotes {
        file_key: Option<String>,
        include_user: bool,
    },
    RemoveUserNote(String),
    StartDraft(ReviewDraftNote),
    UpdateDraft(String),
    CancelDraft,
    SaveDraft(ReviewStoredNote),
    SaveDraftEdit(ReviewStoredNote),
    ToggleExpansion {
        file_key: String,
        gap_id: String,
        expanded: bool,
    },
    SetSourceStatus {
        file_key: String,
        status: ReviewSourceStatus,
    },
}
