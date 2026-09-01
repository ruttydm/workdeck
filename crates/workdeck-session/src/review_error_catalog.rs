//! Total reviewer-facing explanations for every review protocol failure.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use workdeck_review::{
    ReviewIntentPlanningErrorCode, ReviewRequestErrorCode, ReviewResourceErrorCode,
};

use crate::WorkdeckReviewTransportErrorCode;

/// Every failure code a review client can receive, composed across producer and transport tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkdeckReviewClientErrorCodeV1 {
    StaleGeneration,
    InvalidRequest,
    UnsupportedAction,
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
    UnknownResource,
    ResourceUnavailable,
    ResourceTooLarge,
    ResourceIntegrity,
    InvalidRange,
    Unauthorized,
    NoPublication,
    PayloadTooLarge,
    MethodNotAllowed,
    UnsupportedMediaType,
    ForbiddenOrigin,
    TooManyStreams,
}

impl WorkdeckReviewClientErrorCodeV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StaleGeneration => "stale-generation",
            Self::InvalidRequest => "invalid-request",
            Self::UnsupportedAction => "unsupported-action",
            Self::FileNotFound => "file-not-found",
            Self::HunkNotFound => "hunk-not-found",
            Self::GapNotFound => "gap-not-found",
            Self::DraftMissing => "draft-missing",
            Self::DraftActive => "draft-active",
            Self::DraftModeMismatch => "draft-mode-mismatch",
            Self::NoteNotFound => "note-not-found",
            Self::NoteNotEditable => "note-not-editable",
            Self::NoteHasReplies => "note-has-replies",
            Self::NoteIdConflict => "note-id-conflict",
            Self::InvalidNoteParent => "invalid-note-parent",
            Self::BlankNote => "blank-note",
            Self::NoteTooLarge => "note-too-large",
            Self::MissingFact => "missing-fact",
            Self::UnknownResource => "unknown-resource",
            Self::ResourceUnavailable => "resource-unavailable",
            Self::ResourceTooLarge => "resource-too-large",
            Self::ResourceIntegrity => "resource-integrity",
            Self::InvalidRange => "invalid-range",
            Self::Unauthorized => "unauthorized",
            Self::NoPublication => "no-publication",
            Self::PayloadTooLarge => "payload-too-large",
            Self::MethodNotAllowed => "method-not-allowed",
            Self::UnsupportedMediaType => "unsupported-media-type",
            Self::ForbiddenOrigin => "forbidden-origin",
            Self::TooManyStreams => "too-many-streams",
        }
    }
}

impl From<ReviewResourceErrorCode> for WorkdeckReviewClientErrorCodeV1 {
    fn from(code: ReviewResourceErrorCode) -> Self {
        match code {
            ReviewResourceErrorCode::UnknownResource => Self::UnknownResource,
            ReviewResourceErrorCode::ResourceUnavailable => Self::ResourceUnavailable,
            ReviewResourceErrorCode::ResourceTooLarge => Self::ResourceTooLarge,
            ReviewResourceErrorCode::ResourceIntegrity => Self::ResourceIntegrity,
            ReviewResourceErrorCode::InvalidRange => Self::InvalidRange,
        }
    }
}

impl From<ReviewRequestErrorCode> for WorkdeckReviewClientErrorCodeV1 {
    fn from(code: ReviewRequestErrorCode) -> Self {
        match code {
            ReviewRequestErrorCode::StaleGeneration => Self::StaleGeneration,
            ReviewRequestErrorCode::InvalidRequest => Self::InvalidRequest,
        }
    }
}

