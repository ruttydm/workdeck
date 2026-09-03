//! Immutable semantic review publications and their external resource catalog.

use std::{collections::BTreeMap, sync::Arc};

use workdeck_core::{
    Changeset, ChangesetSource, DiffFile, ReviewFileChangeKind, ReviewSide, SemanticReviewDocument,
    SemanticReviewFile, project_review_document,
};

use crate::{
    REVIEW_CANONICAL_FILE_CONTENT_TYPE, REVIEW_PATCH_CONTENT_TYPE, REVIEW_SOURCE_CONTENT_TYPE,
    ReviewContentManifest, ReviewResourceAddress, ReviewResourceDescriptor,
    ReviewResourceDescriptorBase, ReviewResourceKind, build_review_content_manifest,
    review_resource_id,
};

/// One immutable generation of a review and every resource it advertises.
#[derive(Debug, Clone)]
pub struct ReviewPublication {
    pub generation: String,
    pub document: Arc<SemanticReviewDocument>,
    pub manifest: ReviewContentManifest,
    pub resources: Vec<ReviewResourceDescriptor>,
    /// Renderer-model files retained in document order for source materialization.
    pub diff_files_by_key: BTreeMap<String, DiffFile>,
}

fn descriptor_base(
    generation: &str,
    file_key: &str,
    kind: ReviewResourceKind,
    side: Option<ReviewSide>,
) -> ReviewResourceDescriptorBase {
    ReviewResourceDescriptorBase {
        id: review_resource_id(&ReviewResourceAddress {
            kind,
            file_key: file_key.to_owned(),
            side,
        }),
        generation: generation.to_owned(),
        file_key: file_key.to_owned(),
        byte_length: None,
        digest: None,
    }
}

fn file_resources(file: &SemanticReviewFile, generation: &str) -> Vec<ReviewResourceDescriptor> {
    let mut resources = vec![
        ReviewResourceDescriptor::CanonicalFile {
            descriptor: descriptor_base(
                generation,
                &file.key,
                ReviewResourceKind::CanonicalFile,
                None,
            ),
            content_type: REVIEW_CANONICAL_FILE_CONTENT_TYPE.to_owned(),
        },
        ReviewResourceDescriptor::Patch {
            descriptor: descriptor_base(generation, &file.key, ReviewResourceKind::Patch, None),
            content_type: REVIEW_PATCH_CONTENT_TYPE.to_owned(),
        },
    ];
    if let Some(source_identity) = &file.source_identity {
        let side = if file.change_kind == ReviewFileChangeKind::Deleted {
            ReviewSide::Old
        } else {
            ReviewSide::New
        };
        resources.push(ReviewResourceDescriptor::Source {
            descriptor: descriptor_base(
                generation,
                &file.key,
                ReviewResourceKind::Source,
                Some(side),
            ),
            content_type: REVIEW_SOURCE_CONTENT_TYPE.to_owned(),
            side,
            source_identity: source_identity.clone(),
        });
    }
    resources
}

/// Project files into one immutable, generation-addressed review publication.
#[must_use]
pub fn build_review_publication(
    files: &[DiffFile],
    generation: impl Into<String>,
    source_label: Option<&str>,
) -> ReviewPublication {
    let generation = generation.into();
    let source_label = source_label.unwrap_or("review");
    let changeset = Changeset {
        id: source_label.to_owned(),
        source_label: source_label.to_owned(),
        title: source_label.to_owned(),
        summary: None,
        agent_summary: None,
        source: ChangesetSource::Patch {
            label: source_label.to_owned(),
        },
        files: files.to_vec(),
    };
    let document = Arc::new(project_review_document(&changeset, Some(source_label)));
    let manifest = build_review_content_manifest(&document);
    let resources = document
        .files
        .iter()
        .flat_map(|file| file_resources(file, &generation))
        .collect();
    let diff_files_by_key = document
        .files
        .iter()
        .zip(files)
        .map(|(file, diff_file)| (file.key.clone(), diff_file.clone()))
        .collect();
    ReviewPublication {
        generation,
        document,
        manifest,
        resources,
        diff_files_by_key,
    }
}

