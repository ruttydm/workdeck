#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use tempfile::TempDir;
use workdeck_pm::{
    ContentHash, ErrorCode, HookApply, HookFaultPoint, HookMode, PmError, Repository, RequestId,
    apply_hook, apply_hook_with_faults, hook_preview, hook_status, recover_hook,
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
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn fixture() -> (TempDir, Repository) {
    let directory = TempDir::new().unwrap();
    git(directory.path(), &["init", "--quiet"]);
    git(
        directory.path(),
        &["config", "core.hooksPath", ".git/hooks"],
    );
    let repository = Repository::init(directory.path(), "WD").unwrap();
    (directory, repository)
}
fn input(plan: &workdeck_pm::HookPlan) -> HookApply {
    HookApply {
        mode: plan.mode,
        expected_plan: plan.fingerprint.clone(),
    }
}

#[test]
fn reviewed_install_preserves_index_and_status_then_update_and_remove_are_explicit() {
    let (directory, _) = fixture();
    fs::write(directory.path().join("unrelated.txt"), "staged\n").unwrap();
    git(
        directory.path(),
        &["add", "--", ".workdeck", "unrelated.txt"],
    );
    fs::write(directory.path().join("unrelated.txt"), "unstaged\n").unwrap();
    let before_index = fs::read(directory.path().join(".git/index")).unwrap();
    let before_status = git(
        directory.path(),
        &["status", "--porcelain", "--untracked-files=all"],
    );
    let plan = hook_preview(directory.path(), HookMode::Install).unwrap();
    assert!(plan.allowed && plan.changed);
    assert!(!plan.target.exists());
    assert_eq!(
        before_status,
        git(
            directory.path(),
            &["status", "--porcelain", "--untracked-files=all"]
        )
    );
    let receipt = apply_hook(directory.path(), &input(&plan), &RequestId::new()).unwrap();
    assert_eq!(receipt.plan, plan);
    assert_eq!(
        fs::read(&plan.target).unwrap(),
        plan.generated_hook.unwrap().as_bytes()
    );
    assert_ne!(
        fs::metadata(&plan.target).unwrap().permissions().mode() & 0o111,
        0
    );
    let status = hook_status(directory.path()).unwrap();
    assert!(status.owned && status.executable && status.pending_request.is_none());
    assert_eq!(
        before_index,
        fs::read(directory.path().join(".git/index")).unwrap()
    );
    assert_eq!(
        before_status,
        git(
            directory.path(),
            &["status", "--porcelain", "--untracked-files=all"]
        )
    );
    let update = hook_preview(directory.path(), HookMode::Update).unwrap();
    assert!(update.allowed && !update.changed);
    apply_hook(directory.path(), &input(&update), &RequestId::new()).unwrap();
    let remove = hook_preview(directory.path(), HookMode::Remove).unwrap();
    assert!(remove.allowed && remove.after.is_none());
    apply_hook(directory.path(), &input(&remove), &RequestId::new()).unwrap();
    assert!(!remove.target.exists());
}

#[test]
fn unmanaged_hooks_are_preserved_with_actionable_manual_integration() {
    let (directory, _) = fixture();
    let path = directory.path().join(".git/hooks/pre-commit");
    let original = b"#!/bin/sh\n# workdeck doctor --staged is only a comment\nexit 7\n";
    fs::write(&path, original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o751)).unwrap();
    for mode in [HookMode::Install, HookMode::Update, HookMode::Remove] {
        let plan = hook_preview(directory.path(), mode).unwrap();
        assert!(!plan.allowed);
        assert!(!plan.blockers.is_empty());
        assert!(
            plan.integration_snippet
                .contains("workdeck doctor --staged")
        );
        assert_eq!(
            apply_hook(directory.path(), &input(&plan), &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::PolicyBlocked
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o751
        );
    }
    assert!(
        !directory
            .path()
            .join(".workdeck/.local/hooks/requests")
            .exists()
    );
}

#[test]
fn configured_external_hook_target_and_configuration_are_pinned_by_review() {
    let (directory, _) = fixture();
    let external = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    let external_path = external.path().canonicalize().unwrap();
    let other_path = other.path().canonicalize().unwrap();
    git(
        directory.path(),
        &["config", "core.hooksPath", external_path.to_str().unwrap()],
    );
    let plan = hook_preview(directory.path(), HookMode::Install).unwrap();
    assert_eq!(plan.target, external_path.join("pre-commit"));
    git(
        directory.path(),
        &["config", "core.hooksPath", other_path.to_str().unwrap()],
    );
    assert_eq!(
        apply_hook(directory.path(), &input(&plan), &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert!(!plan.target.exists());
    assert!(!other.path().join("pre-commit").exists());
    let current = hook_preview(directory.path(), HookMode::Install).unwrap();
    apply_hook(directory.path(), &input(&current), &RequestId::new()).unwrap();
    assert!(other.path().join("pre-commit").is_file());
    assert_eq!(
        String::from_utf8(git(
            directory.path(),
            &["config", "--get", "core.hooksPath"]
        ))
        .unwrap()
        .trim(),
        other_path.to_str().unwrap()
    );
}

#[test]
fn interrupted_publication_recovers_and_original_request_replays_after_later_removal() {
    let (directory, _) = fixture();
    let plan = hook_preview(directory.path(), HookMode::Install).unwrap();
    let request = RequestId::new();
    let error = apply_hook_with_faults(directory.path(), &input(&plan), &request, |point| {
        if point == HookFaultPoint::AfterPublish {
            Err(PmError::new(ErrorCode::Io, "lost acknowledgment"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert!(plan.target.exists());
    assert_eq!(error.details.as_ref().unwrap()["hook_published"], true);
    assert_eq!(
        hook_status(directory.path())
            .unwrap()
            .pending_request
            .as_ref(),
        Some(&request)
    );
    assert_eq!(
        hook_preview(directory.path(), HookMode::Install)
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    let receipt = recover_hook(directory.path(), &request).unwrap();
    let remove = hook_preview(directory.path(), HookMode::Remove).unwrap();
    apply_hook(directory.path(), &input(&remove), &RequestId::new()).unwrap();
    assert_eq!(
        apply_hook(directory.path(), &input(&plan), &request).unwrap(),
        receipt
    );
    assert!(
        !plan.target.exists(),
        "historical retry must not republish the removed hook"
    );
    let changed = HookApply {
        mode: HookMode::Install,
        expected_plan: ContentHash::of(b"changed input"),
    };
    assert_eq!(
        apply_hook(directory.path(), &changed, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
}

#[test]
fn direct_target_edits_and_disappearing_journal_stop_publication() {
    let (directory, _) = fixture();
    let plan = hook_preview(directory.path(), HookMode::Install).unwrap();
    let request = RequestId::new();
    let error = apply_hook_with_faults(directory.path(), &input(&plan), &request, |point| {
        if point == HookFaultPoint::BeforePublish {
            fs::remove_file(directory.path().join(".workdeck/.local/hooks/journal.json")).unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert!(!plan.target.exists());
    let plan = hook_preview(directory.path(), HookMode::Install).unwrap();
    fs::write(&plan.target, b"#!/bin/sh\nexit 3\n").unwrap();
    assert_eq!(
        apply_hook(directory.path(), &input(&plan), &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(fs::read(&plan.target).unwrap(), b"#!/bin/sh\nexit 3\n");
}

#[test]
fn replacing_local_journal_directory_or_writer_lock_during_publication_is_stale() {
    for directory_swap in [true, false] {
        let (directory, repository) = fixture();
        let plan = hook_preview(directory.path(), HookMode::Install).unwrap();
        let local = repository.root().join(".local/hooks");
        let error = apply_hook_with_faults(
            directory.path(),
            &input(&plan),
            &RequestId::new(),
            |point| {
                if point == HookFaultPoint::BeforePublish {
                    if directory_swap {
                        let saved = repository.root().join(".local/previous-hooks");
                        fs::rename(&local, &saved).unwrap();
                        fs::create_dir_all(local.join("requests")).unwrap();
                        for name in [".gitignore", "journal.json", "writer.lock"] {
                            fs::copy(saved.join(name), local.join(name)).unwrap();
                        }
                    } else {
                        fs::remove_file(local.join("writer.lock")).unwrap();
                        fs::write(local.join("writer.lock"), b"").unwrap();
                    }
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleSource);
        assert!(
            !plan.target.exists(),
            "a detached coordinator must not publish a hook"
        );
        assert!(local.join("journal.json").is_file());
    }
}

#[test]
fn changed_planning_identity_or_target_parent_prevents_journal_and_hook_publication() {
    for change_configuration in [true, false] {
        let (directory, repository) = fixture();
        let hook_directory = repository.root().parent().unwrap().join("custom-hooks");
        fs::create_dir(&hook_directory).unwrap();
        git(
            directory.path(),
            &["config", "core.hooksPath", hook_directory.to_str().unwrap()],
        );
        let plan = hook_preview(directory.path(), HookMode::Install).unwrap();
        let error = apply_hook_with_faults(
            directory.path(),
            &input(&plan),
            &RequestId::new(),
            |point| {
                if point == HookFaultPoint::BeforeJournal {
                    if change_configuration {
                        let mut config = repository.config().unwrap();
                        config.repository = workdeck_pm::RepositoryId::new();
                        fs::write(
                            repository.root().join("config.yml"),
                            serde_yaml_ng::to_string(&config).unwrap(),
                        )
                        .unwrap();
                    } else {
                        fs::rename(
                            &hook_directory,
                            hook_directory.with_file_name("previous-hooks"),
                        )
                        .unwrap();
                        fs::create_dir(&hook_directory).unwrap();
                    }
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleSource);
        assert!(!plan.target.exists());
        assert!(!repository.root().join(".local/hooks/journal.json").exists());
    }
}

#[test]
fn forged_historical_receipt_is_rejected_without_rewriting_current_hook() {
    let (directory, repository) = fixture();
    let plan = hook_preview(directory.path(), HookMode::Install).unwrap();
    let request = RequestId::new();
    apply_hook(directory.path(), &input(&plan), &request).unwrap();
    let original = fs::read(&plan.target).unwrap();
    let path = repository
        .root()
        .join(format!(".local/hooks/requests/{request}.json"));
    let mut proof: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    proof["receipt"]["plan"]["generated_hook"] = serde_json::json!("#!/bin/sh\nexit 0\n");
    fs::write(&path, serde_json::to_vec(&proof).unwrap()).unwrap();
    assert_eq!(
        apply_hook(directory.path(), &input(&plan), &request)
            .unwrap_err()
            .code,
        ErrorCode::CorruptStore
    );
    assert_eq!(
        recover_hook(directory.path(), &request).unwrap_err().code,
        ErrorCode::CorruptStore
    );
    assert_eq!(fs::read(plan.target).unwrap(), original);
}

#[test]
fn special_hook_and_journal_sources_fail_without_following_or_blocking() {
    let (directory, repository) = fixture();
    let hook = repository
        .root()
        .parent()
        .unwrap()
        .join(".git/hooks/pre-commit");
    let outside = repository.root().parent().unwrap().join("outside-hook");
    fs::write(&outside, b"untouched").unwrap();
    std::os::unix::fs::symlink(&outside, &hook).unwrap();
    assert_eq!(
        hook_preview(directory.path(), HookMode::Install)
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    fs::remove_file(&hook).unwrap();
    let name = std::ffi::CString::new(hook.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(
        hook_preview(directory.path(), HookMode::Install)
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    fs::remove_file(&hook).unwrap();
    let local = repository.root().join(".local/hooks");
    fs::create_dir_all(&local).unwrap();
    let journal = local.join("journal.json");
    let name = std::ffi::CString::new(journal.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(
        hook_status(directory.path()).unwrap_err().code,
        ErrorCode::UnsafePath
    );
    assert_eq!(fs::read(outside).unwrap(), b"untouched");
}