impl From<ReviewIntentPlanningErrorCode> for WorkdeckReviewClientErrorCodeV1 {
    fn from(code: ReviewIntentPlanningErrorCode) -> Self {
        match code {
            ReviewIntentPlanningErrorCode::FileNotFound => Self::FileNotFound,
            ReviewIntentPlanningErrorCode::HunkNotFound => Self::HunkNotFound,
            ReviewIntentPlanningErrorCode::GapNotFound => Self::GapNotFound,
            ReviewIntentPlanningErrorCode::DraftMissing => Self::DraftMissing,
            ReviewIntentPlanningErrorCode::DraftActive => Self::DraftActive,
            ReviewIntentPlanningErrorCode::DraftModeMismatch => Self::DraftModeMismatch,
            ReviewIntentPlanningErrorCode::NoteNotFound => Self::NoteNotFound,
            ReviewIntentPlanningErrorCode::NoteNotEditable => Self::NoteNotEditable,
            ReviewIntentPlanningErrorCode::NoteHasReplies => Self::NoteHasReplies,
            ReviewIntentPlanningErrorCode::NoteIdConflict => Self::NoteIdConflict,
            ReviewIntentPlanningErrorCode::InvalidNoteParent => Self::InvalidNoteParent,
            ReviewIntentPlanningErrorCode::BlankNote => Self::BlankNote,
            ReviewIntentPlanningErrorCode::NoteTooLarge => Self::NoteTooLarge,
            ReviewIntentPlanningErrorCode::MissingFact => Self::MissingFact,
        }
    }
}

impl From<WorkdeckReviewTransportErrorCode> for WorkdeckReviewClientErrorCodeV1 {
    fn from(code: WorkdeckReviewTransportErrorCode) -> Self {
        match code {
            WorkdeckReviewTransportErrorCode::Unauthorized => Self::Unauthorized,
            WorkdeckReviewTransportErrorCode::NoPublication => Self::NoPublication,
            WorkdeckReviewTransportErrorCode::PayloadTooLarge => Self::PayloadTooLarge,
            WorkdeckReviewTransportErrorCode::MethodNotAllowed => Self::MethodNotAllowed,
            WorkdeckReviewTransportErrorCode::UnsupportedMediaType => Self::UnsupportedMediaType,
            WorkdeckReviewTransportErrorCode::ForbiddenOrigin => Self::ForbiddenOrigin,
            WorkdeckReviewTransportErrorCode::UnsupportedAction => Self::UnsupportedAction,
            WorkdeckReviewTransportErrorCode::TooManyStreams => Self::TooManyStreams,
        }
    }
}

/// One stable failure explanation and the action a reviewer can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewErrorDoc {
    pub message: &'static str,
    pub remedy: &'static str,
}

pub const REVIEW_CLIENT_ERROR_CODES: [WorkdeckReviewClientErrorCodeV1; 29] = [
    WorkdeckReviewClientErrorCodeV1::StaleGeneration,
    WorkdeckReviewClientErrorCodeV1::InvalidRequest,
    WorkdeckReviewClientErrorCodeV1::UnsupportedAction,
    WorkdeckReviewClientErrorCodeV1::FileNotFound,
    WorkdeckReviewClientErrorCodeV1::HunkNotFound,
    WorkdeckReviewClientErrorCodeV1::GapNotFound,
    WorkdeckReviewClientErrorCodeV1::DraftMissing,
    WorkdeckReviewClientErrorCodeV1::DraftActive,
    WorkdeckReviewClientErrorCodeV1::DraftModeMismatch,
    WorkdeckReviewClientErrorCodeV1::NoteNotFound,
    WorkdeckReviewClientErrorCodeV1::NoteNotEditable,
    WorkdeckReviewClientErrorCodeV1::NoteHasReplies,
    WorkdeckReviewClientErrorCodeV1::NoteIdConflict,
    WorkdeckReviewClientErrorCodeV1::InvalidNoteParent,
    WorkdeckReviewClientErrorCodeV1::BlankNote,
    WorkdeckReviewClientErrorCodeV1::NoteTooLarge,
    WorkdeckReviewClientErrorCodeV1::MissingFact,
    WorkdeckReviewClientErrorCodeV1::UnknownResource,
    WorkdeckReviewClientErrorCodeV1::ResourceUnavailable,
    WorkdeckReviewClientErrorCodeV1::ResourceTooLarge,
    WorkdeckReviewClientErrorCodeV1::ResourceIntegrity,
    WorkdeckReviewClientErrorCodeV1::InvalidRange,
    WorkdeckReviewClientErrorCodeV1::Unauthorized,
    WorkdeckReviewClientErrorCodeV1::NoPublication,
    WorkdeckReviewClientErrorCodeV1::PayloadTooLarge,
    WorkdeckReviewClientErrorCodeV1::MethodNotAllowed,
    WorkdeckReviewClientErrorCodeV1::UnsupportedMediaType,
    WorkdeckReviewClientErrorCodeV1::ForbiddenOrigin,
    WorkdeckReviewClientErrorCodeV1::TooManyStreams,
];

