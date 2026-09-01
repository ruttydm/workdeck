//! Validation and lowering of cross-surface semantic review intents.

use std::collections::BTreeSet;

use thiserror::Error;
use workdeck_core::{
    ReviewFileChangeKind, ReviewNoteSource, ReviewSide, SemanticReviewFile, SemanticReviewHunk,
    SemanticReviewHunkBlock, SemanticReviewLineAddress, SemanticReviewNote,
    SemanticReviewRangeAnchor,
};

use crate::{
    REVIEW_FILE_JUMP_HUNK_INDEX, REVIEW_FILE_JUMP_REVEAL, REVIEW_VIEWPORT_ANCHOR_REVEAL,
    ReviewDraftKind, ReviewDraftNote, ReviewNavigationModel, ReviewNoteResolution,
    ReviewRevealAnchor, ReviewRevealRequest, ReviewSelectionMove, ReviewSelectionScope,
    ReviewStoredNote, SemanticReviewAction, SemanticReviewAnnotationIndex, SemanticReviewState,
    SemanticReviewStore, is_review_gap_expanded, is_review_note_within_clear_scope,
    plan_review_selection_move, review_gap_address, review_note_current_owner_hunk_index,
    review_note_has_descendants, review_note_within_size_limit,
    select_normalized_semantic_selection, select_semantic_review_file_by_key,
    select_semantic_review_navigation_files, select_stored_review_note_by_id,
    select_stored_review_notes, semantic_review_gap_source,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewIntentFacts {
    pub note_id: Option<String>,
    pub draft_id: Option<String>,
    pub timestamp: Option<String>,
    pub annotations: Option<SemanticReviewAnnotationIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticReviewIntent {
    Select {
        file_key: String,
        hunk_index: isize,
        reveal: ReviewRevealRequest,
    },
    Move {
        scope: ReviewSelectionScope,
        delta: isize,
    },
    SelectFile {
        file_key: String,
        reveal: Option<ReviewRevealRequest>,
    },
    Anchor {
        file_key: String,
        hunk_index: isize,
    },
    SetFilter(String),
    SetNoteVisibility(bool),
    StartDraft {
        file_key: String,
        hunk_index: isize,
        target: Option<SemanticReviewLineAddress>,
        reveal: Option<ReviewRevealRequest>,
    },
    StartEdit {
        note_id: String,
        reveal: Option<ReviewRevealRequest>,
    },
    StartReply {
        note_id: String,
        reveal: Option<ReviewRevealRequest>,
    },
    UpdateDraft(String),
    CancelDraft,
    CreateUserNote,
    UpdateUserNote {
        note_id: String,
    },
    RemoveUserNote {
        note_id: String,
    },
    RemoveLiveNote {
        note_id: String,
    },
    ClearNotes {
        file_key: Option<String>,
        include_user: bool,
    },
    ToggleExpansion {
        file_key: String,
        gap_id: String,
    },
}

pub const REVIEW_INTENT_TYPES: [&str; 17] = [
    "selection/select",
    "selection/move",
    "selection/select-file",
    "selection/anchor",
    "filter/set",
    "notes/set-visibility",
    "notes/start-draft",
    "notes/start-edit",
    "notes/start-reply",
    "notes/update-draft",
    "notes/cancel-draft",
    "notes/create-user",
    "notes/update-user",
    "notes/remove-user",
    "notes/remove-live",
    "notes/clear",
    "expansion/toggle",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RemovedReviewNoteSource {
    User,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewIntentOutcome {
    SelectionChanged {
        file_key: String,
        hunk_index: usize,
    },
    DraftStarted(ReviewDraftNote),
    NoteCreated(ReviewStoredNote),
    NoteUpdated(ReviewStoredNote),
    NoteRemoved {
        note_id: String,
        source: RemovedReviewNoteSource,
    },
    NotesCleared {
        removed_live_count: usize,
        removed_user_count: usize,
        remaining_live_count: usize,
        remaining_user_count: usize,
    },
    ExpansionToggled {
        file_key: String,
        gap_id: String,
        expanded: bool,
        side: ReviewSide,
        old_range: [u32; 2],
        new_range: [u32; 2],
        line_count: usize,
        source_identity: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewIntentPlan {
    pub actions: Vec<SemanticReviewAction>,
    pub outcome: Option<ReviewIntentOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewIntentPlanningErrorCode {
    FileNotFound,
    HunkNotFound,
    GapNotFound,
    DraftMissing,
    DraftActive,
    DraftModeMismatch,
    NoteNotFound,
    NoteNotEditable,
    NoteHasReplies,
    NoteIdConflict,
    InvalidNoteParent,
    BlankNote,
    NoteTooLarge,
    MissingFact,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ReviewIntentPlanningError {
    pub code: ReviewIntentPlanningErrorCode,
    pub message: String,
}

impl ReviewIntentPlanningError {
    fn new(code: ReviewIntentPlanningErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[must_use]
pub fn is_blank_review_note_body(body: &str) -> bool {
    body.trim().is_empty()
}

fn require_fact<'a>(
    value: Option<&'a str>,
    label: &str,
) -> Result<&'a str, ReviewIntentPlanningError> {
    value.filter(|value| !value.is_empty()).ok_or_else(|| {
        ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::MissingFact,
            format!("Review intent requires {label}."),
        )
    })
}

pub fn require_semantic_review_file<'a>(
    state: &'a SemanticReviewState,
    file_key: &str,
) -> Result<&'a SemanticReviewFile, ReviewIntentPlanningError> {
    select_semantic_review_file_by_key(state, Some(file_key)).ok_or_else(|| {
        ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::FileNotFound,
            format!("Review file {file_key} does not exist in the current review."),
        )
    })
}

fn require_hunk(
    file: &SemanticReviewFile,
    hunk_index: isize,
) -> Result<(usize, &SemanticReviewHunk), ReviewIntentPlanningError> {
    let index = usize::try_from(hunk_index).ok();
    index
        .and_then(|index| file.hunks.get(index).map(|hunk| (index, hunk)))
        .ok_or_else(|| {
            ReviewIntentPlanningError::new(
                ReviewIntentPlanningErrorCode::HunkNotFound,
                format!("Review hunk {hunk_index} does not exist in {}.", file.path),
            )
        })
}

fn plan_selection(
    file_key: String,
    hunk_index: usize,
    reveal: ReviewRevealRequest,
) -> ReviewIntentPlan {
    ReviewIntentPlan {
        actions: vec![SemanticReviewAction::Select {
            file_key: file_key.clone(),
            hunk_index: isize::try_from(hunk_index).unwrap_or(isize::MAX),
            reveal: Some(reveal),
        }],
        outcome: Some(ReviewIntentOutcome::SelectionChanged {
            file_key,
            hunk_index,
        }),
    }
}

fn plan_selection_move(
    state: &SemanticReviewState,
    scope: ReviewSelectionScope,
    delta: isize,
    facts: &ReviewIntentFacts,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    let annotated = matches!(
        scope,
        ReviewSelectionScope::AnnotatedHunk | ReviewSelectionScope::AnnotatedFile
    );
    if annotated && facts.annotations.is_none() {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::MissingFact,
            format!(
                "Review intent requires annotations for {} navigation.",
                match scope {
                    ReviewSelectionScope::AnnotatedHunk => "annotated-hunk",
                    ReviewSelectionScope::AnnotatedFile => "annotated-file",
                    _ => unreachable!(),
                }
            ),
        ));
    }
    let target = plan_review_selection_move(
        &ReviewNavigationModel {
            files: select_semantic_review_navigation_files(state),
            annotations: facts.annotations.clone().unwrap_or_default(),
        },
        &select_normalized_semantic_selection(state),
        ReviewSelectionMove { scope, delta },
    );
    Ok(target.map_or(
        ReviewIntentPlan {
            actions: Vec::new(),
            outcome: None,
        },
        |target| plan_selection(target.file_key, target.hunk_index, target.reveal),
    ))
}

pub const REVIEW_DRAFT_START_REVEAL: ReviewRevealRequest = ReviewRevealRequest {
    anchor: ReviewRevealAnchor::Hunk,
    scroll_to_note: true,
};

fn hunk_range(hunk: &SemanticReviewHunk, side: ReviewSide) -> [u32; 2] {
    let (start, count) = match side {
        ReviewSide::Old => (hunk.deletion_start, hunk.deletion_count),
        ReviewSide::New => (hunk.addition_start, hunk.addition_count),
    };
    [start, start.saturating_add(count.max(1)).saturating_sub(1)]
}

fn default_hunk_line_target(hunk: &SemanticReviewHunk) -> SemanticReviewLineAddress {
    let mut deletion_line = hunk.deletion_start;
    let mut addition_line = hunk.addition_start;
    let mut first_deletion = None;
    for block in &hunk.hunk_content {
        match block {
            SemanticReviewHunkBlock::Context { lines, .. } => {
                let lines = u32::try_from(*lines).unwrap_or(u32::MAX);
                deletion_line = deletion_line.saturating_add(lines);
                addition_line = addition_line.saturating_add(lines);
            }
            SemanticReviewHunkBlock::Change {
                additions,
                deletions,
                ..
            } => {
                if *additions > 0 {
                    return SemanticReviewLineAddress {
                        side: ReviewSide::New,
                        line: addition_line,
                    };
                }
                if *deletions > 0 && first_deletion.is_none() {
                    first_deletion = Some(deletion_line);
                }
                deletion_line =
                    deletion_line.saturating_add(u32::try_from(*deletions).unwrap_or(u32::MAX));
                addition_line =
                    addition_line.saturating_add(u32::try_from(*additions).unwrap_or(u32::MAX));
            }
        }
    }
    first_deletion.map_or(
        SemanticReviewLineAddress {
            side: ReviewSide::New,
            line: hunk_range(hunk, ReviewSide::New)[0],
        },
        |line| SemanticReviewLineAddress {
            side: ReviewSide::Old,
            line,
        },
    )
}

fn line_anchor(
    hunks: &[SemanticReviewHunk],
    hunk_index: usize,
    target: SemanticReviewLineAddress,
) -> SemanticReviewRangeAnchor {
    let range = [target.line, target.line];
    let intersecting_hunk_indices = hunks
        .iter()
        .enumerate()
        .filter_map(|(index, hunk)| {
            let extent = hunk_range(hunk, target.side);
            (extent[0] <= target.line && target.line <= extent[1]).then_some(index)
        })
        .collect::<Vec<_>>();
    let preferred_owner = hunks.iter().position(|hunk| {
        let extent = hunk_range(hunk, target.side);
        extent[0] <= target.line && target.line <= extent[1]
    });
    SemanticReviewRangeAnchor {
        old_range: (target.side == ReviewSide::Old).then_some(range),
        new_range: (target.side == ReviewSide::New).then_some(range),
        preferred: Some(target),
        intersecting_hunk_indices,
        owner_hunk_index: preferred_owner
            .or_else(|| (hunk_index < hunks.len()).then_some(hunk_index))
            .or_else(|| (!hunks.is_empty()).then_some(0)),
    }
}

fn plan_draft_start(
    state: &SemanticReviewState,
    file_key: &str,
    hunk_index: isize,
    target: Option<SemanticReviewLineAddress>,
    reveal: Option<ReviewRevealRequest>,
    facts: &ReviewIntentFacts,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    let file = require_semantic_review_file(state, file_key)?;
    let (hunk_index, hunk) = require_hunk(file, hunk_index)?;
    let target = target.unwrap_or_else(|| default_hunk_line_target(hunk));
    if state.draft_note.is_some() {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::DraftActive,
            "A review note draft is already active.",
        ));
    }
    let draft = ReviewDraftNote {
        id: require_fact(facts.draft_id.as_deref(), "draftId")?.to_owned(),
        file_key: file.key.clone(),
        hunk_index,
        side: target.side,
        line: target.line,
        body: String::new(),
        kind: ReviewDraftKind::Create,
    };
    Ok(ReviewIntentPlan {
        actions: vec![
            SemanticReviewAction::StartDraft(draft.clone()),
            SemanticReviewAction::Select {
                file_key: file.key.clone(),
                hunk_index: isize::try_from(hunk_index).unwrap_or(isize::MAX),
                reveal: Some(reveal.unwrap_or(REVIEW_DRAFT_START_REVEAL)),
            },
        ],
        outcome: Some(ReviewIntentOutcome::DraftStarted(draft)),
    })
}

