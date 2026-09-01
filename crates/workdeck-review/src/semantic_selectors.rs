//! Pure shared queries over authoritative semantic review state.

use std::collections::{BTreeMap, BTreeSet};

use workdeck_core::{
    ReviewNoteSource, ReviewSide, SemanticReviewDocument, SemanticReviewFile,
    SemanticReviewLineAddress, SemanticReviewNote,
};

use crate::{
    ReviewGapHunk, ReviewGapPosition, ReviewGapSource, ReviewNavigationFile, ReviewStoredNote,
    SemanticReviewSelection, SemanticReviewState, is_renderable_stored_review_note, review_gap_id,
    review_leading_gap, review_note_anchor_line, review_note_owner_hunk_index,
    review_note_visible_by_policy, review_trailing_gap,
};

#[must_use]
pub fn select_semantic_review_file_by_key<'a>(
    state: &'a SemanticReviewState,
    file_key: Option<&str>,
) -> Option<&'a SemanticReviewFile> {
    let file_key = file_key?;
    state
        .document
        .files
        .iter()
        .find(|file| file.key == file_key)
}

#[must_use]
pub fn review_note_current_owner_hunk_index(
    note: &SemanticReviewNote,
    file: &SemanticReviewFile,
) -> usize {
    review_note_owner_hunk_index(note).min(file.hunks.len().saturating_sub(1))
}

#[must_use]
pub fn is_review_note_within_clear_scope(entry: &ReviewStoredNote, file_key: Option<&str>) -> bool {
    file_key.is_none_or(|file_key| entry.note.file_key == file_key)
}

