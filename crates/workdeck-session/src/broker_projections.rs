//! Read-only projections from live broker entries to public session views.

use workdeck_core::ReviewNoteSource;

use crate::{
    ListedSession, SelectedHunkSummary, SelectedSessionContext, SessionFileSummary,
    SessionLiveCommentSummary, SessionReview, SessionReviewFile, SessionReviewNoteSummary,
    WorkdeckSessionRegistration, WorkdeckSessionSnapshot,
};

/// One registration and its latest snapshot as retained by the broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckSessionEntry {
    pub registration: WorkdeckSessionRegistration,
    pub snapshot: WorkdeckSessionSnapshot,
}

/// Options controlling the size of an exported review.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionReviewOptions {
    pub include_patch: bool,
    pub include_notes: bool,
}

/// Optional filters for live comments.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionCommentFilter<'a> {
    pub file_path: Option<&'a str>,
}

/// Optional filters for persisted review notes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionNoteFilter<'a> {
    pub file_path: Option<&'a str>,
    pub source: Option<ReviewNoteSource>,
}

fn selected_summary(session: &ListedSession) -> Option<&SessionFileSummary> {
    session.files.iter().find(|file| {
        session.snapshot.state.selected_file_id.as_deref() == Some(file.id.as_str())
            || session.snapshot.state.selected_file_path.as_deref() == Some(file.path.as_str())
            || file.previous_path.as_deref() == session.snapshot.state.selected_file_path.as_deref()
    })
}

fn selected_review_file(entry: &WorkdeckSessionEntry) -> Option<&SessionReviewFile> {
    entry.registration.info.files.iter().find(|file| {
        entry.snapshot.state.selected_file_id.as_deref() == Some(file.summary.id.as_str())
            || entry.snapshot.state.selected_file_path.as_deref()
                == Some(file.summary.path.as_str())
            || file.summary.previous_path.as_deref()
                == entry.snapshot.state.selected_file_path.as_deref()
    })
}

/// Reduce one review-export file to the summary used by session listings.
#[must_use]
pub fn summarize_review_file(review_file: &SessionReviewFile) -> SessionFileSummary {
    review_file.summary.clone()
}

/// Serialize a review file while keeping raw patch text opt-in for callers.
#[must_use]
pub fn serialize_review_file(
    review_file: &SessionReviewFile,
    include_patch: bool,
) -> SessionReviewFile {
    let mut serialized = review_file.clone();
    if !include_patch {
        serialized.patch = None;
    }
    serialized
}

/// Project one raw broker entry into the session list shape used by the CLI.
#[must_use]
pub fn build_listed_workdeck_session(entry: &WorkdeckSessionEntry) -> ListedSession {
    ListedSession {
        session_id: entry.registration.session_id.clone(),
        pid: entry.registration.pid,
        cwd: entry.registration.cwd.clone(),
        repo_root: entry.registration.repo_root.clone(),
        launched_at: entry.registration.launched_at.clone(),
        terminal: entry.registration.terminal.clone(),
        input_kind: entry.registration.info.input_kind,
        title: entry.registration.info.title.clone(),
        source_label: entry.registration.info.source_label.clone(),
        experimental_features: Some(
            entry
                .registration
                .info
                .experimental_features
                .clone()
                .unwrap_or_default(),
        ),
        file_count: u64::try_from(entry.registration.info.files.len()).unwrap_or(u64::MAX),
        files: entry
            .registration
            .info
            .files
            .iter()
            .map(summarize_review_file)
            .collect(),
        snapshot: entry.snapshot.clone(),
    }
}

/// Project the focused file and hunk for one session.
#[must_use]
pub fn build_selected_workdeck_session_context(session: &ListedSession) -> SelectedSessionContext {
    let selected_file = selected_summary(session).cloned();
    SelectedSessionContext {
        session_id: session.session_id.clone(),
        title: session.title.clone(),
        source_label: session.source_label.clone(),
        cwd: Some(session.cwd.clone()),
        repo_root: session.repo_root.clone(),
        input_kind: session.input_kind,
        experimental_features: session.experimental_features.clone(),
        selected_hunk: selected_file.as_ref().map(|_| SelectedHunkSummary {
            index: session.snapshot.state.selected_hunk_index,
            old_range: session.snapshot.state.selected_hunk_old_range,
            new_range: session.snapshot.state.selected_hunk_new_range,
        }),
        selected_file,
        show_agent_notes: session.snapshot.state.show_agent_notes,
        note_markup_width: session.snapshot.state.note_markup_width,
        live_comment_count: session.snapshot.state.live_comment_count,
    }
}

