//! Stable author-facing models adapted from Hunk's public extension facade.
//!
//! Hunk passed callback-bearing TypeScript objects in process. Workdeck keeps the same public
//! facts and actions but moves behavior across the native JSON-RPC boundary, so this module owns
//! the renderer-free values an extension can safely compile against.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ExtensionDiffFile, ExtensionFileSide, ExtensionReviewNoteChange, ExtensionReviewSnapshotNote,
};

/// Detection priority used by Workdeck's ordinary bundled backend.
pub const WORKDECK_VCS_DETECTION_BASELINE_PRIORITY: i32 = 0;
/// Deprecated compatibility name for the baseline priority.
pub const WORKDECK_CORE_VCS_DETECTION_PRIORITY: i32 = WORKDECK_VCS_DETECTION_BASELINE_PRIORITY;
/// Priority assigned to a native adapter that does not select one explicitly.
pub const WORKDECK_DEFAULT_VCS_DETECTION_PRIORITY: i32 = -100;

/// Provider-neutral changeset projection exposed to native extensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionChangeset {
    pub id: String,
    pub source_label: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_summary: Option<String>,
    pub files: Vec<ExtensionDiffFile>,
}

/// User-selected layout policy reported to extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionLayoutMode {
    Auto,
    Split,
    Stack,
}

/// Concrete layout selected after responsive resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionResolvedLayout {
    Split,
    Stack,
}

/// Why a live review was rebuilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionReloadReason {
    Watch,
    Daemon,
    Manual,
}

/// User-note projection carried by draft and save lifecycle events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewNote {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub file_id: String,
    pub file_path: String,
    pub hunk_index: usize,
    pub side: ExtensionFileSide,
    pub line: u32,
    pub body: String,
    pub draft: bool,
}

/// Closed lifecycle-event domain from Hunk's API v15, expressed as owned native values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionLifecycleEvent {
    Startup {
        cwd: PathBuf,
    },
    ChangesetLoaded {
        changeset: ExtensionChangeset,
    },
    CommandExecuted {
        command_id: String,
    },
    SelectionChanged {
        file_id: Option<String>,
        hunk_index: Option<usize>,
    },
    FileViewed {
        file: ExtensionDiffFile,
        hunk_index: Option<usize>,
    },
    HunkViewed {
        file: ExtensionDiffFile,
        hunk_index: usize,
    },
    FilterChanged {
        filter: String,
    },
    ThemeChanged {
        theme_id: String,
    },
    LayoutChanged {
        mode: ExtensionLayoutMode,
        layout: ExtensionResolvedLayout,
    },
    WatchReloadPending,
    NoteCreated {
        note: ExtensionReviewNote,
    },
    NoteEdited {
        note: ExtensionReviewNote,
    },
    NoteChanged {
        change: ExtensionReviewNoteChange,
    },
    SessionReload {
        changeset: ExtensionChangeset,
        reason: SessionReloadReason,
    },
    Shutdown,
}

impl ExtensionLifecycleEvent {
    /// Split an event into the exact lifecycle name and JSON payload used by native callbacks.
    pub fn into_parts(self) -> (String, Value) {
        match self {
            Self::Startup { cwd } => ("startup".into(), serde_json::json!({ "cwd": cwd })),
            Self::ChangesetLoaded { changeset } => (
                "changeset_loaded".into(),
                serde_json::json!({ "changeset": changeset }),
            ),
            Self::CommandExecuted { command_id } => (
                "command_executed".into(),
                serde_json::json!({ "commandId": command_id }),
            ),
            Self::SelectionChanged {
                file_id,
                hunk_index,
            } => (
                "selection_changed".into(),
                serde_json::json!({ "fileId": file_id, "hunkIndex": hunk_index }),
            ),
            Self::FileViewed { file, hunk_index } => (
                "file_viewed".into(),
                serde_json::json!({ "file": file, "hunkIndex": hunk_index }),
            ),
            Self::HunkViewed { file, hunk_index } => (
                "hunk_viewed".into(),
                serde_json::json!({ "file": file, "hunkIndex": hunk_index }),
            ),
            Self::FilterChanged { filter } => (
                "filter_changed".into(),
                serde_json::json!({ "filter": filter }),
            ),
            Self::ThemeChanged { theme_id } => (
                "theme_changed".into(),
                serde_json::json!({ "themeId": theme_id }),
            ),
            Self::LayoutChanged { mode, layout } => (
                "layout_changed".into(),
                serde_json::json!({ "mode": mode, "layout": layout }),
            ),
            Self::WatchReloadPending => ("watch_reload_pending".into(), serde_json::json!({})),
            Self::NoteCreated { note } => {
                ("note_created".into(), serde_json::json!({ "note": note }))
            }
            Self::NoteEdited { note } => {
                ("note_edited".into(), serde_json::json!({ "note": note }))
            }
            Self::NoteChanged { change } => (
                "note_changed".into(),
                serde_json::to_value(change).expect("review-note changes are serializable"),
            ),
            Self::SessionReload { changeset, reason } => (
                "session_reload".into(),
                serde_json::json!({ "changeset": changeset, "reason": reason }),
            ),
            Self::Shutdown => ("shutdown".into(), serde_json::json!({})),
        }
    }
}

