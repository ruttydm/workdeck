use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use workdeck_core::{CommonOptions, VcsRangeEndpoints};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn make_git_input() -> VcsDiffCommandInput {
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

fn context(cwd: &Path) -> GitCommandContext {
    GitCommandContext {
        cwd: cwd.to_owned(),
        ..GitCommandContext::default()
    }
}

fn rooted_context(root: &Path) -> GitCommandContext {
    GitCommandContext {
        cwd: root.to_owned(),
        repo_root: Some(root.to_owned()),
        ..GitCommandContext::default()
    }
}

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
    git(directory.path(), &["init", "-q"]);
    git(directory.path(), &["config", "user.name", "Test User"]);
    git(
        directory.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(directory.path(), &["config", "commit.gpgSign", "false"]);
    directory
}

fn commit_file(repo: &Path, contents: &str, message: &str) -> String {
    fs::write(repo.join("x.txt"), contents).unwrap();
    git(repo, &["add", "x.txt"]);
    git(repo, &["commit", "-q", "-m", message]);
    git(repo, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn comparable(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

#[test]
fn enables_deterministic_color_moved_output_for_patch_parsing() {
    let arguments = build_git_diff_args(
        &make_git_input(),
        &[],
        Some(&GitColorMovedOptions {
            mode: "zebra".into(),
            whitespace_mode: Some("allow-indentation-change".into()),
        }),
    )
    .unwrap();
    for expected in [
        "--color=always",
        "--color-moved=zebra",
        "--color-moved-ws=allow-indentation-change",
        "color.diff.oldMoved=magenta bold",
        "color.diff.newMoved=cyan bold",
    ] {
        assert!(arguments.iter().any(|argument| argument == expected));
    }
    assert!(!arguments.iter().any(|argument| argument == "--no-color"));
}

#[test]
fn forces_byte_safe_git_path_quoting_before_stdout_decoding() {
    assert!(
        build_git_diff_args(&make_git_input(), &[], None)
            .unwrap()
            .iter()
            .any(|argument| argument == "core.quotePath=true")
    );
}

#[test]
fn spells_two_named_revisions_as_exact_git_range_arguments() {
    let mut input = make_git_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "feature".into(),
    });
    let prefix = strings(DIFF_PREFIX_NORMALIZATION_ARGS);
    assert_eq!(
        build_git_diff_args(&input, &[], None).unwrap(),
        prefix
            .iter()
            .cloned()
            .chain(strings(&[
                "diff",
                "--no-ext-diff",
                "--find-renames",
                "--no-color",
                "main..feature",
            ]))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        build_git_diff_numstat_args(&input).unwrap(),
        prefix
            .into_iter()
            .chain(strings(&[
                "diff",
                "--no-ext-diff",
                "--find-renames",
                "--no-color",
                "--numstat",
                "-z",
                "main..feature",
            ]))
            .collect::<Vec<_>>()
    );
}

#[test]
fn rejects_option_like_endpoints_before_commands_probes_or_source_resolution() {
    for (from, to) in [
        ("--output=/tmp/from", "feature"),
        ("main", "--output=/tmp/to"),
    ] {
        let mut input = make_git_input();
        input.range_endpoints = Some(VcsRangeEndpoints {
            from: from.into(),
            to: to.into(),
        });
        assert!(
            build_git_diff_args(&input, &[], None)
                .unwrap_err()
                .to_string()
                .contains("looks like a Git option")
        );
        assert!(
            list_git_untracked_files(&input, &GitCommandContext::default())
                .unwrap_err()
                .to_string()
                .contains("looks like a Git option")
        );
        assert!(
            resolve_git_diff_endpoints(&input, &GitCommandContext::default())
                .unwrap_err()
                .to_string()
                .contains("looks like a Git option")
        );
    }
    for (from, to) in [("", "feature"), ("main", "")] {
        let mut input = make_git_input();
        input.range_endpoints = Some(VcsRangeEndpoints {
            from: from.into(),
            to: to.into(),
        });
        for message in [
            build_git_diff_args(&input, &[], None)
                .unwrap_err()
                .to_string(),
            list_git_untracked_files(&input, &GitCommandContext::default())
                .unwrap_err()
                .to_string(),
            resolve_git_diff_endpoints(&input, &GitCommandContext::default())
                .unwrap_err()
                .to_string(),
        ] {
            assert!(message.contains("empty revision"));
        }
    }
}

#[test]
fn refuses_option_like_revision_values_before_spawning_git() {
    let mut input = make_git_input();
    input.range = Some("--output=/tmp/workdeck-poc".into());
    assert!(
        build_git_diff_args(&input, &[], None)
            .unwrap_err()
            .to_string()
            .contains("looks like a Git option")
    );
    input.range = Some("-R".into());
    assert!(
        build_git_diff_numstat_args(&input)
            .unwrap_err()
            .to_string()
            .contains("looks like a Git option")
    );
    for error in [
        build_git_show_args(&show_input(Some("--output=/tmp/workdeck-poc")), None).unwrap_err(),
        build_git_stash_show_args(&stash_input(Some("--output=/tmp/workdeck-poc")), None)
            .unwrap_err(),
    ] {
        assert!(error.to_string().contains("looks like a Git option"));
    }
    input.range = Some("--output=/tmp/workdeck-poc".into());
    assert!(
        list_git_untracked_files(&input, &GitCommandContext::default())
            .unwrap_err()
            .to_string()
            .contains("looks like a Git option")
    );
    input.range = Some("main..feature".into());
    assert!(
        build_git_diff_args(&input, &[], None)
            .unwrap()
            .iter()
            .any(|argument| argument == "main..feature")
    );
}

#[test]
fn stash_patch_disables_external_diff_tools() {
    assert!(
        build_git_stash_show_args(&stash_input(None), None)
            .unwrap()
            .iter()
            .any(|argument| argument == "--no-ext-diff")
    );
}

#[test]
fn status_query_prevents_optional_index_locks() {
    assert_eq!(
        build_git_status_args(&make_git_input()),
        strings(&[
            "--no-optional-locks",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ])
    );
}

#[test]
fn builds_the_collapsed_ignored_directory_query() {
    assert_eq!(
        build_git_ignored_directory_args(),
        strings(&[
            "ls-files",
            "--full-name",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ])
    );
}

#[test]
fn parses_only_unique_nul_delimited_collapsed_directory_roots() {
    let root = std::env::temp_dir().join("workdeck-ignored-parser");
    assert_eq!(
        parse_git_ignored_directory_roots(
            "dependencies/\0ignored.log\0build/nested/\0dependencies/\0",
            &root,
        ),
        vec![root.join("dependencies"), root.join("build/nested")]
    );
}

#[test]
fn missing_git_executable_has_a_friendly_user_error() {
    let git_context = GitCommandContext {
        git_executable: "definitely-not-a-real-git-binary".into(),
        ..GitCommandContext::default()
    };
    let error = run_git_text(
        &GitBackedInput::from(&make_git_input()),
        &strings(&["status"]),
        &git_context,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Git is required for `workdeck diff`, but `definitely-not-a-real-git-binary` was not found in PATH."
    );
}

#[test]
fn ignored_discovery_collapses_dependencies_without_pruning_visible_paths() {
    let repo = create_repo();
    fs::write(
        repo.path().join(".gitignore"),
        "node_modules/\nignored.log\n",
    )
    .unwrap();
    for index in 0..25 {
        let package = repo
            .path()
            .join("node_modules")
            .join(format!("package-{index}"))
            .join("cache");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("index.js"), format!("{index}\n")).unwrap();
    }
    fs::write(repo.path().join("ignored.log"), "ignored file\n").unwrap();
    fs::create_dir(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/untracked.ts"), "export {};\n").unwrap();
    let roots = list_git_ignored_directory_roots(
        &GitBackedInput::from(&make_git_input()),
        &context(&repo.path().join("src")),
    );
    assert_eq!(
        roots
            .iter()
            .map(|path| comparable(path))
            .collect::<Vec<_>>(),
        vec![comparable(&repo.path().join("node_modules"))]
    );
    assert!(!roots.iter().any(|root| root.ends_with("src")));
}

#[test]
fn ignored_discovery_honors_nested_negation() {
    let repo = create_repo();
    fs::write(
        repo.path().join(".gitignore"),
        "generated/*\n!generated/.gitignore\n!generated/keep/\n",
    )
    .unwrap();
    fs::create_dir_all(repo.path().join("generated/discard")).unwrap();
    fs::create_dir_all(repo.path().join("generated/keep")).unwrap();
    fs::write(
        repo.path().join("generated/.gitignore"),
        "keep/*\n!keep/visible.txt\n",
    )
    .unwrap();
    fs::write(repo.path().join("generated/discard/output.js"), "ignored\n").unwrap();
    fs::write(repo.path().join("generated/keep/hidden.txt"), "ignored\n").unwrap();
    fs::write(repo.path().join("generated/keep/visible.txt"), "visible\n").unwrap();
    let roots = list_git_ignored_directory_roots(
        &GitBackedInput::from(&make_git_input()),
        &context(repo.path()),
    );
    assert_eq!(
        roots
            .iter()
            .map(|path| comparable(path))
            .collect::<Vec<_>>(),
        vec![comparable(&repo.path().join("generated/discard"))]
    );
}

#[test]
fn ignored_discovery_honors_repository_local_excludes() {
    let repo = create_repo();
    fs::write(repo.path().join(".git/info/exclude"), "local-cache/\n").unwrap();
    fs::create_dir_all(repo.path().join("local-cache/nested")).unwrap();
    fs::write(repo.path().join("local-cache/nested/data"), "ignored\n").unwrap();
    let roots = list_git_ignored_directory_roots(
        &GitBackedInput::from(&make_git_input()),
        &context(repo.path()),
    );
    assert_eq!(
        roots
            .iter()
            .map(|path| comparable(path))
            .collect::<Vec<_>>(),
        vec![comparable(&repo.path().join("local-cache"))]
    );
}

#[test]
fn ignored_discovery_does_not_prune_a_forced_tracked_descendant() {
    let repo = create_repo();
    fs::write(repo.path().join(".gitignore"), "vendor/\n").unwrap();
    fs::create_dir_all(repo.path().join("vendor/generated")).unwrap();
    fs::write(repo.path().join("vendor/tracked.txt"), "tracked\n").unwrap();
    fs::write(repo.path().join("vendor/generated/output.txt"), "ignored\n").unwrap();
    git(repo.path(), &["add", ".gitignore"]);
    git(repo.path(), &["add", "-f", "vendor/tracked.txt"]);
    git(repo.path(), &["commit", "-q", "-m", "track forced file"]);
    let roots = list_git_ignored_directory_roots(
        &GitBackedInput::from(&make_git_input()),
        &context(repo.path()),
    );
    assert_eq!(
        roots
            .iter()
            .map(|path| comparable(path))
            .collect::<Vec<_>>(),
        vec![comparable(&repo.path().join("vendor/generated"))]
    );
    let tracked = comparable(&repo.path().join("vendor/tracked.txt"));
    assert!(!roots.iter().any(|root| tracked.starts_with(root)));
}

#[test]
fn ignored_discovery_failure_falls_back_to_no_pruning() {
    let directory = TempDir::new().unwrap();
    let git_context = GitCommandContext {
        git_executable: "definitely-not-a-real-git-binary".into(),
        ..rooted_context(directory.path())
    };
    assert!(
        list_git_ignored_directory_roots(&GitBackedInput::from(&make_git_input()), &git_context,)
            .is_empty()
    );
}

#[test]
fn resolves_normal_and_linked_worktree_metadata_directories() {
    let repo = create_repo();
    commit_file(repo.path(), "x\n", "initial");
    let normal = resolve_git_metadata(
        &GitBackedInput::from(&make_git_input()),
        &context(repo.path()),
    )
    .unwrap();
    assert_eq!(comparable(&normal.repo_root), comparable(repo.path()));
    assert_eq!(normal.git_dir, normal.common_dir);

    let linked = TempDir::new().unwrap();
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            linked.path().to_str().unwrap(),
            "-b",
            "linked-test",
        ],
    );
    let linked_metadata = resolve_git_metadata(
        &GitBackedInput::from(&make_git_input()),
        &context(linked.path()),
    )
    .unwrap();
    assert_eq!(
        comparable(&linked_metadata.repo_root),
        comparable(linked.path())
    );
    assert_ne!(linked_metadata.git_dir, linked_metadata.common_dir);
    assert!(
        linked_metadata
            .git_dir
            .to_string_lossy()
            .contains("/worktrees/")
    );
    assert_eq!(linked_metadata.common_dir, normal.common_dir);
}