/// The sentence and remedy one code stands for. The exhaustive match is the totality gate.
#[must_use]
pub const fn describe_review_error(code: WorkdeckReviewClientErrorCodeV1) -> ReviewErrorDoc {
    use WorkdeckReviewClientErrorCodeV1 as Code;
    match code {
        Code::StaleGeneration => ReviewErrorDoc {
            message: "The review changed after this request was prepared.",
            remedy: "Reload the review and try again; nothing was applied.",
        },
        Code::InvalidRequest => ReviewErrorDoc {
            message: "The review request was not expressible.",
            remedy: "This is a client bug rather than something to retry; report it with what you did.",
        },
        Code::UnsupportedAction => ReviewErrorDoc {
            message: "This Workdeck session does not know the action that was requested.",
            remedy: "The client and the session are different versions; update whichever is older.",
        },
        Code::FileNotFound => ReviewErrorDoc {
            message: "That file is not part of the review any more.",
            remedy: "Reload the review; the file was removed or the changeset moved on.",
        },
        Code::HunkNotFound => ReviewErrorDoc {
            message: "That hunk is not part of the file any more.",
            remedy: "Reload the review and pick the change again.",
        },
        Code::GapNotFound => ReviewErrorDoc {
            message: "That collapsed region no longer covers the line it was expanded from.",
            remedy: "Reload the file and expand the region again.",
        },
        Code::DraftMissing => ReviewErrorDoc {
            message: "There is no open note draft at that line.",
            remedy: "Start the note again; another surface may have saved or cancelled this draft.",
        },
        Code::DraftActive => ReviewErrorDoc {
            message: "Another note draft is already open.",
            remedy: "Save or cancel the open draft before starting another one.",
        },
        Code::DraftModeMismatch => ReviewErrorDoc {
            message: "The open composer is for a different note action.",
            remedy: "Reload the review, then reopen the note action you want.",
        },
        Code::NoteNotFound => ReviewErrorDoc {
            message: "That note is no longer on the review.",
            remedy: "Reload the review; someone else may have removed it.",
        },
        Code::NoteNotEditable => ReviewErrorDoc {
            message: "That note cannot be edited.",
            remedy: "Only editable notes written by the reviewer can be changed.",
        },
        Code::NoteHasReplies => ReviewErrorDoc {
            message: "That note still has replies attached to it.",
            remedy: "Remove its replies first, then remove the note.",
        },
        Code::NoteIdConflict => ReviewErrorDoc {
            message: "That note identity is already in use.",
            remedy: "Retry the action so the client allocates a new identity.",
        },
        Code::InvalidNoteParent => ReviewErrorDoc {
            message: "The note being replied to is no longer a valid parent.",
            remedy: "Reload the review and choose the comment again.",
        },
        Code::BlankNote => ReviewErrorDoc {
            message: "An edited note cannot be blank.",
            remedy: "Enter some text, cancel the edit, or delete the note explicitly.",
        },
        Code::NoteTooLarge => ReviewErrorDoc {
            message: "That note is larger than Workdeck can store or publish.",
            remedy: "Shorten the note and try again.",
        },
        Code::MissingFact => ReviewErrorDoc {
            message: "The action arrived without something only its caller can supply.",
            remedy: "This is a client bug rather than something to retry; report it with what you did.",
        },
        Code::UnknownResource => ReviewErrorDoc {
            message: "The review does not offer that content.",
            remedy: "Reload the review; the content belonged to an earlier version of it.",
        },
        Code::ResourceUnavailable => ReviewErrorDoc {
            message: "Workdeck could not read that content from the session that published it.",
            remedy: "The file may have changed on disk. Refresh the session and try again.",
        },
        Code::ResourceTooLarge => ReviewErrorDoc {
            message: "That content is larger than Workdeck will send to a review client.",
            remedy: "Open the file in an editor instead; very large files are not reviewed in full here.",
        },
        Code::ResourceIntegrity => ReviewErrorDoc {
            message: "The content that arrived does not match what the session measured.",
            remedy: "Reload the review. If it keeps happening, the file is changing while it is read.",
        },
        Code::InvalidRange => ReviewErrorDoc {
            message: "The requested part of that content does not exist.",
            remedy: "This is a client bug rather than something to retry; report it with what you did.",
        },
        Code::Unauthorized => ReviewErrorDoc {
            message: "This review link is not valid for that session.",
            remedy: "Open the review from the terminal running it to get a current link.",
        },
        Code::NoPublication => ReviewErrorDoc {
            message: "That Workdeck session is not publishing a review yet.",
            remedy: "Wait for the session to finish loading, then reload.",
        },
        Code::PayloadTooLarge => ReviewErrorDoc {
            message: "The request body is larger than the review surface accepts.",
            remedy: "Split the work into smaller actions.",
        },
        Code::MethodNotAllowed => ReviewErrorDoc {
            message: "That review route does not answer this kind of request.",
            remedy: "This is a client bug rather than something to retry; report it with what you did.",
        },
        Code::UnsupportedMediaType => ReviewErrorDoc {
            message: "Review actions must be sent as JSON.",
            remedy: "This is a client bug rather than something to retry; report it with what you did.",
        },
        Code::ForbiddenOrigin => ReviewErrorDoc {
            message: "The review surface only answers requests from this machine.",
            remedy: "Open the review link on the machine running Workdeck.",
        },
        Code::TooManyStreams => ReviewErrorDoc {
            message: "This review already has as many live connections as it will keep open.",
            remedy: "Close another review tab and reconnect.",
        },
    }
}

