//! Typed app-facing routing for commands delivered by the generic session broker.

use serde_json::Value;
use workdeck_review::ReviewProducer;

use crate::{
    AppliedCommentBatchResult, AppliedCommentResult, AppliedHighlightResult, ClearedCommentsResult,
    ClearedHighlightsResult, CommentTargetInput, CommentToolInput, HighlightToolInput,
    NavigateToHunkToolInput, NavigatedSelectionResult, ReloadSessionOptions, ReloadedSessionResult,
    RemovedCommentResult, SessionBrokerConnectionBridge, SessionServerMessage,
    WorkdeckReviewFailureCodeV1, WorkdeckReviewFailureV1, WorkdeckReviewResourceReadResultV1,
    WorkdeckSessionCommandInput, WorkdeckSessionCommandResult, WorkdeckSessionServerMessage,
    apply_session_review_action, read_session_review_resource,
};

fn typed_server_message(
    message: SessionServerMessage<String, WorkdeckSessionCommandInput>,
) -> WorkdeckSessionServerMessage {
    let SessionServerMessage {
        request_id,
        command,
        command_version,
        input,
    } = message;
    macro_rules! typed {
        ($variant:ident, $input:expr) => {
            WorkdeckSessionServerMessage::$variant(SessionServerMessage {
                request_id,
                command,
                command_version,
                input: $input,
            })
        };
    }
    match input {
        WorkdeckSessionCommandInput::Comment(input) => typed!(Comment, input),
        WorkdeckSessionCommandInput::CommentBatch(input) => typed!(CommentBatch, input),
        WorkdeckSessionCommandInput::NavigateToHunk(input) => typed!(NavigateToHunk, input),
        WorkdeckSessionCommandInput::ReloadSession(input) => typed!(ReloadSession, input),
        WorkdeckSessionCommandInput::RemoveComment(input) => typed!(RemoveComment, input),
        WorkdeckSessionCommandInput::ClearComments(input) => typed!(ClearComments, input),
        WorkdeckSessionCommandInput::ReadReviewResource(input) => {
            typed!(ReadReviewResource, input)
        }
        WorkdeckSessionCommandInput::ApplyReviewAction(input) => typed!(ApplyReviewAction, input),
        WorkdeckSessionCommandInput::Highlight(input) => typed!(Highlight, input),
        WorkdeckSessionCommandInput::ClearHighlights(input) => typed!(ClearHighlights, input),
    }
}

/// Route one generic broker envelope through the typed Workdeck handlers.
///
/// The direct helper lets an event-loop owner apply a queued request on its
/// mutable UI thread without requiring that UI state itself be `Send + Sync`.
pub fn dispatch_workdeck_session_command<H>(
    handlers: &H,
    message: SessionServerMessage<String, WorkdeckSessionCommandInput>,
) -> Result<WorkdeckSessionCommandResult, String>
where
    H: WorkdeckSessionBridgeHandlers,
{
    WorkdeckSessionBridge { handlers }.dispatch_command(&typed_server_message(message))
}

pub trait WorkdeckSessionBridgeHandlers {
    /// Called only by the connection after a validated successful result enters
    /// its socket queue; ordinary dispatch does not imply transport completion.
    fn command_result_queued(&self, _request_id: &str) {}

    fn add_live_comment(
        &self,
        input: &CommentToolInput,
        comment_id: &str,
        reveal: bool,
    ) -> Result<AppliedCommentResult, String>;

    fn add_live_comment_batch(
        &self,
        comments: &[CommentTargetInput],
        request_id: &str,
        reveal_first: bool,
    ) -> Result<AppliedCommentBatchResult, String>;

    fn clear_live_comments(
        &self,
        file_path: Option<&str>,
        include_user: Option<bool>,
    ) -> Result<ClearedCommentsResult, String>;

    fn navigate_to_location(
        &self,
        input: &NavigateToHunkToolInput,
    ) -> Result<NavigatedSelectionResult, String>;

    fn add_agent_line_highlight(
        &self,
        input: &HighlightToolInput,
    ) -> Result<AppliedHighlightResult, String>;

    fn clear_agent_line_highlights(
        &self,
        file_path: Option<&str>,
    ) -> Result<ClearedHighlightsResult, String>;

    fn open_agent_notes(&self);

