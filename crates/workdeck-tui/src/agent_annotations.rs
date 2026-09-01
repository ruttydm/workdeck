//! Visible inline-note projection over the shared review anchor model.

use workdeck_core::{
    AgentAnnotation, DiffFile, DiffHunk, FileChangeKind, LineRange, ReviewNoteSource, ReviewSide,
};
use workdeck_diff::sanitize_terminal_line;
use workdeck_review::{
    ResolvedReviewNoteAnchor, ReviewNoteAnchorInput, ReviewPreferredLine,
    resolve_review_note_anchor, review_annotation_overlaps_hunk, review_gap_owner_hunk_index,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotationAnchor {
    pub side: ReviewSide,
    pub line_number: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleNoteSource {
    Ai,
    Agent,
    User,
    Draft,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleAgentNoteThread {
    pub note_id: String,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub has_next_sibling: Option<bool>,
    pub ancestor_has_next_sibling: Vec<bool>,
}

/// Declarative capabilities replace JavaScript callbacks at the native host boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibleAgentNoteActions {
    pub edit: bool,
    pub reply: bool,
    pub delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleAgentNoteDraft {
    pub body: String,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleAgentNote {
    pub id: String,
    pub annotation: AgentAnnotation,
    pub anchor: ResolvedReviewNoteAnchor,
    pub source: Option<VisibleNoteSource>,
    pub editable: bool,
    pub thread: Option<VisibleAgentNoteThread>,
    pub actions: Option<VisibleAgentNoteActions>,
    pub draft: Option<VisibleAgentNoteDraft>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleNoteTarget {
    pub hunk_index: usize,
    pub side: ReviewSide,
    pub line: u32,
}

/// Build the source/author title rendered on an inline note.
#[must_use]
pub fn inline_note_title(
    annotation: &AgentAnnotation,
    note_index: usize,
    note_count: usize,
) -> String {
    if annotation.source.as_deref() == Some("user-draft") {
        let title = sanitize_terminal_line(annotation.title.as_deref().unwrap_or_default().trim());
        return if title.is_empty() {
            "Draft note".into()
        } else {
            title
        };
    }
    let source = review_note_source(annotation);
    let author = sanitize_terminal_line(annotation.author.as_deref().unwrap_or_default().trim());
    let label = if source == ReviewNoteSource::User {
        "Your note".into()
    } else if author.is_empty() {
        "Agent note".into()
    } else {
        format!("{author} note")
    };
    if note_count > 1 {
        format!("{label} {}/{}", note_index + 1, note_count)
    } else {
        label
    }
}

/// Normalize the user-facing source vocabulary of legacy annotations.
#[must_use]
pub fn review_note_source(annotation: &AgentAnnotation) -> ReviewNoteSource {
    match annotation.source.as_deref() {
        Some("user") => ReviewNoteSource::User,
        Some("mcp" | "agent") => ReviewNoteSource::Agent,
        _ => ReviewNoteSource::Ai,
    }
}

/// Borrow the annotations overlapping the selected hunk.
#[must_use]
pub fn get_selected_annotations<'a>(
    file: Option<&'a DiffFile>,
    hunk: Option<&DiffHunk>,
) -> Vec<&'a AgentAnnotation> {
    let (Some(file), Some(hunk)) = (file, hunk) else {
        return Vec::new();
    };
    file.agent.as_ref().map_or_else(Vec::new, |agent| {
        agent
            .annotations
            .iter()
            .filter(|annotation| review_annotation_overlaps_hunk(annotation, hunk))
            .collect()
    })
}

/// Prefer the new-side range and fall back to the old-side range.
#[must_use]
pub fn annotation_anchor(annotation: &AgentAnnotation) -> Option<AnnotationAnchor> {
    annotation
        .new_range
        .map(|range| AnnotationAnchor {
            side: ReviewSide::New,
            line_number: range.start,
        })
        .or_else(|| {
            annotation.old_range.map(|range| AnnotationAnchor {
                side: ReviewSide::Old,
                line_number: range.start,
            })
        })
}

/// Resolve a visible note through the shared hunk/gap ownership algorithm.
#[must_use]
pub fn create_visible_agent_note(
    hunks: &[DiffHunk],
    mut note: VisibleAgentNote,
    target: Option<VisibleNoteTarget>,
) -> VisibleAgentNote {
    let range_anchor = annotation_anchor(&note.annotation);
    let preferred = target
        .map(|target| ReviewPreferredLine {
            side: target.side,
            line: target.line,
        })
        .or_else(|| {
            range_anchor.map(|anchor| ReviewPreferredLine {
                side: anchor.side,
                line: anchor.line_number,
            })
        });
    let fallback_owner_hunk_index = target.map(|target| target.hunk_index).or_else(|| {
        preferred.and_then(|preferred| {
            review_gap_owner_hunk_index(hunks, preferred.side, preferred.line)
        })
    });
    note.anchor = resolve_review_note_anchor(
        hunks,
        ReviewNoteAnchorInput {
            old_range: note.annotation.old_range,
            new_range: note.annotation.new_range,
            preferred,
            fallback_owner_hunk_index,
        },
    );
    note
}

fn github_style_range(prefix: char, range: LineRange) -> String {
    if range.start == range.end {
        format!("{prefix}{}", range.start)
    } else {
        format!("{prefix}{}–{prefix}{}", range.start, range.end)
    }
}

fn file_label(file: &DiffFile) -> String {
    let path = sanitize_terminal_line(&file.path);
    let base = file
        .previous_path
        .as_deref()
        .map(sanitize_terminal_line)
        .filter(|previous| previous != &path)
        .map_or_else(|| path.clone(), |previous| format!("{previous} -> {path}"));
    let state = match file.change_kind {
        FileChangeKind::Untracked => " (untracked)",
        FileChangeKind::Added => " (new)",
        FileChangeKind::Deleted => " (deleted)",
        _ => "",
    };
    format!("{base}{state}")
}

/// Concise GitHub-style file and inclusive line label.
#[must_use]
pub fn annotation_range_label(annotation: &AgentAnnotation, file: Option<&DiffFile>) -> String {
    let mut locations = Vec::new();
    if let Some(range) = annotation.old_range {
        locations.push(github_style_range('L', range));
    }
    if let Some(range) = annotation.new_range {
        locations.push(github_style_range('R', range));
    }
    let location = if locations.is_empty() {
        "hunk".into()
    } else {
        locations.join(" → ")
    };
    file.map_or_else(
        || location.clone(),
        |file| format!("{} {location}", file_label(file)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{AgentFileContext, FileFlags, FileSourceSnapshots, FileStats};

    fn annotation() -> AgentAnnotation {
        AgentAnnotation {
            id: Some("note-1".into()),
            old_range: Some(LineRange { start: 4, end: 5 }),
            new_range: Some(LineRange { start: 7, end: 7 }),
            summary: "summary".into(),
            rationale: None,
            markup: None,
            tags: Vec::new(),
            confidence: None,
            source: Some("agent".into()),
            title: None,
            author: Some("Codex".into()),
            created_at: None,
            updated_at: None,
            editable: false,
        }
    }

    fn hunk(index: usize, start: u32) -> DiffHunk {
        DiffHunk {
            index,
            header: String::new(),
            context: None,
            old_start: start,
            old_count: 2,
            new_start: start,
            new_count: 2,
            split_row_start: 0,
            split_row_count: 1,
            stack_row_start: 0,
            stack_row_count: 1,
            lines: Vec::new(),
        }
    }

    fn file(annotation: AgentAnnotation) -> DiffFile {
        DiffFile {
            key: "file".into(),
            runtime_id: "runtime".into(),
            path: "src/new.rs".into(),
            previous_path: Some("src/old.rs".into()),
            change_kind: FileChangeKind::Untracked,
            language: Some("rust".into()),
            stats: FileStats::default(),
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: vec![hunk(0, 7)],
            content_identity: "content".into(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: Some(AgentFileContext {
                path: "src/new.rs".into(),
                summary: None,
                annotations: vec![annotation],
            }),
        }
    }

    fn unresolved(annotation: AgentAnnotation) -> VisibleAgentNote {
        VisibleAgentNote {
            id: "visible-1".into(),
            annotation,
            anchor: ResolvedReviewNoteAnchor {
                old_range: None,
                new_range: None,
                preferred: None,
                intersecting_hunk_indices: Vec::new(),
                owner_hunk_index: None,
            },
            source: None,
            editable: false,
            thread: None,
            actions: None,
            draft: None,
        }
    }

    #[test]
    fn titles_and_sources_preserve_hunks_user_facing_vocabulary() {
        let mut note = annotation();
        assert_eq!(inline_note_title(&note, 0, 2), "Codex note 1/2");
        assert_eq!(review_note_source(&note), ReviewNoteSource::Agent);
        note.source = Some("user".into());
        assert_eq!(inline_note_title(&note, 0, 1), "Your note");
        note.source = Some("user-draft".into());
        note.title = Some(" Draft\nspoof ".into());
        assert_eq!(inline_note_title(&note, 0, 1), "Draftspoof");
        note.title = None;
        assert_eq!(inline_note_title(&note, 0, 1), "Draft note");
    }

    #[test]
    fn selection_anchor_and_range_labels_use_both_source_sides() {
        let note = annotation();
        let file = file(note.clone());
        assert_eq!(
            get_selected_annotations(Some(&file), file.hunks.first()),
            [&note]
        );
        assert_eq!(annotation_anchor(&note).unwrap().side, ReviewSide::New);
        assert_eq!(annotation_anchor(&note).unwrap().line_number, 7);
        assert_eq!(
            annotation_range_label(&note, Some(&file)),
            "src/old.rs -> src/new.rs (untracked) L4–L5 → R7"
        );
    }

    #[test]
    fn visible_note_resolves_declared_targets_and_collapsed_gap_owners() {
        let hunks = [hunk(0, 2), hunk(1, 20)];
        let mut note = annotation();
        note.old_range = None;
        note.new_range = Some(LineRange { start: 10, end: 10 });
        let visible = create_visible_agent_note(&hunks, unresolved(note.clone()), None);
        assert_eq!(visible.anchor.owner_hunk_index, Some(1));
        assert_eq!(visible.anchor.preferred.unwrap().line, 10);

        let targeted = create_visible_agent_note(
            &hunks,
            unresolved(note),
            Some(VisibleNoteTarget {
                hunk_index: 0,
                side: ReviewSide::Old,
                line: 3,
            }),
        );
        assert_eq!(targeted.anchor.owner_hunk_index, Some(0));
        assert_eq!(targeted.anchor.preferred.unwrap().side, ReviewSide::Old);
    }
}