#[test]
fn resolves_two_named_revision_endpoints() {
    let repo = create_repo();
    let from = commit_file(repo.path(), "first\n", "first");
    let to = commit_file(repo.path(), "second\n", "second");
    let mut input = make_git_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: from.clone(),
        to: to.clone(),
    });
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(from),
            new: GitDiffEndpoint::GitRef(to),
        })
    );
}

#[test]
fn staged_diff_compares_head_against_index() {
    let repo = create_repo();
    let head = commit_file(repo.path(), "x\n", "initial");
    let mut input = make_git_input();
    input.staged = true;
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(head),
            new: GitDiffEndpoint::Index,
        })
    );
}

#[test]
fn staged_diff_in_unborn_repo_uses_missing_old_source() {
    let repo = create_repo();
    fs::write(repo.path().join("x.txt"), "x\n").unwrap();
    git(repo.path(), &["add", "x.txt"]);
    let mut input = make_git_input();
    input.staged = true;
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::None,
            new: GitDiffEndpoint::Index,
        })
    );
}

#[test]
fn staged_diff_against_explicit_ref_compares_ref_to_index() {
    let repo = create_repo();
    let first = commit_file(repo.path(), "first\n", "first");
    commit_file(repo.path(), "second\n", "second");
    fs::write(repo.path().join("x.txt"), "staged\n").unwrap();
    git(repo.path(), &["add", "x.txt"]);
    let mut input = make_git_input();
    input.staged = true;
    input.range = Some(first.clone());
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(first),
            new: GitDiffEndpoint::Index,
        })
    );
}

