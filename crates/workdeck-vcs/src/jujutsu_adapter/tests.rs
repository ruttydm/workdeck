use super::*;
use crate::VcsFileSourceRequest;
use std::fs;
use tempfile::{TempDir, tempdir};
use workdeck_core::{CommonOptions, VcsRangeEndpoints};

fn diff_input() -> VcsDiffCommandInput {
    VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions::default(),
    }
}

fn show_input(reference: Option<&str>) -> VcsShowCommandInput {
    VcsShowCommandInput {
        reference: reference.map(str::to_owned),
        pathspecs: Vec::new(),
        options: CommonOptions::default(),
    }
}

fn load(
    adapter: &VcsAdapter,
    kind: VcsReviewOperationKind,
    input: VcsReviewInput,
    cwd: &Path,
) -> Result<VcsPatchResult, VcsCatalogError> {
    (adapter.operations[&kind].load)(
        &input,
        &VcsLoadContext {
            cwd: cwd.to_owned(),
        },
    )
}

fn signature(
    adapter: &VcsAdapter,
    kind: VcsReviewOperationKind,
    input: VcsReviewInput,
    cwd: &Path,
) -> Result<String, VcsCatalogError> {
    adapter.operations[&kind].watch_signature.as_ref().unwrap()(
        &input,
        &VcsLoadContext {
            cwd: cwd.to_owned(),
        },
    )
}