    fn reload_session(
        &self,
        next_input: &Value,
        options: ReloadSessionOptions,
    ) -> Result<ReloadedSessionResult, String>;

    fn remove_live_comment(&self, comment_id: &str) -> Result<RemovedCommentResult, String>;

    fn review_producer(&self) -> Option<ReviewProducer> {
        None
    }
}

impl<T> WorkdeckSessionBridgeHandlers for &T
where
    T: WorkdeckSessionBridgeHandlers + ?Sized,
{
    fn command_result_queued(&self, request_id: &str) {
        (**self).command_result_queued(request_id);
    }

    fn add_live_comment(
        &self,
        input: &CommentToolInput,
        comment_id: &str,
        reveal: bool,
    ) -> Result<AppliedCommentResult, String> {
        (**self).add_live_comment(input, comment_id, reveal)
    }

    fn add_live_comment_batch(
        &self,
        comments: &[CommentTargetInput],
        request_id: &str,
        reveal_first: bool,
    ) -> Result<AppliedCommentBatchResult, String> {
        (**self).add_live_comment_batch(comments, request_id, reveal_first)
    }

    fn clear_live_comments(
        &self,
        file_path: Option<&str>,
        include_user: Option<bool>,
    ) -> Result<ClearedCommentsResult, String> {
        (**self).clear_live_comments(file_path, include_user)
    }

    fn navigate_to_location(
        &self,
        input: &NavigateToHunkToolInput,
    ) -> Result<NavigatedSelectionResult, String> {
        (**self).navigate_to_location(input)
    }

    fn add_agent_line_highlight(
        &self,
        input: &HighlightToolInput,
    ) -> Result<AppliedHighlightResult, String> {
        (**self).add_agent_line_highlight(input)
    }

    fn clear_agent_line_highlights(
        &self,
        file_path: Option<&str>,
    ) -> Result<ClearedHighlightsResult, String> {
        (**self).clear_agent_line_highlights(file_path)
    }

    fn open_agent_notes(&self) {
        (**self).open_agent_notes();
    }

    fn reload_session(
        &self,
        next_input: &Value,
        options: ReloadSessionOptions,
    ) -> Result<ReloadedSessionResult, String> {
        (**self).reload_session(next_input, options)
    }

    fn remove_live_comment(&self, comment_id: &str) -> Result<RemovedCommentResult, String> {
        (**self).remove_live_comment(comment_id)
    }

    fn review_producer(&self) -> Option<ReviewProducer> {
        (**self).review_producer()
    }
}

#[must_use]
pub fn no_review_producer_failure() -> WorkdeckReviewFailureV1 {
    WorkdeckReviewFailureV1 {
        ok: false,
        code: WorkdeckReviewFailureCodeV1::StaleGeneration,
        message: "This session is not serving a published review.".into(),
        current_generation: String::new(),
    }
}

pub struct WorkdeckSessionBridge<H> {
    handlers: H,
}

#[must_use]
pub const fn create_workdeck_session_bridge<H>(handlers: H) -> WorkdeckSessionBridge<H>
where
    H: WorkdeckSessionBridgeHandlers,
{
    WorkdeckSessionBridge { handlers }
}