#[test]
fn no_range_compares_index_to_working_tree() {
    let repo = create_repo();
    assert_eq!(
        resolve_git_diff_endpoints(&make_git_input(), &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::Index,
            new: GitDiffEndpoint::Worktree,
        })
    );
}

#[test]
fn a_single_revision_compares_revision_to_working_tree() {
    let repo = create_repo();
    let head = commit_file(repo.path(), "first\n", "first");
    let mut input = make_git_input();
    input.range = Some("HEAD".into());
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(head),
            new: GitDiffEndpoint::Worktree,
        })
    );
}

#[test]
fn two_dot_range_resolves_to_old_and_new_revision() {
    let repo = create_repo();
    let first = commit_file(repo.path(), "first\n", "first");
    let second = commit_file(repo.path(), "second\n", "second");
    let mut input = make_git_input();
    input.range = Some(format!("{first}..{second}"));
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(first),
            new: GitDiffEndpoint::GitRef(second),
        })
    );
}

#[test]
fn parent_bang_range_resolves_parent_and_commit_pair() {
    let repo = create_repo();
    let first = commit_file(repo.path(), "first\n", "first");
    let second = commit_file(repo.path(), "second\n", "second");
    let mut input = make_git_input();
    input.range = Some("HEAD^!".into());
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(first),
            new: GitDiffEndpoint::GitRef(second),
        })
    );
}

