use std::sync::Arc;

use workdeck_core::{
    ReviewFileChangeKind, ReviewNoteSource, SemanticReviewDocument, SemanticReviewFile,
    SemanticReviewFileFlags, SemanticReviewFileStats, SemanticReviewHunk, SemanticReviewNote,
    SemanticReviewRangeAnchor,
};

pub(crate) fn document(files: &[(&str, usize)]) -> Arc<SemanticReviewDocument> {
    Arc::new(SemanticReviewDocument {
        files: files
            .iter()
            .map(|(key, hunk_count)| file(key, *hunk_count, None, false))
            .collect(),
    })
}

pub(crate) fn document_with_sources(
    files: &[(&str, Option<&str>, bool)],
) -> Arc<SemanticReviewDocument> {
    Arc::new(SemanticReviewDocument {
        files: files
            .iter()
            .map(|(key, identity, attested)| file(key, 1, *identity, *attested))
            .collect(),
    })
}

fn file(
    key: &str,
    hunk_count: usize,
    source_identity: Option<&str>,
    source_attested: bool,
) -> SemanticReviewFile {
    SemanticReviewFile {
        key: key.into(),
        runtime_id: key.into(),
        path: key.into(),
        previous_path: None,
        change_kind: ReviewFileChangeKind::Change,
        language: None,
        agent_summary: None,
        stats: SemanticReviewFileStats {
            additions: hunk_count,
            deletions: hunk_count,
            truncated: false,
        },
        flags: SemanticReviewFileFlags {
            untracked: false,
            binary: false,
            too_large: false,
            partial: false,
        },
        patch: String::new(),
        split_line_count: hunk_count,
        unified_line_count: hunk_count,
        addition_lines: Vec::new(),
        deletion_lines: Vec::new(),
        line_move_kinds: None,
        hunks: (0..hunk_count)
            .map(|index| SemanticReviewHunk {
                index,
                collapsed_before: 0,
                split_line_start: index,
                split_line_count: 1,
                unified_line_start: index,
                unified_line_count: 1,
                addition_start: (index * 10 + 1) as u32,
                addition_count: 1,
                addition_lines: 1,
                addition_line_index: index,
                deletion_start: (index * 10 + 1) as u32,
                deletion_count: 1,
                deletion_lines: 1,
                deletion_line_index: index,
                hunk_content: Vec::new(),
                hunk_specs: None,
                hunk_context: None,
                no_eofcr_additions: false,
                no_eofcr_deletions: false,
            })
            .collect(),
        content_identity: format!("content:{key}"),
        source_identity: source_identity.map(str::to_owned),
        source_attested: source_identity.map(|_| source_attested),
    }
}

pub(crate) fn note(id: &str, file_key: &str, source: ReviewNoteSource) -> SemanticReviewNote {
    SemanticReviewNote {
        id: id.into(),
        parent_id: None,
        source,
        original_source: None,
        file_key: file_key.into(),
        anchor: SemanticReviewRangeAnchor {
            old_range: None,
            new_range: None,
            preferred: None,
            intersecting_hunk_indices: vec![0],
            owner_hunk_index: None,
        },
        summary: format!("note {id}"),
        rationale: None,
        markup: None,
        title: None,
        author: None,
        created_at: None,
        updated_at: None,
        editable: source == ReviewNoteSource::User,
        tags: Vec::new(),
        confidence: None,
    }
}
