//! User-note targeting, focus, and public event coordination.
//!
//! Clean-room Rust port of Hunk's `useUserNoteComposer` hook at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Semantic draft transitions stay
//! behind the host trait; this controller owns only UI fallback and lifecycle policy.

use workdeck_core::ReviewSide;
use workdeck_extension_api::{ExtensionFileSide, ExtensionLifecycleEvent, ExtensionReviewNote};
use workdeck_review::{TerminalDraftReviewNote, TerminalDraftReviewNoteKind, TerminalReviewNote};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserNoteLineTarget {
    pub side: ReviewSide,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserNoteLineCursor {
    pub file_id: String,
    pub hunk_index: usize,
    pub target: UserNoteLineTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveAddNoteTarget {
    pub file_id: String,
    pub hunk_index: usize,
    pub target: Option<UserNoteLineTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartUserNoteArguments {
    pub file_id: Option<String>,
    pub hunk_index: Option<usize>,
    pub target: Option<UserNoteLineTarget>,
    pub preserve_viewport: bool,
}

/// Replaceable semantic actions and focus/event seams supplied by the mounted review.
pub trait UserNoteComposerHost {
    fn get_line_cursor(&mut self) -> Option<UserNoteLineCursor>;
    fn start_draft(&mut self, arguments: StartUserNoteArguments)
    -> Option<TerminalDraftReviewNote>;
    fn start_edit(
        &mut self,
        note_id: &str,
        preserve_viewport: bool,
    ) -> Option<TerminalDraftReviewNote>;
    fn start_reply(
        &mut self,
        note_id: &str,
        preserve_viewport: bool,
    ) -> Option<TerminalDraftReviewNote>;
    fn update_draft(&mut self, body: &str);
    fn save_draft(&mut self) -> Option<TerminalReviewNote>;
    fn cancel_draft(&mut self);
    fn focus_draft(&mut self);
    fn focus_review(&mut self);
    fn blur_draft(&mut self);
    fn publish_event(&mut self, event: ExtensionLifecycleEvent);
}

#[derive(Debug, Clone, Copy)]
pub struct ProjectableReviewNote<'a> {
    pub id: &'a str,
    pub parent_id: Option<&'a str>,
    pub file_id: &'a str,
    pub file_path: &'a str,
    pub hunk_index: usize,
    pub side: ReviewSide,
    pub line: u32,
    pub body: Option<&'a str>,
    pub summary: Option<&'a str>,
}

/// Project terminal note identity and location into the stable extension event shape.
#[must_use]
pub fn project_extension_review_note(
    note: ProjectableReviewNote<'_>,
    draft: bool,
) -> ExtensionReviewNote {
    ExtensionReviewNote {
        id: note.id.to_owned(),
        parent_id: note
            .parent_id
            .filter(|parent_id| !parent_id.is_empty())
            .map(str::to_owned),
        file_id: note.file_id.to_owned(),
        file_path: note.file_path.to_owned(),
        hunk_index: note.hunk_index,
        side: match note.side {
            ReviewSide::Old => ExtensionFileSide::Old,
            ReviewSide::New => ExtensionFileSide::New,
        },
        line: note.line,
        body: note.body.or(note.summary).unwrap_or_default().to_owned(),
        draft,
    }
}

fn draft_projection<'a>(
    note: &'a TerminalDraftReviewNote,
    id: &'a str,
    body: &'a str,
) -> ProjectableReviewNote<'a> {
    ProjectableReviewNote {
        id,
        parent_id: note.parent_id.as_deref(),
        file_id: &note.file_id,
        file_path: &note.file_path,
        hunk_index: note.hunk_index,
        side: note.side,
        line: note.line,
        body: Some(body),
        summary: None,
    }
}