/// Project one raw broker entry into the review export used by `workdeck session review`.
#[must_use]
pub fn build_workdeck_session_review(
    entry: &WorkdeckSessionEntry,
    options: SessionReviewOptions,
) -> SessionReview {
    let selected_file = selected_review_file(entry);
    SessionReview {
        session_id: entry.registration.session_id.clone(),
        title: entry.registration.info.title.clone(),
        source_label: entry.registration.info.source_label.clone(),
        cwd: Some(entry.registration.cwd.clone()),
        repo_root: entry.registration.repo_root.clone(),
        input_kind: entry.registration.info.input_kind,
        experimental_features: Some(
            entry
                .registration
                .info
                .experimental_features
                .clone()
                .unwrap_or_default(),
        ),
        selected_file: selected_file.map(|file| serialize_review_file(file, options.include_patch)),
        selected_hunk: selected_file.and_then(|file| {
            usize::try_from(entry.snapshot.state.selected_hunk_index)
                .ok()
                .and_then(|index| file.hunks.get(index))
                .cloned()
        }),
        show_agent_notes: entry.snapshot.state.show_agent_notes,
        live_comment_count: entry.snapshot.state.live_comment_count,
        review_note_count: Some(entry.snapshot.state.review_note_count.unwrap_or_else(|| {
            entry
                .snapshot
                .state
                .review_notes
                .as_ref()
                .map_or(0, |notes| u64::try_from(notes.len()).unwrap_or(u64::MAX))
        })),
        review_notes: options.include_notes.then(|| {
            entry
                .snapshot
                .state
                .review_notes
                .clone()
                .unwrap_or_default()
        }),
        files: entry
            .registration
            .info
            .files
            .iter()
            .map(|file| serialize_review_file(file, options.include_patch))
            .collect(),
    }
}

/// Return visible live comments, optionally filtered to one exact file path.
#[must_use]
pub fn list_workdeck_session_comments(
    session: &ListedSession,
    filter: SessionCommentFilter<'_>,
) -> Vec<SessionLiveCommentSummary> {
    let file_path = filter.file_path.filter(|path| !path.is_empty());
    session
        .snapshot
        .state
        .live_comments
        .iter()
        .filter(|comment| file_path.is_none_or(|path| comment.file_path == path))
        .cloned()
        .collect()
}

