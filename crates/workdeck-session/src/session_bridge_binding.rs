//! Native lifecycle binding between one mounted review and the local session client.
//!
//! This is a clean-room Rust port of Hunk's `useHunkSessionBridge` hook at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. React effect lifetimes become an
//! RAII attachment, while snapshot projection remains explicit and renderer-neutral.

use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use workdeck_core::{DiffFile, DiffHunk};
use workdeck_review::{ReviewPublicationAddress, review_hunk_ranges};

use crate::{
    SessionBrokerClientError, SessionLiveCommentSummary, SessionReviewNoteSummary,
    WorkdeckSessionAppBridge, WorkdeckSessionBrokerClient, WorkdeckSessionSnapshot,
    WorkdeckSessionState,
};

/// The two host-client operations owned by the mounted-review lifecycle.
pub trait WorkdeckSessionBridgeHost: Send + Sync + 'static {
    fn set_bridge(&self, bridge: Option<Arc<WorkdeckSessionAppBridge>>);

    fn update_snapshot(
        &self,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError>;
}

impl WorkdeckSessionBridgeHost for WorkdeckSessionBrokerClient {
    fn set_bridge(&self, bridge: Option<Arc<WorkdeckSessionAppBridge>>) {
        WorkdeckSessionBrokerClient::set_bridge(self, bridge);
    }

    fn update_snapshot(
        &self,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError> {
        WorkdeckSessionBrokerClient::update_snapshot(self, snapshot)
    }
}

/// State dependencies that cause Hunk's source hook to publish a new daemon snapshot.
#[derive(Debug, Clone)]
pub struct SessionBridgeSnapshotFacts<'a> {
    pub selected_file: Option<&'a DiffFile>,
    pub selected_hunk: Option<&'a DiffHunk>,
    pub selected_hunk_index: usize,
    pub show_agent_notes: bool,
    pub note_markup_width: Option<u64>,
    pub live_comment_count: u64,
    pub live_comments: Vec<SessionLiveCommentSummary>,
    pub review_note_count: u64,
    pub review_notes: Vec<SessionReviewNoteSummary>,
    /// The producer publication is independent from the review store's revision.
    pub publication_generation: Option<&'a str>,
    pub review_state_revision: u64,
}

/// Project the exact broker snapshot published by the mounted review hook.
#[must_use]
pub fn project_session_bridge_snapshot(
    facts: SessionBridgeSnapshotFacts<'_>,
    updated_at: impl Into<String>,
) -> WorkdeckSessionSnapshot {
    let selected_ranges = facts.selected_hunk.map(review_hunk_ranges);
    WorkdeckSessionSnapshot {
        updated_at: updated_at.into(),
        state: WorkdeckSessionState {
            selected_file_id: facts.selected_file.map(|file| file.runtime_id.clone()),
            selected_file_path: facts.selected_file.map(|file| file.path.clone()),
            selected_hunk_index: u64::try_from(facts.selected_hunk_index).unwrap_or(u64::MAX),
            selected_hunk_old_range: selected_ranges
                .map(|(old, _)| [u64::from(old.start), u64::from(old.end)]),
            selected_hunk_new_range: selected_ranges
                .map(|(_, new)| [u64::from(new.start), u64::from(new.end)]),
            show_agent_notes: facts.show_agent_notes,
            note_markup_width: facts.note_markup_width,
            live_comment_count: facts.live_comment_count,
            live_comments: facts.live_comments,
            review_note_count: Some(facts.review_note_count),
            review_notes: Some(facts.review_notes),
            review_publication: facts.publication_generation.map(|generation| {
                ReviewPublicationAddress {
                    generation: generation.to_owned(),
                    state_revision: facts.review_state_revision,
                }
            }),
        },
    }
}

/// One attached bridge. Dropping it performs the source hook's effect cleanup.
pub struct WorkdeckSessionBridgeBinding {
    host: Option<Arc<dyn WorkdeckSessionBridgeHost>>,
}

