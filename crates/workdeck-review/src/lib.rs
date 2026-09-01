//! Stateful review navigation built entirely on renderer-neutral models.

mod anchors;
mod annotations;
mod canonical_file;
mod content_manifest;
mod expansion;
mod file_view_plan;
mod generation_order;
mod geometry;
mod live_comments;
mod publication;
mod resource_assembly;
mod resource_store;
mod resources;
mod responsive;
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
pub use content_manifest::*;
pub use expansion::*;
pub use file_view_plan::*;
pub use generation_order::*;
pub use geometry::*;
pub use live_comments::*;
pub use publication::*;
pub use resource_assembly::*;
pub use resource_store::*;
pub use resources::*;
pub use responsive::*;
pub use semantic_actions::*;
pub use semantic_intents::*;
pub use semantic_navigation::*;
pub use semantic_reducer::*;
pub use semantic_selectors::*;
pub use semantic_state::*;
pub use semantic_store::*;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use workdeck_core::{Changeset, LineRange, ReviewSelection, ReviewSide, ReviewSnapshot};

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<workdeck_core::AgentAnnotationConfidence>,
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
            comments: Vec::new(),
        }
    }

    pub fn changeset(&self) -> &Changeset {
        &self.changeset
    }

    pub fn selection(&self) -> ReviewSelection {
        self.selection
    }

    pub fn selected_file(&self) -> Option<&workdeck_core::DiffFile> {
        self.changeset.files.get(self.selection.file_index)
    }

    pub fn layout(&self) -> LayoutMode {
        self.layout
    }

    pub fn set_layout(&mut self, layout: LayoutMode) {
        self.layout = layout;
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
        self.selection = ReviewSelection {
            file_index,
            hunk_index: (!file.hunks.is_empty()).then_some(0),
            side: None,
            line: None,
        };
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
        self.selection = ReviewSelection {
            file_index,
            hunk_index: Some(hunk_index),
            side: None,
            line: None,
        };
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
        self.selection = ReviewSelection {
            file_index,
            hunk_index: Some(hunk_index),
            side: Some(side),
            line: Some(line),
        };
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
        if !self
            .changeset
            .files
            .iter()
            .any(|file| file.key == comment.anchor.file_key)
        {
            return Err(ReviewError::UnknownCommentFile(comment.anchor.file_key));
        }
        self.comments.push(comment);
        Ok(())
    }

    pub fn remove_comment(&mut self, id: &str) -> Option<ReviewComment> {
        let index = self.comments.iter().position(|comment| comment.id == id)?;
        Some(self.comments.remove(index))
    }
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
            title: "Test".into(),
            source: ChangesetSource::WorkingTree { staged: false },
            files,
        }
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
                tags: Vec::new(),
                confidence: None,
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
        assert_eq!(
            state.remove_comment("note-1").unwrap().summary,
            "check this"
        );
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
            tags: Vec::new(),
            confidence: None,
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