/// Stable export retained for extension consumers that only need the saved-note shape.
pub type ExtensionSavedReviewNote = ExtensionReviewSnapshotNote;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExtensionDiffStats, ExtensionReviewNoteChangeKind, ExtensionReviewNoteResolution};
    use workdeck_core::ReviewNoteSource;

    fn file() -> ExtensionDiffFile {
        ExtensionDiffFile {
            id: "run:alpha".into(),
            path: "src/alpha.rs".into(),
            previous_path: None,
            patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
            language: Some("rust".into()),
            stats: ExtensionDiffStats {
                additions: 1,
                deletions: 1,
            },
            metadata: serde_json::json!({ "hunks": [] }),
            change_type: Some(crate::ExtensionVcsFileChangeType::Change),
            stats_truncated: false,
            hunks: Vec::new(),
            agent: None,
            is_untracked: false,
            is_binary: false,
            is_too_large: false,
        }
    }

    #[test]
    fn lifecycle_event_names_and_payloads_cover_the_complete_hunk_contract() {
        let changeset = ExtensionChangeset {
            id: "review:1".into(),
            source_label: "working tree".into(),
            title: "Changes".into(),
            summary: None,
            agent_summary: None,
            files: vec![file()],
        };
        let note = ExtensionReviewNote {
            id: "user:1".into(),
            parent_id: None,
            file_id: "run:alpha".into(),
            file_path: "src/alpha.rs".into(),
            hunk_index: 0,
            side: ExtensionFileSide::New,
            line: 1,
            body: "Explain this".into(),
            draft: false,
        };
        let saved_note = crate::ExtensionReviewSnapshotNote {
            id: "user:1".into(),
            parent_id: None,
            source: ReviewNoteSource::User,
            original_source: None,
            file_key: "alpha".into(),
            anchor: crate::ExtensionReviewSnapshotNoteAnchor {
                old_range: None,
                new_range: Some([1, 1]),
                preferred: None,
                intersecting_hunk_indices: vec![0],
                owner_hunk_index: Some(0),
            },
            summary: "Explain this".into(),
            rationale: None,
            markup: None,
            title: None,
            author: None,
            created_at: None,
            updated_at: None,
            editable: true,
            tags: Vec::new(),
            confidence: None,
            resolution: ExtensionReviewNoteResolution::Active,
        };
        let events = vec![
            ExtensionLifecycleEvent::Startup {
                cwd: "/repo".into(),
            },
            ExtensionLifecycleEvent::ChangesetLoaded {
                changeset: changeset.clone(),
            },
            ExtensionLifecycleEvent::CommandExecuted {
                command_id: "workdeck.review.nextHunk".into(),
            },
            ExtensionLifecycleEvent::SelectionChanged {
                file_id: Some("run:alpha".into()),
                hunk_index: Some(0),
            },
            ExtensionLifecycleEvent::FileViewed {
                file: file(),
                hunk_index: Some(0),
            },
            ExtensionLifecycleEvent::HunkViewed {
                file: file(),
                hunk_index: 0,
            },
            ExtensionLifecycleEvent::FilterChanged {
                filter: "src/".into(),
            },
            ExtensionLifecycleEvent::ThemeChanged {
                theme_id: "github-light-default".into(),
            },
            ExtensionLifecycleEvent::LayoutChanged {
                mode: ExtensionLayoutMode::Auto,
                layout: ExtensionResolvedLayout::Split,
            },
            ExtensionLifecycleEvent::WatchReloadPending,
            ExtensionLifecycleEvent::NoteCreated { note: note.clone() },
            ExtensionLifecycleEvent::NoteEdited { note },
            ExtensionLifecycleEvent::NoteChanged {
                change: ExtensionReviewNoteChange {
                    kind: ExtensionReviewNoteChangeKind::Created,
                    note: saved_note,
                },
            },
            ExtensionLifecycleEvent::SessionReload {
                changeset,
                reason: SessionReloadReason::Manual,
            },
            ExtensionLifecycleEvent::Shutdown,
        ];
        let parts = events
            .into_iter()
            .map(ExtensionLifecycleEvent::into_parts)
            .collect::<Vec<_>>();
        assert_eq!(
            parts
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            crate::LIFECYCLE_EVENT_NAMES
        );
        assert_eq!(
            parts[2].1,
            serde_json::json!({ "commandId": "workdeck.review.nextHunk" })
        );
        assert_eq!(
            parts[8].1,
            serde_json::json!({ "mode": "auto", "layout": "split" })
        );
        assert_eq!(parts[13].1["reason"], "manual");
    }

    #[test]
    fn public_changeset_and_detection_priorities_keep_the_pinned_values() {
        assert_eq!(WORKDECK_VCS_DETECTION_BASELINE_PRIORITY, 0);
        assert_eq!(WORKDECK_CORE_VCS_DETECTION_PRIORITY, 0);
        assert_eq!(WORKDECK_DEFAULT_VCS_DETECTION_PRIORITY, -100);
        let changeset = ExtensionChangeset {
            id: "review:1".into(),
            source_label: "patch".into(),
            title: "Patch".into(),
            summary: Some("commit prose".into()),
            agent_summary: Some("agent prose".into()),
            files: vec![file()],
        };
        let value = serde_json::to_value(changeset).unwrap();
        assert_eq!(value["sourceLabel"], "patch");
        assert_eq!(value["agentSummary"], "agent prose");
    }
}