impl std::fmt::Debug for WorkdeckSessionBridgeBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkdeckSessionBridgeBinding")
            .field("attached", &self.host.is_some())
            .finish()
    }
}

impl WorkdeckSessionBridgeBinding {
    /// Attach the stable app bridge when a live host client exists.
    #[must_use]
    pub fn attach(
        host: Option<Arc<dyn WorkdeckSessionBridgeHost>>,
        bridge: Arc<WorkdeckSessionAppBridge>,
    ) -> Self {
        if let Some(host) = &host {
            host.set_bridge(Some(bridge));
        }
        Self { host }
    }

    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.host.is_some()
    }

    /// Publish current review state with an ISO timestamp, if this mount has a host.
    pub fn update_snapshot(
        &self,
        facts: SessionBridgeSnapshotFacts<'_>,
    ) -> Result<(), SessionBrokerClientError> {
        let Some(host) = &self.host else {
            return Ok(());
        };
        host.update_snapshot(project_session_bridge_snapshot(
            facts,
            Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        ))
    }
}

impl Drop for WorkdeckSessionBridgeBinding {
    fn drop(&mut self) {
        if let Some(host) = &self.host {
            host.set_bridge(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use workdeck_core::{
        DiffHunk, DiffLine, DiffLineKind, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats,
    };

    use super::*;
    use crate::{
        ClearedHighlightsResult, SessionServerMessage, WorkdeckSessionCommandInput,
        WorkdeckSessionCommandResult,
    };

    #[derive(Default)]
    struct HostState {
        attachments: Vec<bool>,
        snapshots: Vec<WorkdeckSessionSnapshot>,
    }

    #[derive(Default)]
    struct MockHost {
        state: Mutex<HostState>,
    }

    fn oracle() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/hunk-session-bridge-hook.json"
        ))
        .unwrap()
    }

    impl WorkdeckSessionBridgeHost for MockHost {
        fn set_bridge(&self, bridge: Option<Arc<WorkdeckSessionAppBridge>>) {
            self.state
                .lock()
                .unwrap()
                .attachments
                .push(bridge.is_some());
        }

        fn update_snapshot(
            &self,
            snapshot: WorkdeckSessionSnapshot,
        ) -> Result<(), SessionBrokerClientError> {
            self.state.lock().unwrap().snapshots.push(snapshot);
            Ok(())
        }
    }

