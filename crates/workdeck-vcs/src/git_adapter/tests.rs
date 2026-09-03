use super::*;
use crate::VcsFileSourceRequest;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use workdeck_core::{CommonOptions, VcsRangeEndpoints};

fn git(repo_root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repo_root)
        .output()
        .expect("run Git fixture command");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn create_repo() -> TempDir {
    let directory = TempDir::new().unwrap();
    git(
        directory.path(),
        &["init", "-q", "--initial-branch", "master"],
    );
    git(directory.path(), &["config", "user.name", "Test User"]);
    git(
        directory.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(directory.path(), &["config", "commit.gpgsign", "false"]);
    directory
}

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

fn stash_input(reference: Option<&str>) -> VcsStashShowCommandInput {
    VcsStashShowCommandInput {
        reference: reference.map(str::to_owned),
        options: CommonOptions::default(),
    }
}

fn adapter() -> VcsAdapter {
    create_git_vcs_adapter(GitVcsAdapterOptions::default())
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

fn watch_plan(
    adapter: &VcsAdapter,
    kind: VcsReviewOperationKind,
    input: VcsReviewInput,
    cwd: &Path,
) -> Result<VcsWatchPlan, VcsCatalogError> {
    adapter.operations[&kind].watch_plan.as_ref().unwrap()(
        &input,
        &VcsLoadContext {
            cwd: cwd.to_owned(),
        },
    )
}

fn watch_signature(
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
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

fn source_content(result: Result<VcsFileSourceResult, VcsCatalogError>) -> Option<String> {
    match result.unwrap() {
        VcsFileSourceResult::Source(source) => Some(source.content),
        VcsFileSourceResult::Missing | VcsFileSourceResult::TooLarge { .. } => None,
    }
}

fn directory_target<'a>(plan: &'a VcsWatchPlan, directory: &Path) -> Option<&'a VcsWatchTarget> {
    plan.targets.iter().find(|target| match target {
        VcsWatchTarget::DirectoryTree {
            directory: candidate,
            ..
        }
        | VcsWatchTarget::DirectoryEntries {
            directory: candidate,
            ..
        } => comparable(candidate) == comparable(directory),
    })
}

#[test]
fn published_surface_implements_every_review_operation() {
    let adapter = adapter();
    for kind in [
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewOperationKind::RevisionShow,
        VcsReviewOperationKind::StashShow,
    ] {
        assert!(adapter.operations.contains_key(&kind));
    }
    assert_eq!(
        adapter.detection_priority,
        Some(GIT_VCS_DETECTION_BASELINE_PRIORITY)
    );
}

#[test]
fn detects_git_repositories_from_nested_directories() {
    let repo = create_repo();
    let nested = repo.path().join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(
        (adapter().detect)(&nested).unwrap(),
        Some(VcsDetection {
            id: "git".into(),
            repo_root: comparable(repo.path()),
        })
    );
}

#[test]
fn direct_adapter_call_rejects_option_like_endpoints() {
    let repo = create_repo();
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "--output=unsafe".into(),
    });
    let error = match load(
        &adapter(),
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input),
        repo.path(),
    ) {
        Ok(_) => panic!("option-like endpoint unexpectedly loaded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("looks like a Git option"));
}

#[test]
fn loads_working_tree_and_untracked_paths_through_neutral_operation() {
    let repo = create_repo();
    fs::write(repo.path().join("tracked.txt"), "old\n").unwrap();
    git(repo.path(), &["add", "tracked.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "initial"]);
    fs::write(repo.path().join("tracked.txt"), "new\n").unwrap();
    fs::write(repo.path().join("untracked.txt"), "fresh\n").unwrap();
    let adapter = adapter();
    let input = diff_input();
    let result = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input.clone()),
        repo.path(),
    )
    .unwrap();
    assert_eq!(comparable(&result.repo_root), comparable(repo.path()));
    assert!(result.title.contains("working tree"));
    assert!(
        result
            .patch_text
            .contains("diff --git a/tracked.txt b/tracked.txt")
    );
    assert!(result.patch_text.contains("+new"));
    assert!(
        result
            .untracked_paths
            .contains(&PathBuf::from("untracked.txt"))
    );
    let cache_key = result.source_cache_key.as_deref().unwrap();
    assert!(cache_key.contains("git-source-v1"));
    let equivalent = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input.clone()),
        repo.path(),
    )
    .unwrap();
    assert_eq!(equivalent.source_cache_key.as_deref(), Some(cache_key));
    let reader = result.source_reader.as_ref().unwrap();
    let request = VcsFileSourceRequest {
        path: "tracked.txt".into(),
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        is_untracked: false,
        side: ReviewSide::Old,
    };
    assert_eq!(source_content(reader(&request)), Some("old\n".into()));
    assert_eq!(
        source_content(reader(&VcsFileSourceRequest {
            side: ReviewSide::New,
            ..request
        })),
        Some("new\n".into())
    );
    git(repo.path(), &["add", "tracked.txt"]);
    let changed_index = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input),
        repo.path(),
    )
    .unwrap();
    assert_ne!(changed_index.source_cache_key.as_deref(), Some(cache_key));
    assert!(result.extra_files.is_empty());
}

