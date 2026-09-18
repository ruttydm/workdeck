//! Convert provider-neutral patch results into the core changeset rendered by Workdeck.

use crate::{
    VcsAdapter, VcsCatalog, VcsCatalogError, VcsFileSourceResult, VcsLoadContext, VcsPatchResult,
    VcsReviewInput, build_filesystem_untracked_diff_file, load_vcs_review, operation_from_input,
};
use std::path::{Path, PathBuf};
use workdeck_core::{Changeset, ChangesetSource, FileSourceSnapshots, ReviewSide};
use workdeck_diff::changeset_from_patch;

/// Loaded provider content together with the authoritative root needed for reloads.
pub struct LoadedVcsChangeset {
    pub changeset: Changeset,
    pub repo_root: PathBuf,
    pub source_capabilities: crate::VcsSourceCapabilities,
}

#[derive(Clone, Copy)]
enum SourceReadMode {
    Eager,
    Deferred,
}

/// Eager compatibility boundary for consumers requiring complete source snapshots.
pub fn load_selected_vcs_changeset(
    cwd: &Path,
    adapter: &VcsAdapter,
    catalog: &VcsCatalog,
    input: &VcsReviewInput,
) -> Result<LoadedVcsChangeset, VcsCatalogError> {
    load_selected_with_source_mode(cwd, adapter, catalog, input, SourceReadMode::Eager)
}

/// Interactive review loads the patch now and reads full source only on demand.
pub fn load_selected_vcs_changeset_deferred(
    cwd: &Path,
    adapter: &VcsAdapter,
    catalog: &VcsCatalog,
    input: &VcsReviewInput,
) -> Result<LoadedVcsChangeset, VcsCatalogError> {
    load_selected_with_source_mode(cwd, adapter, catalog, input, SourceReadMode::Deferred)
}

fn load_selected_with_source_mode(
    cwd: &Path,
    adapter: &VcsAdapter,
    catalog: &VcsCatalog,
    input: &VcsReviewInput,
    source_mode: SourceReadMode,
) -> Result<LoadedVcsChangeset, VcsCatalogError> {
    let operation = operation_from_input(input.clone());
    let result = load_vcs_review(
        adapter,
        &operation,
        &VcsLoadContext {
            cwd: cwd.to_owned(),
        },
        catalog,
    )?;
    let (suffix, source) = match input {
        VcsReviewInput::Diff(input) => {
            let source = if let Some(endpoints) = &input.range_endpoints {
                ChangesetSource::Revision {
                    from: Some(endpoints.from.clone()),
                    to: endpoints.to.clone(),
                }
            } else if let Some(range) = &input.range {
                ChangesetSource::Revision {
                    from: Some(range.clone()),
                    to: "WORKTREE".into(),
                }
            } else {
                ChangesetSource::WorkingTree {
                    staged: input.staged,
                }
            };
            ("working".to_owned(), source)
        }
        VcsReviewInput::Show(input) => {
            let reference = input.reference.as_deref().unwrap_or("HEAD");
            (
                format!("show:{reference}"),
                ChangesetSource::Revision {
                    from: None,
                    to: reference.into(),
                },
            )
        }
        VcsReviewInput::StashShow(input) => {
            let reference = input.reference.as_deref().unwrap_or("stash@{0}");
            (
                format!("stash:{reference}"),
                ChangesetSource::Stash {
                    reference: reference.into(),
                },
            )
        }
    };
    let repo_root = result.repo_root.clone();
    let (changeset, source_capabilities) = materialize_with_source_mode(
        result,
        format!("{}:{suffix}", adapter.id),
        source,
        source_mode,
    )?;
    Ok(LoadedVcsChangeset {
        changeset,
        repo_root,
        source_capabilities,
    })
}

pub fn materialize_vcs_patch_result(
    result: VcsPatchResult,
    changeset_id: impl Into<String>,
    source: ChangesetSource,
) -> Result<Changeset, VcsCatalogError> {
    materialize_vcs_patch_result_with_sources(result, changeset_id, source)
        .map(|(changeset, _)| changeset)
}

/// Retain executable source capabilities for the review runtime as well as the
/// currently materialized snapshots. Existing headless callers retain eager reads.
pub fn materialize_vcs_patch_result_with_sources(
    result: VcsPatchResult,
    changeset_id: impl Into<String>,
    source: ChangesetSource,
) -> Result<(Changeset, crate::VcsSourceCapabilities), VcsCatalogError> {
    materialize_with_source_mode(result, changeset_id, source, SourceReadMode::Eager)
}