    fn file() -> DiffFile {
        DiffFile {
            key: "file:alpha".into(),
            runtime_id: "alpha".into(),
            path: "alpha.ts".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("typescript".into()),
            stats: FileStats {
                additions: 2,
                deletions: 1,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: 2,
            stack_row_count: 3,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -1 +1,2 @@".into(),
                context: None,
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 2,
                split_row_start: 0,
                split_row_count: 2,
                stack_row_start: 0,
                stack_row_count: 3,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        content: "export const alpha = 1;".into(),
                        old_line: Some(1),
                        new_line: None,
                        moved: false,
                        no_newline_at_eof: false,
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        content: "export const alpha = 2;".into(),
                        old_line: None,
                        new_line: Some(1),
                        moved: false,
                        no_newline_at_eof: false,
                    },
                ],
            }],
            content_identity: "content:alpha".into(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        }
    }

    fn facts(file: &DiffFile) -> SessionBridgeSnapshotFacts<'_> {
        SessionBridgeSnapshotFacts {
            selected_file: Some(file),
            selected_hunk: file.hunks.first(),
            selected_hunk_index: 0,
            show_agent_notes: true,
            note_markup_width: None,
            live_comment_count: 0,
            live_comments: Vec::new(),
            review_note_count: 0,
            review_notes: Vec::new(),
            publication_generation: Some("generation:oracle:0"),
            review_state_revision: 0,
        }
    }

    #[test]
    fn projects_the_frozen_hunk_oracle_snapshot() {
        let file = file();
        let snapshot = project_session_bridge_snapshot(facts(&file), "<iso>");
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            oracle()["directOracle"]["output"]["latest"]
        );
    }

    #[test]
    fn preserves_absent_selection_and_publication_without_inventing_fields() {
        let snapshot = project_session_bridge_snapshot(
            SessionBridgeSnapshotFacts {
                selected_file: None,
                selected_hunk: None,
                selected_hunk_index: 0,
                show_agent_notes: false,
                note_markup_width: Some(72),
                live_comment_count: 0,
                live_comments: Vec::new(),
                review_note_count: 0,
                review_notes: Vec::new(),
                publication_generation: None,
                review_state_revision: 99,
            },
            "<iso>",
        );
        assert_eq!(snapshot.state.selected_file_id, None);
        assert_eq!(snapshot.state.selected_hunk_old_range, None);
        assert_eq!(snapshot.state.review_publication, None);
        assert_eq!(snapshot.state.note_markup_width, Some(72));
    }

    #[test]
    fn attaches_publishes_and_detaches_with_raii_lifetime() {
        let host = Arc::new(MockHost::default());
        let erased_host: Arc<dyn WorkdeckSessionBridgeHost> = host.clone();
        let bridge: Arc<WorkdeckSessionAppBridge> = Arc::new(
            |_message: SessionServerMessage<String, WorkdeckSessionCommandInput>| {
                Ok(WorkdeckSessionCommandResult::ClearedHighlights(
                    ClearedHighlightsResult {
                        removed_count: 0,
                        remaining_count: 0,
                        file_path: None,
                    },
                ))
            },
        );
        let file = file();
        {
            let binding = WorkdeckSessionBridgeBinding::attach(Some(erased_host), bridge);
            assert!(binding.is_attached());
            binding.update_snapshot(facts(&file)).unwrap();
            let state = host.state.lock().unwrap();
            assert_eq!(state.attachments, [true]);
            assert_eq!(state.snapshots.len(), 1);
        }
        assert_eq!(host.state.lock().unwrap().attachments, [true, false]);
    }

    #[test]
    fn no_host_is_a_noop_and_never_requires_a_fake_snapshot_sink() {
        let bridge: Arc<WorkdeckSessionAppBridge> = Arc::new(
            |_message: SessionServerMessage<String, WorkdeckSessionCommandInput>| {
                Err("unused".into())
            },
        );
        let binding = WorkdeckSessionBridgeBinding::attach(None, bridge);
        assert!(!binding.is_attached());
        binding.update_snapshot(facts(&file())).unwrap();
    }

    #[test]
    fn typed_bridge_trait_object_accepts_the_generic_broker_envelope() {
        let bridge: Arc<WorkdeckSessionAppBridge> = Arc::new(
            |message: SessionServerMessage<String, WorkdeckSessionCommandInput>| {
                assert_eq!(message.command, "clear_highlights");
                assert!(matches!(
                    message.input,
                    WorkdeckSessionCommandInput::ClearHighlights(_)
                ));
                Ok(WorkdeckSessionCommandResult::ClearedHighlights(
                    ClearedHighlightsResult {
                        removed_count: 0,
                        remaining_count: 0,
                        file_path: Some("alpha.ts".into()),
                    },
                ))
            },
        );
        let result = bridge
            .dispatch_command(SessionServerMessage {
                request_id: "oracle".into(),
                command: "clear_highlights".into(),
                command_version: None,
                input: WorkdeckSessionCommandInput::ClearHighlights(
                    crate::ClearHighlightsToolInput {
                        target_session: crate::SessionSelector::default(),
                        file_path: Some("alpha.ts".into()),
                    },
                ),
            })
            .unwrap();
        let WorkdeckSessionCommandResult::ClearedHighlights(result) = result else {
            panic!("wrong result")
        };
        assert_eq!(result.file_path.as_deref(), Some("alpha.ts"));
    }
}