#[test]
fn loads_two_revision_diff_with_exact_sources_and_no_untracked_paths() {
    let repo = create_repo();
    fs::write(repo.path().join("tracked.txt"), "old\ncontext\n").unwrap();
    git(repo.path(), &["add", "tracked.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "old"]);
    let from = git(repo.path(), &["rev-parse", "HEAD"]).trim().to_owned();
    fs::write(repo.path().join("tracked.txt"), "new\ncontext\n").unwrap();
    git(repo.path(), &["commit", "-q", "-am", "new"]);
    let to = git(repo.path(), &["rev-parse", "HEAD"]).trim().to_owned();
    fs::write(repo.path().join("untracked.txt"), "not in revisions\n").unwrap();
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: from.clone(),
        to: to.clone(),
    });
    let result = load(
        &adapter(),
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input),
        repo.path(),
    )
    .unwrap();
    assert!(result.title.contains(&format!("{from}..{to}")));
    assert!(result.untracked_paths.is_empty());
    let reader = result.source_reader.unwrap();
    let request = VcsFileSourceRequest {
        path: "tracked.txt".into(),
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        is_untracked: false,
        side: ReviewSide::Old,
    };
    assert_eq!(
        source_content(reader(&request)),
        Some("old\ncontext\n".into())
    );
    assert_eq!(
        source_content(reader(&VcsFileSourceRequest {
            side: ReviewSide::New,
            ..request
        })),
        Some("new\ncontext\n".into())
    );
}

#[test]
fn loads_revision_and_stash_patches_with_source_capabilities() {
    let repo = create_repo();
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "initial"]);
    fs::write(repo.path().join("file.txt"), "two\n").unwrap();
    git(repo.path(), &["commit", "-q", "-am", "change"]);
    let adapter = adapter();
    let shown = load(
        &adapter,
        VcsReviewOperationKind::RevisionShow,
        VcsReviewInput::Show(show_input(Some("HEAD"))),
        repo.path(),
    )
    .unwrap();
    assert!(shown.title.contains("show HEAD"));
    assert!(
        shown
            .patch_text
            .contains("diff --git a/file.txt b/file.txt")
    );
    assert!(shown.patch_text.contains("+two"));
    assert!(shown.source_cache_key.unwrap().contains("git-source-v1"));
    let reader = shown.source_reader.unwrap();
    let request = VcsFileSourceRequest {
        path: "file.txt".into(),
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        is_untracked: false,
        side: ReviewSide::Old,
    };
    assert_eq!(source_content(reader(&request)), Some("one\n".into()));
    assert_eq!(
        source_content(reader(&VcsFileSourceRequest {
            side: ReviewSide::New,
            ..request
        })),
        Some("two\n".into())
    );

    fs::write(repo.path().join("file.txt"), "three\n").unwrap();
    git(repo.path(), &["stash", "push", "-m", "adapter stash"]);
    let stash = load(
        &adapter,
        VcsReviewOperationKind::StashShow,
        VcsReviewInput::StashShow(stash_input(None)),
        repo.path(),
    )
    .unwrap();
    assert!(stash.title.contains("stash"));
    assert!(
        stash
            .patch_text
            .contains("diff --git a/file.txt b/file.txt")
    );
    assert!(stash.patch_text.contains("+three"));
    assert!(stash.source_cache_key.unwrap().contains("git-source-v1"));
}

