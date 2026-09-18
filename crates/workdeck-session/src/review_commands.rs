//! App-facing semantic review commands backed by the generation-owning producer.

use chrono::{SecondsFormat, Utc};
use workdeck_core::{ReviewSide, SemanticReviewFile};
use workdeck_review::{
    ReviewIntentFacts, ReviewProducer, ReviewProducerChunkResult, ReviewProducerErrorCode,
    ReviewProducerIntentError, ReviewPublicationAddress, ReviewPublicationOrder,
    classify_review_publication, require_semantic_review_file, review_gap_address,
    semantic_review_gap_source,
};

use crate::{
    WorkdeckReviewActionAppliedV1, WorkdeckReviewActionEnvelopeV1, WorkdeckReviewActionResultV1,
    WorkdeckReviewActionV1, WorkdeckReviewExpandedLineProofV1, WorkdeckReviewFailureCodeV1,
    WorkdeckReviewFailureV1, WorkdeckReviewLineAddressV1, WorkdeckReviewResourceReadEnvelopeV1,
    WorkdeckReviewResourceReadResultV1, to_semantic_review_intent,
};

impl From<ReviewProducerErrorCode> for WorkdeckReviewFailureCodeV1 {
    fn from(value: ReviewProducerErrorCode) -> Self {
        match value {
            ReviewProducerErrorCode::StaleGeneration => Self::StaleGeneration,
            ReviewProducerErrorCode::InvalidRequest => Self::InvalidRequest,
            ReviewProducerErrorCode::UnknownResource => Self::UnknownResource,
            ReviewProducerErrorCode::ResourceUnavailable => Self::ResourceUnavailable,
            ReviewProducerErrorCode::ResourceTooLarge => Self::ResourceTooLarge,
            ReviewProducerErrorCode::ResourceIntegrity => Self::ResourceIntegrity,
            ReviewProducerErrorCode::InvalidRange => Self::InvalidRange,
        }
    }
}

fn fail(
    producer: &ReviewProducer,
    code: WorkdeckReviewFailureCodeV1,
    message: impl Into<String>,
) -> WorkdeckReviewFailureV1 {
    WorkdeckReviewFailureV1 {
        ok: false,
        code,
        message: message.into(),
        current_generation: producer.get_publication().generation.clone(),
    }
}

#[must_use]
pub fn read_session_review_resource(
    producer: &ReviewProducer,
    envelope: &WorkdeckReviewResourceReadEnvelopeV1,
) -> WorkdeckReviewResourceReadResultV1 {
    let request = serde_json::to_value(&envelope.request).expect("review requests serialize");
    match producer.read_resource(&request) {
        ReviewProducerChunkResult::Chunk(chunk) => {
            WorkdeckReviewResourceReadResultV1::Chunk { ok: true, chunk }
        }
        ReviewProducerChunkResult::Failure(failure) => WorkdeckReviewResourceReadResultV1::Failed(
            fail(producer, failure.code.into(), failure.message),
        ),
    }
}

fn check_position(
    producer: &ReviewProducer,
    envelope: &WorkdeckReviewActionEnvelopeV1,
) -> Option<WorkdeckReviewFailureV1> {
    let current = producer.get_publication_address();
    if envelope.generation != current.generation {
        return Some(fail(
            producer,
            WorkdeckReviewFailureCodeV1::StaleGeneration,
            format!(
                "Review generation {} is not being served; the review is now at {}.",
                envelope.generation, current.generation
            ),
        ));
    }
    let expected = envelope.expected_state_revision?;
    let claimed = ReviewPublicationAddress {
        generation: envelope.generation.clone(),
        state_revision: expected,
    };
    (classify_review_publication(&claimed, &current) != ReviewPublicationOrder::Stale).then(|| {
        fail(
            producer,
            WorkdeckReviewFailureCodeV1::StaleGeneration,
            format!(
                "The review advanced to revision {} after {expected}; reload before acting on it.",
                current.state_revision
            ),
        )
    })
}