impl<H> WorkdeckSessionBridge<H>
where
    H: WorkdeckSessionBridgeHandlers,
{
    #[must_use]
    pub fn handlers(&self) -> &H {
        &self.handlers
    }

    pub fn dispatch_command(
        &self,
        message: &WorkdeckSessionServerMessage,
    ) -> Result<WorkdeckSessionCommandResult, String> {
        Ok(match message {
            WorkdeckSessionServerMessage::Comment(message) => {
                let reveal = message.input.reveal.unwrap_or(false);
                let result = self.handlers.add_live_comment(
                    &message.input,
                    &format!("mcp:{}", message.request_id),
                    reveal,
                )?;
                if reveal {
                    self.handlers.open_agent_notes();
                }
                WorkdeckSessionCommandResult::AppliedComment(result)
            }
            WorkdeckSessionServerMessage::CommentBatch(message) => {
                let reveal_first = matches!(
                    message.input.reveal_mode,
                    Some(crate::CommentBatchRevealMode::First)
                );
                let result = self.handlers.add_live_comment_batch(
                    &message.input.comments,
                    &message.request_id,
                    reveal_first,
                )?;
                if reveal_first && !result.applied.is_empty() {
                    self.handlers.open_agent_notes();
                }
                WorkdeckSessionCommandResult::AppliedCommentBatch(result)
            }
            WorkdeckSessionServerMessage::NavigateToHunk(message) => {
                WorkdeckSessionCommandResult::NavigatedSelection(
                    self.handlers.navigate_to_location(&message.input)?,
                )
            }
            WorkdeckSessionServerMessage::Highlight(message) => {
                WorkdeckSessionCommandResult::AppliedHighlight(
                    self.handlers.add_agent_line_highlight(&message.input)?,
                )
            }
            WorkdeckSessionServerMessage::ClearHighlights(message) => {
                WorkdeckSessionCommandResult::ClearedHighlights(
                    self.handlers
                        .clear_agent_line_highlights(message.input.file_path.as_deref())?,
                )
            }
            WorkdeckSessionServerMessage::ReloadSession(message) => {
                WorkdeckSessionCommandResult::ReloadedSession(self.handlers.reload_session(
                    &message.input.next_input,
                    ReloadSessionOptions {
                        reset_app: Some(false),
                        source_path: message.input.source_path.clone(),
                        ..ReloadSessionOptions::default()
                    },
                )?)
            }
            WorkdeckSessionServerMessage::RemoveComment(message) => {
                WorkdeckSessionCommandResult::RemovedComment(
                    self.handlers
                        .remove_live_comment(&message.input.comment_id)?,
                )
            }
            WorkdeckSessionServerMessage::ClearComments(message) => {
                WorkdeckSessionCommandResult::ClearedComments(self.handlers.clear_live_comments(
                    message.input.file_path.as_deref(),
                    message.input.include_user,
                )?)
            }
            WorkdeckSessionServerMessage::ReadReviewResource(message) => {
                let result = self.handlers.review_producer().map_or_else(
                    || WorkdeckReviewResourceReadResultV1::Failed(no_review_producer_failure()),
                    |producer| read_session_review_resource(&producer, &message.input.review),
                );
                WorkdeckSessionCommandResult::ReviewResource(result)
            }
            WorkdeckSessionServerMessage::ApplyReviewAction(message) => {
                let result = self.handlers.review_producer().map_or_else(
                    || crate::WorkdeckReviewActionResultV1::Failed(no_review_producer_failure()),
                    |producer| apply_session_review_action(&producer, &message.input.review),
                );
                WorkdeckSessionCommandResult::ReviewAction(result)
            }
        })
    }
}

impl<H> SessionBrokerConnectionBridge<WorkdeckSessionCommandInput, WorkdeckSessionCommandResult>
    for WorkdeckSessionBridge<H>