fn draft_for_stored_note(
    state: &SemanticReviewState,
    entry: &ReviewStoredNote,
    facts: &ReviewIntentFacts,
    edit: bool,
) -> Result<ReviewDraftNote, ReviewIntentPlanningError> {
    let file = require_semantic_review_file(state, &entry.note.file_key)?;
    let hunk_index = review_note_current_owner_hunk_index(&entry.note, file);
    let _ = require_hunk(file, isize::try_from(hunk_index).unwrap_or(isize::MAX))?;
    let target = entry
        .note
        .anchor
        .preferred
        .unwrap_or(SemanticReviewLineAddress {
            side: ReviewSide::New,
            line: 1,
        });
    Ok(ReviewDraftNote {
        id: require_fact(facts.draft_id.as_deref(), "draftId")?.to_owned(),
        file_key: file.key.clone(),
        hunk_index,
        side: target.side,
        line: target.line,
        body: if edit {
            entry.note.summary.clone()
        } else {
            String::new()
        },
        kind: if edit {
            ReviewDraftKind::Edit {
                target_note_id: entry.note.id.clone(),
            }
        } else {
            ReviewDraftKind::Reply {
                parent_id: entry.note.id.clone(),
            }
        },
    })
}

fn plan_draft_edit_start(
    state: &SemanticReviewState,
    note_id: &str,
    reveal: Option<ReviewRevealRequest>,
    facts: &ReviewIntentFacts,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    if state.draft_note.is_some() {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::DraftActive,
            "A review note draft is already active.",
        ));
    }
    let entry = state
        .user_notes
        .iter()
        .find(|entry| entry.note.id == note_id)
        .ok_or_else(|| {
            ReviewIntentPlanningError::new(
                ReviewIntentPlanningErrorCode::NoteNotFound,
                format!("No user review note matches id {note_id}."),
            )
        })?;
    if !entry.note.editable {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::NoteNotEditable,
            format!("Review note {note_id} is not editable."),
        ));
    }
    let draft = draft_for_stored_note(state, entry, facts, true)?;
    Ok(draft_start_plan(draft, reveal))
}

fn plan_draft_reply_start(
    state: &SemanticReviewState,
    note_id: &str,
    reveal: Option<ReviewRevealRequest>,
    facts: &ReviewIntentFacts,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    if state.draft_note.is_some() {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::DraftActive,
            "A review note draft is already active.",
        ));
    }
    let entry = select_stored_review_note_by_id(state, note_id)
        .filter(|entry| entry.resolution != ReviewNoteResolution::Orphaned)
        .ok_or_else(|| {
            ReviewIntentPlanningError::new(
                ReviewIntentPlanningErrorCode::NoteNotFound,
                format!("No replyable review note matches id {note_id}."),
            )
        })?;
    let draft = draft_for_stored_note(state, entry, facts, false)?;
    Ok(draft_start_plan(draft, reveal))
}