#[must_use]
pub fn review_file_keys_with_retired_content(
    previous: &SemanticReviewDocument,
    next: &SemanticReviewDocument,
) -> BTreeSet<String> {
    let next_by_key = next
        .files
        .iter()
        .map(|file| (file.key.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    previous
        .files
        .iter()
        .filter(|file| {
            next_by_key
                .get(file.key.as_str())
                .is_none_or(|replacement| replacement.source_identity != file.source_identity)
        })
        .map(|file| file.key.clone())
        .collect()
}

fn normalized_diff_path(path: &str) -> &str {
    path.trim_end_matches(['\r', '\n'])
}

#[must_use]
pub fn review_file_matches_filter(file: &SemanticReviewFile, filter: &str) -> bool {
    let query = filter.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let mut fields = Vec::new();
    let path = normalized_diff_path(&file.path);
    if !path.is_empty() {
        fields.push(path);
    }
    if let Some(previous_path) = file
        .previous_path
        .as_deref()
        .map(normalized_diff_path)
        .filter(|path| !path.is_empty())
    {
        fields.push(previous_path);
    }
    if let Some(summary) = file
        .agent_summary
        .as_deref()
        .filter(|summary| !summary.is_empty())
    {
        fields.push(summary);
    }
    fields.join(" ").to_lowercase().contains(&query)
}

#[must_use]
pub fn select_visible_review_files(state: &SemanticReviewState) -> Vec<&SemanticReviewFile> {
    state
        .document
        .files
        .iter()
        .filter(|file| review_file_matches_filter(file, &state.filter))
        .collect()
}

#[must_use]
pub fn select_semantic_review_navigation_files(
    state: &SemanticReviewState,
) -> Vec<ReviewNavigationFile> {
    select_visible_review_files(state)
        .into_iter()
        .map(|file| ReviewNavigationFile {
            file_key: file.key.clone(),
            hunk_count: file.hunks.len(),
        })
        .collect()
}

#[must_use]
pub fn select_fallback_file_key(state: &SemanticReviewState) -> Option<String> {
    select_visible_review_files(state)
        .first()
        .map(|file| file.key.clone())
}

#[must_use]
pub fn select_normalized_semantic_selection(
    state: &SemanticReviewState,
) -> SemanticReviewSelection {
    let Some(file) = select_semantic_review_file_by_key(state, state.selection.file_key.as_deref())
    else {
        return SemanticReviewSelection {
            file_key: select_fallback_file_key(state),
            hunk_index: 0,
        };
    };
    SemanticReviewSelection {
        file_key: Some(file.key.clone()),
        hunk_index: state
            .selection
            .hunk_index
            .min(file.hunks.len().saturating_sub(1)),
    }
}

#[must_use]
pub fn select_semantic_reveal_target(
    state: &SemanticReviewState,
) -> Option<SemanticReviewLineAddress> {
    let file = select_semantic_review_file_by_key(state, state.selection.file_key.as_deref())?;
    let hunk = file.hunks.get(state.selection.hunk_index)?;
    if hunk.addition_count > 0 {
        Some(SemanticReviewLineAddress {
            side: ReviewSide::New,
            line: hunk.addition_start,
        })
    } else if hunk.deletion_count > 0 {
        Some(SemanticReviewLineAddress {
            side: ReviewSide::Old,
            line: hunk.deletion_start,
        })
    } else {
        None
    }
}

#[must_use]
pub fn select_stored_review_notes(state: &SemanticReviewState) -> Vec<ReviewStoredNote> {
    state
        .live_notes
        .iter()
        .chain(&state.user_notes)
        .cloned()
        .collect()
}

#[must_use]
pub fn select_stored_review_note_by_id<'a>(
    state: &'a SemanticReviewState,
    note_id: &str,
) -> Option<&'a ReviewStoredNote> {
    state
        .live_notes
        .iter()
        .chain(&state.user_notes)
        .find(|entry| entry.note.id == note_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThreadedStoredNote {
    pub entry: ReviewStoredNote,
    pub root_id: String,
    pub depth: usize,
    pub parent_id: Option<String>,
}

fn append_note_tree(
    root: ReviewStoredNote,
    root_id: &str,
    children: &BTreeMap<String, Vec<ReviewStoredNote>>,
    visited: &mut BTreeSet<String>,
    result: &mut Vec<ReviewThreadedStoredNote>,
) {
    let mut pending = vec![(root, 0_usize)];
    while let Some((entry, depth)) = pending.pop() {
        if !visited.insert(entry.note.id.clone()) {
            continue;
        }
        result.push(ReviewThreadedStoredNote {
            parent_id: entry.note.parent_id.clone(),
            entry: entry.clone(),
            root_id: root_id.to_owned(),
            depth,
        });
        if let Some(descendants) = children.get(&entry.note.id) {
            pending.extend(
                descendants
                    .iter()
                    .rev()
                    .cloned()
                    .map(|descendant| (descendant, depth.saturating_add(1))),
            );
        }
    }
}

#[must_use]
pub fn select_threaded_stored_review_notes(
    state: &SemanticReviewState,
) -> Vec<ReviewThreadedStoredNote> {
    let entries = select_stored_review_notes(state);
    let by_id = entries
        .iter()
        .map(|entry| entry.note.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut children = BTreeMap::<String, Vec<ReviewStoredNote>>::new();
    let mut roots = Vec::new();
    for entry in &entries {
        match entry.note.parent_id.as_deref() {
            Some(parent_id) if parent_id != entry.note.id && by_id.contains(parent_id) => children
                .entry(parent_id.to_owned())
                .or_default()
                .push(entry.clone()),
            _ => roots.push(entry.clone()),
        }
    }
    let mut result = Vec::new();
    let mut visited = BTreeSet::new();
    for root in roots {
        let root_id = root.note.id.clone();
        append_note_tree(root, &root_id, &children, &mut visited, &mut result);
    }
    for entry in entries {
        if !visited.contains(&entry.note.id) {
            let root_id = entry.note.id.clone();
            append_note_tree(entry, &root_id, &children, &mut visited, &mut result);
        }
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewVisibleThreadedStoredNote {
    pub threaded: ReviewThreadedStoredNote,
    pub visible_depth: usize,
    pub visible_parent_id: Option<String>,
    pub has_next_visible_sibling: bool,
    pub visible_ancestor_has_next_sibling: Vec<bool>,
}

#[must_use]
pub fn select_visible_threaded_stored_review_notes(
    state: &SemanticReviewState,
) -> Vec<ReviewVisibleThreadedStoredNote> {
    let threaded = select_threaded_stored_review_notes(state);
    let mut nearest_visible_depth = BTreeMap::<String, usize>::new();
    let mut nearest_visible_id = BTreeMap::<String, String>::new();
    let mut visible = Vec::<(ReviewThreadedStoredNote, usize, Option<String>)>::new();
    for item in threaded {
        let parent_depth = item
            .entry
            .note
            .parent_id
            .as_ref()
            .and_then(|parent| nearest_visible_depth.get(parent).copied());
        let visible_parent_id = item
            .entry
            .note
            .parent_id
            .as_ref()
            .and_then(|parent| nearest_visible_id.get(parent).cloned());
        if is_renderable_stored_review_note(&item.entry)
            && review_note_visible_by_policy(&item.entry.note, state.show_agent_notes)
        {
            let visible_depth = parent_depth.map_or(0, |depth| depth.saturating_add(1));
            nearest_visible_depth.insert(item.entry.note.id.clone(), visible_depth);
            nearest_visible_id.insert(item.entry.note.id.clone(), item.entry.note.id.clone());
            visible.push((item, visible_depth, visible_parent_id));
        } else if let (Some(parent_depth), Some(parent_id)) = (parent_depth, visible_parent_id) {
            nearest_visible_depth.insert(item.entry.note.id.clone(), parent_depth);
            nearest_visible_id.insert(item.entry.note.id.clone(), parent_id);
        }
    }
    let has_next_visible_sibling = visible
        .iter()
        .enumerate()
        .map(|(index, (_, depth, parent_id))| {
            parent_id.is_some()
                && visible[index + 1..]
                    .iter()
                    .any(|(_, candidate_depth, candidate_parent)| {
                        candidate_depth >= depth && candidate_parent == parent_id
                    })
        })
        .collect::<Vec<_>>();
    let mut decorated = BTreeMap::<String, ReviewVisibleThreadedStoredNote>::new();
    let mut result = Vec::with_capacity(visible.len());
    for (index, (threaded, visible_depth, visible_parent_id)) in visible.into_iter().enumerate() {
        let mut ancestors = visible_parent_id
            .as_ref()
            .and_then(|parent| decorated.get(parent))
            .map(|parent| {
                let mut ancestors = parent.visible_ancestor_has_next_sibling.clone();
                ancestors.push(parent.has_next_visible_sibling);
                ancestors
            })
            .unwrap_or_default();
        ancestors.truncate(visible_depth);
        let note = ReviewVisibleThreadedStoredNote {
            threaded,
            visible_depth,
            visible_parent_id,
            has_next_visible_sibling: has_next_visible_sibling[index],
            visible_ancestor_has_next_sibling: ancestors,
        };
        decorated.insert(note.threaded.entry.note.id.clone(), note.clone());
        result.push(note);
    }
    result
}

#[must_use]
pub fn review_note_has_descendants(state: &SemanticReviewState, note_id: &str) -> bool {
    let mut children = BTreeMap::<String, Vec<String>>::new();
    for entry in select_stored_review_notes(state) {
        if let Some(parent_id) = entry.note.parent_id {
            children.entry(parent_id).or_default().push(entry.note.id);
        }
    }
    let mut pending = children.get(note_id).cloned().unwrap_or_default();
    let mut visited = BTreeSet::new();
    while let Some(child_id) = pending.pop() {
        if !visited.insert(child_id.clone()) {
            continue;
        }
        if child_id != note_id {
            return true;
        }
        if let Some(descendants) = children.get(&child_id) {
            pending.extend(descendants.iter().cloned());
        }
    }
    false
}

fn renderable_notes(state: &SemanticReviewState) -> Vec<SemanticReviewNote> {
    select_threaded_stored_review_notes(state)
        .into_iter()
        .filter(|item| is_renderable_stored_review_note(&item.entry))
        .map(|item| item.entry.note)
        .collect()
}

#[must_use]
pub fn select_notes_by_hunk(
    state: &SemanticReviewState,
    file_key: &str,
) -> BTreeMap<usize, Vec<SemanticReviewNote>> {
    let mut by_hunk = BTreeMap::<usize, Vec<SemanticReviewNote>>::new();
    for note in renderable_notes(state) {
        if note.file_key == file_key {
            by_hunk
                .entry(review_note_owner_hunk_index(&note))
                .or_default()
                .push(note);
        }
    }
    by_hunk
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRevealNoteCandidate {
    pub id: String,
    pub line: u32,
    pub draft: bool,
}

#[must_use]
pub fn resolve_review_reveal_note_id(candidates: &[ReviewRevealNoteCandidate]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| candidate.draft)
        .or_else(|| {
            candidates
                .iter()
                .enumerate()
                .min_by_key(|(arrival, candidate)| (candidate.line, *arrival))
                .map(|(_, candidate)| candidate)
        })
        .map(|candidate| candidate.id.clone())
}

fn select_active_stored_note_id(
    state: &SemanticReviewState,
    accepts: impl Fn(&ReviewStoredNote) -> bool,
) -> Option<String> {
    let file = select_semantic_review_file_by_key(state, state.selection.file_key.as_deref())?;
    select_visible_threaded_stored_review_notes(state)
        .into_iter()
        .map(|item| item.threaded.entry)
        .filter(|entry| {
            is_renderable_stored_review_note(entry)
                && state.selection.file_key.as_deref() == Some(entry.note.file_key.as_str())
                && review_note_current_owner_hunk_index(&entry.note, file)
                    == state.selection.hunk_index
                && accepts(entry)
        })
        .min_by_key(|entry| review_note_anchor_line(&entry.note).line)
        .map(|entry| entry.note.id)
}

#[must_use]
pub fn select_active_editable_review_note_id(state: &SemanticReviewState) -> Option<String> {
    select_active_stored_note_id(state, |entry| {
        entry.note.source == ReviewNoteSource::User && entry.note.editable
    })
}

#[must_use]
pub fn select_active_replyable_review_note_id(state: &SemanticReviewState) -> Option<String> {
    select_active_stored_note_id(state, |_| true)
}

#[must_use]
pub fn select_active_reveal_note_id(state: &SemanticReviewState) -> Option<String> {
    let file_key = state.selection.file_key.as_deref()?;
    let mut candidates = Vec::new();
    if let Some(draft) = state.draft_note.as_ref().filter(|draft| {
        draft.file_key == file_key && draft.hunk_index == state.selection.hunk_index
    }) {
        candidates.push(ReviewRevealNoteCandidate {
            id: draft.id.clone(),
            line: draft.line,
            draft: true,
        });
    }
    candidates.extend(
        renderable_notes(state)
            .into_iter()
            .filter(|note| {
                note.file_key == file_key
                    && review_note_owner_hunk_index(note) == state.selection.hunk_index
            })
            .map(|note| {
                let line = review_note_anchor_line(&note).line;
                ReviewRevealNoteCandidate {
                    id: note.id,
                    line,
                    draft: false,
                }
            }),
    );
    resolve_review_reveal_note_id(&candidates)
}

#[must_use]
pub fn is_review_gap_expanded(state: &SemanticReviewState, file_key: &str, gap_id: &str) -> bool {
    state
        .expanded_gaps
        .iter()
        .any(|gap| gap.file_key == file_key && gap.gap_id == gap_id && gap.expanded)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewGapTarget {
    pub file_key: String,
    pub gap_id: String,
}

pub fn semantic_review_gap_source(file: &SemanticReviewFile) -> ReviewGapSource {
    ReviewGapSource {
        hunks: file
            .hunks
            .iter()
            .map(|hunk| ReviewGapHunk {
                collapsed_before: hunk.collapsed_before,
                addition_start: hunk.addition_start,
                addition_count: hunk.addition_count,
                deletion_start: hunk.deletion_start,
                deletion_count: hunk.deletion_count,
                addition_line_index: hunk.addition_line_index,
                deletion_line_index: hunk.deletion_line_index,
            })
            .collect(),
        addition_lines: file.addition_lines.clone(),
        deletion_lines: file.deletion_lines.clone(),
        is_partial: file.flags.partial,
    }
}

#[must_use]
pub fn select_review_gap_for_selection(state: &SemanticReviewState) -> Option<ReviewGapTarget> {
    let selection = select_normalized_semantic_selection(state);
    let file = select_semantic_review_file_by_key(state, selection.file_key.as_deref())?;
    file.source_identity.as_ref()?;
    if file.hunks.is_empty() {
        return None;
    }
    let source = semantic_review_gap_source(file);
    for hunk_index in selection.hunk_index..file.hunks.len() {
        if review_leading_gap(&source, hunk_index).is_some() {
            return Some(ReviewGapTarget {
                file_key: file.key.clone(),
                gap_id: review_gap_id(ReviewGapPosition::Before, hunk_index),
            });
        }
    }
    let trailing = review_trailing_gap(&source)?;
    Some(ReviewGapTarget {
        file_key: file.key.clone(),
        gap_id: review_gap_id(ReviewGapPosition::Trailing, trailing.hunk_index),
    })
}

#[must_use]
pub fn select_expanded_gap_ids_by_file_key(
    state: &SemanticReviewState,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut result = BTreeMap::<String, BTreeSet<String>>::new();
    for gap in &state.expanded_gaps {
        let gaps = result.entry(gap.file_key.clone()).or_default();
        if gap.expanded {
            gaps.insert(gap.gap_id.clone());
        } else {
            gaps.remove(&gap.gap_id);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use workdeck_core::{ReviewNoteSource, ReviewSide};

    use crate::{
        ReviewDraftKind, ReviewDraftNote, ReviewExpandedGapState, ReviewNoteResolution,
        SemanticReviewState,
        semantic_test_support::{document, document_with_sources, note},
    };

    fn state(files: &[(&str, usize)]) -> SemanticReviewState {
        SemanticReviewState::new(document(files), false)
    }

    fn stored(
        id: &str,
        file_key: &str,
        source: ReviewNoteSource,
        resolution: ReviewNoteResolution,
        parent_id: Option<&str>,
        hunk_index: usize,
        line: u32,
    ) -> ReviewStoredNote {
        let mut note = note(id, file_key, source);
        note.parent_id = parent_id.map(str::to_owned);
        note.anchor.owner_hunk_index = Some(hunk_index);
        note.anchor.preferred = Some(SemanticReviewLineAddress {
            side: ReviewSide::New,
            line,
        });
        ReviewStoredNote { note, resolution }
    }

    fn draft(hunk_index: usize) -> ReviewDraftNote {
        ReviewDraftNote {
            id: "draft:1".into(),
            file_key: "alpha".into(),
            hunk_index,
            side: ReviewSide::New,
            line: 3,
            body: String::new(),
            kind: ReviewDraftKind::Create,
        }
    }

    #[test]
    fn resolves_files_and_content_retirement_by_semantic_identity() {
        let state = state(&[("alpha", 1), ("beta", 1)]);
        assert_eq!(
            select_semantic_review_file_by_key(&state, Some("beta")).map(|file| file.key.as_str()),
            Some("beta")
        );
        assert!(select_semantic_review_file_by_key(&state, Some("missing")).is_none());
        assert!(select_semantic_review_file_by_key(&state, None).is_none());

        let previous = document_with_sources(&[
            ("alpha", Some("source-1"), true),
            ("beta", Some("source-1"), true),
            ("gamma", None, false),
        ]);
        let next = document_with_sources(&[
            ("alpha", Some("source-1"), true),
            ("beta", Some("source-2"), true),
            ("delta", None, false),
        ]);
        assert_eq!(
            review_file_keys_with_retired_content(&previous, &next),
            ["beta".into(), "gamma".into()].into()
        );
        assert!(review_file_keys_with_retired_content(&previous, &previous).is_empty());
    }

    #[test]
    fn expansion_queries_keep_last_value_and_file_entries() {
        let mut state = state(&[("alpha", 1), ("beta", 1)]);
        state.expanded_gaps = vec![
            ReviewExpandedGapState {
                file_key: "alpha".into(),
                gap_id: "before:1".into(),
                expanded: true,
            },
            ReviewExpandedGapState {
                file_key: "alpha".into(),
                gap_id: "before:2".into(),
                expanded: false,
            },
            ReviewExpandedGapState {
                file_key: "beta".into(),
                gap_id: "trailing:0".into(),
                expanded: true,
            },
        ];
        assert!(is_review_gap_expanded(&state, "alpha", "before:1"));
        assert!(!is_review_gap_expanded(&state, "alpha", "before:2"));
        assert!(!is_review_gap_expanded(&state, "gamma", "before:1"));
        assert_eq!(
            select_expanded_gap_ids_by_file_key(&state),
            BTreeMap::from([
                ("alpha".into(), BTreeSet::from(["before:1".into()])),
                ("beta".into(), BTreeSet::from(["trailing:0".into()])),
            ])
        );
    }

    #[test]
    fn filter_matches_normalized_paths_previous_path_summary_and_cross_field_text() {
        let mut state = state(&[("alpha", 1), ("beta", 2)]);
        let alpha = &mut Arc::make_mut(&mut state.document).files[0];
        alpha.path = "src/review/stream.ts\r\n".into();
        alpha.previous_path = Some("src/legacy/stream.ts".into());
        alpha.agent_summary = Some("Rewrites the note placement policy".into());
        for query in [
            "",
            "   ",
            "REVIEW/stream",
            "legacy",
            "placement policy",
            "stream.ts src/legacy",
        ] {
            assert!(review_file_matches_filter(alpha, query), "query {query:?}");
        }
        assert!(!review_file_matches_filter(alpha, "unrelated"));
        state.filter = "beta".into();
        assert_eq!(
            select_visible_review_files(&state)
                .iter()
                .map(|file| file.key.as_str())
                .collect::<Vec<_>>(),
            ["beta"]
        );
        assert_eq!(
            select_semantic_review_navigation_files(&state),
            [ReviewNavigationFile {
                file_key: "beta".into(),
                hunk_count: 2
            }]
        );
    }

    #[test]
    fn normalized_selection_preserves_hidden_files_falls_back_and_clamps() {
        let mut state = state(&[("alpha", 2), ("beta", 2)]);
        state.filter = "beta".into();
        state.selection = SemanticReviewSelection {
            file_key: Some("alpha".into()),
            hunk_index: 1,
        };
        assert_eq!(
            select_normalized_semantic_selection(&state),
            state.selection
        );
        state.selection = SemanticReviewSelection {
            file_key: Some("vanished".into()),
            hunk_index: 3,
        };
        assert_eq!(select_fallback_file_key(&state).as_deref(), Some("beta"));
        assert_eq!(
            select_normalized_semantic_selection(&state),
            SemanticReviewSelection {
                file_key: Some("beta".into()),
                hunk_index: 0
            }
        );
        state.filter = "nothing-matches".into();
        assert_eq!(
            select_normalized_semantic_selection(&state),
            SemanticReviewSelection {
                file_key: None,
                hunk_index: 0
            }
        );
        state.filter.clear();
        state.selection = SemanticReviewSelection {
            file_key: Some("alpha".into()),
            hunk_index: 9,
        };
        assert_eq!(select_normalized_semantic_selection(&state).hunk_index, 1);
    }

    #[test]
    fn reveal_target_uses_a_side_backed_by_rows() {
        let mut state = state(&[("alpha", 2)]);
        assert_eq!(select_semantic_reveal_target(&state).unwrap().line, 1);
        state.selection.hunk_index = 1;
        assert_eq!(select_semantic_reveal_target(&state).unwrap().line, 11);
        let file = &mut Arc::make_mut(&mut state.document).files[0];
        file.hunks[1].addition_count = 0;
        file.hunks[1].deletion_start = 12;
        assert_eq!(
            select_semantic_reveal_target(&state),
            Some(SemanticReviewLineAddress {
                side: ReviewSide::Old,
                line: 12
            })
        );
        state.selection.file_key = None;
        assert!(select_semantic_reveal_target(&state).is_none());
    }

    #[test]
    fn stored_notes_preserve_collection_order_resolution_and_exclude_drafts() {
        let mut state = state(&[("alpha", 1)]);
        state.live_notes = vec![
            stored(
                "live-active",
                "alpha",
                ReviewNoteSource::Agent,
                ReviewNoteResolution::Active,
                None,
                0,
                1,
            ),
            stored(
                "live-orphaned",
                "gone",
                ReviewNoteSource::Agent,
                ReviewNoteResolution::Orphaned,
                None,
                0,
                1,
            ),
        ];
        state.user_notes = vec![stored(
            "user-stale",
            "alpha",
            ReviewNoteSource::User,
            ReviewNoteResolution::Stale,
            None,
            0,
            1,
        )];
        state.draft_note = Some(draft(0));
        assert_eq!(
            select_stored_review_notes(&state)
                .iter()
                .map(|entry| (entry.note.id.as_str(), entry.resolution))
                .collect::<Vec<_>>(),
            [
                ("live-active", ReviewNoteResolution::Active),
                ("live-orphaned", ReviewNoteResolution::Orphaned),
                ("user-stale", ReviewNoteResolution::Stale),
            ]
        );
        assert_eq!(
            select_stored_review_note_by_id(&state, "user-stale")
                .unwrap()
                .note
                .id,
            "user-stale"
        );
    }

    #[test]
    fn reply_trees_are_depth_first_and_visible_connectors_collapse_hidden_parents() {
        let mut state = state(&[("alpha", 1)]);
        state.show_agent_notes = true;
        state.live_notes = vec![stored(
            "root",
            "alpha",
            ReviewNoteSource::Agent,
            ReviewNoteResolution::Active,
            None,
            0,
            1,
        )];
        state.user_notes = vec![
            stored(
                "child-a",
                "alpha",
                ReviewNoteSource::User,
                ReviewNoteResolution::Active,
                Some("root"),
                0,
                2,
            ),
            stored(
                "child-b",
                "alpha",
                ReviewNoteSource::User,
                ReviewNoteResolution::Active,
                Some("root"),
                0,
                3,
            ),
            stored(
                "grandchild",
                "alpha",
                ReviewNoteSource::User,
                ReviewNoteResolution::Active,
                Some("child-a"),
                0,
                4,
            ),
        ];
        assert_eq!(
            select_threaded_stored_review_notes(&state)
                .iter()
                .map(|item| (
                    item.entry.note.id.as_str(),
                    item.root_id.as_str(),
                    item.depth
                ))
                .collect::<Vec<_>>(),
            [
                ("root", "root", 0),
                ("child-a", "root", 1),
                ("grandchild", "root", 2),
                ("child-b", "root", 1),
            ]
        );
        let visible = select_visible_threaded_stored_review_notes(&state);
        assert_eq!(
            visible
                .iter()
                .map(|item| (
                    item.threaded.entry.note.id.as_str(),
                    item.visible_ancestor_has_next_sibling.clone(),
                    item.has_next_visible_sibling,
                ))
                .collect::<Vec<_>>(),
            [
                ("root", vec![], false),
                ("child-a", vec![false], true),
                ("grandchild", vec![false, true], false),
                ("child-b", vec![false], false),
            ]
        );
        assert!(review_note_has_descendants(&state, "root"));
        assert!(!review_note_has_descendants(&state, "grandchild"));

        state.show_agent_notes = false;
        state.user_notes = vec![stored(
            "user-reply",
            "alpha",
            ReviewNoteSource::User,
            ReviewNoteResolution::Active,
            Some("root"),
            0,
            2,
        )];
        let visible = select_visible_threaded_stored_review_notes(&state);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].threaded.entry.note.id, "user-reply");
        assert_eq!(visible[0].visible_depth, 0);
        assert_eq!(
            select_active_replyable_review_note_id(&state).as_deref(),
            Some("user-reply")
        );
    }

    #[test]
    fn malformed_missing_parent_and_cycles_are_retained_once() {
        let mut state = state(&[("alpha", 1)]);
        state.user_notes = vec![
            stored(
                "missing-parent",
                "alpha",
                ReviewNoteSource::User,
                ReviewNoteResolution::Active,
                Some("gone"),
                0,
                1,
            ),
            stored(
                "cycle-a",
                "alpha",
                ReviewNoteSource::User,
                ReviewNoteResolution::Active,
                Some("cycle-b"),
                0,
                1,
            ),
            stored(
                "cycle-b",
                "alpha",
                ReviewNoteSource::User,
                ReviewNoteResolution::Active,
                Some("cycle-a"),
                0,
                1,
            ),
        ];
        assert_eq!(
            select_threaded_stored_review_notes(&state)
                .iter()
                .map(|item| item.entry.note.id.as_str())
                .collect::<Vec<_>>(),
            ["missing-parent", "cycle-a", "cycle-b"]
        );
    }

    #[test]
    fn note_grouping_uses_owner_not_range_and_hides_only_orphans() {
        let mut state = state(&[("alpha", 2), ("beta", 1)]);
        state.live_notes = vec![
            stored(
                "live-1",
                "alpha",
                ReviewNoteSource::Agent,
                ReviewNoteResolution::Active,
                None,
                1,
                11,
            ),
            stored(
                "live-2",
                "beta",
                ReviewNoteSource::Agent,
                ReviewNoteResolution::Active,
                None,
                0,
                1,
            ),
            stored(
                "orphaned",
                "alpha",
                ReviewNoteSource::Agent,
                ReviewNoteResolution::Orphaned,
                None,
                0,
                1,
            ),
        ];
        state.user_notes = vec![stored(
            "user-1",
            "alpha",
            ReviewNoteSource::User,
            ReviewNoteResolution::Active,
            None,
            1,
            12,
        )];
        let grouped = select_notes_by_hunk(&state, "alpha");
        assert_eq!(grouped.keys().copied().collect::<Vec<_>>(), [1]);
        assert_eq!(
            grouped[&1]
                .iter()
                .map(|note| note.id.as_str())
                .collect::<Vec<_>>(),
            ["live-1", "user-1"]
        );
    }

    #[test]
    fn reveal_prefers_draft_then_earliest_note_with_arrival_ties() {
        let mut state = state(&[("alpha", 2)]);
        state.live_notes = vec![
            stored(
                "live-late",
                "alpha",
                ReviewNoteSource::Agent,
                ReviewNoteResolution::Active,
                None,
                0,
                3,
            ),
            stored(
                "live-early",
                "alpha",
                ReviewNoteSource::Agent,
                ReviewNoteResolution::Active,
                None,
                0,
                1,
            ),
        ];
        state.user_notes = vec![stored(
            "user-late",
            "alpha",
            ReviewNoteSource::User,
            ReviewNoteResolution::Active,
            None,
            0,
            2,
        )];
        state.draft_note = Some(draft(0));
        assert_eq!(
            select_active_reveal_note_id(&state).as_deref(),
            Some("draft:1")
        );
        state.draft_note = Some(draft(1));
        assert_eq!(
            select_active_reveal_note_id(&state).as_deref(),
            Some("live-early")
        );
        assert_eq!(
            resolve_review_reveal_note_id(&[
                ReviewRevealNoteCandidate {
                    id: "late".into(),
                    line: 9,
                    draft: false,
                },
                ReviewRevealNoteCandidate {
                    id: "draft".into(),
                    line: 40,
                    draft: true,
                },
                ReviewRevealNoteCandidate {
                    id: "early".into(),
                    line: 1,
                    draft: false,
                },
            ])
            .as_deref(),
            Some("draft")
        );
        assert_eq!(
            resolve_review_reveal_note_id(&[
                ReviewRevealNoteCandidate {
                    id: "first".into(),
                    line: 4,
                    draft: false,
                },
                ReviewRevealNoteCandidate {
                    id: "second".into(),
                    line: 4,
                    draft: false,
                },
            ])
            .as_deref(),
            Some("first")
        );
        assert!(resolve_review_reveal_note_id(&[]).is_none());
        state.live_notes.clear();
        state.user_notes.clear();
        state.draft_note = None;
        assert!(select_active_reveal_note_id(&state).is_none());
    }

    #[test]
    fn active_editable_note_requires_visible_user_capability_and_clamps_owner() {
        let mut state = state(&[("alpha", 1)]);
        let mut user = stored(
            "user",
            "alpha",
            ReviewNoteSource::User,
            ReviewNoteResolution::Active,
            None,
            9,
            2,
        );
        user.note.editable = true;
        state.user_notes.push(user);
        assert_eq!(
            select_active_editable_review_note_id(&state).as_deref(),
            Some("user")
        );
    }

    #[test]
    fn gap_selection_prefers_current_then_later_then_trailing_and_requires_source() {
        let mut state = SemanticReviewState::new(
            document_with_sources(&[("alpha", Some("source:alpha"), true)]),
            false,
        );
        let file = &mut Arc::make_mut(&mut state.document).files[0];
        file.hunks.push(file.hunks[0].clone());
        file.hunks[1].index = 1;
        file.hunks[1].addition_start = 11;
        file.hunks[1].deletion_start = 11;
        file.hunks[1].addition_line_index = 10;
        file.hunks[1].deletion_line_index = 10;
        file.hunks[1].collapsed_before = 9;
        file.addition_lines = (1..=11).map(|line| format!("new {line}")).collect();
        file.deletion_lines = (1..=11).map(|line| format!("old {line}")).collect();
        assert_eq!(
            select_review_gap_for_selection(&state),
            Some(ReviewGapTarget {
                file_key: "alpha".into(),
                gap_id: "before:1".into()
            })
        );
        state.selection.hunk_index = 1;
        assert_eq!(
            select_review_gap_for_selection(&state).unwrap().gap_id,
            "before:1"
        );

        let file = &mut Arc::make_mut(&mut state.document).files[0];
        file.hunks[1].collapsed_before = 0;
        file.addition_lines.push("tail".into());
        file.deletion_lines.push("tail".into());
        assert_eq!(
            select_review_gap_for_selection(&state).unwrap().gap_id,
            "trailing:1"
        );
        Arc::make_mut(&mut state.document).files[0].source_identity = None;
        assert!(select_review_gap_for_selection(&state).is_none());
        let mut no_gap = SemanticReviewState::new(
            document_with_sources(&[("alpha", Some("source:alpha"), true)]),
            false,
        );
        let file = &mut Arc::make_mut(&mut no_gap.document).files[0];
        file.addition_lines = vec!["one".into()];
        file.deletion_lines = vec!["one".into()];
        assert!(select_review_gap_for_selection(&no_gap).is_none());
    }
}