#[test]
fn symmetric_range_resolves_merge_base_and_right_revision() {
    let repo = create_repo();
    let base = commit_file(repo.path(), "base\n", "base");
    let base_branch = git(repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_owned();
    git(repo.path(), &["checkout", "-q", "-b", "feature"]);
    let feature = commit_file(repo.path(), "feature\n", "feature");
    git(repo.path(), &["checkout", "-q", &base_branch]);
    commit_file(repo.path(), "main-2\n", "main-2");
    let mut input = make_git_input();
    input.range = Some(format!("{base_branch}...feature"));
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(base.clone()),
            new: GitDiffEndpoint::GitRef(feature.clone()),
        })
    );
    assert_eq!(
        git(repo.path(), &["merge-base", &base_branch, "feature"]).trim(),
        base
    );
    assert_ne!(feature, base);
}

#[test]
fn unrepresentable_multi_revision_range_returns_none() {
    let repo = create_repo();
    let first = commit_file(repo.path(), "first\n", "first");
    commit_file(repo.path(), "second\n", "second");
    commit_file(repo.path(), "third\n", "third");
    let mut input = make_git_input();
    input.range = Some(format!("{first} HEAD"));
    assert_eq!(
        resolve_git_diff_endpoints(&input, &rooted_context(repo.path())).unwrap(),
        None
    );
}