fn comparable(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

fn source_content(result: Result<VcsFileSourceResult, VcsCatalogError>) -> Option<String> {
    match result.unwrap() {
        VcsFileSourceResult::Source(source) => Some(source.content),
        VcsFileSourceResult::Missing | VcsFileSourceResult::TooLarge { .. } => None,
    }
}

#[cfg(unix)]
fn fake_jj(repo_root: &Path, command_log: &Path) -> TempDir {
    use std::os::unix::fs::PermissionsExt;

    fn quote(path: &Path) -> String {
        format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
    }

    let directory = tempdir().unwrap();
    let executable = directory.path().join("jj");
    let script = format!(
        concat!(
            "#!/bin/sh\n",
            "printf '%s|%s\\n' \"$(pwd)\" \"$*\" >> {}\n",
            "case \"$*\" in\n",
            "  *\" root\") printf '%s\\n' {} ;;\n",
            "  *\" file show \"*\" -r aaaa \"*) printf 'old\\ncontext\\n' ;;\n",
            "  *\" file show \"*\" -r 1111 \"*) printf 'from\\ncontext\\n' ;;\n",
            "  *\" file show \"*\" -r bbbb \"*) printf 'two\\ncontext\\n' ;;\n",
            "  *\" file show \"*\" -r 2222 \"*) printf 'to\\ncontext\\n' ;;\n",
            "  *\" file show \"*\" -r dddd \"*) printf 'merge result\\n' ;;\n",
            "  *\" -r @ -T \"*) printf 'bbbb\\n' ;;\n",
            "  *\" -r bbbb- -T \"*) printf 'aaaa\\n' ;;\n",
            "  *\" -r main -T \"*) printf '1111\\n' ;;\n",
            "  *\" -r feature -T \"*) printf '2222\\n' ;;\n",
            "  *\" -r multi -T \"*) printf '1111\\n2222\\n' ;;\n",
            "  *\" -r merge -T \"*) printf 'dddd\\n' ;;\n",
            "  *\" -r dddd- -T \"*) printf 'cccc\\naaaa\\n' ;;\n",
            "  *\" diff \"*) if [ \"$(basename \"$(pwd)\")\" = sub ]; then p='sub/file.txt'; else p='file.txt'; fi; printf 'diff --git a/%s b/%s\\n--- a/%s\\n+++ b/%s\\n@@ -1 +1 @@\\n-old\\n+two\\n' \"$p\" \"$p\" \"$p\" \"$p\" ;;\n",
            "esac\n"
        ),
        quote(command_log),
        quote(repo_root)
    );
    fs::write(&executable, script).unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    directory
}

#[cfg(unix)]
fn fixture_adapter(repo: &Path) -> (VcsAdapter, TempDir, PathBuf) {
    let log = repo.join("commands.log");
    let binary = fake_jj(repo, &log);
    let adapter = create_jujutsu_vcs_adapter(JujutsuVcsAdapterOptions {
        jj_executable: binary.path().join("jj"),
    });
    (adapter, binary, log)
}

#[test]
fn publishes_both_review_operations_and_no_stash_operation() {
    let adapter = create_jujutsu_vcs_adapter(JujutsuVcsAdapterOptions::default());
    assert!(
        adapter
            .operations
            .contains_key(&VcsReviewOperationKind::WorkingTreeDiff)
    );
    assert!(
        adapter
            .operations
            .contains_key(&VcsReviewOperationKind::RevisionShow)
    );
    assert!(
        !adapter
            .operations
            .contains_key(&VcsReviewOperationKind::StashShow)
    );
    assert_eq!(
        adapter.detection_priority,
        Some(JUJUTSU_VCS_DETECTION_BASELINE_PRIORITY)
    );
}

#[test]
fn detects_jj_workspace_markers_from_nested_directories() {
    let repo = tempdir().unwrap();
    fs::create_dir(repo.path().join(".jj")).unwrap();
    let nested = repo.path().join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    let adapter = create_jujutsu_vcs_adapter(JujutsuVcsAdapterOptions::default());
    assert_eq!(
        (adapter.detect)(&nested).unwrap(),
        Some(VcsDetection {
            id: "jj".into(),
            repo_root: comparable(repo.path()),
        })
    );
}

#[test]
fn returns_none_without_a_jj_marker_to_the_filesystem_root() {
    let repo = tempdir().unwrap();
    let adapter = create_jujutsu_vcs_adapter(JujutsuVcsAdapterOptions::default());
    assert_eq!((adapter.detect)(repo.path()).unwrap(), None);
}

#[test]
fn source_cache_identity_changes_with_either_resolved_side() {
    let root = Path::new("/repo");
    let executable = Path::new("jj");
    let baseline = create_jujutsu_source_capability(
        root,
        &JujutsuDiffEndpoints {
            new_commit_id: "bbbb".into(),
            old_commit_ids: vec!["aaaa".into()],
        },
        executable,
    );
    let equivalent = create_jujutsu_source_capability(
        root,
        &JujutsuDiffEndpoints {
            new_commit_id: "bbbb".into(),
            old_commit_ids: vec!["aaaa".into()],
        },
        executable,
    );
    let new_moved = create_jujutsu_source_capability(
        root,
        &JujutsuDiffEndpoints {
            new_commit_id: "cccc".into(),
            old_commit_ids: vec!["aaaa".into()],
        },
        executable,
    );
    let old_moved = create_jujutsu_source_capability(
        root,
        &JujutsuDiffEndpoints {
            new_commit_id: "bbbb".into(),
            old_commit_ids: vec!["9999".into()],
        },
        executable,
    );
    assert_eq!(baseline.source_cache_key, equivalent.source_cache_key);
    assert_ne!(baseline.source_cache_key, new_moved.source_cache_key);
    assert_ne!(baseline.source_cache_key, old_moved.source_cache_key);
}

#[test]
fn rejects_staged_reviews_before_spawning_jj() {
    let mut input = diff_input();
    input.staged = true;
    let adapter = create_jujutsu_vcs_adapter(JujutsuVcsAdapterOptions {
        jj_executable: "definitely-not-a-real-jj-binary".into(),
    });
    let error = match load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input),
        std::env::temp_dir().as_path(),
    ) {
        Ok(_) => panic!("staged Jujutsu review unexpectedly loaded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("Jujutsu has no staging area"));
}

#[cfg(unix)]
#[test]
fn rejects_option_like_endpoints_from_direct_adapter_callers() {
    let repo = tempdir().unwrap();
    let (adapter, _binary, _log) = fixture_adapter(repo.path());
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "--at-operation".into(),
    });
    let error = match load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input),
        repo.path(),
    ) {
        Ok(_) => panic!("option-like Jujutsu endpoint unexpectedly loaded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("looks like a Jujutsu option"));
}

#[cfg(unix)]
#[test]
fn loads_working_copy_and_revision_patches_with_immutable_sources() {
    let repo = tempdir().unwrap();
    let (adapter, _binary, _log) = fixture_adapter(repo.path());
    let diff = diff_input();
    let result = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(diff.clone()),
        repo.path(),
    )
    .unwrap();
    assert_eq!(comparable(&result.repo_root), comparable(repo.path()));
    assert!(result.title.contains("working copy"));
    assert!(result.patch_text.contains("+two"));
    assert!(
        result
            .source_cache_key
            .as_deref()
            .unwrap()
            .contains("jj-source-v1")
    );
    let reader = result.source_reader.unwrap();
    let request = VcsFileSourceRequest {
        path: "file.txt".into(),
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        is_untracked: false,
        side: ReviewSide::Old,
    };
    assert_eq!(
        source_content(reader(&request)).as_deref(),
        Some("old\ncontext\n")
    );
    assert_eq!(
        source_content(reader(&VcsFileSourceRequest {
            side: ReviewSide::New,
            ..request
        }))
        .as_deref(),
        Some("two\ncontext\n")
    );

    let show = show_input(Some("@"));
    let show_result = load(
        &adapter,
        VcsReviewOperationKind::RevisionShow,
        VcsReviewInput::Show(show.clone()),
        repo.path(),
    )
    .unwrap();
    assert!(show_result.title.contains("show @"));
    assert!(
        show_result
            .source_cache_key
            .as_deref()
            .unwrap()
            .contains("jj-source-v1")
    );
    assert!(
        signature(
            &adapter,
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewInput::Diff(diff),
            repo.path(),
        )
        .unwrap()
        .contains("+two")
    );
    assert!(
        signature(
            &adapter,
            VcsReviewOperationKind::RevisionShow,
            VcsReviewInput::Show(show),
            repo.path(),
        )
        .unwrap()
        .contains("diff --git")
    );
}