fn draft_start_plan(
    draft: ReviewDraftNote,
    reveal: Option<ReviewRevealRequest>,
) -> ReviewIntentPlan {
    ReviewIntentPlan {
        actions: vec![
            SemanticReviewAction::StartDraft(draft.clone()),
            SemanticReviewAction::Select {
                file_key: draft.file_key.clone(),
                hunk_index: isize::try_from(draft.hunk_index).unwrap_or(isize::MAX),
                reveal: Some(reveal.unwrap_or(REVIEW_DRAFT_START_REVEAL)),
            },
        ],
        outcome: Some(ReviewIntentOutcome::DraftStarted(draft)),
    }
}

fn plan_expansion_toggle(
    state: &SemanticReviewState,
    file_key: &str,
    gap_id: &str,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    let file = require_semantic_review_file(state, file_key)?;
    let address =
        review_gap_address(&semantic_review_gap_source(file), gap_id).ok_or_else(|| {
            ReviewIntentPlanningError::new(
                ReviewIntentPlanningErrorCode::GapNotFound,
                format!("Review gap {gap_id} does not exist in {}.", file.path),
            )
        })?;
    let expanded = !is_review_gap_expanded(state, &file.key, gap_id);
    let side = if file.change_kind == ReviewFileChangeKind::Deleted {
        ReviewSide::Old
    } else {
        ReviewSide::New
    };
    Ok(ReviewIntentPlan {
        actions: vec![SemanticReviewAction::ToggleExpansion {
            file_key: file.key.clone(),
            gap_id: gap_id.to_owned(),
            expanded,
        }],
        outcome: Some(ReviewIntentOutcome::ExpansionToggled {
            file_key: file.key.clone(),
            gap_id: gap_id.to_owned(),
            expanded,
            side,
            old_range: [address.old_range.start, address.old_range.end],
            new_range: [address.new_range.start, address.new_range.end],
            line_count: address.line_count,
            source_identity: file.source_identity.clone(),
        }),
    })
}

fn plan_user_note_creation(
    state: &SemanticReviewState,
    facts: &ReviewIntentFacts,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    let draft = state.draft_note.as_ref().ok_or_else(|| {
        ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::DraftMissing,
            "No user note draft is active.",
        )
    })?;
    if matches!(draft.kind, ReviewDraftKind::Edit { .. }) {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::DraftModeMismatch,
            "The active review note draft must be committed as an edit.",
        ));
    }
    let file = require_semantic_review_file(state, &draft.file_key)?;
    let _ = require_hunk(
        file,
        isize::try_from(draft.hunk_index).unwrap_or(isize::MAX),
    )?;
    if is_blank_review_note_body(&draft.body) {
        return Ok(ReviewIntentPlan {
            actions: vec![SemanticReviewAction::CancelDraft],
            outcome: None,
        });
    }
    let note_id = require_fact(facts.note_id.as_deref(), "noteId")?;
    if select_stored_review_note_by_id(state, note_id).is_some() {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::NoteIdConflict,
            format!("Review note id {note_id} is already in use."),
        ));
    }
    let parent = match &draft.kind {
        ReviewDraftKind::Reply { parent_id } => {
            let parent = select_stored_review_note_by_id(state, parent_id)
                .filter(|entry| entry.resolution != ReviewNoteResolution::Orphaned)
                .ok_or_else(|| {
                    ReviewIntentPlanningError::new(
                        ReviewIntentPlanningErrorCode::InvalidNoteParent,
                        format!(
                            "Review note {parent_id} is no longer available as a reply parent."
                        ),
                    )
                })?;
            if parent.note.file_key != file.key {
                return Err(ReviewIntentPlanningError::new(
                    ReviewIntentPlanningErrorCode::InvalidNoteParent,
                    format!(
                        "Review note {} belongs to a different file.",
                        parent.note.id
                    ),
                ));
            }
            Some(parent)
        }
        ReviewDraftKind::Create => None,
        ReviewDraftKind::Edit { .. } => unreachable!(),
    };
    let timestamp = require_fact(facts.timestamp.as_deref(), "timestamp")?;
    let note = ReviewStoredNote {
        note: SemanticReviewNote {
            id: note_id.to_owned(),
            parent_id: parent.map(|parent| parent.note.id.clone()),
            source: ReviewNoteSource::User,
            original_source: Some("user".into()),
            file_key: file.key.clone(),
            anchor: parent.map_or_else(
                || {
                    line_anchor(
                        &file.hunks,
                        draft.hunk_index,
                        SemanticReviewLineAddress {
                            side: draft.side,
                            line: draft.line,
                        },
                    )
                },
                |parent| parent.note.anchor.clone(),
            ),
            summary: draft.body.trim().to_owned(),
            rationale: None,
            markup: None,
            title: None,
            author: Some("user".into()),
            created_at: Some(timestamp.to_owned()),
            updated_at: None,
            editable: true,
            tags: Vec::new(),
            confidence: None,
        },
        resolution: parent.map_or(ReviewNoteResolution::Active, |parent| parent.resolution),
    };
    if !review_note_within_size_limit(&note.note) {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::NoteTooLarge,
            "The review note exceeds the shared note size limit.",
        ));
    }
    Ok(ReviewIntentPlan {
        actions: vec![SemanticReviewAction::SaveDraft(note.clone())],
        outcome: Some(ReviewIntentOutcome::NoteCreated(note)),
    })
}

fn plan_user_note_update(
    state: &SemanticReviewState,
    note_id: &str,
    facts: &ReviewIntentFacts,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    let draft = state.draft_note.as_ref().ok_or_else(|| {
        ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::DraftMissing,
            "No user note draft is active.",
        )
    })?;
    if !matches!(
        &draft.kind,
        ReviewDraftKind::Edit { target_note_id } if target_note_id == note_id
    ) {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::DraftModeMismatch,
            format!("The active draft is not editing review note {note_id}."),
        ));
    }
    let current = state
        .user_notes
        .iter()
        .find(|entry| entry.note.id == note_id)
        .ok_or_else(|| {
            ReviewIntentPlanningError::new(
                ReviewIntentPlanningErrorCode::NoteNotFound,
                format!("No user review note matches id {note_id}."),
            )
        })?;
    if !current.note.editable {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::NoteNotEditable,
            format!("Review note {note_id} is not editable."),
        ));
    }
    if is_blank_review_note_body(&draft.body) {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::BlankNote,
            "An edited review note cannot be blank; cancel or delete it instead.",
        ));
    }
    let mut note = current.clone();
    note.note.summary = draft.body.trim().to_owned();
    note.note.updated_at = Some(require_fact(facts.timestamp.as_deref(), "timestamp")?.to_owned());
    if !review_note_within_size_limit(&note.note) {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::NoteTooLarge,
            "The review note exceeds the shared note size limit.",
        ));
    }
    Ok(ReviewIntentPlan {
        actions: vec![SemanticReviewAction::SaveDraftEdit(note.clone())],
        outcome: Some(ReviewIntentOutcome::NoteUpdated(note)),
    })
}

