use super::*;
use std::fs;
use std::process::Command;
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

fn adapter() -> VcsAdapter {
    create_sapling_vcs_adapter(SaplingVcsAdapterOptions::default())
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
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

fn sl_available() -> bool {
    Command::new("sl")
        .arg("version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn sl(cwd: &Path, arguments: &[&str]) -> String {
    let output = Command::new("sl")
        .args(["--noninteractive", "--color", "never"])
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run Sapling fixture command");
    assert!(
        output.status.success(),
        "sl {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn create_sl_repo() -> TempDir {
    let directory = tempdir().unwrap();
    sl(directory.path(), &["init", "--git"]);
    sl(
        directory.path(),
        &[
            "config",
            "--local",
            "ui.username",
            "Test User <test@example.com>",
        ],
    );
    directory
}

#[cfg(unix)]
fn fake_sl(repo_root: &Path, command_log: &Path) -> TempDir {
    use std::os::unix::fs::PermissionsExt;

    fn quote(value: &Path) -> String {
        format!("'{}'", value.display().to_string().replace('\'', "'\\''"))
    }

    let binary_directory = tempdir().unwrap();
    let executable = binary_directory.path().join("sl");
    let script = format!(
        concat!(
            "#!/bin/sh\n",
            "printf '%s\\n' \"$*\" >> {}\n",
            "case \"$*\" in\n",
            "  *\" root\") printf '%s\\n' {} ;;\n",
            "  *\" status \"*) printf '? fresh.txt\\0' ;;\n",
            "  *\" diff \"*) printf 'diff --git a/file.txt b/file.txt\\n--- a/file.txt\\n+++ b/file.txt\\n@@ -1 +1 @@\\n-old\\n+two\\n' ;;\n",
            "esac\n"
        ),
        quote(command_log),
        quote(repo_root)
    );
    fs::write(&executable, script).unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    binary_directory
}

#[test]
fn publishes_both_review_operations_and_no_stash_operation() {
    let adapter = adapter();
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
        Some(SAPLING_VCS_DETECTION_BASELINE_PRIORITY)
    );
}

#[test]
fn detects_sapling_repositories_from_nested_directories() {
    let repo = tempdir().unwrap();
    fs::create_dir(repo.path().join(".sl")).unwrap();
    let nested = repo.path().join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(
        (adapter().detect)(&nested).unwrap(),
        Some(VcsDetection {
            id: "sl".into(),
            repo_root: comparable(repo.path()),
        })
    );
}

#[test]
fn auto_detects_hg_directories_with_treestate_as_sapling() {
    let repo = tempdir().unwrap();
    fs::create_dir(repo.path().join(".hg")).unwrap();
    fs::write(
        repo.path().join(".hg/requires"),
        "revlogv1\nstore\ntreestate\n",
    )
    .unwrap();
    assert_eq!(
        (adapter().detect)(repo.path()).unwrap(),
        Some(VcsDetection {
            id: "sl".into(),
            repo_root: comparable(repo.path()),
        })
    );
}

#[test]
fn does_not_auto_detect_hg_directories_without_treestate() {
    let repo = tempdir().unwrap();
    fs::create_dir(repo.path().join(".hg")).unwrap();
    fs::write(repo.path().join(".hg/requires"), "revlogv1\nstore\n").unwrap();
    assert_eq!((adapter().detect)(repo.path()).unwrap(), None);
}

#[test]
fn treats_hg_without_requires_as_non_sapling() {
    let repo = tempdir().unwrap();
    fs::create_dir(repo.path().join(".hg")).unwrap();
    assert_eq!((adapter().detect)(repo.path()).unwrap(), None);
}

#[test]
fn returns_none_when_no_sapling_marker_exists_to_the_filesystem_root() {
    let directory = tempdir().unwrap();
    assert_eq!((adapter().detect)(directory.path()).unwrap(), None);
}

#[test]
fn rejects_option_like_endpoints_from_direct_adapter_callers_before_spawn() {
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "--config=unsafe".into(),
    });
    let missing = create_sapling_vcs_adapter(SaplingVcsAdapterOptions {
        sl_executable: "definitely-not-a-real-sl-binary".into(),
    });
    let error = match load(
        &missing,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input),
        std::env::temp_dir().as_path(),
    ) {
        Ok(_) => panic!("option-like endpoint unexpectedly spawned Sapling"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("looks like a Sapling option"));
}