pub fn materialize_vcs_patch_result_deferred(
    result: VcsPatchResult,
    changeset_id: impl Into<String>,
    source: ChangesetSource,
) -> Result<(Changeset, crate::VcsSourceCapabilities), VcsCatalogError> {
    materialize_with_source_mode(result, changeset_id, source, SourceReadMode::Deferred)
}

fn materialize_with_source_mode(
    result: VcsPatchResult,
    changeset_id: impl Into<String>,
    source: ChangesetSource,
    source_mode: SourceReadMode,
) -> Result<(Changeset, crate::VcsSourceCapabilities), VcsCatalogError> {
    let changeset_id = changeset_id.into();
    let source_label = result.source_label.clone();
    let mut changeset = changeset_from_patch(
        &result.patch_text,
        changeset_id.clone(),
        result.title.clone(),
        source_label,
        source,
        None,
    );

    changeset.files.extend(result.extra_files);
    let mut pending_capabilities = vec![None; changeset.files.len()];
    if let Some(reader) = &result.source_reader {
        for (index, file) in changeset.files.iter_mut().enumerate() {
            if file.flags.binary || file.flags.too_large {
                continue;
            }
            file.set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
                cache_key: result.source_cache_key.clone(),
            }));
            let capability = std::sync::Arc::new(crate::VcsFileSourceCapability::new(
                std::sync::Arc::clone(reader),
                file,
            ));
            if matches!(source_mode, SourceReadMode::Eager) {
                let old = capability.read(ReviewSide::Old)?;
                let new = capability.read(ReviewSide::New)?;
                file.set_sources(FileSourceSnapshots {
                    old: source_snapshot(old),
                    new: source_snapshot(new),
                });
            }
            pending_capabilities[index] = Some(capability);
        }
    }

    let review_source_label = changeset.effective_source_label().to_owned();
    for path in result.untracked_paths {
        let relative = if path.is_absolute() {
            path.strip_prefix(&result.repo_root).unwrap_or(&path)
        } else {
            path.as_path()
        };
        let file = build_filesystem_untracked_diff_file(
            &result.repo_root,
            relative,
            changeset.files.len(),
            &review_source_label,
        )
        .map_err(|error| VcsCatalogError::Operation(error.to_string()))?;
        changeset.files.push(file);
    }
    changeset.refresh_review_identities();
    let mut source_capabilities = crate::VcsSourceCapabilities::default();
    for (file, capability) in changeset.files.iter().zip(pending_capabilities) {
        if let Some(capability) = capability {
            source_capabilities.insert(file, capability);
        }
    }
    Ok((changeset, source_capabilities))
}

