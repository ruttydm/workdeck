//! Stateful review navigation built entirely on renderer-neutral models.

mod anchors;
mod annotations;
mod canonical_file;
mod command_catalog;
mod content_manifest;
mod expansion;
mod file_view_plan;
mod generation_order;
mod geometry;
mod live_comments;
mod producer;
mod publication;
mod resource_assembly;
mod resource_store;
mod resources;
mod responsive;
mod review_note_mapping;
mod semantic_actions;
mod semantic_intents;
mod semantic_navigation;
mod semantic_reducer;
mod semantic_selectors;
mod semantic_state;
mod semantic_store;
#[cfg(test)]
mod semantic_test_support;

pub use anchors::*;
pub use annotations::*;
pub use canonical_file::*;
pub use command_catalog::*;
pub use content_manifest::*;
pub use expansion::*;
pub use file_view_plan::*;
pub use generation_order::*;
pub use geometry::*;
pub use live_comments::*;
pub use producer::*;
pub use publication::*;
pub use resource_assembly::*;
pub use resource_store::*;
pub use resources::*;
pub use responsive::*;
pub use review_note_mapping::*;
pub use semantic_actions::*;
pub use semantic_intents::*;
pub use semantic_navigation::*;
pub(crate) use semantic_reducer::*;
pub use semantic_selectors::*;
pub use semantic_state::*;
pub use semantic_store::*;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use workdeck_core::{
    Changeset, LineRange, ReviewNoteSource, ReviewSelection, ReviewSide, ReviewSnapshot,
    review_file_change_kind,
};
use workdeck_extension_api::{
    ExtensionReviewNoteChange, ExtensionReviewNoteChangeKind, ExtensionReviewNoteResolution,
    ExtensionReviewSnapshot, ExtensionReviewSnapshotFile, ExtensionReviewSnapshotFileFlags,
    ExtensionReviewSnapshotFileStats, ExtensionReviewSnapshotLineAddress,
    ExtensionReviewSnapshotNote, ExtensionReviewSnapshotNoteAnchor,
};

pub const MAX_REVIEW_NOTE_BYTES: usize = 256 * 1024;

/// Measure the entire serialized note, including its JSON keys and framing, in UTF-8 bytes.
pub fn review_note_byte_length(note: &impl Serialize) -> usize {
    serde_json::to_vec(note)
        .expect("review note models are JSON serializable")
        .len()
}

pub fn review_note_within_size_limit(note: &impl Serialize) -> bool {
    review_note_byte_length(note) <= MAX_REVIEW_NOTE_BYTES
}

