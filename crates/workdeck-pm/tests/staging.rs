use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;
use workdeck_pm::{
    CreateIssue, ErrorCode, IssueMutation, IssueRecord, PmError, Repository, RequestId,
    StagingFaultPoint, UpdateIssue, transactions::MutationReceipt,
};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .current_dir(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn fixture() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    git(temp.path(), &["init", "--quiet"]);
    git(temp.path(), &["config", "user.name", "PM Test"]);
    git(temp.path(), &["config", "user.email", "pm@example.invalid"]);
    for name in ["notes.txt", "other.txt"] {
        fs::write(temp.path().join(name), "committed\n").unwrap();
    }
    git(temp.path(), &["add", "--", "notes.txt", "other.txt"]);
    git(
        temp.path(),
        &["commit", "--quiet", "--no-gpg-sign", "-m", "fixture"],
    );
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}

fn create(repository: &Repository) -> (MutationReceipt, IssueRecord) {
    let receipt = repository
        .create_issue(
            &CreateIssue::new("Staging test", "# Body\n"),
            &RequestId::new(),
        )
        .unwrap();
    let record = serde_json::from_value(receipt.result.clone()).unwrap();
    (receipt, record)
}

fn update(repository: &Repository, issue: &IssueRecord) -> MutationReceipt {
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &IssueMutation::Update {
                input: UpdateIssue {
                    fields: [("title".into(), json!("Updated title"))]
                        .into_iter()
                        .collect(),
                    body: None,
                },
            },
            &RequestId::new(),
        )
        .unwrap()
}

fn index_path(repository: &Repository) -> PathBuf {
    PathBuf::from(
        String::from_utf8(git(
            repository.root().parent().unwrap(),
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        ))
        .unwrap()
        .trim(),
    )
}

fn index(repository: &Repository) -> Vec<u8> {
    fs::read(index_path(repository)).unwrap()
}

#[test]
fn stages_exact_receipt_paths_and_preserves_mixed_unrelated_work() {
    let (temp, repository) = fixture();
    fs::write(temp.path().join("notes.txt"), "staged user work\n").unwrap();
    git(temp.path(), &["add", "--", "notes.txt"]);
    fs::write(temp.path().join("notes.txt"), "unstaged user work\n").unwrap();
    fs::write(temp.path().join("draft.txt"), "untracked\n").unwrap();
    let (receipt, issue) = create(&repository);
    let report = repository.stage_operation(&receipt).unwrap();
    assert!(report.index_changed);
    assert_eq!(report.paths.len(), 2);
    let expected = [
        Path::new(".workdeck").join(&issue.path),
        PathBuf::from(format!(".workdeck/operations/{}.yml", receipt.operation_id)),
    ];
    for path in expected {
        assert!(report.paths.contains(&path));
        assert_eq!(
            git(temp.path(), &["show", &format!(":{}", path.display())]),
            fs::read(temp.path().join(path)).unwrap()
        );
    }
    assert_eq!(
        git(temp.path(), &["show", ":notes.txt"]),
        b"staged user work\n"
    );
    assert_eq!(
        fs::read(temp.path().join("notes.txt")).unwrap(),
        b"unstaged user work\n"
    );
    let staged = String::from_utf8(git(temp.path(), &["diff", "--cached", "--name-only"])).unwrap();
    assert_eq!(staged.lines().count(), 3);
    assert!(!staged.contains("config.yml"));
    assert!(!staged.contains("draft.txt"));
    let before = index(&repository);
    assert!(!repository.stage_operation(&receipt).unwrap().index_changed);
    assert_eq!(index(&repository), before);
}