#[test]
fn rejects_staged_working_tree_diffs_before_spawn() {
    let mut input = diff_input();
    input.staged = true;
    let missing = create_sapling_vcs_adapter(SaplingVcsAdapterOptions {
        sl_executable: "definitely-not-a-real-sl-binary".into(),
    });
    let error = match load(
        &missing,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input),
        std::env::temp_dir().as_path(),
    ) {
        Ok(_) => panic!("staged Sapling review unexpectedly loaded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("Sapling has no staging area"));
}

#[cfg(unix)]
#[test]
fn range_loads_do_not_probe_working_copy_unknown_files() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("fresh.txt"), "fresh\n").unwrap();
    let log = repo.path().join("commands.log");
    let binary_directory = fake_sl(repo.path(), &log);
    let adapter = create_sapling_vcs_adapter(SaplingVcsAdapterOptions {
        sl_executable: binary_directory.path().join("sl"),
    });
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "feature".into(),
    });
    let result = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(input),
        repo.path(),
    )
    .unwrap();
    assert!(result.untracked_paths.is_empty());
    let commands = fs::read_to_string(log).unwrap();
    assert!(!commands.lines().any(|command| command.contains("status")));
    assert!(
        commands
            .lines()
            .any(|command| { command.contains("-r main") && command.contains("-r feature") })
    );
}

#[cfg(unix)]
#[test]
fn loads_working_copy_and_revision_patches_through_neutral_operations() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("fresh.txt"), "fresh\n").unwrap();
    let log = repo.path().join("commands.log");
    let binary_directory = fake_sl(repo.path(), &log);
    let adapter = create_sapling_vcs_adapter(SaplingVcsAdapterOptions {
        sl_executable: binary_directory.path().join("sl"),
    });
    let diff = diff_input();
    let diff_result = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(diff.clone()),
        repo.path(),
    )
    .unwrap();
    assert_eq!(comparable(&diff_result.repo_root), comparable(repo.path()));
    assert!(diff_result.title.contains("working copy"));
    assert!(
        diff_result
            .patch_text
            .contains("diff --git a/file.txt b/file.txt")
    );
    assert!(diff_result.patch_text.contains("+two"));
    assert_eq!(diff_result.untracked_paths, [PathBuf::from("fresh.txt")]);

    let show = show_input(Some("."));
    let show_result = load(
        &adapter,
        VcsReviewOperationKind::RevisionShow,
        VcsReviewInput::Show(show.clone()),
        repo.path(),
    )
    .unwrap();
    assert!(show_result.title.contains("show ."));
    assert!(show_result.patch_text.contains("diff --git"));
    assert!(
        watch_signature(
            &adapter,
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewInput::Diff(diff),
            repo.path(),
        )
        .unwrap()
        .contains("+two")
    );
    assert!(
        watch_signature(
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
fn product_loader_materializes_tracked_and_unknown_files_from_the_native_adapter() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("fresh.txt"), "fresh\n").unwrap();
    let log = repo.path().join("commands.log");
    let binary_directory = fake_sl(repo.path(), &log);
    let changeset = load_sapling_changeset(
        &VcsReviewInput::Diff(diff_input()),
        &VcsLoadContext {
            cwd: repo.path().into(),
        },
        &SaplingVcsAdapterOptions {
            sl_executable: binary_directory.path().join("sl"),
        },
    )
    .unwrap();
    assert_eq!(changeset.id, "sl:diff");
    assert_eq!(changeset.files.len(), 2);
    assert_eq!(changeset.files[0].path, "file.txt");
    assert_eq!(changeset.files[1].path, "fresh.txt");
    assert!(changeset.files[1].flags.untracked);
}

#[test]
fn loads_real_sapling_working_copy_and_revision_when_available() {
    if !sl_available() {
        return;
    }
    let repo = create_sl_repo();
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    sl(repo.path(), &["add", "file.txt"]);
    sl(repo.path(), &["commit", "-m", "initial"]);
    fs::write(repo.path().join("file.txt"), "two\n").unwrap();

    let adapter = adapter();
    let diff = diff_input();
    let result = load(
        &adapter,
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Diff(diff.clone()),
        repo.path(),
    )
    .unwrap();
    assert!(result.patch_text.contains("+two"));
    assert!(
        watch_signature(
            &adapter,
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewInput::Diff(diff),
            repo.path(),
        )
        .unwrap()
        .contains("+two")
    );

    let show = show_input(Some("."));
    let result = load(
        &adapter,
        VcsReviewOperationKind::RevisionShow,
        VcsReviewInput::Show(show),
        repo.path(),
    )
    .unwrap();
    assert!(result.patch_text.contains("diff --git"));
}