fn saved_projection<'a>(
    note: &'a TerminalReviewNote,
    file_id: &'a str,
) -> ProjectableReviewNote<'a> {
    ProjectableReviewNote {
        id: &note.id,
        parent_id: note.stored.parent_id.as_deref(),
        file_id,
        file_path: &note.file_path,
        hunk_index: note.hunk_index,
        side: note.side,
        line: note.line,
        body: None,
        summary: Some(&note.summary),
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UserNoteComposerController {
    active_add_note_target: Option<ActiveAddNoteTarget>,
}

impl UserNoteComposerController {
    pub fn set_active_add_note_target(&mut self, target: Option<ActiveAddNoteTarget>) {
        self.active_add_note_target = target;
    }

    #[must_use]
    pub fn active_add_note_target(&self) -> Option<&ActiveAddNoteTarget> {
        self.active_add_note_target.as_ref()
    }

    /// Start at an explicit target, then hover, then an enabled measured line cursor.
    pub fn start_user_note(
        &mut self,
        host: &mut impl UserNoteComposerHost,
        file_id: Option<&str>,
        hunk_index: Option<usize>,
        target: Option<UserNoteLineTarget>,
        keyboard_cursor_enabled: bool,
    ) -> Option<TerminalDraftReviewNote> {
        let has_explicit_target = file_id.is_some() || hunk_index.is_some() || target.is_some();
        let implicit_target = if has_explicit_target {
            None
        } else {
            self.active_add_note_target.clone().or_else(|| {
                keyboard_cursor_enabled
                    .then(|| host.get_line_cursor())
                    .flatten()
                    .map(|cursor| ActiveAddNoteTarget {
                        file_id: cursor.file_id,
                        hunk_index: cursor.hunk_index,
                        target: Some(cursor.target),
                    })
            })
        };
        let arguments = StartUserNoteArguments {
            file_id: file_id.map(str::to_owned).or_else(|| {
                implicit_target
                    .as_ref()
                    .map(|target| target.file_id.clone())
            }),
            hunk_index: hunk_index
                .or_else(|| implicit_target.as_ref().map(|target| target.hunk_index)),
            target: target.or_else(|| implicit_target.as_ref().and_then(|target| target.target)),
            preserve_viewport: has_explicit_target || implicit_target.is_some(),
        };
        let draft = host.start_draft(arguments);
        if draft.is_some() {
            self.active_add_note_target = None;
            host.focus_draft();
        }
        draft
    }

    pub fn start_user_note_edit(
        &mut self,
        host: &mut impl UserNoteComposerHost,
        note_id: &str,
        preserve_viewport: bool,
    ) -> Option<TerminalDraftReviewNote> {
        let draft = host.start_edit(note_id, preserve_viewport);
        if draft.is_some() {
            self.active_add_note_target = None;
            host.focus_draft();
        }
        draft
    }

    pub fn start_user_note_reply(
        &mut self,
        host: &mut impl UserNoteComposerHost,
        note_id: &str,
        preserve_viewport: bool,
    ) -> Option<TerminalDraftReviewNote> {
        let draft = host.start_reply(note_id, preserve_viewport);
        if draft.is_some() {
            self.active_add_note_target = None;
            host.focus_draft();
        }
        draft
    }

    pub fn focus_draft_note(&self, host: &mut impl UserNoteComposerHost) {
        host.focus_draft();
    }

    pub fn blur_draft_note(&self, host: &mut impl UserNoteComposerHost) {
        host.blur_draft();
    }

    /// Save once, publish only a successful semantic result, and always restore review focus.
    pub fn save_draft_note(
        &self,
        host: &mut impl UserNoteComposerHost,
        draft_note: Option<&TerminalDraftReviewNote>,
    ) {
        let saved = host.save_draft();
        if let (Some(saved), Some(prior_draft)) = (saved.as_ref(), draft_note) {
            let note =
                project_extension_review_note(saved_projection(saved, &prior_draft.file_id), false);
            let event = if prior_draft.kind == TerminalDraftReviewNoteKind::Edit {
                ExtensionLifecycleEvent::NoteEdited { note }
            } else {
                ExtensionLifecycleEvent::NoteCreated { note }
            };
            host.publish_event(event);
        }
        host.focus_review();
    }

    /// Update the semantic draft, then publish the exact body supplied by the editor.
    pub fn update_draft_note(
        &self,
        host: &mut impl UserNoteComposerHost,
        draft_note: Option<&TerminalDraftReviewNote>,
        body: &str,
    ) {
        host.update_draft(body);
        if let Some(prior_draft) = draft_note {
            let id = if prior_draft.kind == TerminalDraftReviewNoteKind::Edit {
                prior_draft
                    .target_note_id
                    .as_deref()
                    .filter(|target_note_id| !target_note_id.is_empty())
                    .unwrap_or(&prior_draft.id)
            } else {
                &prior_draft.id
            };
            let note = project_extension_review_note(draft_projection(prior_draft, id, body), true);
            host.publish_event(ExtensionLifecycleEvent::NoteEdited { note });
        }
    }

    pub fn cancel_draft_note(&self, host: &mut impl UserNoteComposerHost) {
        host.cancel_draft();
        host.focus_review();
    }
}

#[cfg(test)]
mod tests {
    use workdeck_review::{StoredReviewNoteRenderMetadata, TerminalDraftReviewNoteKind};

    use super::*;

    fn draft() -> TerminalDraftReviewNote {
        TerminalDraftReviewNote {
            id: "draft:alpha:1".into(),
            kind: TerminalDraftReviewNoteKind::Create,
            target_note_id: None,
            parent_id: None,
            file_id: "runtime-alpha".into(),
            file_path: "src/alpha.ts".into(),
            hunk_index: 1,
            side: ReviewSide::New,
            line: 42,
            old_range: None,
            new_range: Some([42, 42]),
            body: "initial body".into(),
        }
    }

    fn saved() -> TerminalReviewNote {
        TerminalReviewNote {
            id: "user:stable-1".into(),
            stored: StoredReviewNoteRenderMetadata {
                review_note_id: "user:stable-1".into(),
                semantically_stored: true,
                ..StoredReviewNoteRenderMetadata::default()
            },
            source: "user".into(),
            author: Some("user".into()),
            created_at: "2026-01-02T03:04:05.000Z".into(),
            updated_at: None,
            file_path: "src/alpha.ts".into(),
            hunk_index: 1,
            side: ReviewSide::New,
            line: 42,
            old_range: None,
            new_range: Some([42, 42]),
            summary: "saved body".into(),
            rationale: None,
            markup: None,
            title: None,
            tags: Vec::new(),
            confidence: None,
            editable: Some(true),
        }
    }

    #[derive(Default)]
    struct Host {
        cursor: Option<UserNoteLineCursor>,
        cursor_reads: usize,
        starts: Vec<StartUserNoteArguments>,
        edits: Vec<(String, bool)>,
        replies: Vec<(String, bool)>,
        start_result: Option<TerminalDraftReviewNote>,
        edit_result: Option<TerminalDraftReviewNote>,
        reply_result: Option<TerminalDraftReviewNote>,
        save_results: Vec<Option<TerminalReviewNote>>,
        transitions: Vec<String>,
        updated_bodies: Vec<String>,
        cancel_count: usize,
        events: Vec<ExtensionLifecycleEvent>,
    }

    impl UserNoteComposerHost for Host {
        fn get_line_cursor(&mut self) -> Option<UserNoteLineCursor> {
            self.cursor_reads += 1;
            self.cursor.clone()
        }

        fn start_draft(
            &mut self,
            arguments: StartUserNoteArguments,
        ) -> Option<TerminalDraftReviewNote> {
            self.starts.push(arguments);
            self.start_result.clone()
        }

        fn start_edit(
            &mut self,
            note_id: &str,
            preserve_viewport: bool,
        ) -> Option<TerminalDraftReviewNote> {
            self.edits.push((note_id.into(), preserve_viewport));
            self.edit_result.clone()
        }

        fn start_reply(
            &mut self,
            note_id: &str,
            preserve_viewport: bool,
        ) -> Option<TerminalDraftReviewNote> {
            self.replies.push((note_id.into(), preserve_viewport));
            self.reply_result.clone()
        }

        fn update_draft(&mut self, body: &str) {
            self.updated_bodies.push(body.into());
        }

        fn save_draft(&mut self) -> Option<TerminalReviewNote> {
            if self.save_results.is_empty() {
                None
            } else {
                self.save_results.remove(0)
            }
        }

        fn cancel_draft(&mut self) {
            self.cancel_count += 1;
        }

        fn focus_draft(&mut self) {
            self.transitions.push("draft".into());
        }

        fn focus_review(&mut self) {
            self.transitions.push("review".into());
        }

        fn blur_draft(&mut self) {
            self.transitions.push("blur".into());
        }

        fn publish_event(&mut self, event: ExtensionLifecycleEvent) {
            self.events.push(event);
        }
    }

    fn event_parts(events: &[ExtensionLifecycleEvent]) -> Vec<(String, serde_json::Value)> {
        events
            .iter()
            .cloned()
            .map(ExtensionLifecycleEvent::into_parts)
            .collect()
    }

    #[test]
    fn prefers_explicit_targets_then_hover_then_the_enabled_keyboard_cursor() {
        let mut controller = UserNoteComposerController::default();
        let mut host = Host {
            cursor: Some(UserNoteLineCursor {
                file_id: "cursor-file".into(),
                hunk_index: 4,
                target: UserNoteLineTarget {
                    side: ReviewSide::Old,
                    line: 18,
                },
            }),
            start_result: Some(draft()),
            ..Host::default()
        };
        controller.set_active_add_note_target(Some(ActiveAddNoteTarget {
            file_id: "hover-file".into(),
            hunk_index: 3,
            target: Some(UserNoteLineTarget {
                side: ReviewSide::New,
                line: 31,
            }),
        }));
        controller.start_user_note(&mut host, None, None, None, true);
        assert_eq!(host.starts[0].file_id.as_deref(), Some("hover-file"));
        assert!(host.starts[0].preserve_viewport);
        assert_eq!(host.cursor_reads, 0);

        controller.set_active_add_note_target(Some(ActiveAddNoteTarget {
            file_id: "stale-hover-file".into(),
            hunk_index: 9,
            target: Some(UserNoteLineTarget {
                side: ReviewSide::New,
                line: 99,
            }),
        }));
        controller.start_user_note(
            &mut host,
            None,
            None,
            Some(UserNoteLineTarget {
                side: ReviewSide::Old,
                line: 7,
            }),
            true,
        );
        assert_eq!(host.starts[1].file_id, None);
        assert_eq!(host.starts[1].target.unwrap().line, 7);
        assert_eq!(host.cursor_reads, 0);

        controller.start_user_note(&mut host, None, None, None, true);
        assert_eq!(host.starts[2].file_id.as_deref(), Some("cursor-file"));
        assert_eq!(host.starts[2].hunk_index, Some(4));
        assert_eq!(host.starts[2].target.unwrap().line, 18);
        assert_eq!(host.cursor_reads, 1);
    }

    #[test]
    fn does_not_consult_the_current_line_when_cursor_navigation_is_off() {
        let mut controller = UserNoteComposerController::default();
        let mut host = Host {
            cursor: Some(UserNoteLineCursor {
                file_id: "cursor-file".into(),
                hunk_index: 0,
                target: UserNoteLineTarget {
                    side: ReviewSide::New,
                    line: 1,
                },
            }),
            ..Host::default()
        };
        controller.start_user_note(&mut host, None, None, None, false);
        assert_eq!(host.cursor_reads, 0);
        assert_eq!(
            host.starts,
            [StartUserNoteArguments {
                file_id: None,
                hunk_index: None,
                target: None,
                preserve_viewport: false,
            }]
        );
    }

    #[test]
    fn failed_draft_creation_preserves_hover_and_does_not_steal_focus() {
        let mut controller = UserNoteComposerController::default();
        let hover = ActiveAddNoteTarget {
            file_id: "hover-file".into(),
            hunk_index: 0,
            target: None,
        };
        controller.set_active_add_note_target(Some(hover.clone()));
        let mut host = Host::default();
        controller.start_user_note(&mut host, None, None, None, true);
        controller.start_user_note(&mut host, None, None, None, true);
        assert_eq!(host.starts[0].file_id.as_deref(), Some("hover-file"));
        assert_eq!(host.starts[1].file_id.as_deref(), Some("hover-file"));
        assert_eq!(controller.active_add_note_target(), Some(&hover));
        assert!(host.transitions.is_empty());
    }

    #[test]
    fn forwards_mouse_viewport_policy_through_edit_and_reply_wrappers() {
        let mut controller = UserNoteComposerController::default();
        let mut host = Host {
            edit_result: Some(draft()),
            reply_result: Some(draft()),
            ..Host::default()
        };
        controller.start_user_note_edit(&mut host, "edit-note", true);
        controller.start_user_note_reply(&mut host, "reply-note", true);
        assert_eq!(host.edits, [("edit-note".into(), true)]);
        assert_eq!(host.replies, [("reply-note".into(), true)]);
        assert_eq!(host.transitions, ["draft", "draft"]);
    }

    #[test]
    fn coordinates_focus_cancel_and_blur_transitions_through_narrow_actions() {
        let controller = UserNoteComposerController::default();
        let mut host = Host::default();
        controller.focus_draft_note(&mut host);
        controller.blur_draft_note(&mut host);
        controller.cancel_draft_note(&mut host);
        assert_eq!(host.transitions, ["draft", "blur", "review"]);
        assert_eq!(host.cancel_count, 1);
    }

    #[test]
    fn publishes_one_created_event_with_prior_draft_file_id_and_saved_identity() {
        let controller = UserNoteComposerController::default();
        let draft = draft();
        let mut host = Host {
            save_results: vec![Some(saved()), None],
            ..Host::default()
        };
        controller.save_draft_note(&mut host, Some(&draft));
        controller.save_draft_note(&mut host, Some(&draft));
        let events = event_parts(&host.events);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "note_created");
        assert_eq!(events[0].1["note"]["id"], "user:stable-1");
        assert_eq!(events[0].1["note"]["fileId"], "runtime-alpha");
        assert_eq!(events[0].1["note"]["body"], "saved body");
        assert_eq!(host.transitions, ["review", "review"]);
    }

    #[test]
    fn publishes_a_committed_edit_distinctly_from_note_creation() {
        let controller = UserNoteComposerController::default();
        let mut draft = draft();
        draft.kind = TerminalDraftReviewNoteKind::Edit;
        draft.target_note_id = Some("user:stable-1".into());
        let mut host = Host {
            save_results: vec![Some(saved())],
            ..Host::default()
        };
        controller.save_draft_note(&mut host, Some(&draft));
        let events = event_parts(&host.events);
        assert_eq!(events[0].0, "note_edited");
        assert_eq!(events[0].1["note"]["id"], "user:stable-1");
        assert_eq!(events[0].1["note"]["draft"], false);
    }

    #[test]
    fn updates_semantic_draft_and_publishes_the_editors_current_body() {
        let controller = UserNoteComposerController::default();
        let draft = draft();
        let mut host = Host::default();
        controller.update_draft_note(&mut host, Some(&draft), "current editor body");
        assert_eq!(host.updated_bodies, ["current editor body"]);
        let events = event_parts(&host.events);
        assert_eq!(events[0].0, "note_edited");
        assert_eq!(events[0].1["note"]["id"], "draft:alpha:1");
        assert_eq!(events[0].1["note"]["body"], "current editor body");
        assert_eq!(events[0].1["note"]["draft"], true);
    }

    #[test]
    fn uses_replacement_draft_facts_and_callbacks_after_a_committed_rerender() {
        let mut controller = UserNoteComposerController::default();
        let stale = Host {
            start_result: Some(draft()),
            ..Host::default()
        };
        let mut replacement = draft();
        replacement.id = "draft:beta:2".into();
        replacement.file_id = "runtime-beta".into();
        replacement.file_path = "src/beta.ts".into();
        replacement.hunk_index = 2;
        replacement.side = ReviewSide::Old;
        replacement.line = 17;
        let mut replacement_saved = saved();
        replacement_saved.id = "user:stable-2".into();
        replacement_saved.file_path = "src/beta.ts".into();
        replacement_saved.hunk_index = 2;
        replacement_saved.side = ReviewSide::Old;
        replacement_saved.line = 17;
        replacement_saved.summary = "replacement saved body".into();
        let mut current = Host {
            start_result: Some(replacement.clone()),
            save_results: vec![Some(replacement_saved)],
            ..Host::default()
        };

        controller.start_user_note(
            &mut current,
            Some("runtime-beta"),
            Some(2),
            Some(UserNoteLineTarget {
                side: ReviewSide::Old,
                line: 17,
            }),
            true,
        );
        controller.focus_draft_note(&mut current);
        controller.blur_draft_note(&mut current);
        controller.update_draft_note(&mut current, Some(&replacement), "current replacement body");
        controller.save_draft_note(&mut current, Some(&replacement));
        controller.cancel_draft_note(&mut current);

        assert!(stale.starts.is_empty());
        assert!(stale.events.is_empty());
        assert_eq!(current.starts.len(), 1);
        assert_eq!(
            current.transitions,
            ["draft", "draft", "blur", "review", "review"]
        );
        let events = event_parts(&current.events);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1["note"]["fileId"], "runtime-beta");
        assert_eq!(events[1].0, "note_created");
        assert_eq!(events[1].1["note"]["id"], "user:stable-2");
    }

    #[test]
    fn projects_draft_bodies_and_saved_summaries_without_changing_ids_or_targets() {
        let draft = draft();
        let projected =
            project_extension_review_note(draft_projection(&draft, &draft.id, &draft.body), true);
        assert_eq!(projected.id, draft.id);
        assert_eq!(projected.file_id, draft.file_id);
        assert_eq!(projected.body, draft.body);
        assert!(projected.draft);

        let mut saved = saved();
        saved.stored.parent_id = Some("user:parent".into());
        let projected =
            project_extension_review_note(saved_projection(&saved, "runtime-alpha"), false);
        assert_eq!(projected.id, saved.id);
        assert_eq!(projected.parent_id.as_deref(), Some("user:parent"));
        assert_eq!(projected.body, saved.summary);
        assert!(!projected.draft);
    }
}
