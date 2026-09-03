//! Convert provider-neutral patch results into the core changeset rendered by Workdeck.

use crate::{
    VcsCatalogError, VcsFileSourceRequest, VcsFileSourceResult, VcsPatchResult,
    build_filesystem_untracked_diff_file,
};
use workdeck_core::{Changeset, ChangesetSource, FileSourceSnapshots, ReviewSide};
use workdeck_diff::parse_patch;

pub fn materialize_vcs_patch_result(
    result: VcsPatchResult,
    changeset_id: impl Into<String>,
    source: ChangesetSource,
) -> Result<Changeset, VcsCatalogError> {
    let changeset_id = changeset_id.into();
    let mut changeset = if result.patch_text.trim().is_empty() {
        Changeset {
            id: changeset_id.clone(),
            title: result.title.clone(),
            source: source.clone(),
            files: Vec::new(),
        }
    } else {
        parse_patch(
            &result.patch_text,
            changeset_id.clone(),
            result.title.clone(),
            source,
        )
        .map_err(|error| VcsCatalogError::Operation(error.to_string()))?
    };

    changeset.files.extend(result.extra_files);
    if let Some(reader) = &result.source_reader {
        for file in &mut changeset.files {
            if file.flags.binary || file.flags.too_large {
                continue;
            }
            let old = reader(&VcsFileSourceRequest {
                path: file.path.clone(),
                previous_path: file.previous_path.clone(),
                change_kind: file.change_kind,
                is_untracked: file.flags.untracked,
                side: ReviewSide::Old,
            })?;
            let new = reader(&VcsFileSourceRequest {
                path: file.path.clone(),
                previous_path: file.previous_path.clone(),
                change_kind: file.change_kind,
                is_untracked: file.flags.untracked,
                side: ReviewSide::New,
            })?;
            file.set_sources(FileSourceSnapshots {
                old: source_snapshot(old),
                new: source_snapshot(new),
            });
        }
    }

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
            &changeset_id,
        )
        .map_err(|error| VcsCatalogError::Operation(error.to_string()))?;
        changeset.files.push(file);
    }
    changeset.refresh_review_identities();
    Ok(changeset)
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
    use workdeck_core::{DiffFile, FileChangeKind, SourceOrigin, SourceSnapshot};

    #[test]
    fn parses_patch_hydrates_exact_sides_and_synthesizes_untracked_files() {
        let repo = TempDir::new().unwrap();
        std::fs::write(repo.path().join("fresh.txt"), "fresh\n").unwrap();
        let reader: VcsSourceReader = Arc::new(|request| {
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
        let changeset = materialize_vcs_patch_result(
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
        assert_eq!(changeset.files.len(), 2);
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
