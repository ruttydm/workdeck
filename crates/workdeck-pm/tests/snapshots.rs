use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use workdeck_pm::{
    AttachmentInput, Config, CreateIssue, CreatePlanning, ErrorCode, IssueMetadata, IssueMutation,
    IssueRecord, NativeSnapshot, PlanningKind, PmError, Repository, RequestId, RetirementInput,
    RetirementKind, RetirementTarget, SnapshotImportMode as Mode, UpdateIssue, decode_snapshot,
    transactions::FaultPoint,
};

fn fixture() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}
fn issue(repository: &Repository) -> (IssueRecord, CreateIssue, RequestId) {
    let mut input = CreateIssue::new("Exact authored subject", "");
    input.body = "Authored **body**.\n\n## Keep section\n".into();
    input.fields = BTreeMap::from([("x-preserved".into(), json!({"nested":[1,true,"text"]}))]);
    let request = RequestId::new();
    let receipt = repository.create_issue(&input, &request).unwrap();
    (
        serde_json::from_value(receipt.result).unwrap(),
        input,
        request,
    )
}
fn clone_source(source: &NativeSnapshot) -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join(".workdeck");
    for file in &source.files {
        let path = root.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.content).unwrap();
    }
    let repository = Repository::open_source(&root).unwrap();
    (temp, repository)
}
fn seed_identity(source: &NativeSnapshot) -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join(".workdeck");
    fs::create_dir(&root).unwrap();
    let config = source
        .files
        .iter()
        .find(|file| file.path == Path::new("config.yml"))
        .unwrap();
    fs::write(root.join("config.yml"), &config.content).unwrap();
    (temp, Repository::open_source(&root).unwrap())
}
fn write_issue(repository: &Repository, metadata: &IssueMetadata, body: &str) {
    let path = repository
        .root()
        .join(format!("issues/{}/item.md", metadata.id));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "---\n{}---\n{body}",
            serde_yaml_ng::to_string(metadata).unwrap()
        ),
    )
    .unwrap();
}
fn state(repository: &Repository) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap();
            if [".tmp", ".local", ".index"]
                .iter()
                .any(|skip| relative == Path::new(skip))
            {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(relative.into(), fs::read(path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(repository.root(), repository.root(), &mut files);
    files
}

#[test]
fn export_preserves_exact_supported_records_payloads_and_retirement_authority() {
    let (_temp, repository) = fixture();
    let (issue, _, _) = issue(&repository);
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Comment {
                author: "human".into(),
                body: "Historical comment\n".into(),
            },
            &RequestId::new(),
        )
        .unwrap();
    repository
        .attach_issue(
            issue.metadata.id.as_str(),
            None,
            &AttachmentInput {
                name: "opaque.bin".into(),
                content: vec![0, 255, 13, 10, 1],
                media_type: None,
                actor: "human".into(),
            },
            &RequestId::new(),
        )
        .unwrap();
    for kind in [
        PlanningKind::Project,
        PlanningKind::Cycle,
        PlanningKind::Label,
    ] {
        repository
            .create_planning(
                kind,
                &CreatePlanning {
                    id: Some("Keep_ID".into()),
                    name: "Preserved".into(),
                    body: String::new(),
                    fields: BTreeMap::new(),
                },
                &RequestId::new(),
            )
            .unwrap();
    }
    let template = repository.root().join("templates/issues/bug.md");
    fs::create_dir_all(template.parent().unwrap()).unwrap();
    fs::write(
        template,
        "---\nschema: 1\nid: bug\nname: Bug\ndefaults: {}\n---\nDescribe it.\n",
    )
    .unwrap();
    repository
        .retire_record(
            &RetirementInput::new(
                RetirementTarget::new(RetirementKind::Issue, issue.metadata.id.to_string())
                    .unwrap(),
            ),
            &RequestId::new(),
        )
        .unwrap();
    fs::write(repository.root().join("config.toml"), "editor = 'keep'\n").unwrap();
    fs::create_dir_all(repository.root().join("extensions")).unwrap();
    fs::write(
        repository.root().join("extensions/inert.lua"),
        "error('never execute')",
    )
    .unwrap();
    let before = state(&repository);
    let snapshot = repository.export_snapshot().unwrap();
    for file in &snapshot.files {
        assert_eq!(before[&file.path], file.content);
    }
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path.starts_with("tombstones"))
    );
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path.starts_with("operations"))
    );
    assert!(
        snapshot
            .files
            .iter()
            .all(|file| file.path != Path::new("config.toml")
                && !file.path.starts_with("extensions"))
    );
    assert_eq!(
        decode_snapshot(&serde_json::to_vec(&snapshot).unwrap()).unwrap(),
        snapshot
    );
    assert_eq!(state(&repository), before);
}

