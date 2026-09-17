use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};
use workdeck_pm::{
    CreateIssue, CreatePlanning, ErrorCode, ImportSource, LegacyExport, LegacyImportContext,
    PlanningKind, Repository, RequestId, SnapshotImportMode as Mode, decode_transfer,
};
fn fixture() -> (tempfile::TempDir, Repository) {
    let root = tempfile::tempdir().unwrap();
    let repo = Repository::init(root.path(), "WD").unwrap();
    (root, repo)
}
fn context() -> LegacyImportContext {
    LegacyImportContext {
        imported_at: "2026-09-09T10:00:00Z".parse().unwrap(),
    }
}
fn legacy(value: Value) -> LegacyExport {
    legacy_bytes(&serde_json::to_vec(&value).unwrap())
}
fn legacy_bytes(bytes: &[u8]) -> LegacyExport {
    match decode_transfer(bytes).unwrap() {
        ImportSource::Legacy(source) => source,
        _ => panic!("expected legacy export"),
    }
}
fn row(id: &str, status: &str) -> Value {
    json!({"key":id,"title":" Preserve title ","description":"## Authored body\n\nExact.\n","status":status,"priority":"high","created_at":"2020-01-01T01:02:03Z","updated_at":"2021-02-02T03:04:05Z","assignee":"human","labels":[],"linked_files":["src/main.rs"],"linked_commits":["abcdef1"],"extra":{"null":null,"unsigned":u64::MAX}})
}
#[test]
fn converts_full_legacy_export_with_exact_origin_and_historical_completion() {
    let (_root, repo) = fixture();
    let mut issue = row("WD-7", "done");
    issue["project"] = json!("Keep_ID");
    issue["cycle"] = json!("Sprint_A");
    issue["labels"] = json!(["Bug"]);
    let source = legacy(
        json!({"repo_root":"/historical/annotation","issues":[issue],"projects":[{"id":"Keep_ID","name":"Project","description":"Project body\n","status":"shipped","extra":null}],"cycles":[{"id":"Sprint_A","name":"Cycle","starts_at":"2020-01-01","ends_at":"2020-01-02","status":"finished"}],"labels":[{"id":"Bug","name":"Bug","color":"any legacy color"}],"agent_sessions":[{"id":"Session_A","title":"Historical commands","commands_run":["never execute"],"extra":null,"big":u64::MAX,"touched_files":[{"path":"src/main.rs","extra":null}]}],"events":[{"kind":"issue.changed","payload":{"key":"WD-7","unknown":null},"created_at":"historical"}]}),
    );
    let plan = repo
        .preview_legacy_import(&source, Some(&context()), Mode::Merge)
        .unwrap();
    assert!(plan.plan.allowed, "{:?}", plan.plan.blockers);
    repo.import_legacy_export(
        &source,
        Some(&context()),
        Mode::Merge,
        Some(&plan.plan.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    let issue = repo.show_issue("WD-7").unwrap();
    assert_eq!(issue.metadata.title, " Preserve title ");
    assert_eq!(issue.body, "## Authored body\n\nExact.\n");
    assert_eq!(
        issue.metadata.created_at,
        "2020-01-01T01:02:03Z"
            .parse::<workdeck_pm::Timestamp>()
            .unwrap()
    );
    assert_eq!(issue.metadata.status, "done");
    assert!(issue.metadata.completed_at.is_none());
    assert!(issue.metadata.manual_acceptance.is_none());
    let proof = issue.metadata.imported_completion.unwrap();
    assert_eq!(proof.source_path, source.source_path().to_str().unwrap());
    assert_eq!(proof.source_content, *source.content_hash());
    assert_eq!(
        issue.metadata.custom["legacy"]["record_selector"],
        "/issues/0"
    );
    assert_eq!(
        issue.metadata.custom["legacy"]["extra"]["extra"]["unsigned"],
        json!(u64::MAX)
    );
    let project = repo
        .planning_record(PlanningKind::Project, "Keep_ID")
        .unwrap();
    assert!(project.metadata.created_at.is_none());
    assert_eq!(project.body, "Project body\n");
    assert_eq!(
        serde_json::to_value(project.metadata.imported.unwrap()).unwrap()["format"],
        "legacy_json"
    );
    let session = repo.recorded_session("Session_A").unwrap();
    assert_eq!(session.evidence, "historical_annotation");
    assert_eq!(
        session.session.extra["extra"]["x-workdeck-json-value"]["type"].as_str(),
        Some("null")
    );
    assert_eq!(
        session.session.extra["big"]["x-workdeck-json-value"]["value"].as_str(),
        Some("18446744073709551615")
    );
    assert_eq!(
        session.session.touched_files[0].extra["extra"]["x-workdeck-json-value"]["type"].as_str(),
        Some("null")
    );
    assert_eq!(repo.historical_events().unwrap().len(), 1);
    assert_eq!(
        fs::read(repo.root().join(source.source_path())).unwrap(),
        source.raw_bytes()
    );
    assert!(repo.doctor().unwrap().valid);
    let exported = repo.export_snapshot().unwrap();
    assert!(
        exported
            .files
            .iter()
            .any(|f| f.path == source.source_path() && f.content == source.raw_bytes())
    );
}
#[test]
fn automatic_context_is_replay_stable_after_later_config_and_record_changes() {
    let (_root, repo) = fixture();
    let source = legacy(json!({"issues":[row("WD-8","todo")]}));
    let request = RequestId::new();
    let receipt = repo
        .import_legacy_export(&source, None, Mode::Merge, None, &request)
        .unwrap();
    repo.mutate_issue(
        "WD-8",
        None,
        &workdeck_pm::IssueMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    let mut config = repo.config().unwrap();
    config.prefix = "NEW".into();
    fs::write(
        repo.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    let replay = repo
        .import_legacy_export(&source, None, Mode::Merge, None, &request)
        .unwrap();
    assert_eq!(receipt.operation_id, replay.operation_id);
    assert_eq!(receipt.result, replay.result);
}
#[test]
fn previewed_legacy_import_rejects_stale_destination_and_contextless_apply() {
    let (_root, repo) = fixture();
    let source = legacy(json!({"issues":[row("WD-9","todo")]}));
    let plan = repo
        .preview_legacy_import(&source, Some(&context()), Mode::Merge)
        .unwrap();
    assert_eq!(
        repo.import_legacy_export(
            &source,
            None,
            Mode::Merge,
            Some(&plan.plan.fingerprint),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
    repo.create_issue(
        &CreateIssue::new("Unrelated changed membership", ""),
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.import_legacy_export(
            &source,
            Some(&context()),
            Mode::Merge,
            Some(&plan.plan.fingerprint),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert!(repo.show_issue("WD-9").is_err());
}
#[test]
fn legacy_framing_preserves_values_and_rejects_ambiguous_or_unknown_payloads() {
    let bare = json!({"repo_root":"/annotation","issues":[row("WD-1","todo")],"events":[{"kind":"event","extra":null}]});
    let normal = legacy(bare.clone()).canonical_document();
    for input in [
        json!({"ok":true,"kind":"export","data":bare}),
        json!({"api_version":1,"ok":true,"kind":"export","result":bare}),
    ] {
        assert_eq!(legacy(input).canonical_document(), normal);
    }
    let stream = format!(
        "{}\n{}\n{}\n",
        json!({"kind":"repo","payload":{"root":"/annotation"}}),
        json!({"kind":"issue","payload":row("WD-1","todo")}),
        json!({"kind":"event","payload":{"kind":"event","extra":null}})
    );
    assert_eq!(legacy_bytes(stream.as_bytes()).canonical_document(), normal);
    for bytes in [b"{}".as_slice(),b"{\"issues\":[],\"issues\":[]}",b"{\"issues\":[{\"key\":\"WD-1\",\"key\":\"WD-2\"}]}",b"{\"issues\":[],\"unknown\":[]}",b"{\"kind\":\"issue\",\"payload\":{}}",b"{\"kind\":\"repo\",\"payload\":{\"root\":\"x\"}}\n{\"kind\":\"repo\",\"payload\":{\"root\":\"x\"}}",b"{\"kind\":\"repo\",\"payload\":{\"root\":\"x\"}}\n{\"kind\":\"future\",\"payload\":{}}",b"{\"kind\":\"repo\",\"payload\":{\"root\":\"x\"}}\n{"]{assert!(decode_transfer(bytes).is_err(),"accepted {}",String::from_utf8_lossy(bytes));}
}
fn authority(repo: &Repository) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    repo.export_snapshot()
        .unwrap()
        .files
        .into_iter()
        .map(|file| (file.path, file.content))
        .collect()
}
#[test]
fn merge_adds_labels_and_event_multiplicity_without_rewriting_existing_rows() {
    let (_root, repo) = fixture();
    repo.create_planning(
        PlanningKind::Label,
        &CreatePlanning {
            id: Some("existing".into()),
            name: "Keep label".into(),
            body: String::new(),
            fields: BTreeMap::new(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let path = repo.root().join("labels.yml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("# authored catalog comment\n{original}")).unwrap();
    let event = json!({"kind":"test","payload":{"value":1}});
    let event_path = repo.root().join("imported-history/events.jsonl");
    fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    let old_event = format!("  {}  ", serde_json::to_string(&event).unwrap());
    fs::write(&event_path, &old_event).unwrap();
    let source =
        legacy(json!({"labels":[{"id":"new_label","name":"Added"}],"events":[event,event,event]}));
    let old_label = repo
        .planning_record(PlanningKind::Label, "existing")
        .unwrap()
        .metadata;
    let plan = repo
        .preview_legacy_import(&source, Some(&context()), Mode::Merge)
        .unwrap();
    assert!(plan.plan.allowed, "{:?}", plan.plan.blockers);
    repo.import_legacy_export(
        &source,
        Some(&context()),
        Mode::Merge,
        None,
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.planning_record(PlanningKind::Label, "existing")
            .unwrap()
            .metadata,
        old_label
    );
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .starts_with("# authored catalog comment\n")
    );
    assert!(
        fs::read_to_string(event_path)
            .unwrap()
            .starts_with(&old_event)
    );
    assert_eq!(
        repo.historical_events().unwrap(),
        vec![event.clone(), event.clone(), event]
    );
    let again = repo
        .preview_legacy_import(&source, Some(&context()), Mode::Merge)
        .unwrap();
    assert!(again.plan.allowed, "{:?}", again.plan.blockers);
    assert!(again.plan.changes.is_empty());
}
#[test]
fn collisions_are_preview_blockers_and_replace_matching_never_overwrites_legacy_id() {
    let (_root, repo) = fixture();
    let initial =
        legacy(json!({"issues":[row("WD-11","todo")],"labels":[{"id":"Keep","name":"Original"}]}));
    repo.import_legacy_export(
        &initial,
        Some(&context()),
        Mode::Merge,
        None,
        &RequestId::new(),
    )
    .unwrap();
    let before = authority(&repo);
    let mut changed = row("WD-11", "done");
    changed["title"] = json!("Must not overwrite");
    let source = legacy(json!({"issues":[changed],"labels":[{"id":"keep","name":"Collision"}]}));
    for mode in [Mode::Merge, Mode::ReplaceMatching] {
        let plan = repo
            .preview_legacy_import(&source, Some(&context()), mode)
            .unwrap();
        assert!(!plan.plan.allowed);
        assert!(
            plan.plan
                .blockers
                .iter()
                .filter(|error| error.code == ErrorCode::Conflict)
                .count()
                >= 2
        );
        assert!(
            repo.import_legacy_export(&source, Some(&context()), mode, None, &RequestId::new())
                .is_err()
        );
        assert_eq!(authority(&repo), before);
    }
}
#[test]
fn unresolved_retired_and_cross_source_references_are_checked_on_projected_source() {
    let (_root, repo) = fixture();
    repo.create_planning(
        PlanningKind::Project,
        &CreatePlanning {
            id: Some("existing".into()),
            name: "Existing".into(),
            body: String::new(),
            fields: BTreeMap::new(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let mut issue = row("WD-12", "todo");
    issue["project"] = json!("existing");
    let source = legacy(json!({"issues":[issue]}));
    assert!(
        repo.preview_legacy_import(&source, Some(&context()), Mode::Merge)
            .unwrap()
            .plan
            .allowed
    );
    repo.retire_record(
        &workdeck_pm::RetirementInput::new(
            workdeck_pm::RetirementTarget::new(workdeck_pm::RetirementKind::Project, "existing")
                .unwrap(),
        ),
        &RequestId::new(),
    )
    .unwrap();
    let before = authority(&repo);
    let plan = repo
        .preview_legacy_import(&source, Some(&context()), Mode::Merge)
        .unwrap();
    assert!(!plan.plan.allowed);
    assert!(
        repo.import_legacy_export(
            &source,
            Some(&context()),
            Mode::Merge,
            None,
            &RequestId::new()
        )
        .is_err()
    );
    assert_eq!(authority(&repo), before);
    let mut missing = row("WD-13", "todo");
    missing["cycle"] = json!("missing");
    let missing = legacy(json!({"issues":[missing]}));
    assert!(
        !repo
            .preview_legacy_import(&missing, Some(&context()), Mode::Merge)
            .unwrap()
            .plan
            .allowed
    );
}
#[test]
fn malformed_semantic_fields_and_duplicate_or_unsafe_ids_never_mutate() {
    let (_root, repo) = fixture();
    let before = authority(&repo);
    let mut missing = row("WD-14", "todo");
    missing.as_object_mut().unwrap().remove("created_at");
    let mut null_title = row("WD-15", "todo");
    null_title["title"] = Value::Null;
    let mut bad_status = row("WD-16", "unknown");
    bad_status["updated_at"] = json!("not-a-time");
    for value in [
        json!({"issues":[missing]}),
        json!({"issues":[null_title]}),
        json!({"issues":[bad_status]}),
        json!({"issues":[row("WD-17","todo"),row("WD-17","todo")]}),
        json!({"issues":[row("../escape","todo")]}),
        json!({"projects":[{"id":"Case","name":"A"},{"id":"case","name":"B"}]}),
        json!({"agent_sessions":[{"id":"../escape","title":"Bad"}]}),
        json!({"agent_sessions":[{"id":"bad","title":null}]}),
    ] {
        let source = legacy(value);
        let plan = repo
            .preview_legacy_import(&source, Some(&context()), Mode::Merge)
            .unwrap();
        assert!(!plan.plan.allowed, "{:?}", source);
        assert!(
            repo.import_legacy_export(
                &source,
                Some(&context()),
                Mode::Merge,
                None,
                &RequestId::new()
            )
            .is_err()
        );
        assert_eq!(authority(&repo), before);
    }
}
#[test]
fn jsonl_conversion_uses_real_line_selector_and_exact_retained_bytes() {
    let (_root, repo) = fixture();
    let bytes = format!(
        "{}\n\n{}\n",
        json!({"kind":"repo","payload":{"root":"/untrusted/annotation"}}),
        json!({"kind":"issue","payload":row("WD-18","done")})
    );
    let source = legacy_bytes(bytes.as_bytes());
    repo.import_legacy_export(
        &source,
        Some(&context()),
        Mode::Merge,
        None,
        &RequestId::new(),
    )
    .unwrap();
    let issue = repo.show_issue("WD-18").unwrap();
    assert_eq!(
        issue.metadata.custom["legacy"]["record_selector"],
        "line:3/payload"
    );
    assert!(source.source_path().to_str().unwrap().ends_with(".jsonl"));
    let export = repo.export_snapshot().unwrap();
    let roundtrip = workdeck_pm::decode_snapshot(export.to_jsonl().unwrap().as_bytes()).unwrap();
    assert!(
        roundtrip
            .files
            .iter()
            .any(|file| file.path == source.source_path() && file.content == bytes.as_bytes())
    );
}
#[test]
fn retained_artifact_tampering_is_a_doctor_error_and_blocks_export() {
    let (_root, repo) = fixture();
    let source = legacy(json!({"issues":[]}));
    repo.import_legacy_export(
        &source,
        Some(&context()),
        Mode::Merge,
        None,
        &RequestId::new(),
    )
    .unwrap();
    fs::write(repo.root().join(source.source_path()), b"{\"issues\":[]} ").unwrap();
    let report = repo.doctor().unwrap();
    assert!(!report.valid);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("source hash"))
    );
    assert!(repo.export_snapshot().is_err());
}
#[test]
fn interrupted_legacy_import_recovers_exact_bytes_and_same_request() {
    let (_root, repo) = fixture();
    let source = legacy(
        json!({"issues":[row("WD-19","done")],"projects":[{"id":"project","name":"Preserved"}]}),
    );
    let request = RequestId::new();
    let mut tripped = false;
    let result = repo.import_legacy_export_with_faults(
        &source,
        Some(&context()),
        Mode::Merge,
        None,
        &request,
        |point| {
            if !tripped && matches!(point, workdeck_pm::transactions::FaultPoint::AfterChange(0)) {
                tripped = true;
                return Err(workdeck_pm::PmError::new(
                    ErrorCode::Io,
                    "injected interrupted publication",
                ));
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(
        repo.show_issue("WD-19").unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    let recovered = repo.recover_operations().unwrap();
    assert_eq!(recovered.len(), 1);
    let replay = repo
        .import_legacy_export(&source, Some(&context()), Mode::Merge, None, &request)
        .unwrap();
    assert_eq!(replay.operation_id, recovered[0].operation_id);
    assert_eq!(
        fs::read(repo.root().join(source.source_path())).unwrap(),
        source.raw_bytes()
    );
    assert!(repo.doctor().unwrap().valid);
}
#[test]
fn direct_edit_after_legacy_preparation_is_rejected_by_shared_readset() {
    let (_root, repo) = fixture();
    let source = legacy(json!({"issues":[row("WD-20","todo")]}));
    let config = repo.root().join("config.yml");
    let mut tripped = false;
    let result = repo.import_legacy_export_with_faults(
        &source,
        Some(&context()),
        Mode::Merge,
        None,
        &RequestId::new(),
        |point| {
            if !tripped && point == workdeck_pm::transactions::FaultPoint::BeforeJournal {
                tripped = true;
                let mut bytes = fs::read(&config).unwrap();
                bytes.extend_from_slice(b"# concurrent edit\n");
                fs::write(&config, bytes).unwrap();
            }
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
    assert!(repo.show_issue("WD-20").is_err());
    assert!(!repo.root().join(source.source_path()).exists());
}
#[test]
fn native_terminal_import_admission_is_not_granted_by_legacy_provenance_fields() {
    let (_root, repo) = fixture();
    let original = repo.export_snapshot().unwrap();
    let other = tempfile::tempdir().unwrap();
    let root = other.path().join(".workdeck");
    for file in original.files {
        let path = root.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.content).unwrap();
    }
    let fork = Repository::open_source(&root).unwrap();
    let source = legacy(json!({"issues":[row("WD-21","done")]}));
    fork.import_legacy_export(
        &source,
        Some(&context()),
        Mode::Merge,
        None,
        &RequestId::new(),
    )
    .unwrap(); // No invented historical operation grant.
    // Recompute exact manifest through a source containing the same receipt authority as destination.
    for entry in fs::read_dir(root.join("operations")).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    let snapshot = fork.export_snapshot().unwrap();
    let plan = repo
        .preview_snapshot_import(&snapshot, Mode::Merge)
        .unwrap();
    assert!(!plan.allowed);
    assert!(
        plan.blockers
            .iter()
            .any(|error| error.message.contains("new terminal history"))
    );
    assert!(repo.show_issue("WD-21").is_err());
}
#[test]
fn changed_request_payload_is_an_idempotency_conflict() {
    let (_root, repo) = fixture();
    let request = RequestId::new();
    let a = legacy(json!({"issues":[row("WD-22","todo")]}));
    repo.import_legacy_export(&a, None, Mode::Merge, None, &request)
        .unwrap();
    let b = legacy(json!({"issues":[row("WD-23","todo")]}));
    assert_eq!(
        repo.import_legacy_export(&b, None, Mode::Merge, None, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert!(repo.show_issue("WD-23").is_err());
}
#[test]
fn second_plain_import_reuses_proven_context_but_edited_native_record_still_conflicts() {
    let (_root, repo) = fixture();
    let source = legacy(json!({"issues":[row("WD-24","done")]}));
    let first = repo
        .import_legacy_export(&source, None, Mode::Merge, None, &RequestId::new())
        .unwrap();
    let second = repo
        .import_legacy_export(&source, None, Mode::Merge, None, &RequestId::new())
        .unwrap();
    assert!(second.changed.is_empty());
    assert_ne!(first.operation_id, second.operation_id);
    assert_eq!(first.result["imported_at"], second.result["imported_at"]);
    repo.mutate_issue(
        "WD-24",
        None,
        &workdeck_pm::IssueMutation::Reopen,
        &RequestId::new(),
    )
    .unwrap();
    let before = authority(&repo);
    let plan = repo
        .preview_legacy_import(&source, None, Mode::Merge)
        .unwrap();
    assert!(!plan.plan.allowed);
    assert!(
        plan.plan
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::Conflict)
    );
    assert!(
        repo.import_legacy_export(&source, None, Mode::Merge, None, &RequestId::new())
            .is_err()
    );
    assert_eq!(authority(&repo), before);
}
#[test]
fn missing_or_misdirected_json_origin_is_reported_by_doctor() {
    let (_root, repo) = fixture();
    let source = legacy(
        json!({"issues":[row("WD-25","done")],"projects":[{"id":"origin","name":"Origin"}],"agent_sessions":[{"id":"origin","title":"Origin"}]}),
    );
    repo.import_legacy_export(
        &source,
        Some(&context()),
        Mode::Merge,
        None,
        &RequestId::new(),
    )
    .unwrap();
    let artifact = repo.root().join(source.source_path());
    fs::remove_file(&artifact).unwrap();
    let report = repo.doctor().unwrap();
    assert!(!report.valid);
    assert!(
        report
            .errors
            .iter()
            .filter(|error| error.message.contains("retained export"))
            .count()
            >= 3
    );
    fs::write(&artifact, source.raw_bytes()).unwrap();
    let issue_path = repo.root().join("issues/WD-25/item.md");
    let bytes = fs::read_to_string(&issue_path).unwrap();
    fs::write(&issue_path, bytes.replace("/issues/0", "/projects/0")).unwrap();
    assert!(!repo.doctor().unwrap().valid);
    assert!(repo.export_snapshot().is_err());
}
#[test]
fn reference_descriptions_without_native_body_slots_remain_in_origin_metadata() {
    let (_root, repo) = fixture();
    let source = legacy(
        json!({"cycles":[{"id":"cycle","name":"Cycle","description":"Preserve unsupported cycle body"}],"labels":[{"id":"label","name":"Label","description":"Preserve unsupported label body"}]}),
    );
    repo.import_legacy_export(
        &source,
        Some(&context()),
        Mode::Merge,
        None,
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.planning_record(PlanningKind::Cycle, "cycle")
            .unwrap()
            .metadata
            .custom["legacy"]["original"]["description"],
        "Preserve unsupported cycle body"
    );
    assert_eq!(
        repo.planning_record(PlanningKind::Label, "label")
            .unwrap()
            .metadata
            .custom["legacy"]["original"]["description"],
        "Preserve unsupported label body"
    );
}
#[test]
fn existing_case_aliases_are_reported_in_preview_without_record_overwrite() {
    let (_root, repo) = fixture();
    repo.create_planning(
        PlanningKind::Project,
        &CreatePlanning {
            id: Some("PreservedCase".into()),
            name: "Existing".into(),
            body: String::new(),
            fields: BTreeMap::new(),
        },
        &RequestId::new(),
    )
    .unwrap();
    repo.create_recorded_session(
        &workdeck_pm::NewRecordedSession {
            id: Some("SessionCase".into()),
            title: "Existing".into(),
            fields: BTreeMap::new(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let before = authority(&repo);
    let source = legacy(
        json!({"projects":[{"id":"preservedcase","name":"Alias"}],"agent_sessions":[{"id":"sessioncase","title":"Alias"}]}),
    );
    let plan = repo
        .preview_legacy_import(&source, Some(&context()), Mode::Merge)
        .unwrap();
    assert!(!plan.plan.allowed);
    assert!(
        plan.plan
            .blockers
            .iter()
            .filter(|error| error.code == ErrorCode::Conflict)
            .count()
            >= 2
    );
    assert_eq!(authority(&repo), before);
}
#[test]
fn forged_legacy_import_result_cannot_replay_or_supply_future_import_context() {
    let (_root, repo) = fixture();
    let source = legacy(json!({"issues":[row("WD-26","done")]}));
    let request = RequestId::new();
    let receipt = repo
        .import_legacy_export(&source, None, Mode::Merge, None, &request)
        .unwrap();
    let path = repo
        .root()
        .join(format!("operations/{}.yml", receipt.operation_id));
    let mut forged = receipt;
    forged.result["imported_at"] = json!("2025-01-01T00:00:00Z");
    fs::write(path, serde_yaml_ng::to_string(&forged).unwrap()).unwrap();
    assert!(!repo.doctor().unwrap().valid);
    assert!(repo.export_snapshot().is_err());
    assert!(
        repo.import_legacy_export(&source, None, Mode::Merge, None, &request)
            .is_err()
    );
    assert!(
        repo.import_legacy_export(&source, None, Mode::Merge, None, &RequestId::new())
            .is_err()
    );
}