#[test]
fn stale_and_forged_receipts_leave_index_unchanged() {
    let (_temp, repository) = fixture();
    let (receipt, issue) = create(&repository);
    let before = index(&repository);
    let mut forged = receipt.clone();
    forged.result["body"] = json!("forged");
    assert_eq!(
        repository.stage_operation(&forged).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(index(&repository), before);
    fs::write(
        repository.root().join(&issue.path),
        "direct editor change\n",
    )
    .unwrap();
    assert_eq!(
        repository.stage_operation(&receipt).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(index(&repository), before);
}

#[test]
fn pre_staged_before_version_can_advance_but_unrelated_staged_content_conflicts() {
    let (temp, repository) = fixture();
    let (receipt, issue) = create(&repository);
    repository.stage_operation(&receipt).unwrap();
    let updated = update(&repository, &issue);
    let before_stale_retry = index(&repository);
    assert_eq!(
        repository.stage_operation(&receipt).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(index(&repository), before_stale_retry);
    repository.stage_operation(&updated).unwrap();
    git(
        temp.path(),
        &["commit", "--quiet", "--no-gpg-sign", "-m", "planning base"],
    );
    let current: IssueRecord = serde_json::from_value(updated.result).unwrap();
    let path = repository.root().join(&current.path);
    let original = fs::read(&path).unwrap();
    fs::write(&path, "unrelated staged document\n").unwrap();
    git(
        temp.path(),
        &[
            "add",
            "--",
            &format!(".workdeck/{}", current.path.display()),
        ],
    );
    fs::write(&path, original).unwrap();
    let receipt = repository
        .mutate_issue(
            current.metadata.id.as_str(),
            Some(&current.source),
            &IssueMutation::Update {
                input: UpdateIssue {
                    fields: [("title".into(), json!("Another title"))]
                        .into_iter()
                        .collect(),
                    body: None,
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let before = index(&repository);
    assert_eq!(
        repository.stage_operation(&receipt).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(index(&repository), before);
}

#[test]
fn existing_git_lock_is_preserved_and_interrupted_preparation_never_publishes() {
    let (_temp, repository) = fixture();
    let (receipt, _) = create(&repository);
    let before = index(&repository);
    let lock = index_path(&repository).with_file_name("index.lock");
    fs::write(&lock, "another Git writer\n").unwrap();
    assert_eq!(
        repository.stage_operation(&receipt).unwrap_err().code,
        ErrorCode::Locked
    );
    assert_eq!(fs::read(&lock).unwrap(), b"another Git writer\n");
    assert_eq!(index(&repository), before);
    fs::remove_file(&lock).unwrap();
    let error = repository
        .stage_operation_with_faults(&receipt, |point| {
            if point == StagingFaultPoint::BeforePublish {
                Err(PmError::new(ErrorCode::Canceled, "test interruption"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Canceled);
    assert_eq!(index(&repository), before);
    assert!(!lock.exists());
    assert!(
        !fs::read_dir(index_path(&repository).parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".workdeck-index-"))
    );
}

#[test]
fn direct_source_race_is_rechecked_after_alternate_index_preparation() {
    let (_temp, repository) = fixture();
    let (receipt, issue) = create(&repository);
    let before = index(&repository);
    let error = repository
        .stage_operation_with_faults(&receipt, |point| {
            if point == StagingFaultPoint::BeforePublish {
                fs::write(
                    repository.root().join(&issue.path),
                    "external edit during preparation\n",
                )
                .unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(index(&repository), before);
    assert_eq!(
        fs::read(repository.root().join(&issue.path)).unwrap(),
        b"external edit during preparation\n"
    );
}

#[test]
fn direct_index_race_is_preserved_instead_of_overwritten() {
    let (temp, repository) = fixture();
    let (receipt, _) = create(&repository);
    let before = index(&repository);
    fs::write(
        temp.path().join("other.txt"),
        "new external staged content\n",
    )
    .unwrap();
    git(temp.path(), &["add", "--", "other.txt"]);
    let external = index(&repository);
    fs::write(index_path(&repository), &before).unwrap();
    let error = repository
        .stage_operation_with_faults(&receipt, |point| {
            if point == StagingFaultPoint::BeforePublish {
                fs::write(index_path(&repository), &external).unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(index(&repository), external);
}

#[test]
fn moved_head_is_rejected_before_index_publication() {
    let (temp, repository) = fixture();
    let (receipt, _) = create(&repository);
    let original_head = String::from_utf8(git(temp.path(), &["rev-parse", "HEAD"])).unwrap();
    git(
        temp.path(),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "--allow-empty",
            "-m",
            "next head",
        ],
    );
    let next_head = String::from_utf8(git(temp.path(), &["rev-parse", "HEAD"])).unwrap();
    git(temp.path(), &["update-ref", "HEAD", original_head.trim()]);
    let before = index(&repository);
    let error = repository
        .stage_operation_with_faults(&receipt, |point| {
            if point == StagingFaultPoint::BeforePublish {
                git(temp.path(), &["update-ref", "HEAD", next_head.trim()]);
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(index(&repository), before);
}

#[test]
fn clean_filters_and_configured_fsmonitor_are_not_executed() {
    let (temp, repository) = fixture();
    fs::write(
        temp.path().join(".gitattributes"),
        ".workdeck/** filter=tripwire\n",
    )
    .unwrap();
    git(
        temp.path(),
        &[
            "config",
            "filter.tripwire.clean",
            "sh -c 'echo filter > filter-ran; cat'",
        ],
    );
    git(temp.path(), &["config", "filter.tripwire.required", "true"]);
    git(
        temp.path(),
        &[
            "config",
            "core.fsmonitor",
            "sh -c 'echo monitor > monitor-ran'",
        ],
    );
    let (receipt, _) = create(&repository);
    repository.stage_operation(&receipt).unwrap();
    assert!(!temp.path().join("filter-ran").exists());
    assert!(!temp.path().join("monitor-ran").exists());
}

#[cfg(unix)]
#[test]
fn index_change_hooks_are_not_executed() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, repository) = fixture();
    let hook = temp.path().join(".git/hooks/post-index-change");
    fs::write(&hook, "#!/bin/sh\nprintf hook > hook-ran\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let (receipt, _) = create(&repository);
    repository.stage_operation(&receipt).unwrap();
    assert!(!temp.path().join("hook-ran").exists());
}

#[test]
fn linked_worktree_staging_uses_its_own_index() {
    let (temp, main) = fixture();
    let second = TempDir::new().unwrap();
    git(
        temp.path(),
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            second.path().to_str().unwrap(),
            "HEAD",
        ],
    );
    let repository = Repository::init(second.path(), "WD").unwrap();
    let original_main = index(&main);
    let (receipt, _) = create(&repository);
    repository.stage_operation(&receipt).unwrap();
    assert_eq!(index(&main), original_main);
    assert_ne!(index_path(&main), index_path(&repository));
    assert_eq!(
        String::from_utf8(git(second.path(), &["diff", "--cached", "--name-only"]))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn staging_works_with_an_unborn_head_and_no_index() {
    let temp = TempDir::new().unwrap();
    git(temp.path(), &["init", "--quiet"]);
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let (receipt, _) = create(&repository);
    assert!(!index_path(&repository).exists());
    repository.stage_operation(&receipt).unwrap();
    assert_eq!(
        String::from_utf8(git(temp.path(), &["diff", "--cached", "--name-only"]))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[cfg(unix)]
#[test]
fn index_permissions_survive_and_symlink_indexes_are_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_temp, repository) = fixture();
    let path = index_path(&repository);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let (receipt, _) = create(&repository);
    repository.stage_operation(&receipt).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let saved = path.with_file_name("saved-index");
    fs::rename(&path, &saved).unwrap();
    symlink(&saved, &path).unwrap();
    let before = fs::read(&saved).unwrap();
    assert_eq!(
        repository.stage_operation(&receipt).unwrap_err().code,
        ErrorCode::UnsafePath
    );
    assert_eq!(fs::read(saved).unwrap(), before);
}

#[test]
fn ambient_git_probe_child() {
    let Some(root) = std::env::var_os("WORKDECK_STAGE_TEST_ROOT") else {
        return;
    };
    let repository = Repository::open_source(&PathBuf::from(root).join(".workdeck")).unwrap();
    let receipt: MutationReceipt = serde_yaml_ng::from_slice(
        &fs::read(std::env::var_os("WORKDECK_STAGE_TEST_RECEIPT").unwrap()).unwrap(),
    )
    .unwrap();
    repository.stage_operation(&receipt).unwrap();
}

#[test]
fn ambient_git_redirections_cannot_target_another_repository_or_index() {
    let (temp, repository) = fixture();
    let (foreign_temp, foreign) = fixture();
    let foreign_index = index(&foreign);
    let (receipt, _) = create(&repository);
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "ambient_git_probe_child", "--nocapture"])
        .env("WORKDECK_STAGE_TEST_ROOT", temp.path())
        .env(
            "WORKDECK_STAGE_TEST_RECEIPT",
            repository
                .root()
                .join(format!("operations/{}.yml", receipt.operation_id)),
        )
        .env("GIT_DIR", foreign_temp.path().join(".git"))
        .env("GIT_WORK_TREE", foreign_temp.path())
        .env("GIT_INDEX_FILE", index_path(&foreign))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(index(&foreign), foreign_index);
    assert_eq!(
        String::from_utf8(git(temp.path(), &["diff", "--cached", "--name-only"]))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn config_and_receipt_races_do_not_stage_an_outdated_operation() {
    for config in [true, false] {
        let (_temp, repository) = fixture();
        let (receipt, _) = create(&repository);
        let before = index(&repository);
        let path = if config {
            repository.root().join("config.yml")
        } else {
            repository
                .root()
                .join(format!("operations/{}.yml", receipt.operation_id))
        };
        let error = repository
            .stage_operation_with_faults(&receipt, |point| {
                if point == StagingFaultPoint::BeforePublish {
                    let mut changed = fs::read(&path).unwrap();
                    changed.extend_from_slice(b"# direct concurrent edit\n");
                    fs::write(&path, changed).unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleSource);
        assert_eq!(index(&repository), before);
    }
}

#[test]
fn replacing_the_owned_index_lock_is_detected_and_the_new_lock_is_preserved() {
    let (_temp, repository) = fixture();
    let (receipt, _) = create(&repository);
    let before = index(&repository);
    let lock = index_path(&repository).with_file_name("index.lock");
    let error = repository
        .stage_operation_with_faults(&receipt, |point| {
            if point == StagingFaultPoint::BeforePublish {
                fs::remove_file(&lock).unwrap();
                fs::write(&lock, "replacement lock\n").unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(fs::read(&lock).unwrap(), b"replacement lock\n");
    assert_eq!(index(&repository), before);
}

#[cfg(unix)]
#[test]
fn fifo_staging_probe_child() {
    let Some(root) = std::env::var_os("WORKDECK_STAGE_FIFO_ROOT") else {
        return;
    };
    let repository = Repository::open_source(&PathBuf::from(root).join(".workdeck")).unwrap();
    let receipt: MutationReceipt = serde_yaml_ng::from_slice(
        &fs::read(std::env::var_os("WORKDECK_STAGE_FIFO_RECEIPT").unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        repository.stage_operation(&receipt).unwrap_err().code,
        ErrorCode::UnsafePath
    );
}

#[cfg(unix)]
#[test]
fn a_fifo_index_is_rejected_before_any_open_can_block() {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let (temp, repository) = fixture();
    let (receipt, _) = create(&repository);
    let path = index_path(&repository);
    fs::remove_file(&path).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fifo_staging_probe_child", "--nocapture"])
        .env("WORKDECK_STAGE_FIFO_ROOT", temp.path())
        .env(
            "WORKDECK_STAGE_FIFO_RECEIPT",
            repository
                .root()
                .join(format!("operations/{}.yml", receipt.operation_id)),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("staging blocked on a nonregular index");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
