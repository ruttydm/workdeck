//! Lossless mapping between semantic review notes and terminal review rows.
//!
//! The semantic store addresses files by stable key and notes by range anchor. The
//! terminal addresses files by invocation-local id and draws notes beside a concrete
//! line. Keeping every conversion here prevents individual panes from silently
//! dropping fields or inventing their own hunk ownership rules.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use workdeck_core::{
    AgentAnnotation, AgentAnnotationConfidence, DiffFile, DiffHunk, LineRange, ReviewNoteSource,
    ReviewSide, SemanticReviewLineAddress, SemanticReviewNote, SemanticReviewRangeAnchor,
};

use crate::{
    ReviewComment, ReviewDraftKind, ReviewDraftNote, ReviewError, ReviewLineTarget,
    ReviewNoteResolution, ReviewStoredNote, ReviewThreadedStoredNote,
    ReviewVisibleThreadedStoredNote, is_renderable_stored_review_note, review_line_anchor,
    review_note_anchor_line, review_note_owner_hunk_index,
};

const UNKNOWN_CREATED_AT: &str = "1970-01-01T00:00:00.000Z";

/// Thread attributes attached to a note projected into the terminal review stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredReviewNoteRenderMetadata {
    pub review_note_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub thread_depth: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_replies: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_next_sibling: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ancestor_has_next_sibling: Option<Vec<bool>>,
    pub semantically_stored: bool,
}

/// The flat annotation shape consumed by the native terminal renderer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalReviewNote {
    pub id: String,
    #[serde(flatten)]
    pub stored: StoredReviewNoteRenderMetadata,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub file_path: String,
    pub hunk_index: usize,
    pub side: ReviewSide,
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_range: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_range: Option<[u32; 2]>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<AgentAnnotationConfidence>,
    /// Agent/live notes omit this field; reviewer-authored notes carry their authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editable: Option<bool>,
}

impl TerminalReviewNote {
    /// Convert the projected note into the annotation record used by row planning.
    #[must_use]
    pub fn annotation(&self) -> AgentAnnotation {
        let serde_json::Value::Object(mut extra) = serde_json::to_value(&self.stored)
            .expect("stored-note render metadata is serializable")
        else {
            unreachable!()
        };
        extra.insert("filePath".into(), serde_json::json!(self.file_path));
        extra.insert("hunkIndex".into(), serde_json::json!(self.hunk_index));
        extra.insert("side".into(), serde_json::json!(self.side));
        extra.insert("line".into(), serde_json::json!(self.line));
        AgentAnnotation {
            extra: extra.into_iter().collect(),
            id: Some(self.id.clone()),
            old_range: self.old_range.map(line_range),
            new_range: self.new_range.map(line_range),
            summary: self.summary.clone(),
            rationale: self.rationale.clone(),
            markup: self.markup.clone(),
            tags: self.tags.clone(),
            confidence: self.confidence,
            source: Some(self.source.clone()),
            title: self.title.clone(),
            author: self.author.clone(),
            created_at: Some(self.created_at.clone()),
            updated_at: self.updated_at.clone(),
            editable: self.editable.unwrap_or(false),
        }
    }
}

/// Derived sibling guides supplied by the shared visible-thread selector.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewThreadGuide {
    pub has_next_sibling: bool,
    pub ancestor_has_next_sibling: Vec<bool>,
}

/// Context used when projecting one semantic note into a threaded terminal row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewNoteProjection {
    pub thread_depth: usize,
    pub has_replies: bool,
    pub thread_guide: Option<ReviewThreadGuide>,
}

/// One in-progress reviewer note, addressed in terminal file coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDraftReviewNote {
    pub id: String,
    pub kind: TerminalDraftReviewNoteKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_note_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub file_id: String,
    pub file_path: String,
    pub hunk_index: usize,
    pub side: ReviewSide,
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_range: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_range: Option<[u32; 2]>,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalDraftReviewNoteKind {
    Create,
    Edit,
    Reply,
}

/// Normalize the user-facing source vocabulary exactly once at the storage boundary.
#[must_use]
pub fn classify_review_note_source(source: &str) -> ReviewNoteSource {
    match source {
        "user" => ReviewNoteSource::User,
        "mcp" | "agent" => ReviewNoteSource::Agent,
        _ => ReviewNoteSource::Ai,
    }
}