#[test]
fn numstat_parser_keeps_well_formed_entries_and_drops_malformed_ones() {
    assert_eq!(
        parse_git_numstat(concat!(
            "3\t1\tsrc/a.ts\0bad-entry\0x\ty\tsrc/b.ts\0",
            "2\t0\tsrc/c.ts"
        )),
        vec![
            GitNumstatFile {
                path: "src/a.ts".into(),
                additions: 3,
                deletions: 1,
            },
            GitNumstatFile {
                path: "src/c.ts".into(),
                additions: 2,
                deletions: 0,
            },
        ]
    );
}

#[test]
fn numstat_parser_drops_binary_entries_with_dash_counts() {
    assert_eq!(
        parse_git_numstat(concat!("-\t-\tsrc/logo.png\0", "3\t1\tsrc/a.ts")),
        vec![GitNumstatFile {
            path: "src/a.ts".into(),
            additions: 3,
            deletions: 1,
        }]
    );
}

#[test]
fn numstat_parser_returns_nothing_for_empty_output() {
    assert!(parse_git_numstat("").is_empty());
}

#[test]
fn tracked_diff_over_line_budget_is_skipped() {
    assert!(should_skip_large_tracked_diff(
        &GitNumstatFile {
            path: "x".into(),
            additions: 19_000,
            deletions: 2_000,
        },
        Path::new("/repo"),
    ));
}

#[test]
fn small_diff_of_very_large_file_is_skipped() {
    let directory = TempDir::new().unwrap();
    fs::write(directory.path().join("big.bin"), vec![b'a'; 1_100_000]).unwrap();
    assert!(should_skip_large_tracked_diff(
        &GitNumstatFile {
            path: "big.bin".into(),
            additions: 1,
            deletions: 0,
        },
        directory.path(),
    ));
}

#[test]
fn small_diff_is_kept_and_missing_file_is_tolerated() {
    let directory = TempDir::new().unwrap();
    fs::write(directory.path().join("small.txt"), "hello\n").unwrap();
    for path in ["small.txt", "gone.txt"] {
        assert!(!should_skip_large_tracked_diff(
            &GitNumstatFile {
                path: path.into(),
                additions: 1,
                deletions: 1,
            },
            directory.path(),
        ));
    }
}