#[test]
fn decoding_distinguishes_bare_envelopes_versions_legacy_and_unknown_formats() {
    let (_temp, repository) = fixture();
    let snapshot = repository.export_snapshot().unwrap();
    for envelope in [
        json!({"ok":true,"kind":"export","data":snapshot}),
        json!({"api_version":1,"ok":true,"kind":"export","source":{"repository":repository.identity()},"result":snapshot}),
    ] {
        assert_eq!(
            decode_snapshot(&serde_json::to_vec(&envelope).unwrap()).unwrap(),
            snapshot
        );
    }
    for legacy in [
        json!({"repo_root":"/old","issues":[],"projects":[]}),
        json!({"ok":true,"kind":"export","data":{"issues":[]}}),
    ] {
        assert_eq!(
            decode_snapshot(&serde_json::to_vec(&legacy).unwrap())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }
    for bad in [
        json!({}),
        json!({"ok":false,"kind":"export","data":snapshot}),
        json!({"ok":true,"kind":"export","data":snapshot,"result":snapshot}),
        json!({"ok":true,"kind":"export","source":{"repository":"wrong"},"result":snapshot}),
    ] {
        assert!(decode_snapshot(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut future = serde_json::to_value(&snapshot).unwrap();
    future["version"] = json!(99);
    assert_eq!(
        decode_snapshot(&serde_json::to_vec(&future).unwrap())
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedSchema
    );
    assert!(decode_snapshot(b"{\"kind\":\"repo\"}\n{\"kind\":\"issue\"}\n").is_err());
}

#[test]
fn decoded_paths_kinds_hashes_and_duplicates_fail_before_any_store_mutation() {
    let (_temp, repository) = fixture();
    issue(&repository);
    let snapshot = repository.export_snapshot().unwrap();
    let before = state(&repository);
    for path in [
        "../outside",
        "/tmp/outside",
        "issues/../outside",
        "issues\\outside",
        ".tmp/journals/injected.yml",
        "features/future/item.md",
    ] {
        let mut value = serde_json::to_value(&snapshot).unwrap();
        value["files"][0]["path"] = json!(path);
        assert!(
            decode_snapshot(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{path}"
        );
    }
    let mut duplicate = serde_json::to_value(&snapshot).unwrap();
    let first = duplicate["files"][0].clone();
    duplicate["files"].as_array_mut().unwrap().push(first);
    assert!(decode_snapshot(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    let mut tampered = serde_json::to_value(&snapshot).unwrap();
    tampered["files"][0]["content"] = json!("Zm9yZ2Vk");
    assert!(decode_snapshot(&serde_json::to_vec(&tampered).unwrap()).is_err());
    assert_eq!(state(&repository), before);
}

#[test]
fn noop_import_and_original_request_replay_preserve_receipt_authority() {
    let (_temp, repository) = fixture();
    let (original, input, creation) = issue(&repository);
    let snapshot = repository.export_snapshot().unwrap();
    let plan = repository
        .preview_snapshot_import(&snapshot, Mode::Merge)
        .unwrap();
    assert!(plan.allowed);
    assert!(plan.changes.is_empty());
    let request = RequestId::new();
    let receipt = repository
        .import_snapshot(&snapshot, Mode::Merge, Some(&plan.fingerprint), &request)
        .unwrap();
    assert!(receipt.changed.is_empty());
    repository
        .update_issue(
            original.metadata.id.as_str(),
            &original.source,
            &UpdateIssue {
                body: Some("Later subject".into()),
                fields: BTreeMap::new(),
            },
            &RequestId::new(),
        )
        .unwrap();
    let before = state(&repository);
    assert_eq!(
        repository
            .import_snapshot(&snapshot, Mode::Merge, Some(&plan.fingerprint), &request)
            .unwrap(),
        receipt
    );
    assert_eq!(
        repository.create_issue(&input, &creation).unwrap().result,
        serde_json::to_value(original).unwrap()
    );
    assert_eq!(state(&repository), before);
}

#[test]
fn merge_adds_direct_authored_record_and_replace_matching_retains_unrelated_records() {
    let (_temp, repository) = fixture();
    let (original, _, _) = issue(&repository);
    let snapshot = repository.export_snapshot().unwrap();
    let (_fork, fork) = clone_source(&snapshot);
    let added = IssueMetadata::new(
        &fork.config().unwrap(),
        "Direct authored addition",
        chrono::Utc::now(),
    )
    .unwrap();
    write_issue(&fork, &added, "Added body\n");
    let merge = fork.export_snapshot().unwrap();
    let plan = repository
        .preview_snapshot_import(&merge, Mode::Merge)
        .unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    assert_eq!(plan.changes.len(), 1);
    repository
        .import_snapshot(
            &merge,
            Mode::Merge,
            Some(&plan.fingerprint),
            &RequestId::new(),
        )
        .unwrap();
    let mut changed = original.metadata.clone();
    changed.title = "Changed with exact bytes".into();
    changed.revision = changed.revision.next().unwrap();
    changed.updated_at += chrono::Duration::seconds(1);
    write_issue(&fork, &changed, "New authored body\n");
    let replacement = fork.export_snapshot().unwrap();
    assert!(
        !repository
            .preview_snapshot_import(&replacement, Mode::Merge)
            .unwrap()
            .allowed
    );
    let plan = repository
        .preview_snapshot_import(&replacement, Mode::ReplaceMatching)
        .unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    repository
        .import_snapshot(
            &replacement,
            Mode::ReplaceMatching,
            Some(&plan.fingerprint),
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(
        fs::read(repository.root().join(&original.path)).unwrap(),
        fs::read(fork.root().join(&original.path)).unwrap()
    );
    assert_eq!(repository.list_issues().unwrap().len(), 2);
}

#[test]
fn new_membership_and_direct_editor_races_invalidate_reviewed_import() {
    let (_temp, repository) = fixture();
    issue(&repository);
    let snapshot = repository.export_snapshot().unwrap();
    let plan = repository
        .preview_snapshot_import(&snapshot, Mode::Merge)
        .unwrap();
    issue(&repository);
    let before = state(&repository);
    assert_eq!(
        repository
            .import_snapshot(
                &snapshot,
                Mode::Merge,
                Some(&plan.fingerprint),
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(state(&repository), before);
    let plan = repository
        .preview_snapshot_import(&snapshot, Mode::Merge)
        .unwrap();
    let extra = IssueMetadata::new(
        &repository.config().unwrap(),
        "Direct racing addition",
        chrono::Utc::now(),
    )
    .unwrap();
    let error = repository
        .import_snapshot_with_faults(
            &snapshot,
            Mode::Merge,
            Some(&plan.fingerprint),
            &RequestId::new(),
            |point| {
                if point == FaultPoint::BeforeJournal {
                    write_issue(&repository, &extra, "Editor owns this\n");
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(
        repository.show_issue(extra.id.as_str()).unwrap().body,
        "Editor owns this\n"
    );
}

#[test]
fn restore_preview_blocks_absent_receipts_and_wrong_repository_without_writes() {
    let (_temp, source) = fixture();
    issue(&source);
    let snapshot = source.export_snapshot().unwrap();
    let (_empty, destination) = seed_identity(&snapshot);
    let before = state(&destination);
    let plan = destination
        .preview_snapshot_import(&snapshot, Mode::Merge)
        .unwrap();
    assert!(!plan.allowed);
    assert!(plan.blockers.iter().any(|error| {
        error.code == ErrorCode::Unsupported
            && error
                .path
                .as_deref()
                .is_some_and(|path| path.starts_with("operations/"))
    }));
    assert!(
        destination
            .import_snapshot(
                &snapshot,
                Mode::Merge,
                Some(&plan.fingerprint),
                &RequestId::new()
            )
            .is_err()
    );
    assert_eq!(state(&destination), before);
    let (_other, other) = fixture();
    let before = state(&other);
    assert!(
        !other
            .preview_snapshot_import(&snapshot, Mode::ReplaceMatching)
            .unwrap()
            .allowed
    );
    assert_eq!(state(&other), before);
}

#[test]
fn projected_workflow_and_completion_provenance_cannot_be_bypassed_by_replacement() {
    for completion in [false, true] {
        let (_temp, repository) = fixture();
        let (original, _, _) = issue(&repository);
        if !completion {
            let mut config = repository.config().unwrap();
            config
                .workflow
                .states
                .iter_mut()
                .find(|state| state.id == "ready")
                .unwrap()
                .transitions
                .clear();
            fs::write(
                repository.root().join("config.yml"),
                serde_yaml_ng::to_string(&config).unwrap(),
            )
            .unwrap();
        }
        let base = repository.export_snapshot().unwrap();
        let (_fork, fork) = clone_source(&base);
        let mut changed = original.metadata.clone();
        changed.revision = changed.revision.next().unwrap();
        changed.updated_at += chrono::Duration::seconds(1);
        changed.status = if completion { "done" } else { "in_progress" }.into();
        if completion {
            changed.completed_at = Some(changed.updated_at);
        }
        write_issue(&fork, &changed, &original.body);
        let input = fork.export_snapshot().unwrap();
        let before = state(&repository);
        let plan = repository
            .preview_snapshot_import(&input, Mode::ReplaceMatching)
            .unwrap();
        assert!(!plan.allowed);
        assert!(
            plan.blockers
                .iter()
                .any(|error| error.code == ErrorCode::PolicyBlocked)
        );
        assert!(
            repository
                .import_snapshot(
                    &input,
                    Mode::ReplaceMatching,
                    Some(&plan.fingerprint),
                    &RequestId::new()
                )
                .is_err()
        );
        assert_eq!(state(&repository), before);
    }
}

#[test]
fn completed_subject_and_immutable_comment_history_cannot_be_replaced() {
    let (_temp, repository) = fixture();
    let (original, _, _) = issue(&repository);
    repository
        .mutate_issue(
            original.metadata.id.as_str(),
            None,
            &IssueMutation::Complete { manual: None },
            &RequestId::new(),
        )
        .unwrap();
    let original = repository
        .show_issue(original.metadata.id.as_str())
        .unwrap();
    let base = repository.export_snapshot().unwrap();
    let (_fork, fork) = clone_source(&base);
    let mut changed = original.metadata.clone();
    changed.revision = changed.revision.next().unwrap();
    changed.updated_at += chrono::Duration::seconds(1);
    write_issue(&fork, &changed, "Different accepted subject\n");
    let input = fork.export_snapshot().unwrap();
    assert!(
        !repository
            .preview_snapshot_import(&input, Mode::ReplaceMatching)
            .unwrap()
            .allowed
    );
    let (_temp, repository) = fixture();
    let (issue, _, _) = issue(&repository);
    let comment = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Comment {
                author: "Actor".into(),
                body: "Keep this".into(),
            },
            &RequestId::new(),
        )
        .unwrap();
    let base = repository.export_snapshot().unwrap();
    let (_fork, fork) = clone_source(&base);
    let path = comment.result["comment"]["path"].as_str().unwrap();
    let text = fs::read_to_string(fork.root().join(path)).unwrap();
    fs::write(
        fork.root().join(path),
        text.replace("Keep this", "Different history"),
    )
    .unwrap();
    let input = fork.export_snapshot().unwrap();
    let plan = repository
        .preview_snapshot_import(&input, Mode::ReplaceMatching)
        .unwrap();
    assert!(!plan.allowed);
    assert!(
        plan.blockers
            .iter()
            .any(|error| error.code == ErrorCode::Conflict)
    );
}

#[test]
fn interrupted_import_recovers_and_returns_the_original_request_result() {
    let (_temp, repository) = fixture();
    let base = repository.export_snapshot().unwrap();
    let (_fork, fork) = clone_source(&base);
    for title in ["One", "Two"] {
        let metadata =
            IssueMetadata::new(&fork.config().unwrap(), title, chrono::Utc::now()).unwrap();
        write_issue(&fork, &metadata, "Body\n");
    }
    let input = fork.export_snapshot().unwrap();
    let plan = repository
        .preview_snapshot_import(&input, Mode::Merge)
        .unwrap();
    let request = RequestId::new();
    assert_eq!(
        repository
            .import_snapshot_with_faults(
                &input,
                Mode::Merge,
                Some(&plan.fingerprint),
                &request,
                |point| if point == FaultPoint::AfterChange(0) {
                    Err(PmError::new(ErrorCode::Canceled, "injected interruption"))
                } else {
                    Ok(())
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    assert_eq!(
        repository.list_issues().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    let recovered = repository.recover_operations().unwrap();
    assert_eq!(recovered.len(), 1);
    let replay = repository
        .import_snapshot(&input, Mode::Merge, Some(&plan.fingerprint), &request)
        .unwrap();
    assert_eq!(replay, recovered[0]);
    assert_eq!(repository.list_issues().unwrap().len(), 2);
}

#[test]
fn migrated_export_is_exact_and_restoration_reports_protocol_authority_blockers() {
    let temp = TempDir::new().unwrap();
    let legacy = temp.path().join(".agents/workdeck");
    fs::create_dir_all(legacy.join("issues")).unwrap();
    fs::write(legacy.join("issues/WD-1.toml"),"key='WD-1'\ntitle='Old work'\nstatus='done'\ncreated_at='2026-09-01T00:00:00Z'\nupdated_at='2026-09-02T00:00:00Z'\n").unwrap();
    fs::write(
        legacy.join("events.jsonl"),
        "{\"kind\":\"old_check_passed\",\"data\":{\"claim\":\"annotation only\"}}\n",
    )
    .unwrap();
    let options = workdeck_pm::migration::PreviewOptions {
        config: Config::new("WD").unwrap(),
        imported_at: "2026-09-09T00:00:00Z".parse().unwrap(),
    };
    let plan =
        workdeck_pm::migration::preview(&legacy, &temp.path().join(".workdeck"), &options).unwrap();
    assert!(plan.complete, "{:?}", plan.blockers);
    workdeck_pm::migration::apply(&plan, &RequestId::new()).unwrap();
    let repository = Repository::discover(temp.path()).unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let decoded = decode_snapshot(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    assert_eq!(decoded, snapshot);
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == Path::new("migration.yml"))
    );
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path.starts_with("migrations"))
    );
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == Path::new("imported-history/events.jsonl"))
    );
    let noop = repository
        .preview_snapshot_import(&snapshot, Mode::Merge)
        .unwrap();
    assert!(noop.allowed, "{:?}", noop.blockers);
    repository
        .import_snapshot(
            &snapshot,
            Mode::Merge,
            Some(&noop.fingerprint),
            &RequestId::new(),
        )
        .unwrap();
    assert!(
        Repository::open_source(repository.root())
            .unwrap()
            .doctor()
            .unwrap()
            .valid
    );
    let (_empty, destination) = seed_identity(&snapshot);
    let blocked = destination
        .preview_snapshot_import(&snapshot, Mode::Merge)
        .unwrap();
    assert!(!blocked.allowed);
    assert!(
        blocked
            .blockers
            .iter()
            .any(|error| error.path.as_deref() == Some("migration.yml"))
    );
    assert!(
        destination
            .import_snapshot(&snapshot, Mode::Merge, None, &RequestId::new())
            .is_err()
    );
}

#[test]
fn unknown_authority_and_oversized_documents_do_not_produce_partial_exports() {
    let (_temp, repository) = fixture();
    issue(&repository);
    fs::create_dir(repository.root().join("future-authority")).unwrap();
    fs::write(
        repository.root().join("future-authority/future.md"),
        "Do not silently skip",
    )
    .unwrap();
    assert_eq!(
        repository.export_snapshot().unwrap_err().code,
        ErrorCode::Unsupported
    );
    fs::remove_file(repository.root().join("future-authority/future.md")).unwrap();
    let path = repository.list_issues().unwrap()[0].path.clone();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(repository.root().join(path))
        .unwrap();
    file.set_len(2 * 1024 * 1024 + 1).unwrap();
    assert!(repository.export_snapshot().is_err());
}

#[test]
fn retained_session_markers_export_as_validated_historical_annotations() {
    let (_temp, repository) = fixture();
    let created = repository
        .create_recorded_session(
            &workdeck_pm::NewRecordedSession {
                id: Some("recorded_old_session".into()),
                title: "Historical only".into(),
                fields: BTreeMap::from([
                    ("tests_run".into(), json!(["claimed check passed"])),
                    ("x-history".into(), json!({"unknown":"keep"})),
                ]),
            },
            &RequestId::new(),
        )
        .unwrap();
    let original: workdeck_pm::SessionRecord = serde_json::from_value(created.result).unwrap();
    repository
        .mutate_recorded_session(
            &original.session.id,
            Some(&original.source),
            &workdeck_pm::SessionMutation::Delete,
            &RequestId::new(),
        )
        .unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    assert!(
        snapshot.files.iter().any(|file| file.path
            == Path::new("imported-history/deleted-sessions/recorded_old_session.yml"))
    );
    assert_eq!(
        decode_snapshot(&serde_json::to_vec(&snapshot).unwrap()).unwrap(),
        snapshot
    );
    let plan = repository
        .preview_snapshot_import(&snapshot, Mode::Merge)
        .unwrap();
    assert!(plan.allowed);
    let record = repository.recorded_session(&original.session.id).unwrap();
    assert!(record.retired);
    assert_eq!(record.evidence, "historical_annotation");
    assert_eq!(record.session.tests_run, vec!["claimed check passed"]);
    let (_fork, fork) = clone_source(&snapshot);
    let marker = fork
        .root()
        .join("imported-history/deleted-sessions/recorded_old_session.yml");
    let text = fs::read_to_string(&marker).unwrap();
    fs::write(
        marker,
        text.replace("recorded_old_session", "forged_identity"),
    )
    .unwrap();
    assert!(fork.export_snapshot().is_err());
}

#[test]
fn different_existing_receipt_is_an_explicit_preview_conflict() {
    let (_temp, repository) = fixture();
    issue(&repository);
    let snapshot = repository.export_snapshot().unwrap();
    let operation = snapshot
        .files
        .iter()
        .find(|file| file.path.starts_with("operations"))
        .unwrap();
    let mut receipt: workdeck_pm::transactions::MutationReceipt =
        serde_yaml_ng::from_slice(&operation.content).unwrap();
    receipt.input_hash = workdeck_pm::ContentHash::of(b"another input");
    fs::write(
        repository.root().join(&operation.path),
        serde_yaml_ng::to_string(&receipt).unwrap(),
    )
    .unwrap();
    let before = state(&repository);
    let plan = repository
        .preview_snapshot_import(&snapshot, Mode::ReplaceMatching)
        .unwrap();
    assert!(!plan.allowed);
    assert!(
        plan.blockers
            .iter()
            .any(|error| error.code == ErrorCode::Conflict
                && error.path.as_deref() == operation.path.to_str())
    );
    assert!(
        repository
            .import_snapshot(&snapshot, Mode::ReplaceMatching, None, &RequestId::new())
            .is_err()
    );
    assert_eq!(state(&repository), before);
}

#[test]
fn replace_matching_cannot_drop_label_identities_or_change_retired_ones() {
    let (_temp, repository) = fixture();
    for id in ["first", "second"] {
        repository
            .create_planning(
                PlanningKind::Label,
                &CreatePlanning {
                    id: Some(id.into()),
                    name: id.into(),
                    body: String::new(),
                    fields: BTreeMap::new(),
                },
                &RequestId::new(),
            )
            .unwrap();
    }
    let snapshot = repository.export_snapshot().unwrap();
    let (_fork, fork) = clone_source(&snapshot);
    let path = fork.root().join("labels.yml");
    let mut labels: workdeck_pm::LabelsMetadata =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    labels.labels.retain(|label| label.id != "second");
    fs::write(path, serde_yaml_ng::to_string(&labels).unwrap()).unwrap();
    let input = fork.export_snapshot().unwrap();
    let plan = repository
        .preview_snapshot_import(&input, Mode::ReplaceMatching)
        .unwrap();
    assert!(!plan.allowed);
    assert!(plan.blockers.iter().any(|error|error.code==ErrorCode::PolicyBlocked&&error.message.contains("remove")));
    repository
        .retire_record(
            &RetirementInput::new(RetirementTarget::new(RetirementKind::Label, "first").unwrap()),
            &RequestId::new(),
        )
        .unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let (_fork, fork) = clone_source(&snapshot);
    let path = fork.root().join("labels.yml");
    let mut labels: workdeck_pm::LabelsMetadata =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    labels
        .labels
        .iter_mut()
        .find(|label| label.id == "first")
        .unwrap()
        .name = "Changed retired history".into();
    fs::write(path, serde_yaml_ng::to_string(&labels).unwrap()).unwrap();
    assert!(fork.export_snapshot().is_err());
}

#[test]
fn whole_snapshot_content_cap_rejects_oversized_aggregate_before_any_import() {
    let (_temp, repository) = fixture();
    let body = "x".repeat(1_900_000);
    for index in 0..18 {
        let metadata = IssueMetadata::new(
            &repository.config().unwrap(),
            &format!("Issue {index}"),
            chrono::Utc::now(),
        )
        .unwrap();
        write_issue(&repository, &metadata, &body);
    }
    assert_eq!(
        repository.export_snapshot().unwrap_err().code,
        ErrorCode::Unsupported
    );
}

fn framed_jsonl(snapshot: &NativeSnapshot) -> String {
    let mut frames = vec![
        json!({"kind":"snapshot","payload":{"format":snapshot.format,"version":snapshot.version,"repository":snapshot.repository,"fingerprint":snapshot.fingerprint,"file_count":snapshot.files.len()}}),
    ];
    frames.extend(
        snapshot
            .files
            .iter()
            .map(|file| json!({"kind":"file","payload":file})),
    );
    frames
        .iter()
        .map(|frame| format!("{}\n", serde_json::to_string(frame).unwrap()))
        .collect()
}

#[test]
fn native_jsonl_frames_roundtrip_exact_content_and_reject_incomplete_streams() {
    let (_temp, repository) = fixture();
    issue(&repository);
    let snapshot = repository.export_snapshot().unwrap();
    let jsonl = framed_jsonl(&snapshot);
    assert_eq!(snapshot.to_jsonl().unwrap(), jsonl);
    assert_eq!(decode_snapshot(jsonl.as_bytes()).unwrap(), snapshot);
    let lines = jsonl.lines().collect::<Vec<_>>();
    let mut malformed = vec![
        lines[..lines.len() - 1].join("\n"),
        lines[1..].join("\n"),
        format!("{jsonl}{{malformed"),
        format!("{jsonl}{}\n", lines[0]),
        format!("{jsonl}{}\n", lines[1]),
    ];
    let mut unknown = serde_json::from_str::<serde_json::Value>(lines[1]).unwrap();
    unknown["kind"] = json!("future-record");
    malformed.push(format!("{}\n{}\n", lines[0], unknown));
    for input in malformed {
        assert!(decode_snapshot(input.as_bytes()).is_err());
    }
}

#[test]
fn legacy_jsonl_is_recognized_without_silent_record_loss() {
    let input=b"{\"kind\":\"repo\",\"payload\":{\"root\":\"/old\"}}\n{\"kind\":\"issue\",\"payload\":{\"key\":\"WD-1\"}}\n";
    assert_eq!(
        decode_snapshot(input).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

#[test]
fn projected_case_collisions_are_preview_blockers_for_opaque_history_too() {
    let (_temp, repository) = fixture();
    let directory = repository.root().join("imported-handoffs");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("note.md"), "Keep original history").unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let (_fork, fork) = clone_source(&snapshot);
    fs::rename(
        fork.root().join("imported-handoffs/note.md"),
        fork.root().join("imported-handoffs/Note.md"),
    )
    .unwrap();
    let incoming = fork.export_snapshot().unwrap();
    let before = state(&repository);
    let preview = repository
        .preview_snapshot_import(&incoming, Mode::Merge)
        .unwrap();
    assert!(
        !preview.allowed,
        "portable destination collision must be caught by preview"
    );
    assert!(
        repository
            .import_snapshot(
                &incoming,
                Mode::Merge,
                Some(&preview.fingerprint),
                &RequestId::new()
            )
            .is_err()
    );
    assert_eq!(state(&repository), before);
}