fn semantic_range(range: LineRange) -> [u32; 2] {
    [range.start, range.end]
}

fn line_range(range: [u32; 2]) -> LineRange {
    LineRange {
        start: range[0],
        end: range[1],
    }
}

fn semantic_anchor(anchor: crate::ResolvedReviewNoteAnchor) -> SemanticReviewRangeAnchor {
    SemanticReviewRangeAnchor {
        old_range: anchor.old_range.map(semantic_range),
        new_range: anchor.new_range.map(semantic_range),
        preferred: anchor.preferred.map(|preferred| SemanticReviewLineAddress {
            side: preferred.side,
            line: preferred.line,
        }),
        intersecting_hunk_indices: anchor.intersecting_hunk_indices,
        owner_hunk_index: anchor.owner_hunk_index,
    }
}

/// Store one live agent comment as a semantic review note.
pub fn live_comment_to_stored_note(
    comment: &ReviewComment,
    file_key: &str,
    hunks: &[DiffHunk],
) -> Result<ReviewStoredNote, ReviewError> {
    let (Some(hunk_index), Some(side), Some(line)) =
        (comment.hunk_index, comment.side, comment.line)
    else {
        return Err(ReviewError::InvalidCommentTarget(format!(
            "live comment {:?} has no complete hunk, side, and line target",
            comment.id
        )));
    };
    let anchor = review_line_anchor(
        hunks,
        ReviewLineTarget {
            hunk_index,
            side,
            line,
        },
    );
    Ok(ReviewStoredNote {
        note: SemanticReviewNote {
            id: comment.id.clone(),
            parent_id: None,
            source: classify_review_note_source(&comment.source),
            original_source: Some(comment.source.clone()),
            file_key: file_key.to_owned(),
            anchor: semantic_anchor(anchor),
            summary: comment.summary.clone(),
            rationale: comment.rationale.clone(),
            markup: comment.markup.clone(),
            title: comment.title.clone(),
            author: comment.author.clone(),
            created_at: comment.created_at.clone(),
            updated_at: comment.updated_at.clone(),
            editable: false,
            tags: comment.tags.clone(),
            confidence: comment.confidence,
        },
        resolution: ReviewNoteResolution::Active,
    })
}

fn project_stored_note(
    note: &SemanticReviewNote,
    file_path: &str,
    source: &str,
    author: Option<String>,
    editable: Option<bool>,
    projection: ReviewNoteProjection,
) -> TerminalReviewNote {
    let anchor_line = review_note_anchor_line(note);
    let (has_next_sibling, ancestor_has_next_sibling) =
        projection.thread_guide.map_or((None, None), |guide| {
            (
                Some(guide.has_next_sibling),
                Some(guide.ancestor_has_next_sibling),
            )
        });
    TerminalReviewNote {
        id: note.id.clone(),
        stored: StoredReviewNoteRenderMetadata {
            review_note_id: note.id.clone(),
            parent_id: note.parent_id.clone(),
            thread_depth: projection.thread_depth,
            has_replies: projection.has_replies.then_some(true),
            has_next_sibling,
            ancestor_has_next_sibling,
            semantically_stored: true,
        },
        source: source.to_owned(),
        author,
        created_at: note
            .created_at
            .clone()
            .unwrap_or_else(|| UNKNOWN_CREATED_AT.to_owned()),
        updated_at: note.updated_at.clone(),
        file_path: file_path.to_owned(),
        hunk_index: review_note_owner_hunk_index(note),
        side: anchor_line.side,
        line: anchor_line.line,
        old_range: note.anchor.old_range,
        new_range: note.anchor.new_range,
        summary: note.summary.clone(),
        rationale: note.rationale.clone(),
        markup: note.markup.clone(),
        title: note.title.clone(),
        tags: note.tags.clone(),
        confidence: note.confidence,
        editable,
    }
}

/// Render one semantic note as an agent live comment in the review stream.
#[must_use]
pub fn stored_note_to_live_comment(
    note: &SemanticReviewNote,
    file_path: &str,
    projection: ReviewNoteProjection,
) -> TerminalReviewNote {
    project_stored_note(
        note,
        file_path,
        "mcp",
        note.author.clone(),
        None,
        projection,
    )
}

