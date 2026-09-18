//! Deterministic file and hunk membership index for merged review annotations.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use workdeck_core::{AgentAnnotation, DiffFile, DiffHunk, ReviewSide};

use crate::{review_hunk_range, review_ranges_overlap};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewAnnotationIndex {
    pub annotated_hunk_indices_by_file_key: BTreeMap<String, BTreeSet<usize>>,
    pub annotated_file_keys: BTreeSet<String>,
}

/// Whether one annotation lands inside a hunk's visible span on either side.
pub fn review_annotation_overlaps_hunk(annotation: &AgentAnnotation, hunk: &DiffHunk) -> bool {
    annotation
        .new_range
        .is_some_and(|range| review_ranges_overlap(range, review_hunk_range(hunk, ReviewSide::New)))
        || annotation.old_range.is_some_and(|range| {
            review_ranges_overlap(range, review_hunk_range(hunk, ReviewSide::Old))
        })
}

/// Which of one file's hunks carry at least one annotation.
pub fn review_annotated_hunk_indices(file: Option<&DiffFile>) -> BTreeSet<usize> {
    let Some(annotations) = file
        .and_then(|file| file.agent.as_ref())
        .map(|agent| agent.annotations.as_slice())
    else {
        return BTreeSet::new();
    };
    file.expect("annotations imply a file")
        .hunks
        .iter()
        .enumerate()
        .filter_map(|(index, hunk)| {
            annotations
                .iter()
                .any(|annotation| review_annotation_overlaps_hunk(annotation, hunk))
                .then_some(index)
        })
        .collect()
}

/// Index annotated files and hunks by semantic file key.
///
/// `key_by_runtime_id` is deliberately supplied by the caller: runtime mounts
/// are not semantic addresses, while the file keys are stable across reloads.
pub fn build_review_annotation_index(
    files: &[DiffFile],
    key_by_runtime_id: &HashMap<String, String>,
) -> ReviewAnnotationIndex {
    let mut index = ReviewAnnotationIndex::default();
    for file in files {
        let Some(file_key) = key_by_runtime_id.get(&file.runtime_id) else {
            continue;
        };
        if file.agent.is_some() {
            index.annotated_file_keys.insert(file_key.clone());
        }
        let annotated_hunks = review_annotated_hunk_indices(Some(file));
        if !annotated_hunks.is_empty() {
            index
                .annotated_hunk_indices_by_file_key
                .insert(file_key.clone(), annotated_hunks);
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{
        AgentFileContext, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats, LineRange,
    };

    fn file() -> DiffFile {
        DiffFile {
            key: "semantic".into(),
            runtime_id: "runtime".into(),
            path: "src/lib.rs".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("rust".into()),
            stats: FileStats::default(),
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: vec![hunk(1), hunk(20)],
            content_identity: "content".into(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_capability: None,
            source_attested: false,
            agent: Some(AgentFileContext {
                path: "src/lib.rs".into(),
                summary: Some("review context".into()),
                annotations: vec![AgentAnnotation {
                    extra: Default::default(),
                    id: None,
                    old_range: None,
                    new_range: Some(LineRange { start: 21, end: 21 }),
                    summary: "note".into(),
                    rationale: None,
                    markup: None,
                    tags: Vec::new(),
                    confidence: None,
                    source: None,
                    title: None,
                    author: None,
                    created_at: None,
                    updated_at: None,
                    editable: false,
                }],
            }),
        }
    }

    fn hunk(start: u32) -> DiffHunk {
        DiffHunk {
            index: 0,
            header: String::new(),
            context: None,
            old_start: start,
            old_count: 3,
            new_start: start,
            new_count: 3,
            split_row_start: 0,
            split_row_count: 0,
            stack_row_start: 0,
            stack_row_count: 0,
            lines: Vec::new(),
        }
    }

    #[test]
    fn indexes_file_context_and_only_overlapping_hunks() {
        let file = file();
        assert_eq!(review_annotated_hunk_indices(Some(&file)), [1].into());
        let index = build_review_annotation_index(
            &[file],
            &HashMap::from([("runtime".into(), "file:key".into())]),
        );
        assert_eq!(index.annotated_file_keys, ["file:key".into()].into());
        assert_eq!(
            index.annotated_hunk_indices_by_file_key["file:key"],
            [1].into()
        );
    }

    #[test]
    fn skips_unaddressed_files_and_keeps_file_only_context() {
        let mut file = file();
        file.agent.as_mut().unwrap().annotations.clear();
        assert_eq!(
            build_review_annotation_index(&[file.clone()], &HashMap::new()),
            ReviewAnnotationIndex::default()
        );
        let index = build_review_annotation_index(
            &[file],
            &HashMap::from([("runtime".into(), "file:key".into())]),
        );
        assert!(index.annotated_file_keys.contains("file:key"));
        assert!(index.annotated_hunk_indices_by_file_key.is_empty());
    }
}