fn check_expanded_line(
    producer: &ReviewProducer,
    file: &SemanticReviewFile,
    target: WorkdeckReviewLineAddressV1,
    proof: &WorkdeckReviewExpandedLineProofV1,
) -> Option<WorkdeckReviewFailureV1> {
    if proof.side != target.side || proof.line != target.line {
        return Some(fail(
            producer,
            WorkdeckReviewFailureCodeV1::InvalidRequest,
            format!(
                "The expanded-line proof describes {:?} line {}, not the {:?} line {} it accompanies.",
                proof.side, proof.line, target.side, target.line
            ),
        ));
    }
    let valid = u32::try_from(proof.line).ok().is_some_and(|line| {
        file.source_identity.as_deref() == Some(&proof.source_identity)
            && review_gap_address(&semantic_review_gap_source(file), &proof.gap_id).is_some_and(
                |gap| {
                    let range = match proof.side {
                        ReviewSide::Old => gap.old_range,
                        ReviewSide::New => gap.new_range,
                    };
                    range.start <= line && line <= range.end
                },
            )
    });
    (!valid).then(|| {
        fail(
            producer,
            WorkdeckReviewFailureCodeV1::GapNotFound,
            format!(
                "Review gap {} in {} no longer contains {:?} line {}.",
                proof.gap_id, file.path, proof.side, proof.line
            ),
        )
    })
}

fn check_against_review(
    producer: &ReviewProducer,
    state: &workdeck_review::SemanticReviewState,
    action: &WorkdeckReviewActionV1,
) -> Result<Option<WorkdeckReviewFailureV1>, workdeck_review::ReviewIntentPlanningError> {
    match action {
        WorkdeckReviewActionV1::NotesStartDraft {
            file_key,
            target: Some(target),
            expanded_line_proof: Some(proof),
            ..
        } => {
            let file = require_semantic_review_file(state, file_key)?;
            Ok(check_expanded_line(producer, file, *target, proof))
        }
        WorkdeckReviewActionV1::NotesCreateUser {
            target: Some(target),
            expanded_line_proof,
            ..
        } => {
            let Some(draft) = &state.draft_note else {
                return Ok(Some(fail(
                    producer,
                    WorkdeckReviewFailureCodeV1::DraftMissing,
                    format!(
                        "No review note draft is open at {:?} line {}.",
                        target.side, target.line
                    ),
                )));
            };
            if draft.side != target.side || u64::from(draft.line) != target.line {
                return Ok(Some(fail(
                    producer,
                    WorkdeckReviewFailureCodeV1::DraftMissing,
                    format!(
                        "No review note draft is open at {:?} line {}.",
                        target.side, target.line
                    ),
                )));
            }
            let Some(proof) = expanded_line_proof else {
                return Ok(None);
            };
            let file = require_semantic_review_file(state, &draft.file_key)?;
            Ok(check_expanded_line(producer, file, *target, proof))
        }
        _ => Ok(None),
    }
}

fn random_uuid() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).expect("operating system randomness is required for note ids");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

