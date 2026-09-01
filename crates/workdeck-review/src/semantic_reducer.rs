//! Pure state transitions for the semantic review model.

use crate::{
    ReviewExpandedGapState, SemanticReviewAction, SemanticReviewState, apply_review_reveal_request,
    is_review_note_within_clear_scope, review_file_keys_with_retired_content,
    select_semantic_review_file_by_key,
};

/// Apply one action, returning `None` when the action is a semantic no-op.
#[must_use]
pub fn reduce_semantic_review_state(
    state: &SemanticReviewState,
    action: SemanticReviewAction,
) -> Option<SemanticReviewState> {
    match action {
        SemanticReviewAction::ReconcileDocument { document } => {
            if std::sync::Arc::ptr_eq(&document, &state.document) {
                return None;
            }
            let retired = review_file_keys_with_retired_content(&state.document, &document);
            let mut next = state.clone();
            next.expanded_gaps
                .retain(|gap| !retired.contains(&gap.file_key));
            next.source_status_by_file_key.retain(|file_key, _| {
                !retired.contains(file_key)
                    && document
                        .files
                        .iter()
                        .find(|file| file.key == file_key.as_str())
                        .is_some_and(|file| file.source_attested == Some(true))
            });
            next.document = document;
            Some(next)
        }
        SemanticReviewAction::Select {
            file_key,
            hunk_index,
            reveal,
        } => {
            let file = select_semantic_review_file_by_key(state, Some(&file_key))?;
            let hunk_index = usize::try_from(hunk_index.max(0))
                .unwrap_or(usize::MAX)
                .min(file.hunks.len().saturating_sub(1));
            let next_reveal = reveal.map_or(state.reveal, |request| {
                apply_review_reveal_request(state.reveal, request)
            });
            if state.selection.file_key.as_deref() == Some(file.key.as_str())
                && state.selection.hunk_index == hunk_index
                && state.reveal == next_reveal
            {
                return None;
            }
            let mut next = state.clone();
            next.selection.file_key = Some(file.key.clone());
            next.selection.hunk_index = hunk_index;
            next.reveal = next_reveal;
            Some(next)
        }
        SemanticReviewAction::SetFilter(filter) => {
            if filter == state.filter {
                return None;
            }
            let mut next = state.clone();
            next.filter = filter;
            Some(next)
        }
        SemanticReviewAction::SetNoteVisibility(visible) => {
            if visible == state.show_agent_notes {
                return None;
            }
            let mut next = state.clone();
            next.show_agent_notes = visible;
            Some(next)
        }
        SemanticReviewAction::AddLiveNotes(notes) => {
            if notes.is_empty() {
                return None;
            }
            let mut next = state.clone();
            next.live_notes.extend(notes);
            Some(next)
        }
        SemanticReviewAction::RemoveLiveNote(note_id) => {
            let index = state
                .live_notes
                .iter()
                .position(|entry| entry.note.id == note_id)?;
            let mut next = state.clone();
            next.live_notes.remove(index);
            Some(next)
        }
        SemanticReviewAction::ClearNotes {
            file_key,
            include_user,
        } => {
            let in_scope = |entry: &crate::ReviewStoredNote| {
                is_review_note_within_clear_scope(entry, file_key.as_deref())
            };
            let live_changed = state.live_notes.iter().any(in_scope);
            let user_changed = include_user && state.user_notes.iter().any(in_scope);
            if !live_changed && !user_changed {
                return None;
            }
            let mut next = state.clone();
            next.live_notes.retain(|entry| {
                file_key
                    .as_ref()
                    .is_some_and(|key| entry.note.file_key != *key)
            });
            if include_user {
                next.user_notes.retain(|entry| {
                    file_key
                        .as_ref()
                        .is_some_and(|key| entry.note.file_key != *key)
                });
            }
            Some(next)
        }
        SemanticReviewAction::RemoveUserNote(note_id) => {
            let index = state
                .user_notes
                .iter()
                .position(|entry| entry.note.id == note_id)?;
            let mut next = state.clone();
            next.user_notes.remove(index);
            Some(next)
        }
        SemanticReviewAction::StartDraft(draft) => {
            let mut next = state.clone();
            next.draft_note = Some(draft);
            Some(next)
        }
        SemanticReviewAction::UpdateDraft(body) => {
            let draft = state.draft_note.as_ref()?;
            if draft.body == body {
                return None;
            }
            let mut next = state.clone();
            next.draft_note.as_mut().expect("draft was checked").body = body;
            Some(next)
        }
        SemanticReviewAction::CancelDraft => {
            state.draft_note.as_ref()?;
            let mut next = state.clone();
            next.draft_note = None;
            Some(next)
        }
        SemanticReviewAction::SaveDraft(note) => {
            state.draft_note.as_ref()?;
            let mut next = state.clone();
            next.draft_note = None;
            next.user_notes.push(note);
            Some(next)
        }
        SemanticReviewAction::SaveDraftEdit(note) => {
            state.draft_note.as_ref()?;
            let index = state
                .user_notes
                .iter()
                .position(|entry| entry.note.id == note.note.id)?;
            let mut next = state.clone();
            next.draft_note = None;
            next.user_notes[index] = note;
            Some(next)
        }
        SemanticReviewAction::ToggleExpansion {
            file_key,
            gap_id,
            expanded,
        } => {
            let index = state
                .expanded_gaps
                .iter()
                .position(|gap| gap.file_key == file_key && gap.gap_id == gap_id);
            if index.is_some_and(|index| state.expanded_gaps[index].expanded == expanded) {
                return None;
            }
            let gap = ReviewExpandedGapState {
                file_key,
                gap_id,
                expanded,
            };
            let mut next = state.clone();
            if let Some(index) = index {
                next.expanded_gaps[index] = gap;
            } else {
                next.expanded_gaps.push(gap);
            }
            Some(next)
        }
        SemanticReviewAction::SetSourceStatus { file_key, status } => {
            if state.source_status_by_file_key.get(&file_key) == Some(&status) {
                return None;
            }
            let mut next = state.clone();
            next.source_status_by_file_key.insert(file_key, status);
            Some(next)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use workdeck_core::{ReviewNoteSource, ReviewSide};

    use crate::{
        ReviewDraftKind, ReviewDraftNote, ReviewNoteResolution, ReviewRevealAnchor,
        ReviewRevealRequest, ReviewSourceStatus, ReviewStoredNote,
        semantic_test_support::{document, document_with_sources, note},
    };

    fn reduce(state: &SemanticReviewState, action: SemanticReviewAction) -> SemanticReviewState {
        reduce_semantic_review_state(state, action).unwrap_or_else(|| state.clone())
    }

    fn stored(id: &str, file_key: &str, source: ReviewNoteSource) -> ReviewStoredNote {
        ReviewStoredNote {
            note: note(id, file_key, source),
            resolution: ReviewNoteResolution::Active,
        }
    }

    fn draft() -> ReviewDraftNote {
        ReviewDraftNote {
            id: "draft-1".into(),
            file_key: "alpha".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 4,
            body: String::new(),
            kind: ReviewDraftKind::Create,
        }
    }

    #[test]
    fn selection_clamps_and_repeated_reveals_advance() {
        let state = SemanticReviewState::new(document(&[("alpha", 2), ("empty", 0)]), false);
        let selected = reduce(
            &state,
            SemanticReviewAction::Select {
                file_key: "alpha".into(),
                hunk_index: 7,
                reveal: Some(ReviewRevealRequest {
                    anchor: ReviewRevealAnchor::Hunk,
                    scroll_to_note: false,
                }),
            },
        );
        assert_eq!(selected.selection.hunk_index, 1);
        assert_eq!(selected.reveal.hunk_token, 1);
        let repeated = reduce(
            &selected,
            SemanticReviewAction::Select {
                file_key: "alpha".into(),
                hunk_index: 1,
                reveal: Some(ReviewRevealRequest {
                    anchor: ReviewRevealAnchor::Hunk,
                    scroll_to_note: false,
                }),
            },
        );
        assert_eq!(repeated.reveal.hunk_token, 2);
        let empty = reduce(
            &repeated,
            SemanticReviewAction::Select {
                file_key: "empty".into(),
                hunk_index: 3,
                reveal: None,
            },
        );
        assert_eq!(empty.selection.hunk_index, 0);
        let below_zero = reduce(
            &selected,
            SemanticReviewAction::Select {
                file_key: "alpha".into(),
                hunk_index: -3,
                reveal: Some(ReviewRevealRequest {
                    anchor: ReviewRevealAnchor::FileTop,
                    scroll_to_note: false,
                }),
            },
        );
        assert_eq!(below_zero.selection.hunk_index, 0);
        assert_eq!(below_zero.reveal.file_top_token, 1);
        assert_eq!(below_zero.reveal.hunk_token, 1);
        assert!(
            reduce_semantic_review_state(
                &state,
                SemanticReviewAction::Select {
                    file_key: "missing".into(),
                    hunk_index: 0,
                    reveal: None,
                }
            )
            .is_none()
        );
    }

    #[test]
    fn viewport_preservation_can_retire_note_scroll_without_moving() {
        let state = SemanticReviewState::new(document(&[("alpha", 1)]), false);
        let note_selected = reduce(
            &state,
            SemanticReviewAction::Select {
                file_key: "alpha".into(),
                hunk_index: 0,
                reveal: Some(ReviewRevealRequest {
                    anchor: ReviewRevealAnchor::Hunk,
                    scroll_to_note: true,
                }),
            },
        );
        let anchored = reduce(
            &note_selected,
            SemanticReviewAction::Select {
                file_key: "alpha".into(),
                hunk_index: 0,
                reveal: Some(crate::REVIEW_VIEWPORT_ANCHOR_REVEAL),
            },
        );
        assert_eq!(anchored.reveal.hunk_token, note_selected.reveal.hunk_token);
        assert!(!anchored.reveal.scroll_to_note);
    }

    #[test]
    fn reconcile_retires_changed_content_and_unattested_source_cache() {
        let before = document_with_sources(&[
            ("alpha", Some("source-1"), true),
            ("beta", Some("source-1"), true),
            ("gamma", Some("same"), false),
        ]);
        let mut state = SemanticReviewState::new(before, false);
        for key in ["alpha", "beta", "gamma"] {
            state = reduce(
                &state,
                SemanticReviewAction::ToggleExpansion {
                    file_key: key.into(),
                    gap_id: "before:1".into(),
                    expanded: true,
                },
            );
            state = reduce(
                &state,
                SemanticReviewAction::SetSourceStatus {
                    file_key: key.into(),
                    status: ReviewSourceStatus::Loaded { text: key.into() },
                },
            );
        }
        state
            .live_notes
            .push(stored("live", "alpha", ReviewNoteSource::Agent));
        state.draft_note = Some(draft());
        let after = document_with_sources(&[
            ("alpha", Some("source-2"), true),
            ("beta", Some("source-1"), true),
            ("gamma", Some("same"), false),
        ]);
        let next = reduce(
            &state,
            SemanticReviewAction::ReconcileDocument { document: after },
        );
        assert_eq!(
            next.expanded_gaps
                .iter()
                .map(|gap| gap.file_key.as_str())
                .collect::<Vec<_>>(),
            ["beta", "gamma"]
        );
        assert_eq!(
            next.source_status_by_file_key
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["beta"]
        );
        assert_eq!(next.live_notes.len(), 1);
        assert_eq!(next.draft_note.as_ref().unwrap().body, "");

        let retired = reduce(
            &next,
            SemanticReviewAction::ReconcileDocument {
                document: document_with_sources(&[("beta", Some("source-1"), true)]),
            },
        );
        assert_eq!(retired.expanded_gaps.len(), 1);
        assert_eq!(retired.expanded_gaps[0].file_key, "beta");
    }

    #[test]
    fn same_document_arc_is_a_noop() {
        let document = document(&[("alpha", 1)]);
        let state = SemanticReviewState::new(Arc::clone(&document), false);
        assert!(
            reduce_semantic_review_state(
                &state,
                SemanticReviewAction::ReconcileDocument { document }
            )
            .is_none()
        );
    }

    #[test]
    fn notes_preserve_order_scope_and_user_policy() {
        let state = SemanticReviewState::new(document(&[("alpha", 1), ("beta", 1)]), false);
        let added = reduce(
            &state,
            SemanticReviewAction::AddLiveNotes(vec![
                stored("live-1", "alpha", ReviewNoteSource::Agent),
                stored("live-2", "beta", ReviewNoteSource::Agent),
            ]),
        );
        assert_eq!(
            added
                .live_notes
                .iter()
                .map(|entry| entry.note.id.as_str())
                .collect::<Vec<_>>(),
            ["live-1", "live-2"]
        );
        let removed = reduce(
            &added,
            SemanticReviewAction::RemoveLiveNote("live-1".into()),
        );
        assert_eq!(removed.live_notes[0].note.id, "live-2");
        assert!(
            reduce_semantic_review_state(
                &removed,
                SemanticReviewAction::RemoveLiveNote("unknown".into())
            )
            .is_none()
        );
        let with_user = reduce(
            &reduce(&added, SemanticReviewAction::StartDraft(draft())),
            SemanticReviewAction::SaveDraft(stored("user-1", "alpha", ReviewNoteSource::User)),
        );
        let cleared = reduce(
            &with_user,
            SemanticReviewAction::ClearNotes {
                file_key: Some("alpha".into()),
                include_user: false,
            },
        );
        assert_eq!(cleared.live_notes[0].note.id, "live-2");
        assert_eq!(cleared.user_notes[0].note.id, "user-1");
        let all = reduce(
            &cleared,
            SemanticReviewAction::ClearNotes {
                file_key: None,
                include_user: true,
            },
        );
        assert!(all.live_notes.is_empty());
        assert!(all.user_notes.is_empty());
    }

    #[test]
    fn drafts_update_cancel_save_and_replace_in_place() {
        let state = SemanticReviewState::new(document(&[("alpha", 1)]), false);
        assert!(
            reduce_semantic_review_state(
                &state,
                SemanticReviewAction::UpdateDraft("ignored".into())
            )
            .is_none()
        );
        assert!(reduce_semantic_review_state(&state, SemanticReviewAction::CancelDraft).is_none());
        let started = reduce(&state, SemanticReviewAction::StartDraft(draft()));
        let edited = reduce(&started, SemanticReviewAction::UpdateDraft("hello".into()));
        assert_eq!(edited.draft_note.as_ref().unwrap().body, "hello");
        assert!(
            reduce(&edited, SemanticReviewAction::CancelDraft)
                .draft_note
                .is_none()
        );

        let original = stored("user-1", "alpha", ReviewNoteSource::User);
        let second = stored("user-2", "alpha", ReviewNoteSource::User);
        let mut state = state;
        state.user_notes = vec![original.clone(), second];
        state.draft_note = Some(ReviewDraftNote {
            kind: ReviewDraftKind::Edit {
                target_note_id: "user-1".into(),
            },
            ..draft()
        });
        let mut replacement = original;
        replacement.note.summary = "updated".into();
        let saved = reduce(&state, SemanticReviewAction::SaveDraftEdit(replacement));
        assert!(saved.draft_note.is_none());
        assert_eq!(saved.user_notes[0].note.summary, "updated");
        assert_eq!(saved.user_notes[1].note.id, "user-2");
    }

    #[test]
    fn expansions_statuses_filter_and_visibility_preserve_noops() {
        let state = SemanticReviewState::new(document(&[("alpha", 1)]), false);
        assert!(
            reduce_semantic_review_state(&state, SemanticReviewAction::SetFilter(String::new()))
                .is_none()
        );
        assert!(
            reduce_semantic_review_state(&state, SemanticReviewAction::SetNoteVisibility(false))
                .is_none()
        );
        let filtered = reduce(&state, SemanticReviewAction::SetFilter("alpha".into()));
        assert_eq!(filtered.filter, "alpha");
        assert_eq!(filtered.selection, state.selection);
        let expanded = reduce(
            &filtered,
            SemanticReviewAction::ToggleExpansion {
                file_key: "alpha".into(),
                gap_id: "before:1".into(),
                expanded: true,
            },
        );
        assert!(
            reduce_semantic_review_state(
                &expanded,
                SemanticReviewAction::ToggleExpansion {
                    file_key: "alpha".into(),
                    gap_id: "before:1".into(),
                    expanded: true,
                }
            )
            .is_none()
        );
        let second_gap = reduce(
            &expanded,
            SemanticReviewAction::ToggleExpansion {
                file_key: "alpha".into(),
                gap_id: "before:2".into(),
                expanded: true,
            },
        );
        let collapsed = reduce(
            &second_gap,
            SemanticReviewAction::ToggleExpansion {
                file_key: "alpha".into(),
                gap_id: "before:1".into(),
                expanded: false,
            },
        );
        assert_eq!(collapsed.expanded_gaps.len(), 2);
        assert!(!collapsed.expanded_gaps[0].expanded);
        assert!(collapsed.expanded_gaps[1].expanded);
        let loaded = reduce(
            &collapsed,
            SemanticReviewAction::SetSourceStatus {
                file_key: "alpha".into(),
                status: ReviewSourceStatus::Loaded {
                    text: "source".into(),
                },
            },
        );
        assert!(
            reduce_semantic_review_state(
                &loaded,
                SemanticReviewAction::SetSourceStatus {
                    file_key: "alpha".into(),
                    status: ReviewSourceStatus::Loaded {
                        text: "source".into(),
                    },
                }
            )
            .is_none()
        );
        let changed = reduce(
            &loaded,
            SemanticReviewAction::SetSourceStatus {
                file_key: "alpha".into(),
                status: ReviewSourceStatus::Loaded {
                    text: "changed".into(),
                },
            },
        );
        assert_ne!(
            changed.source_status_by_file_key,
            loaded.source_status_by_file_key
        );
    }
}