fn review_note_resolution_is_active(resolution: &ReviewNoteResolution) -> bool {
    *resolution == ReviewNoteResolution::Active
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutMode {
    #[default]
    Auto,
    Split,
    Stack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentAnchor {
    pub file_key: String,
    pub old_range: Option<LineRange>,
    pub new_range: Option<LineRange>,
    pub preferred_side: Option<ReviewSide>,
    pub preferred_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intersecting_hunk_indices: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_hunk_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewComment {
    pub id: String,
    pub parent_id: Option<String>,
    pub source: String,
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<ReviewSide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub summary: String,
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<workdeck_core::AgentAnnotationConfidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "review_note_resolution_is_active")]
    pub resolution: ReviewNoteResolution,
    pub anchor: CommentAnchor,
    pub editable: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReviewError {
    #[error("file index {0} is outside the review")]
    FileOutOfRange(usize),
    #[error("hunk index {hunk} is outside file {file}")]
    HunkOutOfRange { file: usize, hunk: usize },
    #[error("line {line} on the {side:?} side is not in a changed hunk")]
    LineNotInHunk { side: ReviewSide, line: u32 },
    #[error("comment {0:?} already exists")]
    DuplicateComment(String),
    #[error("comment {0:?} does not exist")]
    UnknownComment(String),
    #[error("comment targets an unknown file key {0:?}")]
    UnknownCommentFile(String),
    #[error("invalid live comment target: {0}")]
    InvalidCommentTarget(String),
}

#[derive(Debug, Clone)]
pub struct ReviewState {
    changeset: Changeset,
    selection: ReviewSelection,
    layout: LayoutMode,
    generation: u64,
    state_revision: u64,
    comments: Vec<ReviewComment>,
}

impl ReviewState {
    pub fn new(changeset: Changeset) -> Self {
        let selection = first_selection(&changeset);
        Self {
            changeset,
            selection,
            layout: LayoutMode::Auto,
            generation: 1,
            state_revision: 0,
            comments: Vec::new(),
        }
    }

    pub fn changeset(&self) -> &Changeset {
        &self.changeset
    }

    pub fn selection(&self) -> ReviewSelection {
        self.selection
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn state_revision(&self) -> u64 {
        self.state_revision
    }

    pub fn selected_file(&self) -> Option<&workdeck_core::DiffFile> {
        self.changeset.files.get(self.selection.file_index)
    }

    pub fn layout(&self) -> LayoutMode {
        self.layout
    }

    pub fn set_layout(&mut self, layout: LayoutMode) {
        if self.layout != layout {
            self.layout = layout;
            self.state_revision = self.state_revision.saturating_add(1);
        }
    }

    pub fn resolved_layout(&self, terminal_width: u16) -> LayoutMode {
        resolve_responsive_layout(self.layout, terminal_width).layout
    }

    pub fn responsive_layout(&self, terminal_width: u16) -> ResponsiveLayout {
        resolve_responsive_layout(self.layout, terminal_width)
    }

    pub fn select_file(&mut self, file_index: usize) -> Result<(), ReviewError> {
        let file = self
            .changeset
            .files
            .get(file_index)
            .ok_or(ReviewError::FileOutOfRange(file_index))?;
        let selection = ReviewSelection {
            file_index,
            hunk_index: (!file.hunks.is_empty()).then_some(0),
            side: None,
            line: None,
        };
        if self.selection != selection {
            self.selection = selection;
            self.state_revision = self.state_revision.saturating_add(1);
        }
        Ok(())
    }

    pub fn select_hunk(&mut self, file_index: usize, hunk_index: usize) -> Result<(), ReviewError> {
        let file = self
            .changeset
            .files
            .get(file_index)
            .ok_or(ReviewError::FileOutOfRange(file_index))?;
        if hunk_index >= file.hunks.len() {
            return Err(ReviewError::HunkOutOfRange {
                file: file_index,
                hunk: hunk_index,
            });
        }
        let selection = ReviewSelection {
            file_index,
            hunk_index: Some(hunk_index),
            side: None,
            line: None,
        };
        if self.selection != selection {
            self.selection = selection;
            self.state_revision = self.state_revision.saturating_add(1);
        }
        Ok(())
    }

    pub fn next_file(&mut self) -> bool {
        let next = self.selection.file_index.saturating_add(1);
        if next >= self.changeset.files.len() {
            return false;
        }
        self.select_file(next).expect("validated file index");
        true
    }

    pub fn previous_file(&mut self) -> bool {
        let Some(previous) = self.selection.file_index.checked_sub(1) else {
            return false;
        };
        self.select_file(previous).expect("validated file index");
        true
    }

    pub fn next_hunk(&mut self) -> bool {
        if self.changeset.files.is_empty() {
            return false;
        }
        let file_index = self.selection.file_index;
        let next_hunk = self.selection.hunk_index.map_or(0, |index| index + 1);
        if self
            .changeset
            .files
            .get(file_index)
            .is_some_and(|file| next_hunk < file.hunks.len())
        {
            self.select_hunk(file_index, next_hunk)
                .expect("validated hunk index");
            return true;
        }
        for next_file in file_index + 1..self.changeset.files.len() {
            if !self.changeset.files[next_file].hunks.is_empty() {
                self.select_hunk(next_file, 0)
                    .expect("validated file and hunk indices");
                return true;
            }
        }
        false
    }

    pub fn previous_hunk(&mut self) -> bool {
        if self.changeset.files.is_empty() {
            return false;
        }
        let file_index = self.selection.file_index;
        if let Some(previous) = self
            .selection
            .hunk_index
            .and_then(|index| index.checked_sub(1))
        {
            self.select_hunk(file_index, previous)
                .expect("validated hunk index");
            return true;
        }
        for previous_file in (0..file_index).rev() {
            if let Some(previous_hunk) = self.changeset.files[previous_file]
                .hunks
                .len()
                .checked_sub(1)
            {
                self.select_hunk(previous_file, previous_hunk)
                    .expect("validated file and hunk indices");
                return true;
            }
        }
        false
    }

    pub fn reveal_line(
        &mut self,
        file_index: usize,
        side: ReviewSide,
        line: u32,
    ) -> Result<(), ReviewError> {
        let file = self
            .changeset
            .files
            .get(file_index)
            .ok_or(ReviewError::FileOutOfRange(file_index))?;
        let hunk_index = file
            .hunk_at_line(side, line)
            .ok_or(ReviewError::LineNotInHunk { side, line })?;
        let selection = ReviewSelection {
            file_index,
            hunk_index: Some(hunk_index),
            side: Some(side),
            line: Some(line),
        };
        if self.selection != selection {
            self.selection = selection;
            self.state_revision = self.state_revision.saturating_add(1);
        }
        Ok(())
    }

    pub fn reload(&mut self, changeset: Changeset) {
        let previous_file = self.selected_file().map(|file| {
            (
                file.key.clone(),
                file.path.clone(),
                self.selection.hunk_index,
                self.selection.side,
                self.selection.line,
            )
        });
        self.changeset = changeset;
        self.generation = self.generation.saturating_add(1);
        self.state_revision = 0;
        self.selection = previous_file
            .and_then(|(key, path, hunk, side, line)| {
                let file_index = self
                    .changeset
                    .files
                    .iter()
                    .position(|file| file.key == key)
                    .or_else(|| {
                        self.changeset
                            .files
                            .iter()
                            .position(|file| file.path == path)
                    })?;
                let file = &self.changeset.files[file_index];
                let hunk_index = match (side, line) {
                    (Some(side), Some(line)) => file.hunk_at_line(side, line),
                    _ => hunk.filter(|index| *index < file.hunks.len()),
                };
                Some(ReviewSelection {
                    file_index,
                    hunk_index,
                    side: hunk_index.and(side),
                    line: hunk_index.and(line),
                })
            })
            .unwrap_or_else(|| first_selection(&self.changeset));
    }

    pub fn snapshot(&self) -> ReviewSnapshot {
        ReviewSnapshot {
            generation: self.generation,
            changeset: self.changeset.clone(),
            selection: self.selection,
        }
    }

    pub fn comments(&self) -> &[ReviewComment] {
        &self.comments
    }

    pub fn add_comment(&mut self, comment: ReviewComment) -> Result<(), ReviewError> {
        if self
            .comments
            .iter()
            .any(|existing| existing.id == comment.id)
        {
            return Err(ReviewError::DuplicateComment(comment.id));
        }
        if comment.resolution != ReviewNoteResolution::Orphaned
            && !self
                .changeset
                .files
                .iter()
                .any(|file| file.key == comment.anchor.file_key)
        {
            return Err(ReviewError::UnknownCommentFile(comment.anchor.file_key));
        }
        self.comments.push(comment);
        self.state_revision = self.state_revision.saturating_add(1);
        Ok(())
    }

    pub fn remove_comment(&mut self, id: &str) -> Option<ReviewComment> {
        let index = self.comments.iter().position(|comment| comment.id == id)?;
        let comment = self.comments.remove(index);
        self.state_revision = self.state_revision.saturating_add(1);
        Some(comment)
    }

    pub fn edit_comment_summary(
        &mut self,
        id: &str,
        summary: String,
    ) -> Result<ReviewComment, ReviewError> {
        let comment = self
            .comments
            .iter_mut()
            .find(|comment| comment.id == id)
            .ok_or_else(|| ReviewError::UnknownComment(id.to_owned()))?;
        if comment.summary != summary {
            comment.summary = summary;
            self.state_revision = self.state_revision.saturating_add(1);
        }
        Ok(comment.clone())
    }
}

/// Project every saved note in authoritative arrival/creation order into extension API v1.
#[must_use]
pub fn project_extension_review_notes(state: &ReviewState) -> Vec<ExtensionReviewSnapshotNote> {
    state
        .comments()
        .iter()
        .map(|comment| {
            let resolution = match comment.resolution {
                ReviewNoteResolution::Active => ExtensionReviewNoteResolution::Active,
                ReviewNoteResolution::Stale => ExtensionReviewNoteResolution::Stale,
                ReviewNoteResolution::Orphaned => ExtensionReviewNoteResolution::Orphaned,
            };
            let source = match comment.source.as_str() {
                "ai" => ReviewNoteSource::Ai,
                "user" => ReviewNoteSource::User,
                _ if comment.editable => ReviewNoteSource::User,
                _ => ReviewNoteSource::Agent,
            };
            let original_source = (!matches!(comment.source.as_str(), "ai" | "agent" | "user"))
                .then(|| comment.source.clone());
            ExtensionReviewSnapshotNote {
                id: comment.id.clone(),
                parent_id: comment.parent_id.clone(),
                source,
                original_source,
                file_key: comment.anchor.file_key.clone(),
                anchor: ExtensionReviewSnapshotNoteAnchor {
                    old_range: comment
                        .anchor
                        .old_range
                        .map(|range| [range.start, range.end]),
                    new_range: comment
                        .anchor
                        .new_range
                        .map(|range| [range.start, range.end]),
                    preferred: comment
                        .anchor
                        .preferred_side
                        .zip(comment.anchor.preferred_line)
                        .map(|(side, line)| ExtensionReviewSnapshotLineAddress { side, line }),
                    intersecting_hunk_indices: comment.anchor.intersecting_hunk_indices.clone(),
                    owner_hunk_index: comment.anchor.owner_hunk_index,
                },
                summary: comment.summary.clone(),
                rationale: comment.rationale.clone(),
                markup: comment.markup.clone(),
                title: comment.title.clone(),
                author: comment.author.clone(),
                created_at: comment.created_at.clone(),
                updated_at: comment.updated_at.clone(),
                editable: comment.editable,
                tags: comment.tags.clone(),
                confidence: comment.confidence,
                resolution,
            }
        })
        .collect()
}

/// Diff saved-note snapshots with removals first, then next-list updates and creates.
#[must_use]
pub fn diff_extension_review_notes(
    previous: &[ExtensionReviewSnapshotNote],
    next: &[ExtensionReviewSnapshotNote],
) -> Vec<ExtensionReviewNoteChange> {
    let previous_by_id = previous
        .iter()
        .map(|note| (note.id.as_str(), note))
        .collect::<BTreeMap<_, _>>();
    let next_by_id = next
        .iter()
        .map(|note| (note.id.as_str(), note))
        .collect::<BTreeMap<_, _>>();
    let mut changes = previous
        .iter()
        .filter(|note| !next_by_id.contains_key(note.id.as_str()))
        .cloned()
        .map(|note| ExtensionReviewNoteChange {
            kind: ExtensionReviewNoteChangeKind::Removed,
            note,
        })
        .collect::<Vec<_>>();
    for note in next {
        match previous_by_id.get(note.id.as_str()) {
            None => changes.push(ExtensionReviewNoteChange {
                kind: ExtensionReviewNoteChangeKind::Created,
                note: note.clone(),
            }),
            Some(previous) if *previous != note => changes.push(ExtensionReviewNoteChange {
                kind: ExtensionReviewNoteChangeKind::Updated,
                note: note.clone(),
            }),
            Some(_) => {}
        }
    }
    changes
}

/// Project the current authoritative review and saved notes into extension API v1.
#[must_use]
pub fn build_extension_review_snapshot_with_generation(
    generation: impl Into<String>,
    state: &ReviewState,
) -> ExtensionReviewSnapshot {
    let files = state
        .changeset()
        .files
        .iter()
        .map(|file| ExtensionReviewSnapshotFile {
            file_key: file.key.clone(),
            runtime_id: file.runtime_id.clone(),
            path: file.path.clone(),
            previous_path: file.previous_path.clone(),
            change_kind: review_file_change_kind(file),
            stats: ExtensionReviewSnapshotFileStats {
                additions: file.stats.additions,
                deletions: file.stats.deletions,
                truncated: file.stats.truncated,
            },
            flags: ExtensionReviewSnapshotFileFlags {
                untracked: file.flags.untracked,
                binary: file.flags.binary,
                too_large: file.flags.too_large,
                partial: file.flags.partial,
            },
            content_identity: file.content_identity.clone(),
            source_identity: file.source_identity.clone(),
            source_attested: file.source_identity.as_ref().map(|_| file.source_attested),
        })
        .collect();
    ExtensionReviewSnapshot {
        generation: generation.into(),
        state_revision: state.state_revision(),
        files,
        notes: project_extension_review_notes(state),
    }
}

/// Project the current authoritative review and saved notes into extension API v1.
#[must_use]
pub fn build_extension_review_snapshot(state: &ReviewState) -> ExtensionReviewSnapshot {
    build_extension_review_snapshot_with_generation(
        format!("generation:workdeck-tui:{}", state.generation()),
        state,
    )
}

fn first_selection(changeset: &Changeset) -> ReviewSelection {
    ReviewSelection {
        file_index: 0,
        hunk_index: changeset
            .files
            .first()
            .is_some_and(|file| !file.hunks.is_empty())
            .then_some(0),
        side: None,
        line: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use workdeck_core::{
        ChangesetSource, DiffFile, DiffHunk, FileChangeKind, FileFlags, FileStats,
    };

    fn file(path: &str, key: &str, hunk_count: usize) -> DiffFile {
        DiffFile {
            key: key.into(),
            runtime_id: format!("runtime:{path}"),
            path: path.into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: None,
            stats: FileStats::default(),
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: hunk_count,
            stack_row_count: hunk_count,
            hunks: (0..hunk_count)
                .map(|index| DiffHunk {
                    index,
                    header: format!("@@ -{0} +{0} @@", index + 1),
                    context: None,
                    old_start: index as u32 + 1,
                    old_count: 1,
                    new_start: index as u32 + 1,
                    new_count: 1,
                    split_row_start: index,
                    split_row_count: 1,
                    stack_row_start: index,
                    stack_row_count: 1,
                    lines: vec![],
                })
                .collect(),
            content_identity: key.into(),
            sources: workdeck_core::FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        }
    }

    fn changeset(files: Vec<DiffFile>) -> Changeset {
        Changeset {
            id: "test".into(),
            source_label: "test".into(),
            title: "Test".into(),
            summary: None,
            agent_summary: None,
            source: ChangesetSource::WorkingTree { staged: false },
            files,
        }
    }

    fn comment(id: &str, file_key: &str, summary: &str) -> ReviewComment {
        ReviewComment {
            id: id.into(),
            parent_id: None,
            source: "agent".into(),
            author: None,
            created_at: None,
            file_path: None,
            hunk_index: None,
            side: None,
            line: None,
            summary: summary.into(),
            rationale: None,
            markup: None,
            title: None,
            tags: Vec::new(),
            confidence: None,
            updated_at: None,
            resolution: ReviewNoteResolution::Active,
            anchor: CommentAnchor {
                file_key: file_key.into(),
                old_range: None,
                new_range: None,
                preferred_side: None,
                preferred_line: None,
                intersecting_hunk_indices: Vec::new(),
                owner_hunk_index: None,
            },
            editable: false,
        }
    }

    #[test]
    fn extension_review_snapshot_copies_complete_saved_notes_and_authoritative_file_identity() {
        let mut alpha = file("src/alpha.ts", "alpha", 1);
        alpha.runtime_id = "alpha".into();
        alpha.previous_path = Some("src/old-alpha.ts".into());
        alpha.change_kind = FileChangeKind::Renamed;
        alpha.stats = FileStats {
            additions: 2,
            deletions: 2,
            truncated: false,
        };
        alpha.content_identity = "sha256:alpha".into();
        alpha.source_identity = Some("git:alpha".into());
        alpha.source_attested = true;
        let mut state = ReviewState::new(changeset(vec![alpha]));

        let mut live = comment("live:1", "alpha", "Check this edge case.");
        live.source = "mcp".into();
        live.rationale = Some("The fallback changes behavior.".into());
        live.tags = vec!["correctness".into()];
        live.confidence = Some(workdeck_core::AgentAnnotationConfidence::High);
        live.created_at = Some("2026-08-19T12:00:00.000Z".into());
        live.resolution = ReviewNoteResolution::Stale;
        live.anchor.new_range = Some(LineRange { start: 2, end: 2 });
        live.anchor.preferred_side = Some(ReviewSide::New);
        live.anchor.preferred_line = Some(2);
        live.anchor.intersecting_hunk_indices = vec![0];
        live.anchor.owner_hunk_index = Some(0);
        state.add_comment(live).unwrap();

        let mut user = comment(
            "user:1",
            "retired-file",
            "Keep this even when its file disappears.",
        );
        user.source = "user".into();
        user.markup = Some("<b>Keep this</b>".into());
        user.title = Some("Retired finding".into());
        user.author = Some("reviewer".into());
        user.updated_at = Some("2026-08-19T13:00:00.000Z".into());
        user.resolution = ReviewNoteResolution::Orphaned;
        user.anchor.old_range = Some(LineRange { start: 4, end: 4 });
        user.anchor.preferred_side = Some(ReviewSide::Old);
        user.anchor.preferred_line = Some(4);
        user.anchor.owner_hunk_index = Some(0);
        user.editable = true;
        state.add_comment(user).unwrap();
        state.state_revision = 7;

        let unsaved_draft = comment("draft:1", "alpha", "unfinished");
        let snapshot = build_extension_review_snapshot_with_generation("producer:4", &state);
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            serde_json::json!({
                "generation": "producer:4",
                "stateRevision": 7,
                "files": [{
                    "fileKey": "alpha",
                    "runtimeId": "alpha",
                    "path": "src/alpha.ts",
                    "previousPath": "src/old-alpha.ts",
                    "changeKind": "rename-changed",
                    "stats": { "additions": 2, "deletions": 2, "truncated": false },
                    "flags": { "untracked": false, "binary": false, "tooLarge": false, "partial": false },
                    "contentIdentity": "sha256:alpha",
                    "sourceIdentity": "git:alpha",
                    "sourceAttested": true
                }],
                "notes": [
                    {
                        "id": "live:1",
                        "source": "agent",
                        "originalSource": "mcp",
                        "fileKey": "alpha",
                        "anchor": {
                            "newRange": [2, 2],
                            "preferred": { "side": "new", "line": 2 },
                            "intersectingHunkIndices": [0],
                            "ownerHunkIndex": 0
                        },
                        "summary": "Check this edge case.",
                        "rationale": "The fallback changes behavior.",
                        "createdAt": "2026-08-19T12:00:00.000Z",
                        "editable": false,
                        "tags": ["correctness"],
                        "confidence": "high",
                        "resolution": "stale"
                    },
                    {
                        "id": "user:1",
                        "source": "user",
                        "fileKey": "retired-file",
                        "anchor": {
                            "oldRange": [4, 4],
                            "preferred": { "side": "old", "line": 4 },
                            "intersectingHunkIndices": [],
                            "ownerHunkIndex": 0
                        },
                        "summary": "Keep this even when its file disappears.",
                        "markup": "<b>Keep this</b>",
                        "title": "Retired finding",
                        "author": "reviewer",
                        "updatedAt": "2026-08-19T13:00:00.000Z",
                        "editable": true,
                        "resolution": "orphaned"
                    }
                ]
            })
        );
        assert!(
            snapshot
                .notes
                .iter()
                .all(|note| note.id != unsaved_draft.id)
        );
    }

    #[test]
    fn extension_review_snapshot_owns_deep_data_without_mutating_review_state() {
        let mut state = ReviewState::new(changeset(vec![file("a", "alpha", 1)]));
        let mut entry = comment("live:1", "alpha", "before");
        entry.parent_id = Some("parent:1".into());
        entry.tags = vec!["one".into()];
        entry.anchor.new_range = Some(LineRange { start: 2, end: 2 });
        entry.anchor.preferred_side = Some(ReviewSide::New);
        entry.anchor.preferred_line = Some(2);
        entry.anchor.intersecting_hunk_indices = vec![0];
        state.add_comment(entry).unwrap();

        let mut snapshot = build_extension_review_snapshot(&state);
        snapshot.notes[0].tags.push("two".into());
        snapshot.notes[0].anchor.intersecting_hunk_indices.push(9);
        snapshot.files[0].path = "mutated".into();
        assert_eq!(snapshot.notes[0].parent_id.as_deref(), Some("parent:1"));
        assert_eq!(state.comments()[0].tags, ["one"]);
        assert_eq!(state.comments()[0].anchor.intersecting_hunk_indices, [0]);
        assert_eq!(state.changeset().files[0].path, "a");
    }

    #[test]
    fn extension_review_note_diff_reports_removed_updated_and_created_in_stable_order() {
        let mut previous = ReviewState::new(changeset(vec![file("a", "alpha", 1)]));
        for entry in [
            comment("keep", "alpha", "unchanged"),
            comment("edit", "alpha", "before"),
            comment("gone", "alpha", "leave"),
        ] {
            previous.add_comment(entry).unwrap();
        }
        let mut next = ReviewState::new(changeset(vec![file("a", "alpha", 1)]));
        for entry in [
            comment("keep", "alpha", "unchanged"),
            comment("edit", "alpha", "after"),
            comment("new", "alpha", "arrive"),
        ] {
            next.add_comment(entry).unwrap();
        }
        let previous = project_extension_review_notes(&previous);
        let next = project_extension_review_notes(&next);

        let changes = diff_extension_review_notes(&previous, &next);
        assert_eq!(
            changes,
            [
                ExtensionReviewNoteChange {
                    kind: ExtensionReviewNoteChangeKind::Removed,
                    note: previous[2].clone(),
                },
                ExtensionReviewNoteChange {
                    kind: ExtensionReviewNoteChangeKind::Updated,
                    note: next[1].clone(),
                },
                ExtensionReviewNoteChange {
                    kind: ExtensionReviewNoteChangeKind::Created,
                    note: next[2].clone(),
                }
            ]
        );
        assert_eq!(
            serde_json::to_value(&changes).unwrap(),
            serde_json::json!([
                { "kind": "removed", "note": previous[2] },
                { "kind": "updated", "note": next[1] },
                { "kind": "created", "note": next[2] }
            ])
        );
    }

    #[test]
    fn frozen_hunk_extension_review_snapshot_oracle_records_both_pinned_trees() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/extension-review-snapshot.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert_eq!(
            baselines[0]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(baselines[0]["passed"], 3);
        assert_eq!(baselines[0]["failed"], 0);
        assert_eq!(
            baselines[1]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(baselines[1]["passed"], 2);
        assert_eq!(baselines[1]["failed"], 0);
        assert_eq!(oracle["test_mapping"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn hunk_navigation_crosses_file_boundaries() {
        let mut state = ReviewState::new(changeset(vec![file("a", "a", 2), file("b", "b", 1)]));
        assert_eq!(state.selection().hunk_index, Some(0));
        assert!(state.next_hunk());
        assert_eq!(state.selection().hunk_index, Some(1));
        assert!(state.next_hunk());
        assert_eq!(state.selection().file_index, 1);
        assert_eq!(state.selection().hunk_index, Some(0));
        assert!(!state.next_hunk());
        assert!(state.previous_hunk());
        assert_eq!(state.selection().file_index, 0);
        assert_eq!(state.selection().hunk_index, Some(1));
    }

    #[test]
    fn reload_reconciles_by_semantic_key_before_path() {
        let mut state = ReviewState::new(changeset(vec![file("old", "stable", 1)]));
        state.reload(changeset(vec![file("renamed", "stable", 2)]));
        assert_eq!(state.selection().file_index, 0);
        assert_eq!(state.selection().hunk_index, Some(0));
        assert_eq!(state.selected_file().unwrap().path, "renamed");
        assert_eq!(state.snapshot().generation, 2);
    }

    #[test]
    fn removes_live_comments_by_stable_id() {
        let mut state = ReviewState::new(changeset(vec![file("a", "stable", 1)]));
        state
            .add_comment(ReviewComment {
                id: "note-1".into(),
                parent_id: None,
                source: "agent".into(),
                author: None,
                created_at: None,
                file_path: None,
                hunk_index: None,
                side: None,
                line: None,
                summary: "check this".into(),
                rationale: None,
                markup: None,
                title: None,
                tags: Vec::new(),
                confidence: None,
                updated_at: None,
                resolution: ReviewNoteResolution::Active,
                anchor: CommentAnchor {
                    file_key: "stable".into(),
                    old_range: None,
                    new_range: None,
                    preferred_side: None,
                    preferred_line: None,
                    intersecting_hunk_indices: Vec::new(),
                    owner_hunk_index: None,
                },
                editable: false,
            })
            .unwrap();
        let revision = state.state_revision();
        assert_eq!(
            state
                .edit_comment_summary("note-1", "updated".into())
                .unwrap()
                .summary,
            "updated"
        );
        assert_eq!(state.state_revision(), revision + 1);
        assert_eq!(
            state.edit_comment_summary("missing", "nope".into()),
            Err(ReviewError::UnknownComment("missing".into()))
        );
        assert_eq!(state.remove_comment("note-1").unwrap().summary, "updated");
        assert!(state.remove_comment("note-1").is_none());
    }

    #[test]
    fn auto_layout_uses_stack_in_narrow_terminals() {
        let state = ReviewState::new(changeset(vec![]));
        assert_eq!(state.resolved_layout(119), LayoutMode::Stack);
        assert_eq!(state.resolved_layout(120), LayoutMode::Split);
    }

    #[test]
    fn note_size_counts_whole_json_and_multibyte_text() {
        let mut comment = ReviewComment {
            id: "note:1".into(),
            parent_id: None,
            source: "agent".into(),
            author: None,
            created_at: None,
            file_path: None,
            hunk_index: None,
            side: None,
            line: None,
            summary: String::new(),
            rationale: None,
            markup: None,
            title: None,
            tags: Vec::new(),
            confidence: None,
            updated_at: None,
            resolution: ReviewNoteResolution::Active,
            anchor: CommentAnchor {
                file_key: "file:one".into(),
                old_range: None,
                new_range: None,
                preferred_side: None,
                preferred_line: None,
                intersecting_hunk_indices: Vec::new(),
                owner_hunk_index: None,
            },
            editable: false,
        };
        let empty = review_note_byte_length(&comment);
        comment.summary = "abc".into();
        assert_eq!(review_note_byte_length(&comment), empty + 3);
        comment.summary = "🧪".repeat(MAX_REVIEW_NOTE_BYTES / 4);
        assert!(!review_note_within_size_limit(&comment));
    }
}