/// Every code and its explanation, in diagnostic tier order.
pub static REVIEW_ERROR_CATALOG: LazyLock<
    BTreeMap<WorkdeckReviewClientErrorCodeV1, ReviewErrorDoc>,
> = LazyLock::new(|| {
    REVIEW_CLIENT_ERROR_CODES
        .into_iter()
        .map(|code| (code, describe_review_error(code)))
        .collect()
});

/// Render one code as the exact statement/remedy pair clients show.
#[must_use]
pub fn review_error_message(code: WorkdeckReviewClientErrorCodeV1) -> String {
    let doc = describe_review_error(code);
    format!("{} {}", doc.message, doc.remedy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_exactly_the_published_client_vocabulary() {
        assert_eq!(REVIEW_ERROR_CATALOG.len(), 29);
        assert_eq!(REVIEW_ERROR_CATALOG.keys().copied().collect::<Vec<_>>(), {
            let mut codes = REVIEW_CLIENT_ERROR_CODES.to_vec();
            codes.sort_unstable();
            codes
        });
        for code in REVIEW_CLIENT_ERROR_CODES {
            assert_eq!(
                serde_json::to_string(&code).unwrap(),
                format!("\"{}\"", code.as_str())
            );
        }
    }

    #[test]
    fn every_code_has_a_sentence_and_remedy_without_echoing_the_wire_code() {
        for code in REVIEW_CLIENT_ERROR_CODES {
            let doc = describe_review_error(code);
            assert!(!doc.message.is_empty());
            assert!(!doc.remedy.is_empty());
            assert!(doc.message.ends_with('.'));
            assert!(doc.remedy.ends_with('.'));
            assert!(!review_error_message(code).contains(code.as_str()));
        }
    }

    #[test]
    fn rendered_error_is_the_statement_followed_by_the_remedy() {
        let code = WorkdeckReviewClientErrorCodeV1::Unauthorized;
        let doc = REVIEW_ERROR_CATALOG[&code];
        assert_eq!(
            review_error_message(code),
            format!("{} {}", doc.message, doc.remedy)
        );
    }

    #[test]
    fn composed_producer_and_transport_vocabularies_have_catalog_entries() {
        for code in [
            ReviewRequestErrorCode::StaleGeneration.into(),
            ReviewResourceErrorCode::UnknownResource.into(),
            ReviewIntentPlanningErrorCode::FileNotFound.into(),
            ReviewIntentPlanningErrorCode::HunkNotFound.into(),
            ReviewIntentPlanningErrorCode::GapNotFound.into(),
            WorkdeckReviewTransportErrorCode::Unauthorized.into(),
        ] {
            assert!(REVIEW_ERROR_CATALOG.contains_key(&code));
        }
    }
}