#[test]
fn returns_none_when_no_git_marker_exists_to_filesystem_root() {
    let directory = TempDir::new().unwrap();
    assert_eq!((adapter().detect)(directory.path()).unwrap(), None);
}

#[test]
fn watch_plans_are_sensitive_to_worktree_and_metadata_operations() {
    let repo = create_repo();
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    fs::write(repo.path().join(".gitignore"), "generated/\n").unwrap();
    git(repo.path(), &["add", "file.txt", ".gitignore"]);
    git(repo.path(), &["commit", "-q", "-m", "initial"]);
    fs::create_dir_all(repo.path().join("generated/nested")).unwrap();
    fs::write(repo.path().join("generated/nested/output.js"), "ignored\n").unwrap();
    let adapter = adapter();
    let unstaged = watch_plan(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(diff_input()),
        repo.path(),
    )
    .unwrap();
    let worktree = directory_target(&unstaged, repo.path()).unwrap();
    let VcsWatchTarget::DirectoryTree {
        ignored_roots,
        sources,
        ..
    } = worktree
    else {
        panic!("worktree target must recurse");
    };
    assert_eq!(
        ignored_roots
            .iter()
            .map(|path| comparable(path))
            .collect::<Vec<_>>(),
        vec![
            comparable(&repo.path().join(".git")),
            comparable(&repo.path().join("generated")),
        ]
    );
    assert!(sources.contains(&VcsWatchTargetSource::Worktree));
    let metadata = unstaged
        .targets
        .iter()
        .filter(|target| match target {
            VcsWatchTarget::DirectoryTree { sources, .. }
            | VcsWatchTarget::DirectoryEntries { sources, .. } => {
                sources.contains(&VcsWatchTargetSource::VcsMetadata)
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(metadata.len(), 1);
    let VcsWatchTarget::DirectoryTree {
        directory,
        ignored_roots,
        ..
    } = metadata[0]
    else {
        panic!("metadata target must recurse");
    };
    assert_eq!(comparable(directory), comparable(&repo.path().join(".git")));
    assert_eq!(
        ignored_roots
            .iter()
            .map(|path| comparable(path))
            .collect::<Vec<_>>(),
        vec![comparable(&repo.path().join(".git/objects"))]
    );

    let mut single = diff_input();
    single.range = Some("HEAD".into());
    single.pathspecs = vec!["file.txt".into()];
    let single_plan = watch_plan(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(single),
        repo.path(),
    )
    .unwrap();
    assert!(directory_target(&single_plan, repo.path()).is_some());

    let mut staged = diff_input();
    staged.staged = true;
    let mut range = diff_input();
    range.range = Some("HEAD^..HEAD".into());
    for input in [staged, range] {
        let plan = watch_plan(
            &adapter,
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewInput::Diff(input),
            repo.path(),
        )
        .unwrap();
        assert!(directory_target(&plan, repo.path()).is_none());
        assert!(plan.targets.iter().any(|target| match target {
            VcsWatchTarget::DirectoryTree { sources, .. }
            | VcsWatchTarget::DirectoryEntries { sources, .. } => {
                sources.contains(&VcsWatchTargetSource::VcsMetadata)
            }
        }));
    }
}

#[test]
fn stash_watch_plan_keeps_reflog_metadata_observable() {
    let repo = create_repo();
    let plan = watch_plan(
        &adapter(),
        VcsReviewOperationKind::StashShow,
        VcsReviewInput::StashShow(stash_input(Some("stash@{1}"))),
        repo.path(),
    )
    .unwrap();
    let target = plan
        .targets
        .iter()
        .find(|target| match target {
            VcsWatchTarget::DirectoryTree { sources, .. }
            | VcsWatchTarget::DirectoryEntries { sources, .. } => {
                sources.contains(&VcsWatchTargetSource::VcsMetadata)
            }
        })
        .unwrap();
    let VcsWatchTarget::DirectoryTree {
        directory,
        ignored_roots,
        ..
    } = target
    else {
        panic!("metadata target must recurse");
    };
    assert_eq!(comparable(directory), comparable(&repo.path().join(".git")));
    assert_eq!(
        ignored_roots
            .iter()
            .map(|path| comparable(path))
            .collect::<Vec<_>>(),
        vec![comparable(&repo.path().join(".git/objects"))]
    );
}

#[test]
fn linked_worktree_watch_plan_deduplicates_common_metadata() {
    let repo = create_repo();
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "initial"]);
    let linked = TempDir::new().unwrap();
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            linked.path().to_str().unwrap(),
            "-b",
            "linked-plan",
        ],
    );
    let plan = watch_plan(
        &adapter(),
        VcsReviewOperationKind::RevisionShow,
        VcsReviewInput::Show(show_input(Some("HEAD"))),
        linked.path(),
    )
    .unwrap();
    let metadata = plan
        .targets
        .iter()
        .filter(|target| match target {
            VcsWatchTarget::DirectoryTree { sources, .. }
            | VcsWatchTarget::DirectoryEntries { sources, .. } => {
                sources.contains(&VcsWatchTargetSource::VcsMetadata)
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(metadata.len(), 1);
    let common_output = git(linked.path(), &["rev-parse", "--git-common-dir"])
        .trim()
        .to_owned();
    let common = if Path::new(&common_output).is_absolute() {
        PathBuf::from(common_output)
    } else {
        linked.path().join(common_output)
    };
    let VcsWatchTarget::DirectoryTree {
        directory,
        ignored_roots,
        ..
    } = metadata[0]
    else {
        panic!("metadata target must recurse");
    };
    assert_eq!(comparable(directory), comparable(&common));
    assert_eq!(ignored_roots, &vec![directory.join("objects")]);
}

#[test]
fn computes_watch_signatures_for_every_review_operation() {
    let repo = create_repo();
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "initial"]);
    fs::write(repo.path().join("file.txt"), "two\n").unwrap();
    fs::write(repo.path().join("untracked.txt"), "fresh\n").unwrap();
    let adapter = adapter();
    let diff = watch_signature(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(diff_input()),
        repo.path(),
    )
    .unwrap();
    assert!(diff.contains("diff --git a/file.txt b/file.txt"));
    assert!(diff.contains("untracked:"));
    let show = watch_signature(
        &adapter,
        VcsReviewOperationKind::RevisionShow,
        VcsReviewInput::Show(show_input(Some("HEAD"))),
        repo.path(),
    )
    .unwrap();
    assert!(show.contains("diff --git"));
    git(
        repo.path(),
        &["stash", "push", "--include-untracked", "-m", "watch stash"],
    );
    let stash = watch_signature(
        &adapter,
        VcsReviewOperationKind::StashShow,
        VcsReviewInput::StashShow(stash_input(None)),
        repo.path(),
    )
    .unwrap();
    assert!(stash.contains("diff --git"));
}

#[test]
fn stat_signature_distinguishes_present_and_missing_paths() {
    let directory = TempDir::new().unwrap();
    let present = directory.path().join("present.txt");
    fs::write(&present, "data\n").unwrap();
    assert!(stat_signature(&present).starts_with(&format!("{}:", present.display())));
    assert!(!stat_signature(&present).contains(":missing"));
    let missing = directory.path().join("absent.txt");
    assert_eq!(
        stat_signature(&missing),
        format!("{}:missing", missing.display())
    );
}