/// Render one semantic note as an editable reviewer-authored annotation.
#[must_use]
pub fn stored_note_to_user_note(
    note: &SemanticReviewNote,
    file_path: &str,
    projection: ReviewNoteProjection,
) -> TerminalReviewNote {
    project_stored_note(
        note,
        file_path,
        "user",
        Some(note.author.clone().unwrap_or_else(|| "user".to_owned())),
        Some(note.editable),
        projection,
    )
}

/// Rebuild the terminal draft coordinates and line range from the semantic draft.
pub fn stored_draft_to_draft_note(
    draft: &ReviewDraftNote,
    file: &DiffFile,
) -> TerminalDraftReviewNote {
    let anchor = review_line_anchor(
        &file.hunks,
        ReviewLineTarget {
            hunk_index: draft.hunk_index,
            side: draft.side,
            line: draft.line,
        },
    );
    let (kind, target_note_id, parent_id) = match &draft.kind {
        ReviewDraftKind::Create => (TerminalDraftReviewNoteKind::Create, None, None),
        ReviewDraftKind::Edit { target_note_id } => (
            TerminalDraftReviewNoteKind::Edit,
            Some(target_note_id.clone()),
            None,
        ),
        ReviewDraftKind::Reply { parent_id } => (
            TerminalDraftReviewNoteKind::Reply,
            None,
            Some(parent_id.clone()),
        ),
    };
    TerminalDraftReviewNote {
        id: draft.id.clone(),
        kind,
        target_note_id,
        parent_id,
        file_id: file.runtime_id.clone(),
        file_path: file.path.clone(),
        hunk_index: draft.hunk_index,
        side: draft.side,
        line: draft.line,
        old_range: anchor.old_range.map(semantic_range),
        new_range: anchor.new_range.map(semantic_range),
        body: draft.body.clone(),
    }
}

/// Group renderable stored notes by terminal file id while retaining storage order.
#[must_use]
pub fn group_stored_notes_by_file_id<T, F>(
    entries: &[ReviewStoredNote],
    file_by_key: &BTreeMap<String, DiffFile>,
    mut project: F,
) -> BTreeMap<String, Vec<T>>
where
    F: FnMut(&SemanticReviewNote, &str) -> T,
{
    let mut result = BTreeMap::<String, Vec<T>>::new();
    for entry in entries {
        let Some(file) = file_by_key.get(&entry.note.file_key) else {
            continue;
        };
        if !is_renderable_stored_review_note(entry) {
            continue;
        }
        result
            .entry(file.runtime_id.clone())
            .or_default()
            .push(project(&entry.note, &file.path));
    }
    result
}

/// Shared view over raw and visibility-decorated threaded note streams.
pub trait ThreadedStoredNoteProjection {
    fn entry(&self) -> &ReviewStoredNote;
    fn depth(&self) -> usize;
    fn render_depth(&self) -> usize;
    fn thread_guide(&self) -> ReviewThreadGuide;
}

impl ThreadedStoredNoteProjection for ReviewThreadedStoredNote {
    fn entry(&self) -> &ReviewStoredNote {
        &self.entry
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn render_depth(&self) -> usize {
        self.depth
    }

    fn thread_guide(&self) -> ReviewThreadGuide {
        ReviewThreadGuide::default()
    }
}

impl ThreadedStoredNoteProjection for ReviewVisibleThreadedStoredNote {
    fn entry(&self) -> &ReviewStoredNote {
        &self.threaded.entry
    }

    fn depth(&self) -> usize {
        self.threaded.depth
    }

    fn render_depth(&self) -> usize {
        self.visible_depth
    }