#[cfg(unix)]
#[test]
fn expands_both_explicit_revision_endpoints_and_pins_the_patch() {
    let repo = tempdir().unwrap();
    let (adapter, _binary, log) = fixture_adapter(repo.path());
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "feature".into(),
    });
    let result = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input.clone()),
        repo.path(),
    )
    .unwrap();
    assert!(result.title.contains("main..feature"));
    assert!(result.patch_text.contains("+two"));
    let reader = result.source_reader.unwrap();
    let request = VcsFileSourceRequest {
        path: "file.txt".into(),
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        is_untracked: false,
        side: ReviewSide::Old,
    };
    assert_eq!(
        source_content(reader(&request)).as_deref(),
        Some("from\ncontext\n")
    );
    assert_eq!(
        source_content(reader(&VcsFileSourceRequest {
            side: ReviewSide::New,
            ..request
        }))
        .as_deref(),
        Some("to\ncontext\n")
    );
    let commands = fs::read_to_string(log).unwrap();
    assert!(
        commands
            .lines()
            .any(|command| { command.contains("--ignore-working-copy --from 1111 --to 2222") })
    );
    assert!(
        signature(
            &adapter,
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewInput::Diff(input),
            repo.path(),
        )
        .unwrap()
        .contains("+two")
    );
}

#[cfg(unix)]
#[test]
fn preserves_nested_cwd_filesets_for_loads_and_watch_signatures() {
    let repo = tempdir().unwrap();
    let nested = repo.path().join("sub");
    fs::create_dir(&nested).unwrap();
    let (adapter, _binary, log) = fixture_adapter(repo.path());
    let mut diff = diff_input();
    diff.pathspecs = vec!["file.txt".into()];
    let mut show = show_input(Some("@"));
    show.pathspecs = vec!["file.txt".into()];
    for patch in [
        load(
            &adapter,
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewInput::Diff(diff.clone()),
            &nested,
        )
        .unwrap()
        .patch_text,
        signature(
            &adapter,
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewInput::Diff(diff),
            &nested,
        )
        .unwrap(),
        load(
            &adapter,
            VcsReviewOperationKind::RevisionShow,
            VcsReviewInput::Show(show.clone()),
            &nested,
        )
        .unwrap()
        .patch_text,
        signature(
            &adapter,
            VcsReviewOperationKind::RevisionShow,
            VcsReviewInput::Show(show),
            &nested,
        )
        .unwrap(),
    ] {
        assert!(patch.contains("sub/file.txt"));
    }
    let commands = fs::read_to_string(log).unwrap();
    let nested = comparable(&nested);
    assert!(commands.lines().any(|command| {
        command.starts_with(&nested.display().to_string()) && command.ends_with("-- file.txt")
    }));
}