#[must_use]
pub fn apply_session_review_action(
    producer: &ReviewProducer,
    envelope: &WorkdeckReviewActionEnvelopeV1,
) -> WorkdeckReviewActionResultV1 {
    if let Some(position) = check_position(producer, envelope) {
        return WorkdeckReviewActionResultV1::Failed(position);
    }
    let Some(state) = producer.get_review_state() else {
        return WorkdeckReviewActionResultV1::Failed(fail(
            producer,
            WorkdeckReviewFailureCodeV1::InvalidRequest,
            "This session has no live review state attached to act on.",
        ));
    };
    match check_against_review(producer, &state, &envelope.action) {
        Ok(Some(rejected)) => return WorkdeckReviewActionResultV1::Failed(rejected),
        Err(error) => {
            return WorkdeckReviewActionResultV1::Failed(fail(
                producer,
                error.code.into(),
                error.message,
            ));
        }
        Ok(None) => {}
    }
    let Some(intent) = to_semantic_review_intent(&envelope.action) else {
        return WorkdeckReviewActionResultV1::Failed(fail(
            producer,
            WorkdeckReviewFailureCodeV1::InvalidRequest,
            "The review action contains coordinates outside this native build's range.",
        ));
    };
    let facts = ReviewIntentFacts {
        draft_id: Some(format!("draft:{}", random_uuid())),
        note_id: Some(format!("user:{}", random_uuid())),
        timestamp: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
        annotations: None,
    };
    match producer.apply_intent(intent, facts) {
        Ok(_) => {
            let applied = producer.get_publication_address();
            WorkdeckReviewActionResultV1::Applied(WorkdeckReviewActionAppliedV1 {
                ok: true,
                generation: applied.generation,
                state_revision: applied.state_revision,
            })
        }
        Err(ReviewProducerIntentError::Planning(error)) => {
            WorkdeckReviewActionResultV1::Failed(fail(producer, error.code.into(), error.message))
        }
        Err(ReviewProducerIntentError::NoReviewState) => {
            WorkdeckReviewActionResultV1::Failed(fail(
                producer,
                WorkdeckReviewFailureCodeV1::InvalidRequest,
                "This session has no live review state attached to act on.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use base64::Engine as _;
    use workdeck_core::{
        DiffFile, DiffHunk, DiffLine, DiffLineKind, FileChangeKind, FileFlags, FileSourceSnapshots,
        FileStats, SemanticReviewLineAddress, SourceOrigin, SourceSnapshot,
    };
    use workdeck_review::{
        PublishReviewInput, ReviewGapPosition, ReviewIntentOutcome, ReviewProducerOptions,
        ReviewResourceAddress, ReviewResourceKind, SemanticReviewIntent, SemanticReviewStore,
        review_gap_id, review_resource_id,
    };

    use super::*;
    use crate::{
        WORKDECK_REVIEW_PROTOCOL_VERSION, WorkdeckReviewActorKindV1, WorkdeckReviewActorV1,
    };

    fn changed_line(kind: DiffLineKind, line: u32, text: &str) -> DiffLine {
        DiffLine {
            kind,
            content: text.into(),
            old_line: (kind == DiffLineKind::Deletion).then_some(line),
            new_line: (kind == DiffLineKind::Addition).then_some(line),
            moved: false,
            no_newline_at_eof: false,
        }
    }

    fn test_file() -> DiffFile {
        let before = (1..=20)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = (1..=20)
            .map(|line| {
                if matches!(line, 2 | 18) {
                    format!("changed {line}\n")
                } else {
                    format!("line {line}\n")
                }
            })
            .collect::<String>();
        let mut file = DiffFile {
            key: String::new(),
            runtime_id: "file-1".into(),
            path: "src/example.ts".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("typescript".into()),
            stats: FileStats {
                additions: 2,
                deletions: 2,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: "@@ -2 +2 @@\n-line 2\n+changed 2\n@@ -18 +18 @@\n-line 18\n+changed 18\n"
                .into(),
            split_row_count: 2,
            stack_row_count: 4,
            hunks: vec![
                DiffHunk {
                    index: 0,
                    header: "@@ -2 +2 @@".into(),
                    context: None,
                    old_start: 2,
                    old_count: 1,
                    new_start: 2,
                    new_count: 1,
                    split_row_start: 0,
                    split_row_count: 1,
                    stack_row_start: 0,
                    stack_row_count: 2,
                    lines: vec![
                        changed_line(DiffLineKind::Deletion, 2, "line 2"),
                        changed_line(DiffLineKind::Addition, 2, "changed 2"),
                    ],
                },
                DiffHunk {
                    index: 1,
                    header: "@@ -18 +18 @@".into(),
                    context: None,
                    old_start: 18,
                    old_count: 1,
                    new_start: 18,
                    new_count: 1,
                    split_row_start: 1,
                    split_row_count: 1,
                    stack_row_start: 2,
                    stack_row_count: 2,
                    lines: vec![
                        changed_line(DiffLineKind::Deletion, 18, "line 18"),
                        changed_line(DiffLineKind::Addition, 18, "changed 18"),
                    ],
                },
            ],
            content_identity: String::new(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_capability: None,
            source_attested: false,
            agent: None,
        };
        file.refresh_identity();
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(before, SourceOrigin::WorkingTree, true)),
            new: Some(SourceSnapshot::new(after, SourceOrigin::WorkingTree, true)),
        });
        file
    }

    fn producer() -> ReviewProducer {
        let producer = ReviewProducer::new(
            PublishReviewInput {
                files: vec![test_file()],
                source_label: Some("/repo".into()),
            },
            ReviewProducerOptions {
                producer_id: Some("test".into()),
                ..ReviewProducerOptions::default()
            },
        )
        .unwrap();
        let publication = producer.get_publication();
        producer.attach_store(SemanticReviewStore::new(
            Arc::clone(&publication.document),
            false,
        ));
        producer
    }

    fn envelope(
        producer: &ReviewProducer,
        action: WorkdeckReviewActionV1,
        expected_state_revision: Option<u64>,
    ) -> WorkdeckReviewActionEnvelopeV1 {
        WorkdeckReviewActionEnvelopeV1 {
            protocol_version: WORKDECK_REVIEW_PROTOCOL_VERSION,
            generation: producer.get_publication().generation.clone(),
            expected_state_revision,
            actor: WorkdeckReviewActorV1 {
                client_id: "browser-1".into(),
                kind: WorkdeckReviewActorKindV1::Browser,
                display_name: None,
            },
            action,
        }
    }

    fn expect_applied(result: WorkdeckReviewActionResultV1) -> WorkdeckReviewActionAppliedV1 {
        match result {
            WorkdeckReviewActionResultV1::Applied(applied) => applied,
            WorkdeckReviewActionResultV1::Failed(failure) => panic!("action failed: {failure:?}"),
        }
    }

    fn expect_failure(result: WorkdeckReviewActionResultV1) -> WorkdeckReviewFailureV1 {
        match result {
            WorkdeckReviewActionResultV1::Failed(failure) => failure,
            WorkdeckReviewActionResultV1::Applied(applied) => panic!("action applied: {applied:?}"),
        }
    }

    #[test]
    fn plans_live_actions_and_enforces_generation_and_revision_position() {
        let producer = producer();
        let before = producer.get_publication_address().state_revision;
        let applied = expect_applied(apply_session_review_action(
            &producer,
            &envelope(
                &producer,
                WorkdeckReviewActionV1::FilterSet {
                    filter: "example".into(),
                },
                None,
            ),
        ));
        assert_eq!(applied.state_revision, before + 1);
        assert_eq!(producer.get_review_state().unwrap().filter, "example");

        let mut wrong_generation = envelope(
            &producer,
            WorkdeckReviewActionV1::FilterSet { filter: "x".into() },
            None,
        );
        wrong_generation.generation = "generation:test:9".into();
        assert_eq!(
            expect_failure(apply_session_review_action(&producer, &wrong_generation)).code,
            WorkdeckReviewFailureCodeV1::StaleGeneration
        );
        assert_eq!(
            expect_failure(apply_session_review_action(
                &producer,
                &envelope(
                    &producer,
                    WorkdeckReviewActionV1::FilterSet {
                        filter: "two".into()
                    },
                    Some(0),
                ),
            ))
            .code,
            WorkdeckReviewFailureCodeV1::StaleGeneration
        );
        let current = producer.get_publication_address().state_revision;
        expect_applied(apply_session_review_action(
            &producer,
            &envelope(
                &producer,
                WorkdeckReviewActionV1::FilterSet {
                    filter: "two".into(),
                },
                Some(current),
            ),
        ));
    }

    #[test]
    fn planner_rejections_keep_the_shared_code_and_missing_store_is_explicit() {
        let producer = producer();
        let result = apply_session_review_action(
            &producer,
            &envelope(
                &producer,
                WorkdeckReviewActionV1::ExpansionToggle {
                    file_key: "file:deadbeef".into(),
                    gap_id: "before:0".into(),
                },
                None,
            ),
        );
        assert_eq!(
            expect_failure(result).code,
            WorkdeckReviewFailureCodeV1::FileNotFound
        );

        let unattached = ReviewProducer::new(
            PublishReviewInput::default(),
            ReviewProducerOptions {
                producer_id: Some("headless".into()),
                ..ReviewProducerOptions::default()
            },
        )
        .unwrap();
        let result = apply_session_review_action(
            &unattached,
            &envelope(
                &unattached,
                WorkdeckReviewActionV1::FilterSet { filter: "x".into() },
                None,
            ),
        );
        assert_eq!(
            expect_failure(result).code,
            WorkdeckReviewFailureCodeV1::InvalidRequest
        );
    }

    #[test]
    fn expanded_line_proof_anchors_through_shared_gap_geometry() {
        let producer = producer();
        let file = producer.get_publication().document.files[0].clone();
        let gap_id = review_gap_id(ReviewGapPosition::Before, 1);
        let outcome = producer
            .apply_intent(
                SemanticReviewIntent::ToggleExpansion {
                    file_key: file.key.clone(),
                    gap_id: gap_id.clone(),
                },
                ReviewIntentFacts::default(),
            )
            .unwrap()
            .unwrap();
        let ReviewIntentOutcome::ExpansionToggled { new_range, .. } = outcome else {
            panic!("gap was not expanded")
        };
        let line = u64::from(new_range[0]);
        let proof = WorkdeckReviewExpandedLineProofV1 {
            gap_id: gap_id.clone(),
            side: ReviewSide::New,
            line,
            source_identity: file.source_identity.clone().unwrap(),
        };
        expect_applied(apply_session_review_action(
            &producer,
            &envelope(
                &producer,
                WorkdeckReviewActionV1::NotesStartDraft {
                    file_key: file.key.clone(),
                    hunk_index: 1,
                    target: Some(WorkdeckReviewLineAddressV1 {
                        side: ReviewSide::New,
                        line,
                    }),
                    reveal: None,
                    expanded_line_proof: Some(proof.clone()),
                },
                None,
            ),
        ));
        let draft = producer
            .get_review_state()
            .unwrap()
            .draft_note
            .clone()
            .unwrap();
        assert_eq!(
            (draft.side, draft.line, draft.hunk_index),
            (ReviewSide::New, new_range[0], 1)
        );
        expect_applied(apply_session_review_action(
            &producer,
            &envelope(
                &producer,
                WorkdeckReviewActionV1::NotesUpdateDraft {
                    body: "About this restored line".into(),
                },
                None,
            ),
        ));
        expect_applied(apply_session_review_action(
            &producer,
            &envelope(
                &producer,
                WorkdeckReviewActionV1::NotesCreateUser {
                    consume_draft: true,
                    target: Some(WorkdeckReviewLineAddressV1 {
                        side: ReviewSide::New,
                        line,
                    }),
                    expanded_line_proof: Some(proof),
                },
                None,
            ),
        ));
        let note = producer
            .get_review_state()
            .unwrap()
            .user_notes
            .last()
            .unwrap()
            .note
            .clone();
        assert_eq!(
            note.anchor.preferred,
            Some(SemanticReviewLineAddress {
                side: ReviewSide::New,
                line: new_range[0]
            })
        );
        assert!(note.anchor.intersecting_hunk_indices.is_empty());
        assert_eq!(note.anchor.owner_hunk_index, Some(1));
    }

    #[test]
    fn saved_notes_edit_in_place_and_reply_with_parent_identity() {
        let producer = producer();
        let file_key = producer.get_publication().document.files[0].key.clone();
        for action in [
            WorkdeckReviewActionV1::NotesStartDraft {
                file_key,
                hunk_index: 0,
                target: None,
                reveal: None,
                expanded_line_proof: None,
            },
            WorkdeckReviewActionV1::NotesUpdateDraft {
                body: "original".into(),
            },
            WorkdeckReviewActionV1::NotesCreateUser {
                consume_draft: true,
                target: None,
                expanded_line_proof: None,
            },
        ] {
            expect_applied(apply_session_review_action(
                &producer,
                &envelope(&producer, action, None),
            ));
        }
        let original = producer.get_review_state().unwrap().user_notes[0]
            .note
            .clone();
        for action in [
            WorkdeckReviewActionV1::NotesStartEdit {
                note_id: original.id.clone(),
                reveal: None,
            },
            WorkdeckReviewActionV1::NotesUpdateDraft {
                body: "edited".into(),
            },
            WorkdeckReviewActionV1::NotesUpdateUser {
                note_id: original.id.clone(),
                consume_draft: true,
            },
            WorkdeckReviewActionV1::NotesStartReply {
                note_id: original.id.clone(),
                reveal: None,
            },
            WorkdeckReviewActionV1::NotesUpdateDraft {
                body: "reply".into(),
            },
            WorkdeckReviewActionV1::NotesCreateUser {
                consume_draft: true,
                target: None,
                expanded_line_proof: None,
            },
        ] {
            expect_applied(apply_session_review_action(
                &producer,
                &envelope(&producer, action, None),
            ));
        }
        let state = producer.get_review_state().unwrap();
        assert_eq!(state.user_notes[0].note.id, original.id);
        assert_eq!(state.user_notes[0].note.summary, "edited");
        assert_eq!(state.user_notes[0].note.created_at, original.created_at);
        assert_eq!(
            state.user_notes[1].note.parent_id.as_deref(),
            Some(original.id.as_str())
        );
        assert_eq!(state.user_notes[1].note.summary, "reply");
    }

    #[test]
    fn malformed_or_retired_expanded_proofs_and_wrong_draft_target_are_refused() {
        let producer = producer();
        let file = producer.get_publication().document.files[0].clone();
        let gap_id = review_gap_id(ReviewGapPosition::Before, 1);
        let gap = review_gap_address(&semantic_review_gap_source(&file), &gap_id).unwrap();
        let target = WorkdeckReviewLineAddressV1 {
            side: ReviewSide::New,
            line: u64::from(gap.new_range.start),
        };
        let mut proof = WorkdeckReviewExpandedLineProofV1 {
            gap_id,
            side: ReviewSide::New,
            line: target.line,
            source_identity: file.source_identity.clone().unwrap(),
        };
        proof.line += 1;
        let mismatch = WorkdeckReviewActionV1::NotesStartDraft {
            file_key: file.key.clone(),
            hunk_index: 1,
            target: Some(target),
            reveal: None,
            expanded_line_proof: Some(proof.clone()),
        };
        assert_eq!(
            expect_failure(apply_session_review_action(
                &producer,
                &envelope(&producer, mismatch, None)
            ))
            .code,
            WorkdeckReviewFailureCodeV1::InvalidRequest
        );
        proof.line = 1;
        let retired = WorkdeckReviewActionV1::NotesStartDraft {
            file_key: file.key.clone(),
            hunk_index: 1,
            target: Some(WorkdeckReviewLineAddressV1 {
                side: ReviewSide::New,
                line: 1,
            }),
            reveal: None,
            expanded_line_proof: Some(proof),
        };
        assert_eq!(
            expect_failure(apply_session_review_action(
                &producer,
                &envelope(&producer, retired, None)
            ))
            .code,
            WorkdeckReviewFailureCodeV1::GapNotFound
        );

        producer
            .apply_intent(
                SemanticReviewIntent::StartDraft {
                    file_key: file.key,
                    hunk_index: 0,
                    target: None,
                    reveal: None,
                },
                ReviewIntentFacts {
                    draft_id: Some("draft:1".into()),
                    ..ReviewIntentFacts::default()
                },
            )
            .unwrap();
        let wrong = WorkdeckReviewActionV1::NotesCreateUser {
            consume_draft: true,
            target: Some(WorkdeckReviewLineAddressV1 {
                side: ReviewSide::New,
                line: 999,
            }),
            expanded_line_proof: None,
        };
        assert_eq!(
            expect_failure(apply_session_review_action(
                &producer,
                &envelope(&producer, wrong, None)
            ))
            .code,
            WorkdeckReviewFailureCodeV1::DraftMissing
        );
    }

    #[test]
    fn reads_verified_publication_resources_and_attaches_current_generation_on_failure() {
        let producer = producer();
        let publication = producer.get_publication();
        let file = &publication.document.files[0];
        let request = WorkdeckReviewResourceReadEnvelopeV1 {
            protocol_version: WORKDECK_REVIEW_PROTOCOL_VERSION,
            actor: WorkdeckReviewActorV1 {
                client_id: "browser-1".into(),
                kind: WorkdeckReviewActorKindV1::Browser,
                display_name: None,
            },
            request: workdeck_review::ReadReviewResourceRequest {
                generation: publication.generation.clone(),
                resource_id: review_resource_id(&ReviewResourceAddress {
                    kind: ReviewResourceKind::Patch,
                    file_key: file.key.clone(),
                    side: None,
                }),
                offset: 0,
                length: 1024,
            },
        };
        let WorkdeckReviewResourceReadResultV1::Chunk { chunk, .. } =
            read_session_review_resource(&producer, &request)
        else {
            panic!("published patch was not served")
        };
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(chunk.data)
                .unwrap(),
            file.patch.as_bytes()
        );

        let mut stale = request;
        stale.request.generation = "generation:test:9".into();
        let WorkdeckReviewResourceReadResultV1::Failed(failure) =
            read_session_review_resource(&producer, &stale)
        else {
            panic!("retired generation was served")
        };
        assert_eq!(failure.code, WorkdeckReviewFailureCodeV1::StaleGeneration);
        assert_eq!(failure.current_generation, publication.generation);
    }
}