fn source_snapshot(result: VcsFileSourceResult) -> Option<workdeck_core::SourceSnapshot> {
    match result {
        VcsFileSourceResult::Source(source) => Some(source),
        VcsFileSourceResult::Missing | VcsFileSourceResult::TooLarge { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VcsSourceReader;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use workdeck_core::{DiffFile, FileChangeKind, SourceOrigin, SourceSnapshot, review_file_key};

    #[test]
    fn deferred_materialization_reads_no_source_until_a_bound_side_is_requested() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&reads);
        let (changeset, capabilities) = materialize_vcs_patch_result_deferred(
            VcsPatchResult {
                repo_root: PathBuf::from("."),
                source_label: "deferred".into(),
                title: "deferred".into(),
                patch_text: "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -3 +3 @@\n-old\n+new\n".into(),
                untracked_paths: vec![],
                extra_files: vec![],
                source_cache_key: Some("snapshot".into()),
                source_reader: Some(Arc::new(move |request| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(VcsFileSourceResult::Source(SourceSnapshot::new(
                        match request.side {
                            ReviewSide::Old => "old-source",
                            ReviewSide::New => "new-source",
                        }.into(),
                        SourceOrigin::WorkingTree,
                        true,
                    )))
                })),
            },
            "deferred",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        let file = &changeset.files[0];
        assert_eq!(file.sources, FileSourceSnapshots::default());
        assert!(file.source_identity.is_some());
        assert!(file.source_attested);
        let capability = capabilities.get(file).unwrap();
        for (side, expected, count) in [
            (ReviewSide::New, "new-source", 1),
            (ReviewSide::New, "new-source", 1),
            (ReviewSide::Old, "old-source", 2),
        ] {
            let VcsFileSourceResult::Source(source) = capability.read(side).unwrap() else {
                panic!("expected source")
            };
            assert_eq!(source.content, expected);
            assert_eq!(reads.load(Ordering::SeqCst), count);
        }
        assert_eq!(file.sources, FileSourceSnapshots::default());
        let projected = capabilities.with_source_snapshots(file).unwrap();
        let new = projected.sources.new.as_ref().unwrap();
        assert_eq!(new.content, "new-source");
        assert_eq!(new.origin, SourceOrigin::WorkingTree);
        assert!(new.attested);
        assert_eq!(projected.source_identity, file.source_identity);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert_eq!(file.sources, FileSourceSnapshots::default());
    }

    #[test]
    fn parses_patch_hydrates_exact_sides_and_synthesizes_untracked_files() {
        let repo = TempDir::new().unwrap();
        std::fs::write(repo.path().join("fresh.txt"), "fresh\n").unwrap();
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&reads);
        let reader: VcsSourceReader = Arc::new(move |request| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let content = match request.side {
                ReviewSide::Old => "old\n",
                ReviewSide::New => "new\n",
            };
            Ok(VcsFileSourceResult::Source(SourceSnapshot::new(
                content.into(),
                match request.side {
                    ReviewSide::Old => SourceOrigin::Index,
                    ReviewSide::New => SourceOrigin::WorkingTree,
                },
                true,
            )))
        });
        let (changeset, capabilities) = materialize_vcs_patch_result_with_sources(
            VcsPatchResult {
                repo_root: repo.path().into(),
                source_label: repo.path().display().to_string(),
                title: "working tree".into(),
                patch_text: concat!(
                    "diff --git a/tracked.txt b/tracked.txt\n",
                    "--- a/tracked.txt\n",
                    "+++ b/tracked.txt\n",
                    "@@ -1 +1 @@\n",
                    "-old\n",
                    "+new\n"
                )
                .into(),
                untracked_paths: vec!["fresh.txt".into()],
                source_reader: Some(reader),
                source_cache_key: Some("sources".into()),
                extra_files: Vec::new(),
            },
            "git:working",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        assert_eq!(changeset.id, "git:working");
        assert_eq!(changeset.source_label, repo.path().display().to_string());
        assert_eq!(changeset.files.len(), 2);
        assert_eq!(
            changeset.files[0].runtime_id,
            format!("{}:0:tracked.txt", repo.path().display())
        );
        assert!(
            changeset.files[1]
                .runtime_id
                .starts_with(&format!("{}:1:", repo.path().display()))
        );
        assert_eq!(
            changeset.files[0].key,
            review_file_key(&repo.path().display().to_string(), "tracked.txt", None, 0,)
        );
        assert_eq!(
            changeset.files[0].sources.old.as_ref().unwrap().content,
            "old\n"
        );
        assert_eq!(
            changeset.files[0].sources.new.as_ref().unwrap().content,
            "new\n"
        );
        assert_eq!(changeset.files[1].change_kind, FileChangeKind::Added);
        assert!(changeset.files[1].flags.untracked);
        let tracked = &changeset.files[0];
        let retained = capabilities.get(tracked).unwrap();
        for side in [ReviewSide::Old, ReviewSide::New] {
            let expected = match side {
                ReviewSide::Old => tracked.sources.old.as_ref().unwrap(),
                ReviewSide::New => tracked.sources.new.as_ref().unwrap(),
            };
            assert_eq!(
                retained.read(side).unwrap(),
                VcsFileSourceResult::Source(expected.clone())
            );
        }
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            tracked
                .source_capability
                .as_ref()
                .unwrap()
                .cache_key
                .as_deref(),
            Some("sources")
        );
        assert!(tracked.source_attested);
        assert_eq!(
            tracked.source_identity,
            Some(workdeck_core::review_source_identity(
                &tracked.path,
                &tracked.content_identity,
                Some("sources"),
            ))
        );
    }

    #[test]
    fn structural_source_limits_leave_expansion_absent_without_losing_the_diff() {
        let repo = TempDir::new().unwrap();
        let reader: VcsSourceReader =
            Arc::new(|_| Ok(VcsFileSourceResult::TooLarge { max_bytes: 5 }));
        let changeset = materialize_vcs_patch_result(
            VcsPatchResult {
                repo_root: repo.path().into(),
                source_label: "review".into(),
                title: "review".into(),
                patch_text: concat!(
                    "diff --git a/a.txt b/a.txt\n",
                    "--- a/a.txt\n",
                    "+++ b/a.txt\n",
                    "@@ -1 +1 @@\n",
                    "-a\n",
                    "+b\n"
                )
                .into(),
                untracked_paths: Vec::new(),
                source_reader: Some(reader),
                source_cache_key: None,
                extra_files: Vec::new(),
            },
            "review",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        assert_eq!(changeset.files.len(), 1);
        assert_eq!(changeset.files[0].sources, FileSourceSnapshots::default());
        assert_eq!(
            changeset.files[0].source_capability,
            Some(workdeck_core::SourceCapabilityIdentity::default())
        );
        assert!(changeset.files[0].source_identity.is_some());
        assert!(!changeset.files[0].source_attested);
    }

    #[test]
    fn malformed_provider_patch_is_an_empty_descriptive_review() {
        let changeset = materialize_vcs_patch_result(
            VcsPatchResult {
                repo_root: "/repo".into(),
                source_label: "/repo".into(),
                title: "working tree".into(),
                patch_text: "not a patch".into(),
                untracked_paths: Vec::new(),
                source_reader: None,
                source_cache_key: None,
                extra_files: Vec::new(),
            },
            "git:working",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();

        assert_eq!(changeset.id, "git:working");
        assert_eq!(changeset.source_label, "/repo");
        assert_eq!(changeset.title, "working tree");
        assert_eq!(changeset.summary.as_deref(), Some("not a patch"));
        assert!(changeset.files.is_empty());
    }

    #[test]
    fn empty_patch_retains_declarative_extra_files() {
        let repo = TempDir::new().unwrap();
        let mut extra = DiffFile {
            key: String::new(),
            runtime_id: "extra".into(),
            path: "large.txt".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: None,
            stats: Default::default(),
            flags: Default::default(),
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: Vec::new(),
            content_identity: String::new(),
            sources: Default::default(),
            source_identity: None,
            source_capability: None,
            source_attested: false,
            agent: None,
        };
        extra.flags.too_large = true;
        let changeset = materialize_vcs_patch_result(
            VcsPatchResult {
                repo_root: repo.path().into(),
                source_label: "review".into(),
                title: "review".into(),
                patch_text: String::new(),
                untracked_paths: Vec::new(),
                source_reader: None,
                source_cache_key: None,
                extra_files: vec![extra],
            },
            "review",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        assert_eq!(changeset.files.len(), 1);
        assert!(changeset.files[0].flags.too_large);
    }

    #[test]
    fn exact_sources_cover_extra_patches_but_never_binary_or_skipped_files() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let reader: VcsSourceReader = Arc::new(move |request| {
            captured.lock().unwrap().push(request.clone());
            Ok(VcsFileSourceResult::Source(SourceSnapshot::new(
                format!("{:?}\n", request.side),
                SourceOrigin::WorkingTree,
                true,
            )))
        });
        let mut extra = workdeck_diff::parse_single_file_patch(
            concat!(
                "diff --git a/extra.txt b/extra.txt\n",
                "new file mode 100644\n",
                "--- /dev/null\n",
                "+++ b/extra.txt\n",
                "@@ -0,0 +1 @@\n",
                "+extra\n"
            ),
            "extra.txt",
            None,
        )
        .unwrap();
        extra.flags.untracked = true;
        let mut skipped = extra.clone();
        skipped.path = "huge.txt".into();
        skipped.flags.too_large = true;
        skipped.flags.untracked = false;
        let changeset = materialize_vcs_patch_result(
            VcsPatchResult {
                repo_root: "/repo".into(),
                source_label: "/repo".into(),
                title: "review".into(),
                patch_text: concat!(
                    "diff --git a/logo.png b/logo.png\n",
                    "Binary files a/logo.png and b/logo.png differ\n"
                )
                .into(),
                untracked_paths: Vec::new(),
                source_reader: Some(reader),
                source_cache_key: None,
                extra_files: vec![extra, skipped],
            },
            "review",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();

        assert_eq!(changeset.files.len(), 3);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| { request.path == "extra.txt" && request.is_untracked })
        );
        assert!(changeset.files[0].flags.binary);
        assert!(changeset.files[2].flags.too_large);
    }
}
