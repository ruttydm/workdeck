use git2::{Repository, RepositoryInitOptions, Signature};
use std::{fs, path::Path};
use tempfile::TempDir;
use workdeck_cli::repository_panels::RepositoryPanels;
use workdeck_tui::workbench::{PanelPage, PanelRequest, PanelTarget, RepositoryPanelProvider};

fn commit(repo: &Repository, message: &str) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = Signature::now("Synthetic test", "test@example.invalid").unwrap();
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parent.iter().collect::<Vec<_>>(),
    )
    .unwrap()
}
fn fixture() -> (TempDir, Repository, git2::Oid) {
    let temp = TempDir::new().unwrap();
    let mut options = RepositoryInitOptions::new();
    options.initial_head("main");
    let repo = Repository::init_opts(temp.path(), &options).unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/tracked.txt"), "one\n").unwrap();
    let oid = commit(&repo, "Initial synthetic commit");
    (temp, repo, oid)
}
fn request(page: PanelPage) -> PanelRequest {
    PanelRequest {
        page,
        directory: String::new(),
        query: String::new(),
        limit: 100,
    }
}

#[test]
fn changes_separate_staged_and_unstaged_stats_and_preview_without_writing_index() {
    let (temp, repo, _) = fixture();
    let path = temp.path().join("src/tracked.txt");
    fs::write(&path, "one\nstaged\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("src/tracked.txt")).unwrap();
    index.write().unwrap();
    fs::write(&path, "one\nstaged\nunstaged\n").unwrap();
    fs::write(temp.path().join("new.txt"), "new content\n").unwrap();
    let before = fs::read(repo.path().join("index")).unwrap();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    let snapshot = provider.load(&request(PanelPage::Changes)).unwrap();
    for staged in [true, false] {
        let target = PanelTarget::Change {
            path: "src/tracked.txt".into(),
            staged,
        };
        let row = snapshot
            .entries
            .iter()
            .find(|entry| entry.target == target)
            .unwrap();
        assert_eq!(row.changes.as_ref().unwrap().additions, 1);
        assert_eq!(row.changes.as_ref().unwrap().deletions, 0);
        assert!(row.section.contains("src"));
        let preview = provider.preview(&target).unwrap();
        assert!(
            preview
                .body
                .contains(if staged { "+staged" } else { "+unstaged" })
        );
    }
    assert!(snapshot.entries.iter().any(|entry| entry.target
        == PanelTarget::Change {
            path: "new.txt".into(),
            staged: false
        }));
    assert_eq!(fs::read(repo.path().join("index")).unwrap(), before);
    let mut limited = request(PanelPage::Changes);
    limited.limit = 1;
    let limited = provider.load(&limited).unwrap();
    assert_eq!(limited.entries.len(), 1);
    assert!(limited.truncated);
}

#[test]
fn git_overview_and_typed_previews_include_actual_repository_content() {
    let (temp, mut repo, initial) = fixture();
    repo.branch("topic", &repo.find_commit(initial).unwrap(), false)
        .unwrap();
    repo.tag_lightweight("v1", &repo.find_object(initial, None).unwrap(), false)
        .unwrap();
    repo.remote("origin", "https://example.invalid/repository.git")
        .unwrap();
    fs::write(temp.path().join("src/tracked.txt"), "stash content\n").unwrap();
    let signature = Signature::now("Synthetic test", "test@example.invalid").unwrap();
    let stash = repo.stash_save(&signature, "saved work", None).unwrap();
    let provider = RepositoryPanels::new(temp.path(), Some("main".into()), 10).unwrap();
    let snapshot = provider.load(&request(PanelPage::Git)).unwrap();
    for section in [
        "Summary",
        "Branches",
        "Recent commits",
        "Stashes",
        "Tags",
        "Remotes",
    ] {
        assert!(
            snapshot
                .entries
                .iter()
                .any(|entry| entry.section == section),
            "{section}: {:?}",
            snapshot.entries
        );
    }
    let commit = provider
        .preview(&PanelTarget::Commit {
            reference: initial.to_string(),
        })
        .unwrap();
    assert!(commit.body.contains("Initial synthetic commit"));
    assert!(commit.body.contains("+one"));
    let stash = provider
        .preview(&PanelTarget::Stash {
            reference: stash.to_string(),
        })
        .unwrap();
    assert!(stash.body.contains("stash content"));
    let branch = provider
        .preview(&PanelTarget::Branch {
            reference: "refs/heads/topic".into(),
        })
        .unwrap();
    assert!(branch.body.contains("topic"));
    let remote = provider
        .preview(&PanelTarget::Remote {
            name: "origin".into(),
        })
        .unwrap();
    assert!(remote.body.contains("example.invalid/repository.git"));
}

#[test]
fn git_panels_reject_outside_workdir_and_arbitrary_ref_or_path_syntax() {
    let (temp, _, _) = fixture();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    for target in [
        PanelTarget::Change {
            path: "../outside".into(),
            staged: false,
        },
        PanelTarget::Change {
            path: ".git/config".into(),
            staged: false,
        },
        PanelTarget::Commit {
            reference: "HEAD; touch sentinel".into(),
        },
        PanelTarget::Branch {
            reference: "refs/heads/main^{tree}".into(),
        },
        PanelTarget::Stash {
            reference: "stash@{0}".into(),
        },
    ] {
        assert!(provider.preview(&target).is_err(), "{target:?}");
    }
    let nested = temp.path().join("src");
    let provider = RepositoryPanels::new(&nested, None, 10).unwrap();
    assert!(provider.load(&request(PanelPage::Changes)).is_err());
}

#[test]
fn previews_are_bounded_binary_aware_and_literal_path_scoped() {
    let (temp, repo, _) = fixture();
    fs::write(temp.path().join("[literal].txt"), "before literal\n").unwrap();
    fs::write(temp.path().join("l.txt"), "other file\n").unwrap();
    fs::write(temp.path().join("binary.dat"), [0, 1, 2, 3]).unwrap();
    commit(&repo, "Add paths and binary");
    fs::write(temp.path().join("[literal].txt"), "after literal\n").unwrap();
    fs::write(temp.path().join("l.txt"), "unrelated secret\n").unwrap();
    fs::write(temp.path().join("binary.dat"), [0, 4, 5, 6]).unwrap();
    fs::write(
        temp.path().join("src/tracked.txt"),
        "a long changed line\n".repeat(50_000),
    )
    .unwrap();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    let literal = provider
        .preview(&PanelTarget::Change {
            path: "[literal].txt".into(),
            staged: false,
        })
        .unwrap();
    assert!(literal.body.contains("+after literal"));
    assert!(!literal.body.contains("unrelated secret"));
    let binary = provider
        .preview(&PanelTarget::Change {
            path: "binary.dat".into(),
            staged: false,
        })
        .unwrap();
    assert!(binary.binary, "{binary:?}");
    let large = provider
        .preview(&PanelTarget::Change {
            path: "src/tracked.txt".into(),
            staged: false,
        })
        .unwrap();
    assert!(large.truncated);
    assert!(large.body.len() <= 512 * 1024);
    let mut scoped = request(PanelPage::Changes);
    scoped.directory = "src".into();
    scoped.query = "TRACKED".into();
    let scoped = provider.load(&scoped).unwrap();
    assert_eq!(scoped.entries.len(), 1);
    assert_eq!(scoped.entries[0].label, "src/tracked.txt");
}

#[test]
fn unborn_repository_and_ambiguous_bases_have_explicit_results() {
    let temp = TempDir::new().unwrap();
    Repository::init(temp.path()).unwrap();
    fs::write(temp.path().join("new.txt"), "new content\n").unwrap();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    assert_eq!(
        provider
            .load(&request(PanelPage::Changes))
            .unwrap()
            .entries
            .len(),
        1
    );
    assert!(
        provider
            .preview(&PanelTarget::GitSummary)
            .unwrap()
            .body
            .contains("unborn")
    );

    let (temp, repo, initial) = fixture();
    fs::write(temp.path().join("src/tracked.txt"), "second\n").unwrap();
    commit(&repo, "Second commit");
    repo.tag_lightweight("main", &repo.find_object(initial, None).unwrap(), false)
        .unwrap();
    for base in ["main", "HEAD~1", "refs/heads/main^{tree}"] {
        let provider = RepositoryPanels::new(temp.path(), Some(base.into()), 10).unwrap();
        assert!(provider.load(&request(PanelPage::Git)).is_err(), "{base}");
    }
    let provider = RepositoryPanels::new(temp.path(), Some("refs/heads/main".into()), 10).unwrap();
    assert!(provider.load(&request(PanelPage::Git)).is_ok());
}

#[test]
fn stash_oid_remains_stable_and_remote_credentials_are_hidden() {
    let (temp, mut repo, _) = fixture();
    let signature = Signature::now("Synthetic test", "test@example.invalid").unwrap();
    fs::write(temp.path().join("src/tracked.txt"), "first saved content\n").unwrap();
    let first = repo.stash_save(&signature, "first stash", None).unwrap();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    let snapshot = provider.load(&request(PanelPage::Git)).unwrap();
    let target = snapshot
        .entries
        .into_iter()
        .find(|entry| {
            entry.target
                == PanelTarget::Stash {
                    reference: first.to_string(),
                }
        })
        .unwrap()
        .target;
    fs::write(
        temp.path().join("src/tracked.txt"),
        "second saved content\n",
    )
    .unwrap();
    repo.stash_save(&signature, "second stash", None).unwrap();
    let preview = provider.preview(&target).unwrap();
    assert!(preview.body.contains("first saved content"));
    assert!(!preview.body.contains("second saved content"));
    repo.remote(
        "origin",
        "https://synthetic:secret-token@example.invalid/repo.git?access_token=another-secret",
    )
    .unwrap();
    let preview = provider
        .preview(&PanelTarget::Remote {
            name: "origin".into(),
        })
        .unwrap();
    assert!(preview.body.contains("https://example.invalid/repo.git"));
    assert!(!preview.body.contains("secret"));
    let snapshot = provider.load(&request(PanelPage::Git)).unwrap();
    assert!(!format!("{snapshot:?}").contains("secret"));
}

#[test]
fn configured_external_helpers_are_never_executed_or_index_written() {
    let (temp, repo, _) = fixture();
    fs::write(
        temp.path().join(".gitattributes"),
        "*.txt diff=unsafe filter=unsafe\n",
    )
    .unwrap();
    let marker = temp.path().join("helper-was-executed");
    let command = format!("touch '{}'; cat", marker.display());
    let mut config = repo.config().unwrap();
    for key in [
        "diff.external",
        "diff.unsafe.command",
        "diff.unsafe.textconv",
        "core.fsmonitor",
        "core.pager",
        "filter.unsafe.clean",
        "filter.unsafe.smudge",
    ] {
        config.set_str(key, &command).unwrap();
    }
    config
        .set_str("core.hooksPath", temp.path().to_str().unwrap())
        .unwrap();
    fs::write(
        temp.path().join("src/tracked.txt"),
        "after configured helpers\n",
    )
    .unwrap();
    let before_index = fs::read(repo.path().join("index")).unwrap();
    let before_head = fs::read(repo.path().join("HEAD")).unwrap();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    provider.load(&request(PanelPage::Changes)).unwrap();
    provider.load(&request(PanelPage::Git)).unwrap();
    let preview = provider
        .preview(&PanelTarget::Change {
            path: "src/tracked.txt".into(),
            staged: false,
        })
        .unwrap();
    assert!(preview.body.contains("after configured helpers"));
    assert!(!marker.exists());
    assert_eq!(fs::read(repo.path().join("index")).unwrap(), before_index);
    assert_eq!(fs::read(repo.path().join("HEAD")).unwrap(), before_head);
    assert_eq!(
        fs::read_to_string(temp.path().join("src/tracked.txt")).unwrap(),
        "after configured helpers\n"
    );
}

#[test]
fn ambient_git_environment_worker() {
    let Some(root) = std::env::var_os("WORKDECK_GIT_PANEL_TEST_ROOT") else {
        return;
    };
    let provider = RepositoryPanels::new(root, None, 10).unwrap();
    let snapshot = provider.load(&request(PanelPage::Changes)).unwrap();
    assert!(
        snapshot
            .entries
            .iter()
            .any(|entry| entry.label == "src/tracked.txt")
    );
    assert!(
        !snapshot
            .entries
            .iter()
            .any(|entry| entry.label == "foreign.txt")
    );
    let summary = provider.preview(&PanelTarget::GitSummary).unwrap();
    assert!(
        summary
            .body
            .contains(&std::env::var("WORKDECK_GIT_PANEL_TEST_HEAD").unwrap())
    );
    let preview = provider
        .preview(&PanelTarget::Change {
            path: "src/tracked.txt".into(),
            staged: true,
        })
        .unwrap();
    assert!(preview.body.contains("own staged content"));
}

#[test]
fn ambient_git_environment_cannot_redirect_repository_worktree_or_index() {
    let (temp, repo, initial) = fixture();
    fs::write(temp.path().join("src/tracked.txt"), "own staged content\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("src/tracked.txt")).unwrap();
    index.write().unwrap();
    let foreign = TempDir::new().unwrap();
    let foreign_repo = Repository::init(foreign.path()).unwrap();
    fs::write(foreign.path().join("foreign.txt"), "foreign data\n").unwrap();
    commit(&foreign_repo, "Foreign repository");
    let before = fs::read(repo.path().join("index")).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "ambient_git_environment_worker", "--nocapture"])
        .env("WORKDECK_GIT_PANEL_TEST_ROOT", temp.path())
        .env("WORKDECK_GIT_PANEL_TEST_HEAD", initial.to_string())
        .env("GIT_DIR", foreign_repo.path())
        .env("GIT_WORK_TREE", foreign.path())
        .env("GIT_INDEX_FILE", foreign_repo.path().join("index"))
        .env("GIT_NAMESPACE", "foreign")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(repo.path().join("index")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn symlinked_worktree_paths_do_not_read_external_content() {
    use std::os::unix::fs::symlink;
    let (temp, _, _) = fixture();
    let outside = TempDir::new().unwrap();
    fs::write(
        outside.path().join("tracked.txt"),
        "external content must remain unread\n",
    )
    .unwrap();
    fs::remove_file(temp.path().join("src/tracked.txt")).unwrap();
    fs::remove_dir(temp.path().join("src")).unwrap();
    symlink(outside.path(), temp.path().join("src")).unwrap();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    if let Ok(preview) = provider.preview(&PanelTarget::Change {
        path: "src/tracked.txt".into(),
        staged: false,
    }) {
        assert!(!preview.body.contains("external content must remain unread"));
    }
    let changes = provider.load(&request(PanelPage::Changes)).unwrap_err();
    assert!(changes.message.contains("attribute source"));
    assert!(
        !changes
            .message
            .contains("external content must remain unread")
    );
}

#[test]
fn configured_worktree_redirect_and_bare_repository_are_rejected() {
    let (temp, repo, _) = fixture();
    let outside = TempDir::new().unwrap();
    repo.config()
        .unwrap()
        .set_str("core.worktree", outside.path().to_str().unwrap())
        .unwrap();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    assert!(provider.load(&request(PanelPage::Git)).is_err());
    assert!(provider.load(&request(PanelPage::Changes)).is_err());
    let bare = TempDir::new().unwrap();
    Repository::init_bare(bare.path()).unwrap();
    let provider = RepositoryPanels::new(bare.path(), None, 10).unwrap();
    assert!(provider.load(&request(PanelPage::Git)).is_err());
}

#[test]
fn staged_rename_preview_preserves_old_and_new_path_identity() {
    let (temp, repo, _) = fixture();
    fs::rename(
        temp.path().join("src/tracked.txt"),
        temp.path().join("renamed.txt"),
    )
    .unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("src/tracked.txt")).unwrap();
    index.add_path(Path::new("renamed.txt")).unwrap();
    index.write().unwrap();
    let provider = RepositoryPanels::new(temp.path(), None, 10).unwrap();
    let snapshot = provider.load(&request(PanelPage::Changes)).unwrap();
    let target = PanelTarget::Change {
        path: "renamed.txt".into(),
        staged: true,
    };
    let change = snapshot
        .entries
        .iter()
        .find(|entry| entry.target == target)
        .unwrap();
    assert_eq!(change.changes.as_ref().unwrap().additions, 0);
    let preview = provider.preview(&target).unwrap();
    assert!(
        preview.body.contains("rename from src/tracked.txt"),
        "{}",
        preview.body
    );
    assert!(preview.body.contains("rename to renamed.txt"));
}

#[test]
fn cycling_the_comparison_base_rotates_through_snapshot_branches_and_wraps() {
    let (temp, repo, initial) = fixture();
    let start = repo.find_commit(initial).unwrap();
    repo.branch("feature", &start, false).unwrap();
    repo.branch("release", &start, false).unwrap();
    repo.remote("origin", "https://example.invalid/repository.git")
        .unwrap();
    repo.reference("refs/remotes/origin/main", initial, false, "fixture remote")
        .unwrap();
    commit(&repo, "Advance the current branch");
    let provider = RepositoryPanels::new(temp.path(), Some("origin/main".into()), 10)
        .unwrap()
        .with_git_base_key(Some("b".into()));
    assert_eq!(provider.git_base_key().as_deref(), Some("b"));
    let snapshot = provider.load(&request(PanelPage::Git)).unwrap();
    let mut expected = vec!["origin/main".to_string()];
    let extra: Vec<String> = snapshot
        .entries
        .iter()
        .filter(|entry| matches!(entry.target, PanelTarget::Branch { .. }))
        .map(|entry| entry.label.clone())
        .filter(|label| label != "main" && !expected.contains(label))
        .collect();
    expected.extend(extra);
    assert_eq!(expected.len(), 3, "{expected:?}");

    let next = provider.cycle_git_base_branch().unwrap();
    assert_eq!(next, expected[1]);
    let summary = provider.preview(&PanelTarget::GitSummary).unwrap();
    assert!(
        summary.body.contains(&format!("Base: {}", expected[1])),
        "{}",
        summary.body
    );
    assert!(
        summary.body.contains("Ahead: 1; behind: 0"),
        "{}",
        summary.body
    );

    assert_eq!(provider.cycle_git_base_branch().unwrap(), expected[2]);
    // The rotation mirrors the upstream dashboard exactly: the selected base
    // leads the candidate list, so the next press returns to the first
    // alternate instead of the configured base.
    assert_eq!(provider.cycle_git_base_branch().unwrap(), expected[1]);
    // A cloned provider retains the session-local selection.
    assert_eq!(
        provider.clone().cycle_git_base_branch().unwrap(),
        expected[2]
    );

    let (single, _, _) = fixture();
    let provider = RepositoryPanels::new(single.path(), None, 10).unwrap();
    let failure = provider.cycle_git_base_branch().unwrap_err();
    assert_eq!(failure.message, "no alternate base branches available");
}