where
    H: WorkdeckSessionBridgeHandlers + Send + Sync + 'static,
{
    fn command_result_queued(&self, request_id: &str) {
        self.handlers.command_result_queued(request_id);
    }

    fn dispatch_command(
        &self,
        message: SessionServerMessage<String, WorkdeckSessionCommandInput>,
    ) -> Result<WorkdeckSessionCommandResult, String> {
        dispatch_workdeck_session_command(&self.handlers, message)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use workdeck_core::ReviewSide;

    use super::*;
    use crate::{
        ApplyReviewActionToolInput, ClearCommentsToolInput, ClearHighlightsToolInput,
        CommentBatchRevealMode, CommentBatchToolInput, CommentDirection,
        ReadReviewResourceToolInput, ReloadSessionToolInput, RemoveCommentToolInput,
        SessionLineHighlightTone, SessionSelector, SessionServerMessage,
        WORKDECK_REVIEW_PROTOCOL_VERSION, WorkdeckReviewActionEnvelopeV1, WorkdeckReviewActionV1,
        WorkdeckReviewActorKindV1, WorkdeckReviewActorV1, WorkdeckSessionInputKind,
    };

    #[derive(Default)]
    struct Calls {
        queued_results: Vec<String>,
        opened: usize,
        comments: Vec<(String, bool)>,
        batches: Vec<(String, usize, bool)>,
        reloads: Vec<(Value, ReloadSessionOptions)>,
        removed: Vec<String>,
        clears: Vec<(Option<String>, Option<bool>)>,
        highlights: usize,
        highlight_clears: Vec<Option<String>>,
        navigations: usize,
    }

    #[derive(Clone, Default)]
    struct Handlers {
        calls: Arc<Mutex<Calls>>,
        producer: Option<ReviewProducer>,
        reload_error: Option<String>,
    }

    impl WorkdeckSessionBridgeHandlers for Handlers {
        fn command_result_queued(&self, request_id: &str) {
            self.calls
                .lock()
                .unwrap()
                .queued_results
                .push(request_id.into());
        }

        fn add_live_comment(
            &self,
            input: &CommentToolInput,
            comment_id: &str,
            reveal: bool,
        ) -> Result<AppliedCommentResult, String> {
            self.calls
                .lock()
                .unwrap()
                .comments
                .push((comment_id.into(), reveal));
            Ok(AppliedCommentResult {
                comment_id: comment_id.into(),
                file_id: "file-1".into(),
                file_path: input.target.file_path.clone(),
                hunk_index: input.target.hunk_index.unwrap_or(0),
                side: input.target.side.unwrap_or(ReviewSide::New),
                line: input.target.line.unwrap_or(1),
                markup_width: None,
                markup_notes: None,
            })
        }

        fn add_live_comment_batch(
            &self,
            comments: &[CommentTargetInput],
            request_id: &str,
            reveal_first: bool,
        ) -> Result<AppliedCommentBatchResult, String> {
            self.calls.lock().unwrap().batches.push((
                request_id.into(),
                comments.len(),
                reveal_first,
            ));
            Ok(AppliedCommentBatchResult {
                applied: comments
                    .iter()
                    .enumerate()
                    .map(|(index, comment)| AppliedCommentResult {
                        comment_id: format!("{request_id}:{index}"),
                        file_id: "file-1".into(),
                        file_path: comment.file_path.clone(),
                        hunk_index: comment.hunk_index.unwrap_or(index as u64),
                        side: comment.side.unwrap_or(ReviewSide::New),
                        line: comment.line.unwrap_or(index as u64 + 1),
                        markup_width: None,
                        markup_notes: None,
                    })
                    .collect(),
            })
        }

        fn clear_live_comments(
            &self,
            file_path: Option<&str>,
            include_user: Option<bool>,
        ) -> Result<ClearedCommentsResult, String> {
            self.calls
                .lock()
                .unwrap()
                .clears
                .push((file_path.map(str::to_owned), include_user));
            Ok(ClearedCommentsResult {
                removed_count: u64::from(file_path.is_some()),
                remaining_comment_count: 0,
                file_path: file_path.map(str::to_owned),
                include_user,
                removed_live_comment_count: None,
                removed_user_note_count: None,
                remaining_live_comment_count: None,
                remaining_user_note_count: None,
            })
        }

        fn navigate_to_location(
            &self,
            input: &NavigateToHunkToolInput,
        ) -> Result<NavigatedSelectionResult, String> {
            self.calls.lock().unwrap().navigations += 1;
            Ok(NavigatedSelectionResult {
                file_id: "file-1".into(),
                file_path: input.file_path.clone().unwrap_or_else(|| "a.rs".into()),
                hunk_index: input.hunk_index.unwrap_or(0),
                selected_hunk: None,
                revealed: None,
                side: input.side,
                line: input.line,
            })
        }

        fn add_agent_line_highlight(
            &self,
            input: &HighlightToolInput,
        ) -> Result<AppliedHighlightResult, String> {
            self.calls.lock().unwrap().highlights += 1;
            Ok(AppliedHighlightResult {
                file_id: "file-1".into(),
                file_path: input.file_path.clone(),
                hunk_index: 0,
                side: input.side,
                line: input.line,
                start: input.start,
                end: input.end,
                tone: input.tone.unwrap_or(SessionLineHighlightTone::Match),
                file_mark_count: 1,
                revealed: None,
            })
        }

        fn clear_agent_line_highlights(
            &self,
            file_path: Option<&str>,
        ) -> Result<ClearedHighlightsResult, String> {
            self.calls
                .lock()
                .unwrap()
                .highlight_clears
                .push(file_path.map(str::to_owned));
            Ok(ClearedHighlightsResult {
                removed_count: 1,
                remaining_count: 0,
                file_path: file_path.map(str::to_owned),
            })
        }

        fn open_agent_notes(&self) {
            self.calls.lock().unwrap().opened += 1;
        }

        fn reload_session(
            &self,
            next_input: &Value,
            options: ReloadSessionOptions,
        ) -> Result<ReloadedSessionResult, String> {
            if let Some(error) = &self.reload_error {
                return Err(error.clone());
            }
            self.calls
                .lock()
                .unwrap()
                .reloads
                .push((next_input.clone(), options));
            Ok(ReloadedSessionResult {
                session_id: "session-1".into(),
                input_kind: WorkdeckSessionInputKind::Vcs,
                title: "reloaded".into(),
                source_label: "/repo".into(),
                file_count: 1,
                selected_file_path: None,
                selected_hunk_index: 0,
            })
        }

        fn remove_live_comment(&self, comment_id: &str) -> Result<RemovedCommentResult, String> {
            self.calls.lock().unwrap().removed.push(comment_id.into());
            Ok(RemovedCommentResult {
                comment_id: comment_id.into(),
                removed: true,
                remaining_comment_count: 0,
                source: None,
            })
        }

        fn review_producer(&self) -> Option<ReviewProducer> {
            self.producer.clone()
        }
    }

    fn message<Input>(
        request_id: &str,
        command: &str,
        input: Input,
    ) -> SessionServerMessage<String, Input> {
        SessionServerMessage {
            request_id: request_id.into(),
            command: command.into(),
            command_version: None,
            input,
        }
    }

    fn target(summary: &str) -> CommentTargetInput {
        CommentTargetInput {
            file_path: "src/example.ts".into(),
            hunk_index: None,
            side: Some(ReviewSide::New),
            line: Some(4),
            summary: summary.into(),
            rationale: None,
            markup: None,
            author: None,
        }
    }

    #[test]
    fn routes_comment_and_batch_ids_and_opens_notes_only_for_effective_reveal() {
        let bridge = create_workdeck_session_bridge(Handlers::default());
        let comment = WorkdeckSessionServerMessage::Comment(message(
            "request-1",
            "comment",
            CommentToolInput {
                target_session: SessionSelector {
                    session_id: Some("session-1".into()),
                    ..SessionSelector::default()
                },
                target: target("Review note"),
                reveal: Some(true),
            },
        ));
        let WorkdeckSessionCommandResult::AppliedComment(result) =
            bridge.dispatch_command(&comment).unwrap()
        else {
            panic!("comment was not routed")
        };
        assert_eq!(result.comment_id, "mcp:request-1");

        let batch = WorkdeckSessionServerMessage::CommentBatch(message(
            "batch-1",
            "comment_batch",
            CommentBatchToolInput {
                target_session: SessionSelector::default(),
                comments: vec![target("First"), target("Second")],
                reveal_mode: Some(CommentBatchRevealMode::First),
            },
        ));
        let WorkdeckSessionCommandResult::AppliedCommentBatch(result) =
            bridge.dispatch_command(&batch).unwrap()
        else {
            panic!("batch was not routed")
        };
        assert_eq!(result.applied[0].comment_id, "batch-1:0");
        assert_eq!(result.applied[1].comment_id, "batch-1:1");
        assert_eq!(bridge.handlers().calls.lock().unwrap().opened, 2);

        let empty = WorkdeckSessionServerMessage::CommentBatch(message(
            "batch-2",
            "comment_batch",
            CommentBatchToolInput {
                target_session: SessionSelector::default(),
                comments: Vec::new(),
                reveal_mode: Some(CommentBatchRevealMode::First),
            },
        ));
        bridge.dispatch_command(&empty).unwrap();
        assert_eq!(bridge.handlers().calls.lock().unwrap().opened, 2);
    }

    #[test]
    fn routes_navigation_reload_remove_clear_highlight_and_clear_highlight_exactly_once() {
        let bridge = create_workdeck_session_bridge(Handlers::default());
        let commands = [
            WorkdeckSessionServerMessage::NavigateToHunk(message(
                "nav",
                "navigate_to_hunk",
                NavigateToHunkToolInput {
                    target_session: SessionSelector::default(),
                    file_path: Some("src/example.ts".into()),
                    hunk_index: Some(2),
                    side: None,
                    line: None,
                    comment_direction: Some(CommentDirection::Next),
                },
            )),
            WorkdeckSessionServerMessage::ReloadSession(message(
                "reload",
                "reload_session",
                ReloadSessionToolInput {
                    target_session: SessionSelector::default(),
                    next_input: serde_json::json!({"kind":"vcs","staged":false,"options":{}}),
                    source_path: Some("/repo".into()),
                },
            )),
            WorkdeckSessionServerMessage::RemoveComment(message(
                "rm",
                "remove_comment",
                RemoveCommentToolInput {
                    target_session: SessionSelector::default(),
                    comment_id: "comment-1".into(),
                },
            )),
            WorkdeckSessionServerMessage::ClearComments(message(
                "clear",
                "clear_comments",
                ClearCommentsToolInput {
                    target_session: SessionSelector::default(),
                    file_path: Some("src/example.ts".into()),
                    include_user: Some(true),
                },
            )),
            WorkdeckSessionServerMessage::Highlight(message(
                "mark",
                "highlight",
                HighlightToolInput {
                    target_session: SessionSelector::default(),
                    file_path: "src/example.ts".into(),
                    side: ReviewSide::New,
                    line: 4,
                    start: 0,
                    end: 4,
                    tone: Some(SessionLineHighlightTone::Warning),
                    reveal: Some(true),
                },
            )),
            WorkdeckSessionServerMessage::ClearHighlights(message(
                "unmark",
                "clear_highlights",
                ClearHighlightsToolInput {
                    target_session: SessionSelector::default(),
                    file_path: Some("src/example.ts".into()),
                },
            )),
        ];
        for command in &commands {
            bridge.dispatch_command(command).unwrap();
        }
        let calls = bridge.handlers().calls.lock().unwrap();
        assert_eq!(calls.navigations, 1);
        assert_eq!(calls.reloads.len(), 1);
        assert_eq!(calls.reloads[0].1.reset_app, Some(false));
        assert_eq!(calls.reloads[0].1.source_path.as_deref(), Some("/repo"));
        assert_eq!(calls.removed, ["comment-1"]);
        assert_eq!(calls.clears, [(Some("src/example.ts".into()), Some(true))]);
        assert_eq!(calls.highlights, 1);
        assert_eq!(calls.highlight_clears, [Some("src/example.ts".into())]);
    }

    fn review_envelope(generation: &str) -> WorkdeckReviewActionEnvelopeV1 {
        WorkdeckReviewActionEnvelopeV1 {
            protocol_version: WORKDECK_REVIEW_PROTOCOL_VERSION,
            generation: generation.into(),
            expected_state_revision: None,
            actor: WorkdeckReviewActorV1 {
                client_id: "agent-1".into(),
                kind: WorkdeckReviewActorKindV1::Agent,
                display_name: None,
            },
            action: WorkdeckReviewActionV1::FilterSet {
                filter: "src".into(),
            },
        }
    }

    #[test]
    fn review_commands_refuse_headless_mounts_and_delegate_when_a_producer_exists() {
        let headless = create_workdeck_session_bridge(Handlers::default());
        let action = WorkdeckSessionServerMessage::ApplyReviewAction(message(
            "action",
            "apply_review_action",
            ApplyReviewActionToolInput {
                target_session: SessionSelector::default(),
                review: review_envelope("generation:none:0"),
            },
        ));
        let WorkdeckSessionCommandResult::ReviewAction(
            crate::WorkdeckReviewActionResultV1::Failed(failure),
        ) = headless.dispatch_command(&action).unwrap()
        else {
            panic!("headless review command did not refuse")
        };
        assert_eq!(failure.code, WorkdeckReviewFailureCodeV1::StaleGeneration);
        assert!(failure.current_generation.is_empty());

        let producer = ReviewProducer::new(
            workdeck_review::PublishReviewInput::default(),
            workdeck_review::ReviewProducerOptions {
                producer_id: Some("bridge".into()),
                ..workdeck_review::ReviewProducerOptions::default()
            },
        )
        .unwrap();
        let handlers = Handlers {
            producer: Some(producer.clone()),
            ..Handlers::default()
        };
        let bridge = create_workdeck_session_bridge(handlers);
        let action = WorkdeckSessionServerMessage::ApplyReviewAction(message(
            "action",
            "apply_review_action",
            ApplyReviewActionToolInput {
                target_session: SessionSelector::default(),
                review: review_envelope(&producer.get_publication().generation),
            },
        ));
        let WorkdeckSessionCommandResult::ReviewAction(
            crate::WorkdeckReviewActionResultV1::Failed(failure),
        ) = bridge.dispatch_command(&action).unwrap()
        else {
            panic!("producer-backed command did not delegate")
        };
        assert_eq!(failure.code, WorkdeckReviewFailureCodeV1::InvalidRequest);

        let read = WorkdeckSessionServerMessage::ReadReviewResource(message(
            "read",
            "read_review_resource",
            ReadReviewResourceToolInput {
                target_session: SessionSelector::default(),
                review: crate::WorkdeckReviewResourceReadEnvelopeV1 {
                    protocol_version: WORKDECK_REVIEW_PROTOCOL_VERSION,
                    actor: WorkdeckReviewActorV1 {
                        client_id: "agent-1".into(),
                        kind: WorkdeckReviewActorKindV1::Agent,
                        display_name: None,
                    },
                    request: workdeck_review::ReadReviewResourceRequest {
                        generation: producer.get_publication().generation.clone(),
                        resource_id: "resource:patch:file:deadbeef".into(),
                        offset: 0,
                        length: 16,
                    },
                },
            },
        ));
        let WorkdeckSessionCommandResult::ReviewResource(
            WorkdeckReviewResourceReadResultV1::Failed(failure),
        ) = bridge.dispatch_command(&read).unwrap()
        else {
            panic!("producer-backed read did not delegate")
        };
        assert_eq!(failure.code, WorkdeckReviewFailureCodeV1::UnknownResource);
    }

    #[test]
    fn generic_broker_envelope_is_typed_and_routed_by_the_native_bridge() {
        let handlers = Handlers::default();
        let calls = Arc::clone(&handlers.calls);
        let bridge = create_workdeck_session_bridge(handlers);
        let result = SessionBrokerConnectionBridge::dispatch_command(
            &bridge,
            SessionServerMessage {
                request_id: "generic-clear".into(),
                command: "clear_highlights".into(),
                command_version: None,
                input: WorkdeckSessionCommandInput::ClearHighlights(ClearHighlightsToolInput {
                    target_session: SessionSelector::default(),
                    file_path: Some("src/example.ts".into()),
                }),
            },
        )
        .unwrap();
        let WorkdeckSessionCommandResult::ClearedHighlights(result) = result else {
            panic!("generic broker command returned the wrong result")
        };
        assert_eq!(result.file_path.as_deref(), Some("src/example.ts"));
        assert_eq!(
            calls.lock().unwrap().highlight_clears,
            [Some("src/example.ts".into())]
        );
        assert!(
            calls.lock().unwrap().queued_results.is_empty(),
            "dispatch alone must not announce that a result was queued"
        );
        SessionBrokerConnectionBridge::command_result_queued(&bridge, "generic-clear");
        assert_eq!(calls.lock().unwrap().queued_results, ["generic-clear"]);
    }

    #[test]
    fn borrowed_handlers_forward_result_queue_notifications_without_dispatch() {
        let handlers = Handlers::default();
        let borrowed = &handlers;
        WorkdeckSessionBridgeHandlers::command_result_queued(&borrowed, "borrowed-result");
        let calls = handlers.calls.lock().unwrap();
        assert_eq!(calls.queued_results, ["borrowed-result"]);
        assert!(calls.comments.is_empty());
        assert!(calls.highlight_clears.is_empty());
    }

    #[test]
    fn generic_dispatch_preserves_handler_errors_verbatim() {
        let handlers = Handlers {
            reload_error: Some("replacement publication was refused".into()),
            ..Handlers::default()
        };
        let error = dispatch_workdeck_session_command(
            &handlers,
            SessionServerMessage {
                request_id: "failed-reload".into(),
                command: "reload_session".into(),
                command_version: None,
                input: WorkdeckSessionCommandInput::ReloadSession(ReloadSessionToolInput {
                    target_session: SessionSelector::default(),
                    next_input: serde_json::json!({"kind":"vcs","staged":false,"options":{}}),
                    source_path: Some("/repo".into()),
                }),
            },
        )
        .unwrap_err();

        assert_eq!(error, "replacement publication was refused");
        assert!(handlers.calls.lock().unwrap().reloads.is_empty());
    }
}