/// Return review notes, optionally filtered to one exact file path and source.
#[must_use]
pub fn list_workdeck_session_notes(
    session: &ListedSession,
    filter: SessionNoteFilter<'_>,
) -> Vec<SessionReviewNoteSummary> {
    let file_path = filter.file_path.filter(|path| !path.is_empty());
    session
        .snapshot
        .state
        .review_notes
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|note| {
            file_path.is_none_or(|path| note.file_path == path)
                && filter.source.is_none_or(|source| note.source == source)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        SESSION_BROKER_REGISTRATION_VERSION, SessionLiveCommentSummary, SessionReviewHunk,
        SessionReviewNoteSummary, SessionTerminalLocation, SessionTerminalMetadata,
        WorkdeckExperimentalFeature, WorkdeckSessionInfo, WorkdeckSessionInputKind,
        WorkdeckSessionState,
    };
    use workdeck_core::{ReviewNoteSource, ReviewSide};

    use super::*;

    fn review_file() -> SessionReviewFile {
        SessionReviewFile {
            summary: SessionFileSummary {
                id: "runtime-1".into(),
                path: "src/example.ts".into(),
                previous_path: None,
                additions: 1,
                deletions: 1,
                hunk_count: 1,
            },
            patch: Some("@@ -1,1 +1,1 @@".into()),
            hunks: vec![SessionReviewHunk {
                index: 0,
                header: "@@ -1,1 +1,1 @@".into(),
                old_range: Some([1, 1]),
                new_range: Some([1, 1]),
            }],
        }
    }

    fn live_comment(comment_id: &str, file_path: &str, line: u64) -> SessionLiveCommentSummary {
        SessionLiveCommentSummary {
            comment_id: comment_id.into(),
            file_path: file_path.into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line,
            summary: "Review this".into(),
            rationale: None,
            author: None,
            created_at: "2026-05-10T00:00:00.000Z".into(),
        }
    }

    fn note(note_id: &str, source: ReviewNoteSource, file_path: &str) -> SessionReviewNoteSummary {
        SessionReviewNoteSummary {
            note_id: note_id.into(),
            parent_id: None,
            source,
            file_path: file_path.into(),
            hunk_index: None,
            old_range: None,
            new_range: None,
            body: "Review note".into(),
            title: None,
            author: None,
            created_at: "2026-05-10T00:00:00.000Z".into(),
            updated_at: None,
            editable: source == ReviewNoteSource::User,
        }
    }

    fn entry() -> WorkdeckSessionEntry {
        WorkdeckSessionEntry {
            registration: WorkdeckSessionRegistration {
                registration_version: SESSION_BROKER_REGISTRATION_VERSION,
                session_id: "session-1".into(),
                pid: 321,
                cwd: "/repo".into(),
                repo_root: Some("/repo".into()),
                launched_at: "2026-05-10T00:00:00.000Z".into(),
                terminal: None,
                info: WorkdeckSessionInfo {
                    input_kind: WorkdeckSessionInputKind::Diff,
                    title: "Working tree".into(),
                    source_label: "git diff".into(),
                    experimental_features: None,
                    files: vec![review_file()],
                    review_catalog: None,
                    review_capability_digest: None,
                },
            },
            snapshot: WorkdeckSessionSnapshot {
                updated_at: "2026-05-10T00:00:01.000Z".into(),
                state: WorkdeckSessionState {
                    selected_file_id: Some("runtime-1".into()),
                    selected_file_path: Some("src/example.ts".into()),
                    selected_hunk_index: 0,
                    selected_hunk_old_range: Some([1, 1]),
                    selected_hunk_new_range: Some([1, 1]),
                    show_agent_notes: true,
                    note_markup_width: Some(80),
                    live_comment_count: 0,
                    live_comments: Vec::new(),
                    review_note_count: None,
                    review_notes: None,
                    review_publication: None,
                },
            },
        }
    }

    #[test]
    fn listed_session_keeps_terminal_metadata_and_file_summaries() {
        let mut entry = entry();
        entry.registration.info.experimental_features =
            Some(vec![WorkdeckExperimentalFeature::Stml]);
        entry.registration.terminal = Some(SessionTerminalMetadata {
            program: Some("iTerm.app".into()),
            locations: vec![SessionTerminalLocation {
                source: "tty".into(),
                tty: Some("/dev/ttys003".into()),
                ..SessionTerminalLocation::default()
            }],
        });

        let listed = build_listed_workdeck_session(&entry);
        assert_eq!(listed.terminal, entry.registration.terminal);
        assert_eq!(
            listed.experimental_features,
            Some(vec![WorkdeckExperimentalFeature::Stml])
        );
        assert_eq!(listed.file_count, 1);
        assert_eq!(listed.files[0].path, "src/example.ts");
        assert_eq!(listed.files[0].hunk_count, 1);
    }

    #[test]
    fn selected_context_projects_current_file_and_ranges() {
        let mut entry = entry();
        entry.registration.info.experimental_features =
            Some(vec![WorkdeckExperimentalFeature::Stml]);
        entry.snapshot.state.selected_hunk_index = 1;
        entry.snapshot.state.selected_hunk_old_range = Some([8, 8]);
        entry.snapshot.state.selected_hunk_new_range = Some([8, 9]);
        let context =
            build_selected_workdeck_session_context(&build_listed_workdeck_session(&entry));

        assert_eq!(
            context.experimental_features,
            Some(vec![WorkdeckExperimentalFeature::Stml])
        );
        assert_eq!(
            context
                .selected_file
                .as_ref()
                .map(|file| file.path.as_str()),
            Some("src/example.ts")
        );
        assert_eq!(
            context.selected_hunk,
            Some(SelectedHunkSummary {
                index: 1,
                old_range: Some([8, 8]),
                new_range: Some([8, 9]),
            })
        );
    }

    #[test]
    fn review_strips_patch_by_default_and_includes_it_on_demand() {
        let entry = entry();
        let without_patch = build_workdeck_session_review(&entry, SessionReviewOptions::default());
        assert_eq!(without_patch.files[0].patch, None);

        let with_patch = build_workdeck_session_review(
            &entry,
            SessionReviewOptions {
                include_patch: true,
                include_notes: false,
            },
        );
        assert_eq!(
            with_patch.files[0].patch.as_deref(),
            Some("@@ -1,1 +1,1 @@")
        );
    }

    #[test]
    fn review_can_include_live_notes_on_demand() {
        let mut entry = entry();
        entry.snapshot.state.review_note_count = Some(1);
        entry.snapshot.state.review_notes = Some(vec![note(
            "user:1",
            ReviewNoteSource::User,
            "src/example.ts",
        )]);

        assert_eq!(
            build_workdeck_session_review(&entry, SessionReviewOptions::default()).review_notes,
            None
        );
        let review = build_workdeck_session_review(
            &entry,
            SessionReviewOptions {
                include_patch: false,
                include_notes: true,
            },
        );
        assert_eq!(review.review_notes.as_ref().unwrap()[0].note_id, "user:1");
        assert_eq!(
            review.review_notes.as_ref().unwrap()[0].source,
            ReviewNoteSource::User
        );
    }

    #[test]
    fn comments_return_all_visible_values_and_honor_file_filters() {
        let mut entry = entry();
        entry.snapshot.state.live_comment_count = 2;
        entry.snapshot.state.live_comments = vec![
            live_comment("comment-1", "src/example.ts", 1),
            live_comment("comment-2", "src/other.ts", 9),
        ];
        let session = build_listed_workdeck_session(&entry);

        assert_eq!(
            list_workdeck_session_comments(&session, SessionCommentFilter::default()).len(),
            2
        );
        assert_eq!(
            list_workdeck_session_comments(
                &session,
                SessionCommentFilter {
                    file_path: Some("")
                }
            )
            .len(),
            2
        );
        let filtered = list_workdeck_session_comments(
            &session,
            SessionCommentFilter {
                file_path: Some("src/example.ts"),
            },
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].comment_id, "comment-1");
    }

    #[test]
    fn notes_filter_by_file_and_source() {
        let mut entry = entry();
        entry.snapshot.state.review_note_count = Some(2);
        entry.snapshot.state.review_notes = Some(vec![
            note("user:1", ReviewNoteSource::User, "src/example.ts"),
            note("agent:1", ReviewNoteSource::Agent, "src/other.ts"),
        ]);
        let session = build_listed_workdeck_session(&entry);

        let by_source = list_workdeck_session_notes(
            &session,
            SessionNoteFilter {
                file_path: None,
                source: Some(ReviewNoteSource::User),
            },
        );
        assert_eq!(by_source.len(), 1);
        assert_eq!(by_source[0].note_id, "user:1");

        let by_file = list_workdeck_session_notes(
            &session,
            SessionNoteFilter {
                file_path: Some("src/other.ts"),
                source: None,
            },
        );
        assert_eq!(by_file.len(), 1);
        assert_eq!(by_file[0].note_id, "agent:1");
    }

    #[test]
    fn previous_path_selects_renamed_files_and_missing_notes_default_empty() {
        let mut entry = entry();
        entry.registration.info.files[0].summary.previous_path = Some("src/old.ts".into());
        entry.snapshot.state.selected_file_id = None;
        entry.snapshot.state.selected_file_path = Some("src/old.ts".into());
        let session = build_listed_workdeck_session(&entry);

        assert_eq!(
            build_selected_workdeck_session_context(&session)
                .selected_file
                .as_ref()
                .map(|file| file.path.as_str()),
            Some("src/example.ts")
        );
        assert_eq!(
            build_workdeck_session_review(
                &entry,
                SessionReviewOptions {
                    include_patch: false,
                    include_notes: true
                }
            )
            .review_notes,
            Some(Vec::new())
        );
        assert!(list_workdeck_session_notes(&session, SessionNoteFilter::default()).is_empty());
    }
}