#[cfg(unix)]
#[test]
fn reads_rename_addition_and_deletion_sides_from_exact_paths() {
    let repo = tempdir().unwrap();
    let binary = fake_jj(repo.path(), &repo.path().join("commands.log"));
    let capability = create_jujutsu_source_capability(
        repo.path(),
        &JujutsuDiffEndpoints {
            new_commit_id: "bbbb".into(),
            old_commit_ids: vec!["aaaa".into()],
        },
        &binary.path().join("jj"),
    );
    let renamed = VcsFileSourceRequest {
        path: "new-name.txt".into(),
        previous_path: Some("old-name.txt".into()),
        change_kind: FileChangeKind::Renamed,
        is_untracked: false,
        side: ReviewSide::Old,
    };
    assert_eq!(
        source_content((capability.read_file_source)(&renamed)).as_deref(),
        Some("old\ncontext\n")
    );
    assert_eq!(
        source_content((capability.read_file_source)(&VcsFileSourceRequest {
            side: ReviewSide::New,
            ..renamed
        }))
        .as_deref(),
        Some("two\ncontext\n")
    );
    for (change_kind, side, expected) in [
        (FileChangeKind::Added, ReviewSide::Old, None),
        (
            FileChangeKind::Added,
            ReviewSide::New,
            Some("two\ncontext\n"),
        ),
        (
            FileChangeKind::Deleted,
            ReviewSide::Old,
            Some("old\ncontext\n"),
        ),
        (FileChangeKind::Deleted, ReviewSide::New, None),
    ] {
        let result = (capability.read_file_source)(&VcsFileSourceRequest {
            path: "kind.txt".into(),
            previous_path: None,
            change_kind,
            is_untracked: false,
            side,
        });
        assert_eq!(source_content(result).as_deref(), expected);
    }
}

#[cfg(unix)]
#[test]
fn preserves_multi_revision_patches_without_attaching_guessed_sources() {
    let repo = tempdir().unwrap();
    let (adapter, _binary, _log) = fixture_adapter(repo.path());
    let mut diff = diff_input();
    diff.range = Some("multi".into());
    let diff_result = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(diff),
        repo.path(),
    )
    .unwrap();
    let show_result = load(
        &adapter,
        VcsReviewOperationKind::RevisionShow,
        VcsReviewInput::Show(show_input(Some("multi"))),
        repo.path(),
    )
    .unwrap();
    assert!(diff_result.patch_text.contains("diff --git"));
    assert_eq!(show_result.patch_text, diff_result.patch_text);
    assert!(diff_result.source_reader.is_none());
    assert!(diff_result.source_cache_key.is_none());
    assert!(show_result.source_reader.is_none());
    assert!(show_result.source_cache_key.is_none());
}

#[cfg(unix)]
#[test]
fn merge_expands_exact_new_side_without_guessing_virtual_old_tree() {
    let repo = tempdir().unwrap();
    let binary = fake_jj(repo.path(), &repo.path().join("commands.log"));
    let capability = create_jujutsu_source_capability(
        repo.path(),
        &JujutsuDiffEndpoints {
            new_commit_id: "dddd".into(),
            old_commit_ids: vec!["aaaa".into(), "cccc".into()],
        },
        &binary.path().join("jj"),
    );
    assert!(
        capability
            .source_cache_key
            .contains("merged-parents:aaaa,cccc")
    );
    let request = VcsFileSourceRequest {
        path: "file.txt".into(),
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        is_untracked: false,
        side: ReviewSide::Old,
    };
    assert_eq!(
        source_content((capability.read_file_source)(&request)),
        None
    );
    assert_eq!(
        source_content((capability.read_file_source)(&VcsFileSourceRequest {
            side: ReviewSide::New,
            ..request
        }))
        .as_deref(),
        Some("merge result\n")
    );
}

#[cfg(unix)]
#[test]
fn product_loader_materializes_the_native_adapter_and_source_snapshots() {
    let repo = tempdir().unwrap();
    let binary = fake_jj(repo.path(), &repo.path().join("commands.log"));
    let changeset = load_jujutsu_changeset(
        &VcsReviewInput::Diff(diff_input()),
        &VcsLoadContext {
            cwd: repo.path().into(),
        },
        &JujutsuVcsAdapterOptions {
            jj_executable: binary.path().join("jj"),
        },
    )
    .unwrap();
    assert_eq!(changeset.id, "jj:diff");
    assert_eq!(changeset.files.len(), 1);
    assert_eq!(
        changeset.files[0]
            .sources
            .old
            .as_ref()
            .map(|source| source.content.as_str()),
        Some("old\ncontext\n")
    );
    assert_eq!(
        changeset.files[0]
            .sources
            .new
            .as_ref()
            .map(|source| source.content.as_str()),
        Some("two\ncontext\n")
    );
}
