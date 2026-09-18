use std::{path::Path, process::Command};
use workdeck_pm::Repository;
fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_workdeck"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn protocol_preview_is_explicit_and_preserves_repository_instructions() {
    let temp = tempfile::tempdir().unwrap();
    Repository::init(temp.path(), "WD").unwrap();
    std::fs::write(
        temp.path().join("AGENTS.md"),
        "# Existing instructions\n\nKeep this exact text.\n",
    )
    .unwrap();
    let out = run(temp.path(), &["protocol", "preview", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("AGENTS.md")).unwrap(),
        "# Existing instructions\n\nKeep this exact text.\n"
    );
    assert!(!temp.path().join(".workdeck/.local/protocol").exists());
}

#[path = "../src/pm_protocol_install.rs"]
mod installer;
use installer::{FaultPoint, Mode};
use workdeck_pm::{ContentHash, ErrorCode, PmError, RequestId};
fn native() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    Repository::init(temp.path(), "WD").unwrap();
    temp
}
fn original(root: &Path) -> String {
    std::fs::read_to_string(root.join("AGENTS.md")).unwrap()
}
#[cfg(unix)]
fn git(root: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1").env(
        "GIT_CONFIG_GLOBAL",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
    );
    command.current_dir(root).args(args).output().unwrap()
}
#[test]
fn installer_preserves_bytes_replays_original_and_requires_source_cas() {
    let temp = native();
    let root = temp.path();
    let before = "# Existing\r\n\r\nUnrelated instructions without final newline";
    std::fs::write(root.join("AGENTS.md"), before).unwrap();
    let plan = installer::preview(root, Mode::Install).unwrap();
    assert!(plan.changed);
    assert!(
        installer::apply(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            None,
            &RequestId::new()
        )
        .is_err()
    );
    assert_eq!(original(root), before);
    let request = RequestId::new();
    let receipt = installer::apply(
        root,
        Mode::Install,
        Repository::discover(root).unwrap().identity(),
        plan.expected_content.clone(),
        &request,
    )
    .unwrap();
    let after = original(root);
    assert!(after.starts_with(before));
    assert!(after.contains("workdeck skill path workdeck-pm"));
    assert_eq!(receipt.after, ContentHash::of(after.as_bytes()));
    assert_eq!(receipt.scope, "local_protocol_pointer");
    std::fs::write(
        root.join("AGENTS.md"),
        format!("{after}\r\nLater author edit"),
    )
    .unwrap();
    let later = original(root);
    assert_eq!(
        installer::apply(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            plan.expected_content,
            &request
        )
        .unwrap(),
        receipt
    );
    assert_eq!(original(root), later);
    assert_eq!(
        installer::apply(
            root,
            Mode::Update,
            Repository::discover(root).unwrap().identity(),
            Some(receipt.after),
            &request
        )
        .unwrap_err()
        .code,
        ErrorCode::IdempotencyConflict
    );
}
#[test]
fn installer_update_only_replaces_managed_block() {
    let temp = native();
    let root = temp.path();
    let before = "Leading instructions\n<!-- workdeck:pm-protocol:start -->\nOld pointer contents\n<!-- workdeck:pm-protocol:end -->\n\nTrailing instructions stay exact.";
    std::fs::write(root.join("AGENTS.md"), before).unwrap();
    assert!(installer::preview(root, Mode::Install).is_err());
    let plan = installer::preview(root, Mode::Update).unwrap();
    assert!(plan.previous_pointer.unwrap().contains("Old pointer"));
    installer::apply(
        root,
        Mode::Update,
        Repository::discover(root).unwrap().identity(),
        plan.expected_content,
        &RequestId::new(),
    )
    .unwrap();
    let after = original(root);
    assert!(after.starts_with("Leading instructions\n"));
    assert!(after.ends_with("\n\nTrailing instructions stay exact."));
    assert!(!after.contains("Old pointer"));
    let plan = installer::preview(root, Mode::Update).unwrap();
    assert!(!plan.changed);
    let receipt = installer::apply(
        root,
        Mode::Update,
        Repository::discover(root).unwrap().identity(),
        plan.expected_content,
        &RequestId::new(),
    )
    .unwrap();
    assert!(!receipt.changed);
}
#[test]
fn installer_review_rejects_literal_fences_and_unclosed_append_context() {
    for (before, mode) in [
        (
            "# Example only\n\n````markdown\n```\n<!-- workdeck:pm-protocol:start -->\nKeep this literal example unchanged.\n<!-- workdeck:pm-protocol:end -->\n````\n",
            Mode::Update,
        ),
        (
            "# Instructions\n```text\nExample with no closing fence\n",
            Mode::Install,
        ),
        (
            "~~~text\n~~~not a closing fence\n<!-- workdeck:pm-protocol:start -->\nLiteral\n<!-- workdeck:pm-protocol:end -->\n~~~\n",
            Mode::Update,
        ),
    ] {
        let temp = native();
        let root = temp.path();
        std::fs::write(root.join("AGENTS.md"), before).unwrap();
        assert!(
            installer::preview(root, mode).is_err(),
            "accepted literal or incomplete fence"
        );
        assert!(
            installer::apply(
                root,
                mode,
                Repository::discover(root).unwrap().identity(),
                Some(ContentHash::of(before.as_bytes())),
                &RequestId::new()
            )
            .is_err()
        );
        assert_eq!(original(root), before);
        assert!(!root.join(".workdeck/.local/protocol").exists());
    }
}
#[test]
fn installer_review_disappearing_journal_blocks_publication() {
    let temp = native();
    let root = temp.path();
    let error = installer::apply_with_faults(
        root,
        Mode::Install,
        Repository::discover(root).unwrap().identity(),
        None,
        &RequestId::new(),
        |at| {
            if at == FaultPoint::BeforePublish {
                std::fs::remove_file(root.join(".workdeck/.local/protocol/journal.json")).unwrap();
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert!(
        !root.join("AGENTS.md").exists(),
        "published without durable journal: {error}"
    );
}
#[test]
fn installer_review_changed_journal_after_publish_is_preserved() {
    for point in [FaultPoint::AfterPublish, FaultPoint::AfterReceipt] {
        let temp = native();
        let root = temp.path();
        let journal = root.join(".workdeck/.local/protocol/journal.json");
        let result = installer::apply_with_faults(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            None,
            &RequestId::new(),
            |at| {
                if at == point {
                    std::fs::write(&journal, "different durable intent").unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_err(), "ignored changed journal at {point:?}");
        assert_eq!(
            std::fs::read_to_string(&journal).unwrap(),
            "different durable intent"
        );
        assert_eq!(
            result.unwrap_err().details.unwrap()["pointer_published"],
            true
        );
    }
}
#[test]
fn installer_interrupted_journal_publication_and_receipt_resume_original_request() {
    for point in [
        FaultPoint::AfterJournal,
        FaultPoint::AfterPublish,
        FaultPoint::AfterReceipt,
    ] {
        let temp = native();
        let root = temp.path();
        let request = RequestId::new();
        let error = installer::apply_with_faults(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            None,
            &request,
            |at| {
                if at == point {
                    Err(PmError::new(ErrorCode::Io, "interrupted"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Io);
        assert!(root.join(".workdeck/.local/protocol/journal.json").exists());
        assert_eq!(
            installer::preview(root, Mode::Install).unwrap_err().code,
            ErrorCode::RecoveryRequired
        );
        assert_eq!(
            installer::apply(
                root,
                Mode::Install,
                Repository::discover(root).unwrap().identity(),
                None,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
            ErrorCode::RecoveryRequired
        );
        let receipt = installer::apply(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            None,
            &request,
        )
        .unwrap();
        assert_eq!(
            installer::apply(
                root,
                Mode::Install,
                Repository::discover(root).unwrap().identity(),
                None,
                &request
            )
            .unwrap(),
            receipt
        );
        assert!(!root.join(".workdeck/.local/protocol/journal.json").exists());
        assert_eq!(
            original(root)
                .matches("<!-- workdeck:pm-protocol:start -->")
                .count(),
            1
        );
    }
}
#[test]
fn installer_source_races_and_conflicting_recovery_never_overwrite_edits() {
    let temp = native();
    let root = temp.path();
    let error = installer::apply_with_faults(
        root,
        Mode::Install,
        Repository::discover(root).unwrap().identity(),
        None,
        &RequestId::new(),
        |at| {
            if at == FaultPoint::BeforeJournal {
                std::fs::write(root.join("AGENTS.md"), "User edit").unwrap();
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(original(root), "User edit");
    assert!(!root.join(".workdeck/.local/protocol").exists());
    let request = RequestId::new();
    let expected = Some(ContentHash::of(b"User edit"));
    assert!(
        installer::apply_with_faults(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            expected.clone(),
            &request,
            |at| {
                if at == FaultPoint::AfterJournal {
                    Err(PmError::new(ErrorCode::Io, "interrupted"))
                } else {
                    Ok(())
                }
            }
        )
        .is_err()
    );
    std::fs::write(root.join("AGENTS.md"), "Newer user edit").unwrap();
    assert_eq!(
        installer::apply(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            expected.clone(),
            &request
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert_eq!(original(root), "Newer user edit");
    assert!(root.join(".workdeck/.local/protocol/journal.json").exists());
    std::fs::write(root.join("AGENTS.md"), "User edit").unwrap();
    installer::apply(
        root,
        Mode::Install,
        Repository::discover(root).unwrap().identity(),
        expected,
        &request,
    )
    .unwrap();
}
#[test]
fn installer_rejects_ambiguous_markers_oversize_and_uninitialized_sources() {
    let fresh = tempfile::tempdir().unwrap();
    assert!(installer::preview(fresh.path(), Mode::Install).is_err());
    assert!(!fresh.path().join(".workdeck").exists());
    for before in [
        "<!-- workdeck:pm-protocol:start -->\nIncomplete",
        "<!-- workdeck:pm-protocol:end -->\n<!-- workdeck:pm-protocol:start -->",
        "```md\n<!-- workdeck:pm-protocol:start -->\n<!-- workdeck:pm-protocol:end -->\n```",
        "inline <!-- workdeck:pm-protocol:start -->\n<!-- workdeck:pm-protocol:end -->",
    ] {
        let temp = native();
        let root = temp.path();
        std::fs::write(root.join("AGENTS.md"), before).unwrap();
        assert!(
            installer::apply(
                root,
                Mode::Install,
                Repository::discover(root).unwrap().identity(),
                Some(ContentHash::of(before.as_bytes())),
                &RequestId::new()
            )
            .is_err()
        );
        assert_eq!(original(root), before);
        assert!(!root.join(".workdeck/.local/protocol").exists());
    }
    let temp = native();
    std::fs::write(temp.path().join("AGENTS.md"), "x".repeat(128 * 1024 + 1)).unwrap();
    assert!(installer::preview(temp.path(), Mode::Install).is_err());
}
#[test]
fn installer_local_receipt_tampering_cannot_change_surrounding_instructions() {
    let temp = native();
    let root = temp.path();
    std::fs::write(root.join("AGENTS.md"), "Preserve me\n").unwrap();
    let expected = Some(ContentHash::of(b"Preserve me\n"));
    let request = RequestId::new();
    installer::apply(
        root,
        Mode::Install,
        Repository::discover(root).unwrap().identity(),
        expected.clone(),
        &request,
    )
    .unwrap();
    let path = root
        .join(".workdeck/.local/protocol/requests")
        .join(format!("{request}.json"));
    let mut proof: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let forged = proof["after"]
        .as_str()
        .unwrap()
        .replace("Preserve me", "Forged prefix");
    proof["after"] = serde_json::json!(forged);
    proof["receipt"]["after"] = serde_json::json!(ContentHash::of(forged.as_bytes()));
    std::fs::write(path, serde_json::to_vec(&proof).unwrap()).unwrap();
    let before = original(root);
    assert_eq!(
        installer::apply(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            expected,
            &request
        )
        .unwrap_err()
        .code,
        ErrorCode::CorruptStore
    );
    assert_eq!(original(root), before);
}
#[cfg(unix)]
#[test]
fn installer_symlink_paths_and_nonregular_sources_are_rejected() {
    use std::os::unix::fs::symlink;
    let temp = native();
    let root = temp.path();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("AGENTS.md"), "Outside").unwrap();
    symlink(outside.path().join("AGENTS.md"), root.join("AGENTS.md")).unwrap();
    assert_eq!(
        installer::preview(root, Mode::Install).unwrap_err().code,
        ErrorCode::UnsafePath
    );
    std::fs::remove_file(root.join("AGENTS.md")).unwrap();
    std::fs::create_dir(root.join("AGENTS.md")).unwrap();
    assert_eq!(
        installer::preview(root, Mode::Install).unwrap_err().code,
        ErrorCode::UnsafePath
    );
    std::fs::remove_dir(root.join("AGENTS.md")).unwrap();
    std::fs::create_dir_all(root.join(".workdeck/.local")).unwrap();
    symlink(outside.path(), root.join(".workdeck/.local/protocol")).unwrap();
    assert_eq!(
        installer::apply(
            root,
            Mode::Install,
            Repository::discover(root).unwrap().identity(),
            None,
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::UnsafePath
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("AGENTS.md")).unwrap(),
        "Outside"
    );
}
#[cfg(unix)]
#[test]
fn installer_preserves_permissions_and_local_state_is_ignored_by_git() {
    use std::os::unix::fs::PermissionsExt;
    let temp = native();
    let root = temp.path();
    std::fs::write(root.join("AGENTS.md"), "Existing\n").unwrap();
    std::fs::set_permissions(
        root.join("AGENTS.md"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    let out = git(root, &["init", "-q"]);
    assert!(out.status.success());
    installer::apply(
        root,
        Mode::Install,
        Repository::discover(root).unwrap().identity(),
        Some(ContentHash::of(b"Existing\n")),
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        std::fs::metadata(root.join("AGENTS.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    let out = git(root, &["ls-files", "--others", "--exclude-standard"]);
    assert!(out.status.success());
    let names = String::from_utf8(out.stdout).unwrap();
    assert!(names.lines().any(|s| s == "AGENTS.md"));
    assert!(
        !names
            .lines()
            .any(|s| s.contains(".local") || s.contains("journal") || s.contains("requests"))
    );
}
#[test]
fn installer_repository_precondition_rejects_replaced_identity_before_publication() {
    let temp = native();
    let root = temp.path();
    let preview = installer::preview(root, Mode::Install).unwrap();
    let config = root.join(".workdeck/config.yml");
    let before = std::fs::read_to_string(&config).unwrap();
    assert!(before.contains(preview.repository.as_str()));
    let after = before.replace(
        preview.repository.as_str(),
        workdeck_pm::RepositoryId::new().as_str(),
    );
    std::fs::write(&config, after).unwrap();
    assert_eq!(
        installer::apply(
            root,
            Mode::Install,
            &preview.repository,
            None,
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert!(!root.join("AGENTS.md").exists());
    assert!(!root.join(".workdeck/.local/protocol").exists());
}
#[test]
fn installer_config_races_stop_unpublished_work_and_report_completed_effects() {
    for point in [
        FaultPoint::BeforeJournal,
        FaultPoint::BeforePublish,
        FaultPoint::AfterReceipt,
    ] {
        let temp = native();
        let root = temp.path();
        let identity = Repository::discover(root).unwrap().identity().clone();
        let request = RequestId::new();
        let error =
            installer::apply_with_faults(root, Mode::Install, &identity, None, &request, |at| {
                if at == point {
                    use std::io::Write;
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(root.join(".workdeck/config.yml"))
                        .unwrap()
                        .write_all(b"\n# concurrent config edit\n")
                        .unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleSource, "{point:?}: {error}");
        if point == FaultPoint::BeforeJournal {
            assert!(!root.join(".workdeck/.local/protocol").exists());
        }
        if point == FaultPoint::AfterReceipt {
            assert!(root.join("AGENTS.md").exists());
            assert_eq!(error.details.unwrap()["request_effect_recorded"], true);
        } else {
            assert!(!root.join("AGENTS.md").exists());
        }
        let receipt = installer::apply(root, Mode::Install, &identity, None, &request).unwrap();
        assert_eq!(receipt.request_id, request);
    }
}
#[cfg(unix)]
#[test]
fn installer_rejects_local_and_selected_root_symlink_swaps_before_publication() {
    use std::os::unix::fs::symlink;
    for swap_root in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let identity = Repository::init(&root, "WD").unwrap().identity().clone();
        let selected = if swap_root {
            root.clone()
        } else {
            root.join(".workdeck/.local/protocol")
        };
        let saved = temp.path().join("saved");
        let result = installer::apply_with_faults(
            &root,
            Mode::Install,
            &identity,
            None,
            &RequestId::new(),
            |at| {
                if at == FaultPoint::BeforePublish {
                    std::fs::rename(&selected, &saved).unwrap();
                    symlink(&saved, &selected).unwrap();
                }
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::UnsafePath);
        assert!(!root.join("AGENTS.md").exists());
        std::fs::remove_file(&selected).unwrap();
        std::fs::rename(saved, selected).unwrap();
    }
}
#[test]
fn installer_concurrent_absence_cas_publishes_one_pointer() {
    let temp = native();
    let root = temp.path();
    let identity = Repository::discover(root).unwrap().identity().clone();
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    installer::apply(root, Mode::Install, &identity, None, &RequestId::new())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        original(root)
            .matches("<!-- workdeck:pm-protocol:start -->")
            .count(),
        1
    );
}
#[test]
fn protocol_cli_requires_all_source_preconditions_and_replays_local_receipt() {
    let temp = native();
    let root = temp.path();
    let identity = Repository::discover(root).unwrap().identity().to_string();
    for args in [
        vec![
            "protocol",
            "install",
            "--expect-absent",
            "--request-id",
            "missing-repo",
            "--json",
            "--no-input",
        ],
        vec![
            "protocol",
            "install",
            "--expected-repository",
            &identity,
            "--request-id",
            "missing-source",
            "--json",
        ],
        vec![
            "protocol",
            "install",
            "--expected-repository",
            &identity,
            "--expect-absent",
            "--json",
        ],
    ] {
        assert!(!run(root, &args).status.success());
        assert!(!root.join("AGENTS.md").exists());
        assert!(!root.join(".workdeck/.local/protocol").exists());
    }
    let args = [
        "protocol",
        "install",
        "--expected-repository",
        &identity,
        "--expect-absent",
        "--request-id",
        "protocol-once",
        "--json",
        "--no-input",
    ];
    let output = run(root, &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["result"]["repository"], identity);
    assert_eq!(receipt["result"]["scope"], "local_protocol_pointer");
    let later = format!("{}\nLater authored instructions\n", original(root));
    std::fs::write(root.join("AGENTS.md"), &later).unwrap();
    let replay = run(root, &args);
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&replay.stdout).unwrap(),
        receipt
    );
    assert_eq!(original(root), later);
    let fresh = tempfile::tempdir().unwrap();
    assert!(!run(fresh.path(), &args).status.success());
    assert!(!fresh.path().join(".workdeck").exists());
}