fn plan_note_removal(
    state: &SemanticReviewState,
    note_id: &str,
    source: RemovedReviewNoteSource,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    let notes = match source {
        RemovedReviewNoteSource::User => &state.user_notes,
        RemovedReviewNoteSource::Live => &state.live_notes,
    };
    let source_name = match source {
        RemovedReviewNoteSource::User => "user",
        RemovedReviewNoteSource::Live => "live",
    };
    if !notes.iter().any(|entry| entry.note.id == note_id) {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::NoteNotFound,
            format!("No {source_name} review note matches id {note_id}."),
        ));
    }
    if review_note_has_descendants(state, note_id) {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::NoteHasReplies,
            format!("Review note {note_id} cannot be removed while it has replies."),
        ));
    }
    Ok(ReviewIntentPlan {
        actions: vec![match source {
            RemovedReviewNoteSource::User => {
                SemanticReviewAction::RemoveUserNote(note_id.to_owned())
            }
            RemovedReviewNoteSource::Live => {
                SemanticReviewAction::RemoveLiveNote(note_id.to_owned())
            }
        }],
        outcome: Some(ReviewIntentOutcome::NoteRemoved {
            note_id: note_id.to_owned(),
            source,
        }),
    })
}

fn plan_notes_clear(
    state: &SemanticReviewState,
    file_key: Option<&str>,
    include_user: bool,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    let removed_live = state
        .live_notes
        .iter()
        .filter(|entry| is_review_note_within_clear_scope(entry, file_key))
        .collect::<Vec<_>>();
    let removed_user = if include_user {
        state
            .user_notes
            .iter()
            .filter(|entry| is_review_note_within_clear_scope(entry, file_key))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let removed_ids = removed_live
        .iter()
        .chain(&removed_user)
        .map(|entry| entry.note.id.as_str())
        .collect::<BTreeSet<_>>();
    let leaves_dangling_reply = select_stored_review_notes(state).iter().any(|entry| {
        !removed_ids.contains(entry.note.id.as_str())
            && entry
                .note
                .parent_id
                .as_deref()
                .is_some_and(|parent| removed_ids.contains(parent))
    });
    if leaves_dangling_reply {
        return Err(ReviewIntentPlanningError::new(
            ReviewIntentPlanningErrorCode::NoteHasReplies,
            "Review notes cannot be cleared while replies to them would remain.",
        ));
    }
    let removed_live_count = removed_live.len();
    let removed_user_count = removed_user.len();
    Ok(ReviewIntentPlan {
        actions: vec![SemanticReviewAction::ClearNotes {
            file_key: file_key.map(str::to_owned),
            include_user,
        }],
        outcome: Some(ReviewIntentOutcome::NotesCleared {
            removed_live_count,
            removed_user_count,
            remaining_live_count: state.live_notes.len() - removed_live_count,
            remaining_user_count: state.user_notes.len() - removed_user_count,
        }),
    })
}

pub fn plan_semantic_review_intent(
    state: &SemanticReviewState,
    intent: SemanticReviewIntent,
    facts: &ReviewIntentFacts,
) -> Result<ReviewIntentPlan, ReviewIntentPlanningError> {
    match intent {
        SemanticReviewIntent::Select {
            file_key,
            hunk_index,
            reveal,
        } => {
            let file = require_semantic_review_file(state, &file_key)?;
            Ok(ReviewIntentPlan {
                actions: vec![SemanticReviewAction::Select {
                    file_key: file.key.clone(),
                    hunk_index,
                    reveal: Some(reveal),
                }],
                outcome: None,
            })
        }
        SemanticReviewIntent::Move { scope, delta } => {
            plan_selection_move(state, scope, delta, facts)
        }
        SemanticReviewIntent::SelectFile { file_key, reveal } => {
            let file = require_semantic_review_file(state, &file_key)?;
            Ok(plan_selection(
                file.key.clone(),
                REVIEW_FILE_JUMP_HUNK_INDEX,
                reveal.unwrap_or(REVIEW_FILE_JUMP_REVEAL),
            ))
        }
        SemanticReviewIntent::Anchor {
            file_key,
            hunk_index,
        } => {
            let file = require_semantic_review_file(state, &file_key)?;
            Ok(ReviewIntentPlan {
                actions: vec![SemanticReviewAction::Select {
                    file_key: file.key.clone(),
                    hunk_index,
                    reveal: Some(REVIEW_VIEWPORT_ANCHOR_REVEAL),
                }],
                outcome: None,
            })
        }
        SemanticReviewIntent::SetFilter(filter) => Ok(ReviewIntentPlan {
            actions: vec![SemanticReviewAction::SetFilter(filter)],
            outcome: None,
        }),
        SemanticReviewIntent::SetNoteVisibility(visible) => Ok(ReviewIntentPlan {
            actions: vec![SemanticReviewAction::SetNoteVisibility(visible)],
            outcome: None,
        }),
        SemanticReviewIntent::StartDraft {
            file_key,
            hunk_index,
            target,
            reveal,
        } => plan_draft_start(state, &file_key, hunk_index, target, reveal, facts),
        SemanticReviewIntent::StartEdit { note_id, reveal } => {
            plan_draft_edit_start(state, &note_id, reveal, facts)
        }
        SemanticReviewIntent::StartReply { note_id, reveal } => {
            plan_draft_reply_start(state, &note_id, reveal, facts)
        }
        SemanticReviewIntent::UpdateDraft(body) => {
            if state.draft_note.is_none() {
                return Err(ReviewIntentPlanningError::new(
                    ReviewIntentPlanningErrorCode::DraftMissing,
                    "No user note draft is active.",
                ));
            }
            Ok(ReviewIntentPlan {
                actions: vec![SemanticReviewAction::UpdateDraft(body)],
                outcome: None,
            })
        }
        SemanticReviewIntent::CancelDraft => {
            if state.draft_note.is_none() {
                return Err(ReviewIntentPlanningError::new(
                    ReviewIntentPlanningErrorCode::DraftMissing,
                    "No user note draft is active.",
                ));
            }
            Ok(ReviewIntentPlan {
                actions: vec![SemanticReviewAction::CancelDraft],
                outcome: None,
            })
        }
        SemanticReviewIntent::CreateUserNote => plan_user_note_creation(state, facts),
        SemanticReviewIntent::UpdateUserNote { note_id } => {
            plan_user_note_update(state, &note_id, facts)
        }
        SemanticReviewIntent::RemoveUserNote { note_id } => {
            plan_note_removal(state, &note_id, RemovedReviewNoteSource::User)
        }
        SemanticReviewIntent::RemoveLiveNote { note_id } => {
            plan_note_removal(state, &note_id, RemovedReviewNoteSource::Live)
        }
        SemanticReviewIntent::ClearNotes {
            file_key,
            include_user,
        } => plan_notes_clear(state, file_key.as_deref(), include_user),
        SemanticReviewIntent::ToggleExpansion { file_key, gap_id } => {
            plan_expansion_toggle(state, &file_key, &gap_id)
        }
    }
}

pub fn apply_semantic_review_intent(
    store: &SemanticReviewStore,
    intent: SemanticReviewIntent,
    facts: &ReviewIntentFacts,
) -> Result<Option<ReviewIntentOutcome>, ReviewIntentPlanningError> {
    let plan = plan_semantic_review_intent(&store.snapshot(), intent, facts)?;
    for action in plan.actions {
        let _ = store.dispatch(action);
    }
    Ok(plan.outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use workdeck_core::{ReviewNoteSource, SemanticReviewHunkBlock};

    use crate::{
        ReviewExpandedGapState, SemanticReviewSelection, reduce_semantic_review_state,
        semantic_test_support::{document, note},
    };

    fn facts() -> ReviewIntentFacts {
        ReviewIntentFacts {
            note_id: Some("user:1".into()),
            timestamp: Some("2024-01-01T00:00:00.000Z".into()),
            ..ReviewIntentFacts::default()
        }
    }

    fn state() -> SemanticReviewState {
        let mut state = SemanticReviewState::new(document(&[("alpha", 2), ("beta", 2)]), false);
        for file in &mut Arc::make_mut(&mut state.document).files {
            for hunk in &mut file.hunks {
                hunk.addition_count = 3;
                hunk.deletion_count = 3;
                hunk.hunk_content = vec![
                    SemanticReviewHunkBlock::Context {
                        lines: 1,
                        addition_line_index: hunk.addition_line_index,
                        deletion_line_index: hunk.deletion_line_index,
                    },
                    SemanticReviewHunkBlock::Change {
                        additions: 1,
                        deletions: 1,
                        addition_line_index: hunk.addition_line_index + 1,
                        deletion_line_index: hunk.deletion_line_index + 1,
                    },
                ];
            }
            file.hunks[1].collapsed_before = 9;
        }
        state
    }

    fn draft_state(body: &str) -> SemanticReviewState {
        let mut state = state();
        state.draft_note = Some(ReviewDraftNote {
            id: "draft-1".into(),
            file_key: "alpha".into(),
            hunk_index: 1,
            side: ReviewSide::New,
            line: 12,
            body: body.into(),
            kind: ReviewDraftKind::Create,
        });
        state
    }

    fn stored(
        id: &str,
        file_key: &str,
        source: ReviewNoteSource,
        hunk_index: usize,
        line: u32,
        parent_id: Option<&str>,
    ) -> ReviewStoredNote {
        let mut note = note(id, file_key, source);
        note.parent_id = parent_id.map(str::to_owned);
        note.anchor.preferred = Some(SemanticReviewLineAddress {
            side: ReviewSide::New,
            line,
        });
        note.anchor.intersecting_hunk_indices = vec![hunk_index];
        note.anchor.owner_hunk_index = Some(hunk_index);
        ReviewStoredNote {
            note,
            resolution: ReviewNoteResolution::Active,
        }
    }

    fn apply_plan(mut state: SemanticReviewState, plan: ReviewIntentPlan) -> SemanticReviewState {
        for action in plan.actions {
            if let Some(next) = reduce_semantic_review_state(&state, action) {
                state = next;
            }
        }
        state
    }

    fn error_code(
        state: &SemanticReviewState,
        intent: SemanticReviewIntent,
        facts: &ReviewIntentFacts,
    ) -> ReviewIntentPlanningErrorCode {
        plan_semantic_review_intent(state, intent, facts)
            .unwrap_err()
            .code
    }

    #[test]
    fn blank_policy_and_intent_vocabulary_are_total() {
        assert!(is_blank_review_note_body(""));
        assert!(is_blank_review_note_body("  \n\t "));
        assert!(!is_blank_review_note_body(" text "));
        assert_eq!(REVIEW_INTENT_TYPES.len(), 17);
        assert_eq!(REVIEW_INTENT_TYPES[0], "selection/select");
        assert_eq!(REVIEW_INTENT_TYPES[16], "expansion/toggle");
    }

    #[test]
    fn selection_lowering_preserves_reveal_and_rejects_only_missing_files() {
        let state = state();
        let reveal = ReviewRevealRequest {
            anchor: ReviewRevealAnchor::FileTop,
            scroll_to_note: true,
        };
        let plan = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::Select {
                file_key: "beta".into(),
                hunk_index: 9,
                reveal,
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert_eq!(
            plan.actions,
            [SemanticReviewAction::Select {
                file_key: "beta".into(),
                hunk_index: 9,
                reveal: Some(reveal)
            }]
        );
        assert_eq!(
            error_code(
                &state,
                SemanticReviewIntent::Select {
                    file_key: "missing".into(),
                    hunk_index: 0,
                    reveal,
                },
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::FileNotFound
        );
    }

    #[test]
    fn movement_uses_visible_stream_reports_target_and_requires_annotation_facts() {
        let mut state = state();
        state.selection = SemanticReviewSelection {
            file_key: Some("alpha".into()),
            hunk_index: 1,
        };
        let plan = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::Move {
                scope: ReviewSelectionScope::Hunk,
                delta: 1,
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert_eq!(
            plan.outcome,
            Some(ReviewIntentOutcome::SelectionChanged {
                file_key: "beta".into(),
                hunk_index: 0
            })
        );
        assert_eq!(
            plan.actions,
            [SemanticReviewAction::Select {
                file_key: "beta".into(),
                hunk_index: 0,
                reveal: Some(REVIEW_FILE_JUMP_REVEAL)
            }]
        );

        state.selection.file_key = Some("beta".into());
        state.selection.hunk_index = 1;
        let refused = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::Move {
                scope: ReviewSelectionScope::File,
                delta: 1,
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert!(refused.actions.is_empty());
        assert!(refused.outcome.is_none());

        state.filter = "alpha".into();
        state.selection.file_key = Some("alpha".into());
        let filtered = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::Move {
                scope: ReviewSelectionScope::Hunk,
                delta: 1,
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert!(matches!(
            &filtered.outcome,
            Some(ReviewIntentOutcome::SelectionChanged { file_key, hunk_index: 1 })
                if file_key == "alpha"
        ));

        let annotated_intent = SemanticReviewIntent::Move {
            scope: ReviewSelectionScope::AnnotatedHunk,
            delta: 1,
        };
        assert_eq!(
            error_code(
                &state,
                annotated_intent.clone(),
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::MissingFact
        );
        let annotated = ReviewIntentFacts {
            annotations: Some(SemanticReviewAnnotationIndex {
                annotated_hunk_indices_by_file_key: [("alpha".into(), [1].into_iter().collect())]
                    .into_iter()
                    .collect(),
                annotated_file_keys: ["alpha".into()].into_iter().collect(),
            }),
            ..ReviewIntentFacts::default()
        };
        assert_eq!(
            plan_semantic_review_intent(&state, annotated_intent, &annotated)
                .unwrap()
                .actions
                .len(),
            1
        );
    }

    #[test]
    fn file_jump_defaults_to_header_accepts_override_and_anchor_never_reveals() {
        let state = state();
        let jump = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::SelectFile {
                file_key: "beta".into(),
                reveal: None,
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert_eq!(
            jump.outcome,
            Some(ReviewIntentOutcome::SelectionChanged {
                file_key: "beta".into(),
                hunk_index: 0
            })
        );
        assert!(matches!(
            jump.actions[0],
            SemanticReviewAction::Select {
                reveal: Some(REVIEW_FILE_JUMP_REVEAL),
                ..
            }
        ));
        let custom = ReviewRevealRequest {
            anchor: ReviewRevealAnchor::Hunk,
            scroll_to_note: false,
        };
        assert!(matches!(
            plan_semantic_review_intent(
                &state,
                SemanticReviewIntent::SelectFile {
                    file_key: "beta".into(),
                    reveal: Some(custom)
                },
                &ReviewIntentFacts::default()
            )
            .unwrap()
            .actions[0],
            SemanticReviewAction::Select { reveal: Some(value), .. } if value == custom
        ));
        assert_eq!(
            error_code(
                &state,
                SemanticReviewIntent::SelectFile {
                    file_key: "missing".into(),
                    reveal: None
                },
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::FileNotFound
        );
        let anchored = apply_plan(
            state.clone(),
            plan_semantic_review_intent(
                &state,
                SemanticReviewIntent::Anchor {
                    file_key: "beta".into(),
                    hunk_index: 1,
                },
                &ReviewIntentFacts::default(),
            )
            .unwrap(),
        );
        assert_eq!(anchored.selection.hunk_index, 1);
        assert_eq!(anchored.reveal, crate::ReviewRevealIntent::default());
    }

    #[test]
    fn draft_start_uses_default_or_measured_target_and_validates_owner_and_hunk() {
        let state = state();
        let plan = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::StartDraft {
                file_key: "alpha".into(),
                hunk_index: 1,
                target: None,
                reveal: None,
            },
            &ReviewIntentFacts {
                draft_id: Some("draft:1".into()),
                ..ReviewIntentFacts::default()
            },
        )
        .unwrap();
        let Some(ReviewIntentOutcome::DraftStarted(draft)) = plan.outcome else {
            panic!("draft outcome")
        };
        assert_eq!(draft.line, 12);
        assert_eq!(draft.side, ReviewSide::New);
        assert!(matches!(
            plan.actions[1],
            SemanticReviewAction::Select {
                reveal: Some(REVIEW_DRAFT_START_REVEAL),
                ..
            }
        ));

        let measured = SemanticReviewLineAddress {
            side: ReviewSide::Old,
            line: 2,
        };
        let preserve = REVIEW_VIEWPORT_ANCHOR_REVEAL;
        let custom = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::StartDraft {
                file_key: "alpha".into(),
                hunk_index: 0,
                target: Some(measured),
                reveal: Some(preserve),
            },
            &ReviewIntentFacts {
                draft_id: Some("draft:2".into()),
                ..ReviewIntentFacts::default()
            },
        )
        .unwrap();
        assert!(matches!(
            &custom.actions[0],
            SemanticReviewAction::StartDraft(draft) if draft.side == ReviewSide::Old && draft.line == 2
        ));
        assert!(matches!(
            custom.actions[1],
            SemanticReviewAction::Select { reveal: Some(value), .. } if value == preserve
        ));
        assert_eq!(
            error_code(
                &state,
                SemanticReviewIntent::StartDraft {
                    file_key: "alpha".into(),
                    hunk_index: 0,
                    target: None,
                    reveal: None,
                },
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::MissingFact
        );
        assert_eq!(
            error_code(
                &state,
                SemanticReviewIntent::StartDraft {
                    file_key: "alpha".into(),
                    hunk_index: 9,
                    target: None,
                    reveal: None,
                },
                &ReviewIntentFacts {
                    draft_id: Some("draft:3".into()),
                    ..ReviewIntentFacts::default()
                }
            ),
            ReviewIntentPlanningErrorCode::HunkNotFound
        );
        let mut active = state;
        active.draft_note = Some(draft);
        assert_eq!(
            error_code(
                &active,
                SemanticReviewIntent::StartDraft {
                    file_key: "alpha".into(),
                    hunk_index: 0,
                    target: None,
                    reveal: None,
                },
                &ReviewIntentFacts {
                    draft_id: Some("other".into()),
                    ..ReviewIntentFacts::default()
                }
            ),
            ReviewIntentPlanningErrorCode::DraftActive
        );
    }

    #[test]
    fn user_creation_trims_and_anchors_then_blank_creation_only_cancels() {
        let plan = plan_semantic_review_intent(
            &draft_state("  looks wrong  "),
            SemanticReviewIntent::CreateUserNote,
            &facts(),
        )
        .unwrap();
        let Some(ReviewIntentOutcome::NoteCreated(note)) = plan.outcome else {
            panic!("created note")
        };
        assert_eq!(note.note.id, "user:1");
        assert_eq!(note.note.summary, "looks wrong");
        assert_eq!(note.note.anchor.new_range, Some([12, 12]));
        assert_eq!(note.note.anchor.intersecting_hunk_indices, [1]);
        assert_eq!(note.note.anchor.owner_hunk_index, Some(1));
        assert_eq!(
            note.note.created_at.as_deref(),
            facts().timestamp.as_deref()
        );
        assert!(matches!(
            plan.actions[0],
            SemanticReviewAction::SaveDraft(_)
        ));

        let blank = plan_semantic_review_intent(
            &draft_state("   "),
            SemanticReviewIntent::CreateUserNote,
            &facts(),
        )
        .unwrap();
        assert_eq!(blank.actions, [SemanticReviewAction::CancelDraft]);
        assert!(blank.outcome.is_none());
    }

    #[test]
    fn user_creation_rejects_missing_draft_facts_stale_hunk_conflicts_and_edit_mode() {
        assert_eq!(
            error_code(&state(), SemanticReviewIntent::CreateUserNote, &facts()),
            ReviewIntentPlanningErrorCode::DraftMissing
        );
        assert_eq!(
            error_code(
                &draft_state("body"),
                SemanticReviewIntent::CreateUserNote,
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::MissingFact
        );
        let mut stale = draft_state("body");
        Arc::make_mut(&mut stale.document).files[0]
            .hunks
            .truncate(1);
        assert_eq!(
            error_code(&stale, SemanticReviewIntent::CreateUserNote, &facts()),
            ReviewIntentPlanningErrorCode::HunkNotFound
        );
        let mut conflict = draft_state("body");
        conflict.user_notes.push(stored(
            "user:1",
            "alpha",
            ReviewNoteSource::User,
            0,
            1,
            None,
        ));
        assert_eq!(
            error_code(&conflict, SemanticReviewIntent::CreateUserNote, &facts()),
            ReviewIntentPlanningErrorCode::NoteIdConflict
        );
        let mut edit = draft_state("body");
        edit.draft_note.as_mut().unwrap().kind = ReviewDraftKind::Edit {
            target_note_id: "user".into(),
        };
        assert_eq!(
            error_code(&edit, SemanticReviewIntent::CreateUserNote, &facts()),
            ReviewIntentPlanningErrorCode::DraftModeMismatch
        );
    }

    #[test]
    fn edit_prefills_clamps_owner_and_saves_identity_without_losing_creation_fields() {
        let mut state = state();
        let mut original = stored("user-1", "alpha", ReviewNoteSource::User, 1, 12, None);
        original.note.summary = "before".into();
        original.note.created_at = Some("2023-01-01T00:00:00.000Z".into());
        original.note.editable = true;
        let original_anchor = original.note.anchor.clone();
        state.user_notes.push(original);
        Arc::make_mut(&mut state.document).files[0]
            .hunks
            .truncate(1);
        let start = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::StartEdit {
                note_id: "user-1".into(),
                reveal: None,
            },
            &ReviewIntentFacts {
                draft_id: Some("draft:edit".into()),
                ..ReviewIntentFacts::default()
            },
        )
        .unwrap();
        let mut state = apply_plan(state, start);
        let draft = state.draft_note.as_ref().unwrap();
        assert_eq!(draft.hunk_index, 0);
        assert_eq!(draft.body, "before");
        assert!(matches!(draft.kind, ReviewDraftKind::Edit { .. }));
        state = apply_plan(
            state.clone(),
            plan_semantic_review_intent(
                &state,
                SemanticReviewIntent::UpdateDraft("  after  ".into()),
                &ReviewIntentFacts::default(),
            )
            .unwrap(),
        );
        let save = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::UpdateUserNote {
                note_id: "user-1".into(),
            },
            &ReviewIntentFacts {
                timestamp: facts().timestamp,
                ..ReviewIntentFacts::default()
            },
        )
        .unwrap();
        let Some(ReviewIntentOutcome::NoteUpdated(updated)) = &save.outcome else {
            panic!("updated note")
        };
        assert_eq!(updated.note.id, "user-1");
        assert_eq!(updated.note.summary, "after");
        assert_eq!(
            updated.note.created_at.as_deref(),
            Some("2023-01-01T00:00:00.000Z")
        );
        assert_eq!(updated.note.anchor, original_anchor);
        let next = apply_plan(state, save);
        assert!(next.draft_note.is_none());
        assert_eq!(next.user_notes.len(), 1);
    }

    #[test]
    fn edit_validation_keeps_blank_draft_and_rejects_wrong_modes_or_capability() {
        let mut state = state();
        let mut user = stored("user-1", "alpha", ReviewNoteSource::User, 0, 2, None);
        user.note.editable = true;
        state.user_notes.push(user);
        let started = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::StartEdit {
                note_id: "user-1".into(),
                reveal: None,
            },
            &ReviewIntentFacts {
                draft_id: Some("draft:edit".into()),
                ..ReviewIntentFacts::default()
            },
        )
        .unwrap();
        let mut started = apply_plan(state, started);
        started.draft_note.as_mut().unwrap().body = "  ".into();
        assert_eq!(
            error_code(
                &started,
                SemanticReviewIntent::UpdateUserNote {
                    note_id: "user-1".into()
                },
                &ReviewIntentFacts {
                    timestamp: facts().timestamp,
                    ..ReviewIntentFacts::default()
                }
            ),
            ReviewIntentPlanningErrorCode::BlankNote
        );
        assert!(started.draft_note.is_some());
        started.draft_note.as_mut().unwrap().kind = ReviewDraftKind::Create;
        assert_eq!(
            error_code(
                &started,
                SemanticReviewIntent::UpdateUserNote {
                    note_id: "user-1".into()
                },
                &facts()
            ),
            ReviewIntentPlanningErrorCode::DraftModeMismatch
        );
        started.draft_note = None;
        assert_eq!(
            error_code(
                &started,
                SemanticReviewIntent::UpdateDraft("x".into()),
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::DraftMissing
        );
        assert_eq!(
            error_code(
                &started,
                SemanticReviewIntent::CancelDraft,
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::DraftMissing
        );
        started.user_notes[0].note.editable = false;
        assert_eq!(
            error_code(
                &started,
                SemanticReviewIntent::StartEdit {
                    note_id: "user-1".into(),
                    reveal: None
                },
                &ReviewIntentFacts {
                    draft_id: Some("draft".into()),
                    ..ReviewIntentFacts::default()
                }
            ),
            ReviewIntentPlanningErrorCode::NoteNotEditable
        );
    }

    #[test]
    fn replies_copy_parent_identity_anchor_and_resolution_and_validate_parent() {
        let mut state = state();
        let parent = stored("live-1", "alpha", ReviewNoteSource::Agent, 1, 12, None);
        let parent_anchor = parent.note.anchor.clone();
        state.live_notes.push(parent);
        let start = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::StartReply {
                note_id: "live-1".into(),
                reveal: None,
            },
            &ReviewIntentFacts {
                draft_id: Some("draft:reply".into()),
                ..ReviewIntentFacts::default()
            },
        )
        .unwrap();
        let mut state = apply_plan(state, start);
        state.draft_note.as_mut().unwrap().body = "reply body".into();
        let created =
            plan_semantic_review_intent(&state, SemanticReviewIntent::CreateUserNote, &facts())
                .unwrap();
        let Some(ReviewIntentOutcome::NoteCreated(reply)) = created.outcome else {
            panic!("reply")
        };
        assert_eq!(reply.note.parent_id.as_deref(), Some("live-1"));
        assert_eq!(reply.note.anchor, parent_anchor);

        state.draft_note = None;
        state.live_notes[0].resolution = ReviewNoteResolution::Orphaned;
        assert_eq!(
            error_code(
                &state,
                SemanticReviewIntent::StartReply {
                    note_id: "live-1".into(),
                    reveal: None
                },
                &ReviewIntentFacts {
                    draft_id: Some("draft".into()),
                    ..ReviewIntentFacts::default()
                }
            ),
            ReviewIntentPlanningErrorCode::NoteNotFound
        );
    }

    #[test]
    fn reply_creation_rejects_missing_or_cross_file_parent_and_active_composers() {
        let mut missing = draft_state("reply");
        missing.draft_note.as_mut().unwrap().kind = ReviewDraftKind::Reply {
            parent_id: "gone".into(),
        };
        assert_eq!(
            error_code(&missing, SemanticReviewIntent::CreateUserNote, &facts()),
            ReviewIntentPlanningErrorCode::InvalidNoteParent
        );

        let mut cross_file = draft_state("reply");
        cross_file.live_notes.push(stored(
            "parent",
            "beta",
            ReviewNoteSource::Agent,
            0,
            1,
            None,
        ));
        cross_file.draft_note.as_mut().unwrap().kind = ReviewDraftKind::Reply {
            parent_id: "parent".into(),
        };
        assert_eq!(
            error_code(&cross_file, SemanticReviewIntent::CreateUserNote, &facts()),
            ReviewIntentPlanningErrorCode::InvalidNoteParent
        );

        let draft_facts = ReviewIntentFacts {
            draft_id: Some("draft".into()),
            ..ReviewIntentFacts::default()
        };
        for intent in [
            SemanticReviewIntent::StartReply {
                note_id: "parent".into(),
                reveal: None,
            },
            SemanticReviewIntent::StartEdit {
                note_id: "missing".into(),
                reveal: None,
            },
        ] {
            assert_eq!(
                error_code(&cross_file, intent, &draft_facts),
                ReviewIntentPlanningErrorCode::DraftActive
            );
        }
        cross_file.draft_note = None;
        assert_eq!(
            error_code(
                &cross_file,
                SemanticReviewIntent::StartEdit {
                    note_id: "missing".into(),
                    reveal: None
                },
                &draft_facts
            ),
            ReviewIntentPlanningErrorCode::NoteNotFound
        );
    }

    #[test]
    fn removal_targets_owner_collection_and_refuses_parents_with_replies() {
        let mut state = state();
        state.live_notes.push(stored(
            "live-1",
            "alpha",
            ReviewNoteSource::Agent,
            0,
            2,
            None,
        ));
        let removal = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::RemoveLiveNote {
                note_id: "live-1".into(),
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert_eq!(
            removal.actions,
            [SemanticReviewAction::RemoveLiveNote("live-1".into())]
        );
        assert_eq!(
            error_code(
                &state,
                SemanticReviewIntent::RemoveUserNote {
                    note_id: "live-1".into()
                },
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::NoteNotFound
        );
        state.user_notes.push(stored(
            "reply-1",
            "alpha",
            ReviewNoteSource::User,
            0,
            2,
            Some("live-1"),
        ));
        let error = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::RemoveLiveNote {
                note_id: "live-1".into(),
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, ReviewIntentPlanningErrorCode::NoteHasReplies);
        assert_eq!(
            error.message,
            "Review note live-1 cannot be removed while it has replies."
        );

        let mut user_only = SemanticReviewState::new(document(&[("alpha", 1)]), false);
        user_only.user_notes.push(stored(
            "user-1",
            "alpha",
            ReviewNoteSource::User,
            0,
            1,
            None,
        ));
        assert_eq!(
            plan_semantic_review_intent(
                &user_only,
                SemanticReviewIntent::RemoveUserNote {
                    note_id: "user-1".into()
                },
                &ReviewIntentFacts::default()
            )
            .unwrap()
            .actions,
            [SemanticReviewAction::RemoveUserNote("user-1".into())]
        );
    }

    #[test]
    fn bulk_clear_counts_scope_include_user_and_rejects_dangling_replies() {
        let mut state = state();
        state.live_notes = vec![
            stored("live-1", "alpha", ReviewNoteSource::Agent, 0, 1, None),
            stored("live-2", "beta", ReviewNoteSource::Agent, 0, 1, None),
        ];
        let plan = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::ClearNotes {
                file_key: Some("alpha".into()),
                include_user: false,
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert_eq!(
            plan.outcome,
            Some(ReviewIntentOutcome::NotesCleared {
                removed_live_count: 1,
                removed_user_count: 0,
                remaining_live_count: 1,
                remaining_user_count: 0,
            })
        );
        state.user_notes.push(stored(
            "reply-1",
            "alpha",
            ReviewNoteSource::User,
            0,
            1,
            Some("live-1"),
        ));
        assert_eq!(
            error_code(
                &state,
                SemanticReviewIntent::ClearNotes {
                    file_key: None,
                    include_user: false
                },
                &ReviewIntentFacts::default()
            ),
            ReviewIntentPlanningErrorCode::NoteHasReplies
        );
        let all = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::ClearNotes {
                file_key: None,
                include_user: true,
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert!(matches!(
            all.outcome,
            Some(ReviewIntentOutcome::NotesCleared {
                removed_live_count: 2,
                removed_user_count: 1,
                remaining_live_count: 0,
                remaining_user_count: 0
            })
        ));
    }

    #[test]
    fn apply_commits_complete_plan_and_planning_failure_preserves_snapshot_identity() {
        let store = SemanticReviewStore::new(Arc::clone(&state().document), false);
        let _ = store.dispatch(SemanticReviewAction::StartDraft(ReviewDraftNote {
            id: "d".into(),
            file_key: "alpha".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 3,
            body: "note body".into(),
            kind: ReviewDraftKind::Create,
        }));
        let outcome =
            apply_semantic_review_intent(&store, SemanticReviewIntent::CreateUserNote, &facts())
                .unwrap();
        assert!(matches!(outcome, Some(ReviewIntentOutcome::NoteCreated(_))));
        assert!(store.snapshot().draft_note.is_none());
        assert_eq!(store.snapshot().user_notes[0].note.id, "user:1");

        let before = store.snapshot();
        let error = apply_semantic_review_intent(
            &store,
            SemanticReviewIntent::RemoveUserNote {
                note_id: "missing".into(),
            },
            &ReviewIntentFacts::default(),
        );
        assert!(error.is_err());
        assert!(Arc::ptr_eq(&before, &store.snapshot()));
    }

    #[test]
    fn expansion_toggle_resolves_geometry_collapse_source_identity_side_and_invalid_ids() {
        let mut state = state();
        let plan = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::ToggleExpansion {
                file_key: "alpha".into(),
                gap_id: "before:1".into(),
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert_eq!(
            plan.actions,
            [SemanticReviewAction::ToggleExpansion {
                file_key: "alpha".into(),
                gap_id: "before:1".into(),
                expanded: true
            }]
        );
        assert!(matches!(
            plan.outcome,
            Some(ReviewIntentOutcome::ExpansionToggled {
                expanded: true,
                side: ReviewSide::New,
                old_range: [2, 10],
                new_range: [2, 10],
                line_count: 9,
                source_identity: None,
                ..
            })
        ));
        state.expanded_gaps.push(ReviewExpandedGapState {
            file_key: "alpha".into(),
            gap_id: "before:1".into(),
            expanded: true,
        });
        Arc::make_mut(&mut state.document).files[0].source_identity = Some("source:alpha".into());
        Arc::make_mut(&mut state.document).files[0].source_attested = Some(true);
        Arc::make_mut(&mut state.document).files[0].change_kind = ReviewFileChangeKind::Deleted;
        let collapse = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::ToggleExpansion {
                file_key: "alpha".into(),
                gap_id: "before:1".into(),
            },
            &ReviewIntentFacts::default(),
        )
        .unwrap();
        assert!(matches!(
            collapse.outcome,
            Some(ReviewIntentOutcome::ExpansionToggled {
                expanded: false,
                side: ReviewSide::Old,
                source_identity: Some(ref identity),
                ..
            }) if identity == "source:alpha"
        ));
        for gap_id in ["before:0", "trailing:1", "nonsense"] {
            assert_eq!(
                error_code(
                    &state,
                    SemanticReviewIntent::ToggleExpansion {
                        file_key: "alpha".into(),
                        gap_id: gap_id.into()
                    },
                    &ReviewIntentFacts::default()
                ),
                ReviewIntentPlanningErrorCode::GapNotFound
            );
        }
    }

    #[test]
    fn note_size_limit_is_enforced_for_creation_and_updates() {
        let huge = "x".repeat(crate::MAX_REVIEW_NOTE_BYTES);
        let state = draft_state(&huge);
        assert_eq!(
            error_code(&state, SemanticReviewIntent::CreateUserNote, &facts()),
            ReviewIntentPlanningErrorCode::NoteTooLarge
        );

        let mut update = SemanticReviewState::new(document(&[("alpha", 1)]), false);
        let mut user = stored("user-1", "alpha", ReviewNoteSource::User, 0, 1, None);
        user.note.editable = true;
        update.user_notes.push(user);
        update.draft_note = Some(ReviewDraftNote {
            id: "edit".into(),
            file_key: "alpha".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 1,
            body: huge,
            kind: ReviewDraftKind::Edit {
                target_note_id: "user-1".into(),
            },
        });
        assert_eq!(
            error_code(
                &update,
                SemanticReviewIntent::UpdateUserNote {
                    note_id: "user-1".into()
                },
                &ReviewIntentFacts {
                    timestamp: facts().timestamp,
                    ..ReviewIntentFacts::default()
                }
            ),
            ReviewIntentPlanningErrorCode::NoteTooLarge
        );
    }

    #[test]
    fn passthrough_and_update_intents_validate_then_lower_without_side_effects() {
        let state = state();
        assert_eq!(
            plan_semantic_review_intent(
                &state,
                SemanticReviewIntent::SetFilter("alpha".into()),
                &ReviewIntentFacts::default()
            )
            .unwrap()
            .actions,
            [SemanticReviewAction::SetFilter("alpha".into())]
        );
        assert_eq!(
            plan_semantic_review_intent(
                &state,
                SemanticReviewIntent::SetNoteVisibility(true),
                &ReviewIntentFacts::default()
            )
            .unwrap()
            .actions,
            [SemanticReviewAction::SetNoteVisibility(true)]
        );
        let mut active = draft_state("body");
        assert_eq!(
            plan_semantic_review_intent(
                &active,
                SemanticReviewIntent::CancelDraft,
                &ReviewIntentFacts::default()
            )
            .unwrap()
            .actions,
            [SemanticReviewAction::CancelDraft]
        );
        active.draft_note.as_mut().unwrap().kind = ReviewDraftKind::Edit {
            target_note_id: "missing".into(),
        };
        assert_eq!(
            error_code(
                &active,
                SemanticReviewIntent::UpdateUserNote {
                    note_id: "missing".into()
                },
                &ReviewIntentFacts {
                    timestamp: facts().timestamp,
                    ..ReviewIntentFacts::default()
                }
            ),
            ReviewIntentPlanningErrorCode::NoteNotFound
        );

        let timestamp_missing = draft_state("body");
        assert_eq!(
            error_code(
                &timestamp_missing,
                SemanticReviewIntent::CreateUserNote,
                &ReviewIntentFacts {
                    note_id: Some("new".into()),
                    ..ReviewIntentFacts::default()
                }
            ),
            ReviewIntentPlanningErrorCode::MissingFact
        );
    }
}