#[must_use]
pub fn review_publication_file<'a>(
    publication: &'a ReviewPublication,
    file_key: &str,
) -> Option<&'a SemanticReviewFile> {
    publication
        .document
        .files
        .iter()
        .find(|file| file.key == file_key)
}

#[must_use]
pub fn review_publication_resource<'a>(
    publication: &'a ReviewPublication,
    resource_id: &str,
) -> Option<&'a ReviewResourceDescriptor> {
    publication
        .resources
        .iter()
        .find(|resource| resource.base().id == resource_id)
}

#[cfg(test)]
mod tests {
    use workdeck_core::{
        DiffHunk, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats, SourceOrigin,
        SourceSnapshot,
    };

    use super::*;

    fn file(path: &str, change_kind: FileChangeKind, source: Option<&str>) -> DiffFile {
        let mut file = DiffFile {
            key: String::new(),
            runtime_id: format!("runtime:{path}"),
            path: path.into(),
            previous_path: None,
            change_kind,
            language: Some("rust".into()),
            stats: FileStats {
                additions: 1,
                deletions: 1,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
            split_row_count: 1,
            stack_row_count: 2,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -1 +1 @@".into(),
                context: None,
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                split_row_start: 0,
                split_row_count: 1,
                stack_row_start: 0,
                stack_row_count: 2,
                lines: Vec::new(),
            }],
            content_identity: String::new(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        };
        file.refresh_identity();
        if let Some(source) = source {
            let snapshot = SourceSnapshot::new(source.into(), SourceOrigin::WorkingTree, true);
            let snapshots = if change_kind == FileChangeKind::Deleted {
                FileSourceSnapshots {
                    old: Some(snapshot),
                    new: None,
                }
            } else {
                FileSourceSnapshots {
                    old: None,
                    new: Some(snapshot),
                }
            };
            file.set_sources(snapshots);
        }
        file
    }

    #[test]
    fn publishes_canonical_patch_and_only_the_expandable_source_side() {
        let files = [
            file("src/live.rs", FileChangeKind::Modified, Some("new\n")),
            file("src/deleted.rs", FileChangeKind::Deleted, Some("old\n")),
            file("src/no-source.rs", FileChangeKind::Modified, None),
        ];
        let publication = build_review_publication(&files, "generation:test:0", Some("/repo"));

        assert_eq!(publication.document.files.len(), 3);
        assert_eq!(publication.manifest.files.len(), 3);
        assert_eq!(publication.resources.len(), 8);
        let source_sides = publication
            .resources
            .iter()
            .filter_map(|resource| match resource {
                ReviewResourceDescriptor::Source { side, .. } => Some(*side),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(source_sides, [ReviewSide::New, ReviewSide::Old]);
        assert!(publication.resources.iter().all(|resource| {
            resource.base().generation == "generation:test:0"
                && resource.base().byte_length.is_none()
                && resource.base().digest.is_none()
        }));
    }

    #[test]
    fn retains_exact_diff_file_owners_and_resolves_files_and_resources_by_address() {
        let source = file("src/live.rs", FileChangeKind::Modified, Some("new\n"));
        let publication =
            build_review_publication(std::slice::from_ref(&source), "generation:test:0", None);
        let projected = &publication.document.files[0];
        assert_eq!(
            publication.diff_files_by_key[&projected.key].runtime_id,
            source.runtime_id
        );
        assert_eq!(
            review_publication_file(&publication, &projected.key).map(|file| file.path.as_str()),
            Some("src/live.rs")
        );
        let id = review_resource_id(&ReviewResourceAddress {
            kind: ReviewResourceKind::Patch,
            file_key: projected.key.clone(),
            side: None,
        });
        assert_eq!(
            review_publication_resource(&publication, &id)
                .map(|resource| resource.base().id.as_str()),
            Some(id.as_str())
        );
        assert!(
            review_publication_resource(&publication, "resource:patch:file:deadbeef").is_none()
        );
    }
}