    fn thread_guide(&self) -> ReviewThreadGuide {
        ReviewThreadGuide {
            has_next_sibling: self.has_next_visible_sibling,
            ancestor_has_next_sibling: self.visible_ancestor_has_next_sibling.clone(),
        }
    }
}

/// Group a threaded stream without losing raw reply or visible sibling geometry.
#[must_use]
pub fn group_threaded_stored_notes_by_file_id<T, E, F>(
    entries: &[E],
    file_by_key: &BTreeMap<String, DiffFile>,
    mut project: F,
) -> BTreeMap<String, Vec<T>>
where
    E: ThreadedStoredNoteProjection,
    F: FnMut(&SemanticReviewNote, &str, usize, bool, ReviewThreadGuide) -> T,
{
    let mut result = BTreeMap::<String, Vec<T>>::new();
    for (index, item) in entries.iter().enumerate() {
        let entry = item.entry();
        let Some(file) = file_by_key.get(&entry.note.file_key) else {
            continue;
        };
        if !is_renderable_stored_review_note(entry) {
            continue;
        }
        let has_replies = entries
            .get(index.saturating_add(1))
            .is_some_and(|next| next.depth() > item.depth());
        result
            .entry(file.runtime_id.clone())
            .or_default()
            .push(project(
                &entry.note,
                &file.path,
                item.render_depth(),
                has_replies,
                item.thread_guide(),
            ));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommentAnchor;
    use workdeck_core::{
        FileChangeKind, FileFlags, FileSourceSnapshots, FileStats, SemanticReviewRangeAnchor,
    };

    fn hunk(index: usize, line: u32) -> DiffHunk {
        DiffHunk {
            index,
            header: format!("@@ -{line},1 +{line},1 @@"),
            context: None,
            old_start: line,
            old_count: 1,
            new_start: line,
            new_count: 1,
            split_row_start: index,
            split_row_count: 1,
            stack_row_start: index,
            stack_row_count: 1,
            lines: Vec::new(),
        }
    }

    fn test_hunks() -> Vec<DiffHunk> {
        vec![hunk(0, 1), hunk(1, 2), hunk(2, 4)]
    }

    fn test_file(id: &str) -> DiffFile {
        DiffFile {
            key: id.to_owned(),
            runtime_id: id.to_owned(),
            path: format!("{id}.ts"),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("typescript".into()),
            stats: FileStats {
                additions: 1,
                deletions: 1,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: 1,
            stack_row_count: 1,
            hunks: vec![hunk(0, 1)],
            content_identity: format!("content:{id}"),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_capability: None,
            source_attested: false,
            agent: None,
        }
    }

    fn test_live_comment(id: &str) -> ReviewComment {
        ReviewComment {
            id: id.to_owned(),
            parent_id: None,
            source: "mcp".into(),
            author: Some("agent".into()),
            created_at: Some("2024-01-01T00:00:00.000Z".into()),
            file_path: Some("alpha.ts".into()),
            hunk_index: Some(2),
            side: Some(ReviewSide::New),
            line: Some(4),
            summary: "summary".into(),
            rationale: Some("rationale".into()),
            markup: Some("<p>markup</p>".into()),
            title: None,
            tags: vec!["mcp".into()],
            confidence: Some(AgentAnnotationConfidence::High),
            updated_at: None,
            resolution: ReviewNoteResolution::Active,
            anchor: CommentAnchor {
                file_key: "alpha".into(),
                old_range: None,
                new_range: Some(LineRange { start: 4, end: 4 }),
                preferred_side: Some(ReviewSide::New),
                preferred_line: Some(4),
                intersecting_hunk_indices: vec![2],
                owner_hunk_index: Some(2),
            },
            editable: false,
        }
    }

    fn user_note() -> SemanticReviewNote {
        SemanticReviewNote {
            id: "user:1".into(),
            parent_id: None,
            source: ReviewNoteSource::User,
            original_source: None,
            file_key: "alpha".into(),
            anchor: SemanticReviewRangeAnchor {
                old_range: Some([9, 9]),
                new_range: None,
                preferred: Some(SemanticReviewLineAddress {
                    side: ReviewSide::Old,
                    line: 9,
                }),
                intersecting_hunk_indices: vec![1],
                owner_hunk_index: Some(1),
            },
            summary: "needs a test".into(),
            rationale: None,
            markup: None,
            title: None,
            author: None,
            created_at: Some("2024-01-01T00:00:00.000Z".into()),
            updated_at: None,
            editable: true,
            tags: Vec::new(),
            confidence: None,
        }
    }

    #[test]
    fn live_comment_round_trip_restores_every_rendered_field() {
        let comment = test_live_comment("mcp:1");
        let stored = live_comment_to_stored_note(&comment, "alpha", &test_hunks()).unwrap();
        let restored = stored_note_to_live_comment(
            &stored.note,
            comment.file_path.as_deref().unwrap(),
            ReviewNoteProjection::default(),
        );

        assert_eq!(restored.id, comment.id);
        assert_eq!(restored.stored.review_note_id, comment.id);
        assert!(restored.stored.semantically_stored);
        assert_eq!(restored.stored.thread_depth, 0);
        assert_eq!(restored.source, comment.source);
        assert_eq!(restored.author, comment.author);
        assert_eq!(restored.created_at, comment.created_at.unwrap());
        assert_eq!(restored.file_path, comment.file_path.unwrap());
        assert_eq!(restored.hunk_index, 2);
        assert_eq!((restored.side, restored.line), (ReviewSide::New, 4));
        assert_eq!(restored.new_range, Some([4, 4]));
        assert_eq!(restored.summary, "summary");
        assert_eq!(restored.rationale.as_deref(), Some("rationale"));
        assert_eq!(restored.markup.as_deref(), Some("<p>markup</p>"));
        assert_eq!(restored.tags, ["mcp"]);
        assert_eq!(restored.confidence, Some(AgentAnnotationConfidence::High));
        assert_eq!(restored.editable, None);
    }

    #[test]
    fn live_comment_is_classified_once_at_the_boundary() {
        let stored =
            live_comment_to_stored_note(&test_live_comment("mcp:1"), "alpha", &test_hunks())
                .unwrap();
        assert_eq!(stored.note.source, ReviewNoteSource::Agent);
        assert_eq!(stored.note.original_source.as_deref(), Some("mcp"));
        assert!(!stored.note.editable);
        assert_eq!(stored.resolution, ReviewNoteResolution::Active);
        assert_eq!(classify_review_note_source("user"), ReviewNoteSource::User);
        assert_eq!(
            classify_review_note_source("agent"),
            ReviewNoteSource::Agent
        );
        assert_eq!(classify_review_note_source("legacy"), ReviewNoteSource::Ai);
    }

    #[test]
    fn user_note_projection_is_editable_and_uses_fallback_author() {
        let rendered =
            stored_note_to_user_note(&user_note(), "alpha.ts", ReviewNoteProjection::default());
        assert_eq!(rendered.id, "user:1");
        assert_eq!(rendered.stored.review_note_id, "user:1");
        assert!(rendered.stored.semantically_stored);
        assert_eq!(rendered.source, "user");
        assert_eq!(rendered.author.as_deref(), Some("user"));
        assert_eq!(rendered.file_path, "alpha.ts");
        assert_eq!(rendered.hunk_index, 1);
        assert_eq!((rendered.side, rendered.line), (ReviewSide::Old, 9));
        assert_eq!(rendered.old_range, Some([9, 9]));
        assert_eq!(rendered.new_range, None);
        assert_eq!(rendered.summary, "needs a test");
        assert_eq!(rendered.editable, Some(true));
        let annotation = rendered.annotation();
        assert_eq!(annotation.extra["reviewNoteId"], "user:1");
        assert_eq!(annotation.extra["semanticallyStored"], true);
        assert_eq!(annotation.extra["threadDepth"], 0);
        assert_eq!(annotation.extra["filePath"], "alpha.ts");
        assert_eq!(annotation.extra["hunkIndex"], 1);
        assert_eq!(annotation.extra["side"], "old");
        assert_eq!(annotation.extra["line"], 9);
        assert_eq!(
            serde_json::from_value::<AgentAnnotation>(serde_json::to_value(&annotation).unwrap())
                .unwrap(),
            annotation
        );
    }

    #[test]
    fn draft_projection_derives_range_and_preserves_edit_reply_kinds() {
        let file = test_file("alpha");
        let create = stored_draft_to_draft_note(
            &ReviewDraftNote {
                id: "draft:1".into(),
                file_key: "alpha".into(),
                hunk_index: 0,
                side: ReviewSide::New,
                line: 3,
                body: "wip".into(),
                kind: ReviewDraftKind::Create,
            },
            &file,
        );
        assert_eq!(create.kind, TerminalDraftReviewNoteKind::Create);
        assert_eq!(create.file_id, "alpha");
        assert_eq!(create.file_path, "alpha.ts");
        assert_eq!(create.new_range, Some([3, 3]));

        let edit = stored_draft_to_draft_note(
            &ReviewDraftNote {
                kind: ReviewDraftKind::Edit {
                    target_note_id: "user:1".into(),
                },
                ..ReviewDraftNote {
                    id: "draft:2".into(),
                    file_key: "alpha".into(),
                    hunk_index: 0,
                    side: ReviewSide::Old,
                    line: 2,
                    body: "edit".into(),
                    kind: ReviewDraftKind::Create,
                }
            },
            &file,
        );
        assert_eq!(edit.kind, TerminalDraftReviewNoteKind::Edit);
        assert_eq!(edit.target_note_id.as_deref(), Some("user:1"));
    }

    #[test]
    fn grouping_preserves_order_and_drops_missing_or_orphaned_notes() {
        let files = BTreeMap::from([("alpha".into(), test_file("alpha"))]);
        let first =
            live_comment_to_stored_note(&test_live_comment("mcp:1"), "alpha", &test_hunks())
                .unwrap();
        let second =
            live_comment_to_stored_note(&test_live_comment("mcp:2"), "alpha", &test_hunks())
                .unwrap();
        let missing =
            live_comment_to_stored_note(&test_live_comment("mcp:3"), "gone", &test_hunks())
                .unwrap();
        let mut orphaned = second.clone();
        orphaned.resolution = ReviewNoteResolution::Orphaned;

        let grouped = group_stored_notes_by_file_id(
            &[first, second, missing, orphaned],
            &files,
            |note, path| {
                stored_note_to_live_comment(note, path, ReviewNoteProjection::default()).id
            },
        );
        assert_eq!(grouped["alpha"], ["mcp:1", "mcp:2"]);
    }

    #[test]
    fn threaded_grouping_preserves_visible_depth_replies_and_guides() {
        let files = BTreeMap::from([("alpha".into(), test_file("alpha"))]);
        let root = ReviewThreadedStoredNote {
            entry: ReviewStoredNote {
                note: user_note(),
                resolution: ReviewNoteResolution::Active,
            },
            root_id: "user:1".into(),
            depth: 0,
            parent_id: None,
        };
        let mut child_note = user_note();
        child_note.id = "user:2".into();
        child_note.parent_id = Some("user:1".into());
        let child = ReviewThreadedStoredNote {
            entry: ReviewStoredNote {
                note: child_note,
                resolution: ReviewNoteResolution::Active,
            },
            root_id: "user:1".into(),
            depth: 1,
            parent_id: Some("user:1".into()),
        };
        let raw = group_threaded_stored_notes_by_file_id(
            &[root.clone(), child.clone()],
            &files,
            |_note, _path, depth, has_replies, guide| (depth, has_replies, guide),
        );
        assert_eq!(raw["alpha"][0].0, 0);
        assert!(raw["alpha"][0].1);
        assert_eq!(raw["alpha"][0].2, ReviewThreadGuide::default());
        assert_eq!(raw["alpha"][1].0, 1);
        assert!(!raw["alpha"][1].1);

        let entries = vec![
            ReviewVisibleThreadedStoredNote {
                threaded: root,
                visible_depth: 0,
                visible_parent_id: None,
                has_next_visible_sibling: false,
                visible_ancestor_has_next_sibling: Vec::new(),
            },
            ReviewVisibleThreadedStoredNote {
                threaded: child,
                visible_depth: 1,
                visible_parent_id: Some("user:1".into()),
                has_next_visible_sibling: true,
                visible_ancestor_has_next_sibling: vec![false],
            },
        ];

        let grouped = group_threaded_stored_notes_by_file_id(
            &entries,
            &files,
            |note, path, depth, has_replies, guide| {
                stored_note_to_user_note(
                    note,
                    path,
                    ReviewNoteProjection {
                        thread_depth: depth,
                        has_replies,
                        thread_guide: Some(guide),
                    },
                )
            },
        );
        let notes = &grouped["alpha"];
        assert_eq!(notes[0].stored.thread_depth, 0);
        assert_eq!(notes[0].stored.has_replies, Some(true));
        assert_eq!(notes[1].stored.thread_depth, 1);
        assert_eq!(notes[1].stored.has_next_sibling, Some(true));
        assert_eq!(notes[1].stored.ancestor_has_next_sibling, Some(vec![false]));
    }

    #[test]
    fn projection_defaults_match_hunk_for_missing_anchor_metadata() {
        let mut note = user_note();
        note.anchor.preferred = None;
        note.anchor.owner_hunk_index = None;
        note.anchor.intersecting_hunk_indices.clear();
        note.created_at = None;
        let rendered =
            stored_note_to_live_comment(&note, "alpha.ts", ReviewNoteProjection::default());
        assert_eq!((rendered.side, rendered.line), (ReviewSide::New, 1));
        assert_eq!(rendered.hunk_index, 0);
        assert_eq!(rendered.created_at, UNKNOWN_CREATED_AT);
    }
}
