use serde_json::json;
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;
use workdeck_pm::{
    ContentHash, CreateIssue, CreatePlanning, ErrorCode, PlanningKind, PlanningMetadata,
    PlanningMutation, PlanningRecord, Repository, RequestId, Revision, SavePlanning,
};

fn metadata() -> PlanningMetadata {
    serde_json::from_value(json!({
        "schema":1,"id":"legacy_Project_1","revision":1,"name":"Original",
        "created_at":"2026-01-02T10:00:00Z","updated_at":"2026-01-02T10:00:00Z"
    }))
    .unwrap()
}

#[test]
fn missing_historical_times_require_explicit_supported_import_provenance() {
    let mut record = metadata();
    record.created_at = None;
    record.updated_at = None;
    assert!(record.validate(PlanningKind::Project).is_err());
    record.imported = imported(PlanningKind::Project, "legacy").imported;
    record.imported.as_mut().unwrap().source = "migration-input/projects.toml".into();
    record.validate(PlanningKind::Project).unwrap();
    let mut unsupported = serde_json::to_value(&record).unwrap();
    unsupported["imported"]["format"] = json!("anything");
    assert!(serde_json::from_value::<PlanningMetadata>(unsupported).is_err());
    record.imported.as_mut().unwrap().source = "../projects.toml".into();
    assert!(record.validate(PlanningKind::Project).is_err());
}

#[test]
fn kind_specific_fields_and_unknown_keys_cannot_silently_enter_authority() {
    let mut record = metadata();
    record.color = Some("red".into());
    assert!(record.validate(PlanningKind::Project).is_err());
    record.color = None;
    record.extra.insert("statsu".into(), json!("active"));
    assert!(record.validate(PlanningKind::Project).is_err());
}

#[test]
fn paths_reserved_device_ids_and_reversed_known_ranges_are_rejected() {
    for id in ["../outside", "CON", "a/b", "a\\b", ".", ""] {
        let mut record = metadata();
        record.id = id.into();
        assert!(record.validate(PlanningKind::Project).is_err(), "{id}");
    }
    let mut record = metadata();
    record.starts_at = Some("2026-03-05".into());
    record.ends_at = Some("2026-03-04".into());
    assert!(record.validate(PlanningKind::Cycle).is_err());
}

fn repo() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}

fn create(repository: &Repository, kind: PlanningKind, id: &str) -> PlanningRecord {
    let mut input = CreatePlanning::new("Original");
    input.id = Some(id.into());
    serde_json::from_value(
        repository
            .create_planning(kind, &input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}

fn update(fields: impl IntoIterator<Item = (&'static str, serde_json::Value)>) -> PlanningMutation {
    PlanningMutation::Update {
        fields: fields
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
        body: None,
    }
}

fn imported(kind: PlanningKind, id: &str) -> PlanningMetadata {
    let mut value = serde_json::to_value(metadata()).unwrap();
    let object = value.as_object_mut().unwrap();
    object.insert("id".into(), json!(id));
    object.remove("created_at");
    object.remove("updated_at");
    object.insert("imported".into(), json!({"source":format!(".agents/workdeck/{}.toml", match kind { PlanningKind::Project => "projects", PlanningKind::Cycle => "cycles", PlanningKind::Label => "labels", _ => panic!("only baseline kinds have legacy fixtures") }), "format":"legacy_toml", "content":ContentHash::of(b"legacy") }));
    serde_json::from_value(value).unwrap()
}

#[test]
fn empty_reads_do_not_initialize_reference_files() {
    let (_temp, repository) = repo();
    for kind in [
        PlanningKind::Project,
        PlanningKind::Cycle,
        PlanningKind::Label,
    ] {
        assert!(repository.list_planning(kind).unwrap().is_empty());
        assert_eq!(
            repository
                .planning_record(kind, "missing")
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    for path in ["projects", "cycles", "labels.yml"] {
        assert!(!repository.root().join(path).exists());
    }
}

#[test]
fn save_is_one_replayable_operation_and_omitted_body_preserves_markdown() {
    let (_temp, repository) = repo();
    let input = SavePlanning {
        id: "release-one".into(),
        name: "Release One".into(),
        body: Some("# Scope\nPreserve this.\n".into()),
        fields: BTreeMap::new(),
    };
    let request = RequestId::new();
    let first = repository
        .save_planning(PlanningKind::Project, &input, None, &request)
        .unwrap();
    let original: PlanningRecord = serde_json::from_value(first.result.clone()).unwrap();
    let update = SavePlanning {
        name: "Changed".into(),
        body: None,
        ..input.clone()
    };
    let updated = repository
        .save_planning(
            PlanningKind::Project,
            &update,
            Some(&original.source),
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(updated.result["body"], input.body.as_deref().unwrap());
    assert_eq!(
        repository
            .save_planning(PlanningKind::Project, &input, None, &request)
            .unwrap(),
        first
    );
    assert_eq!(
        repository
            .planning_record(PlanningKind::Project, &input.id)
            .unwrap()
            .metadata
            .name,
        "Changed"
    );
    assert_eq!(
        repository
            .save_planning(
                PlanningKind::Project,
                &update,
                Some(&original.source),
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repository
            .save_planning(PlanningKind::Project, &update, None, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let clear = SavePlanning {
        body: Some(String::new()),
        ..update
    };
    let cleared = repository
        .save_planning(PlanningKind::Project, &clear, None, &RequestId::new())
        .unwrap();
    assert_eq!(cleared.result["body"], "");
}

#[test]
fn save_cannot_recreate_retired_identity_or_create_with_a_stale_token() {
    let (_temp, repository) = repo();
    let input = SavePlanning {
        id: "release".into(),
        name: "Release".into(),
        body: None,
        fields: BTreeMap::new(),
    };
    let first = repository
        .save_planning(PlanningKind::Project, &input, None, &RequestId::new())
        .unwrap();
    let record: PlanningRecord = serde_json::from_value(first.result).unwrap();
    fs::remove_file(repository.root().join(&record.path)).unwrap();
    assert_eq!(
        repository
            .save_planning(
                PlanningKind::Project,
                &input,
                Some(&record.source),
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repository
            .save_planning(PlanningKind::Project, &input, None, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn new_records_have_real_times_stable_kind_ids_and_canonical_paths() {
    let (_temp, repository) = repo();
    for (kind, prefix, directory) in [
        (PlanningKind::Project, "PRJ-", "projects"),
        (PlanningKind::Cycle, "CYC-", "cycles"),
        (PlanningKind::Label, "LBL-", "labels"),
    ] {
        let record: PlanningRecord = serde_json::from_value(
            repository
                .create_planning(kind, &CreatePlanning::new("A record"), &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        assert!(record.metadata.id.starts_with(prefix));
        assert_eq!(record.metadata.revision, Revision::INITIAL);
        assert!(record.metadata.created_at.is_some());
        assert_eq!(record.metadata.created_at, record.metadata.updated_at);
        assert!(record.metadata.imported.is_none());
        let expected = if kind == PlanningKind::Label {
            "labels.yml".to_owned()
        } else {
            format!("{directory}/{}/item.md", record.metadata.id)
        };
        assert_eq!(record.path.to_str().unwrap(), expected);
        assert_eq!(
            record,
            repository
                .planning_record(kind, &record.metadata.id)
                .unwrap()
        );
    }
}

#[test]
fn imported_unknown_times_and_metadata_survive_later_native_updates() {
    let (_temp, repository) = repo();
    let mut metadata = imported(PlanningKind::Project, "Legacy_Project");
    metadata.status = Some("We are experimenting / phase A".into());
    metadata.custom.insert(
        "legacy".into(),
        json!({"extra":{"type":"datetime","value":"2025-01-01"}}),
    );
    metadata
        .extra
        .insert("x-team".into(), json!({"name":"Factory"}));
    let directory = repository.root().join("projects/Legacy_Project");
    fs::create_dir_all(&directory).unwrap();
    let yaml = serde_yaml_ng::to_string(&metadata)
        .unwrap()
        .replace("name: Original", "name: 'Original' # retain quote");
    let body = "# Scope\n\nOriginal **description**.\n";
    fs::write(
        directory.join("item.md"),
        format!("---\n# record comment\n{yaml}---\n{body}"),
    )
    .unwrap();
    let before = repository
        .planning_record(PlanningKind::Project, "Legacy_Project")
        .unwrap();
    assert!(before.metadata.created_at.is_none());
    let receipt = repository
        .mutate_planning(
            PlanningKind::Project,
            "Legacy_Project",
            Some(&before.source),
            &update([("status", json!("Holding for a human"))]),
            &RequestId::new(),
        )
        .unwrap();
    let after: PlanningRecord = serde_json::from_value(receipt.result).unwrap();
    assert!(after.metadata.created_at.is_none());
    assert!(after.metadata.updated_at.is_some());
    assert_eq!(after.metadata.imported, before.metadata.imported);
    assert_eq!(after.metadata.custom, before.metadata.custom);
    assert_eq!(after.metadata.extra, before.metadata.extra);
    assert_eq!(after.body, body);
    let text = fs::read_to_string(directory.join("item.md")).unwrap();
    assert!(text.contains("name: 'Original' # retain quote"));
    assert!(text.contains("# record comment"));
}

#[test]
fn source_hash_detects_direct_edits_without_revision_changes() {
    let (_temp, repository) = repo();
    let before = create(&repository, PlanningKind::Project, "project_a");
    let path = repository.root().join(&before.path);
    let direct = fs::read_to_string(&path)
        .unwrap()
        .replace("name: Original", "name: Edited directly");
    fs::write(&path, &direct).unwrap();
    assert_eq!(
        repository
            .mutate_planning(
                PlanningKind::Project,
                "project_a",
                Some(&before.source),
                &update([("status", json!("active"))]),
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(fs::read_to_string(path).unwrap(), direct);
}

#[test]
fn semantic_retries_replay_before_lookup_and_reject_changed_requests() {
    let (_temp, repository) = repo();
    let request = RequestId::new();
    let mut input = CreatePlanning::new("Example");
    input.id = Some("project_a".into());
    let created = repository
        .create_planning(PlanningKind::Project, &input, &request)
        .unwrap();
    let record: PlanningRecord = serde_json::from_value(created.result.clone()).unwrap();
    let mutation = update([("status", json!("In any free-form status"))]);
    let mutation_id = RequestId::new();
    let changed = repository
        .mutate_planning(
            PlanningKind::Project,
            "project_a",
            Some(&record.source),
            &mutation,
            &mutation_id,
        )
        .unwrap();
    fs::remove_file(repository.root().join(&record.path)).unwrap();
    assert_eq!(
        repository
            .create_planning(PlanningKind::Project, &input, &request)
            .unwrap(),
        created
    );
    assert_eq!(
        repository
            .mutate_planning(
                PlanningKind::Project,
                "project_a",
                Some(&record.source),
                &mutation,
                &mutation_id
            )
            .unwrap(),
        changed
    );
    input.name = "Different request".into();
    assert_eq!(
        repository
            .create_planning(PlanningKind::Project, &input, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(
        repository
            .create_planning(PlanningKind::Project, &input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn archive_retains_associations_identity_and_can_restore_without_reuse() {
    let (_temp, repository) = repo();
    let project = create(&repository, PlanningKind::Project, "Legacy_Project");
    let mut issue = CreateIssue::new("Associated", "");
    issue
        .fields
        .insert("project".into(), json!(project.metadata.id));
    let issue: workdeck_pm::IssueRecord = serde_json::from_value(
        repository
            .create_issue(&issue, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let bytes = fs::read(repository.root().join(&issue.path)).unwrap();
    let mutation = PlanningMutation::Archive { archived: true };
    let archived: PlanningRecord = serde_json::from_value(
        repository
            .mutate_planning(
                PlanningKind::Project,
                &project.metadata.id,
                Some(&project.source),
                &mutation,
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    assert!(archived.metadata.archived);
    assert_eq!(archived.metadata.created_at, project.metadata.created_at);
    assert_eq!(
        fs::read(repository.root().join(&issue.path)).unwrap(),
        bytes
    );
    assert_eq!(
        repository
            .list_planning(PlanningKind::Project)
            .unwrap()
            .len(),
        1
    );
    let mut duplicate = CreatePlanning::new("Reuse attempt");
    duplicate.id = Some("legacy_project".into());
    assert_eq!(
        repository
            .create_planning(PlanningKind::Project, &duplicate, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let restored: PlanningRecord = serde_json::from_value(
        repository
            .mutate_planning(
                PlanningKind::Project,
                &project.metadata.id,
                None,
                &PlanningMutation::Archive { archived: false },
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    assert!(!restored.metadata.archived);
    assert_eq!(restored.metadata.id, project.metadata.id);
}

#[test]
fn labels_keep_unrelated_values_quotes_comments_and_outer_extensions() {
    let (_temp, repository) = repo();
    let source = format!(
        "# catalog comment\nschema: 1\nx-owner: 'Human' # owner comment\ncustom:\n  note: retained\nlabels:\n- schema: 1\n  id: first\n  revision: 1\n  name: 'First' # keep this comment\n  color: red\n  imported: {{source: .agents/workdeck/labels.toml, format: legacy_toml, content: {}}}\n- schema: 1\n  id: Second_Label\n  revision: 1\n  name: 'Second' # never rewrite this\n  color: 'free-form color'\n  x-team: [one, two]\n  imported: {{source: .agents/workdeck/labels.toml, format: legacy_toml, content: {}}}\n",
        ContentHash::of(b"legacy"),
        ContentHash::of(b"legacy")
    );
    fs::write(repository.root().join("labels.yml"), &source).unwrap();
    let second = repository
        .planning_record(PlanningKind::Label, "Second_Label")
        .unwrap();
    let first = repository
        .planning_record(PlanningKind::Label, "first")
        .unwrap();
    repository
        .mutate_planning(
            PlanningKind::Label,
            "first",
            Some(&first.source),
            &update([("color", json!("a different arbitrary color"))]),
            &RequestId::new(),
        )
        .unwrap();
    let changed = fs::read_to_string(repository.root().join("labels.yml")).unwrap();
    let original_second = source
        .split("- schema: 1\n  id: Second_Label")
        .nth(1)
        .unwrap();
    assert!(changed.ends_with(original_second));
    assert!(changed.contains("# catalog comment"));
    assert!(changed.contains("x-owner: 'Human' # owner comment"));
    assert!(changed.contains("name: 'First' # keep this comment"));
    let fresh_second = repository
        .planning_record(PlanningKind::Label, "Second_Label")
        .unwrap();
    assert_eq!(fresh_second.metadata, second.metadata);
    assert_ne!(fresh_second.source.content, second.source.content);
    assert_eq!(
        repository
            .mutate_planning(
                PlanningKind::Label,
                "Second_Label",
                Some(&second.source),
                &update([("name", json!("New"))]),
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn unchanged_intents_preserve_revision_times_and_source_bytes() {
    let (_temp, repository) = repo();
    for kind in [
        PlanningKind::Project,
        PlanningKind::Cycle,
        PlanningKind::Label,
    ] {
        let before = create(&repository, kind, "existing");
        let unchanged = repository
            .mutate_planning(
                kind,
                "existing",
                None,
                &update([("name", json!("Original"))]),
                &RequestId::new(),
            )
            .unwrap();
        assert!(unchanged.changed.is_empty());
        assert_eq!(
            serde_json::from_value::<PlanningRecord>(unchanged.result).unwrap(),
            before
        );
    }
}

#[test]
fn managed_fields_and_kind_mismatches_fail_without_rewriting() {
    let (_temp, repository) = repo();
    let project = create(&repository, PlanningKind::Project, "existing");
    let before = fs::read(repository.root().join(&project.path)).unwrap();
    for (field, value) in [
        ("id", json!("other")),
        ("revision", json!(80)),
        ("created_at", json!("2020-01-01T00:00:00Z")),
        ("imported", json!({})),
        ("archived", json!(true)),
        ("color", json!("red")),
        ("name", json!(" ")),
    ] {
        assert!(
            repository
                .mutate_planning(
                    PlanningKind::Project,
                    "existing",
                    None,
                    &update([(field, value)]),
                    &RequestId::new()
                )
                .is_err(),
            "{field}"
        );
        assert_eq!(
            fs::read(repository.root().join(&project.path)).unwrap(),
            before
        );
    }
    let label = create(&repository, PlanningKind::Label, "existing");
    let before = fs::read(repository.root().join(&label.path)).unwrap();
    assert!(
        repository
            .mutate_planning(
                PlanningKind::Label,
                "existing",
                None,
                &PlanningMutation::Update {
                    fields: BTreeMap::new(),
                    body: Some("body".into())
                },
                &RequestId::new()
            )
            .is_err()
    );
    assert_eq!(
        fs::read(repository.root().join(&label.path)).unwrap(),
        before
    );
}

#[test]
fn cycle_dates_and_status_preserve_legacy_text_without_invented_history() {
    let mut cycle = imported(PlanningKind::Cycle, "cycle_a");
    cycle.starts_at = Some("after launch".into());
    cycle.ends_at = Some("2026-09-30".into());
    cycle.status = Some("waiting for beta feedback".into());
    cycle.validate(PlanningKind::Cycle).unwrap();
    assert!(cycle.created_at.is_none());
    cycle.starts_at = Some("2026-10-01T01:00:00+02:00".into());
    cycle.ends_at = Some("2026-09-30T22:00:00Z".into());
    assert!(cycle.validate(PlanningKind::Cycle).is_err());
}

#[test]
fn duplicate_label_ids_and_unsupported_record_versions_fail_closed() {
    let (_temp, repository) = repo();
    let first = imported(PlanningKind::Label, "Label_A");
    let mut second = first.clone();
    second.id = "label_a".into();
    fs::write(
        repository.root().join("labels.yml"),
        serde_yaml_ng::to_string(&json!({"schema":1,"labels":[first,second]})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        repository
            .list_planning(PlanningKind::Label)
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
    fs::write(
        repository.root().join("labels.yml"),
        "schema: 99\nlabels: []\n",
    )
    .unwrap();
    let error = repository.list_planning(PlanningKind::Label).unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedSchema);
    assert_eq!(
        error.path.as_deref(),
        repository.root().join("labels.yml").to_str()
    );
}

#[test]
fn doctor_counts_native_planning_records_and_reports_each_bad_source() {
    let (_temp, repository) = repo();
    let project = create(&repository, PlanningKind::Project, "project_a");
    let cycle = create(&repository, PlanningKind::Cycle, "cycle_a");
    create(&repository, PlanningKind::Label, "label_a");
    create(&repository, PlanningKind::Label, "label_b");
    let valid = repository.doctor().unwrap();
    assert!(valid.valid, "{:?}", valid.errors);
    assert_eq!(valid.checked_records, 4);
    fs::write(
        repository.root().join(&project.path),
        "---\nschema: 99\n---\n",
    )
    .unwrap();
    fs::write(
        repository.root().join(&cycle.path),
        "---\nname: [unterminated\n---\n",
    )
    .unwrap();
    fs::write(
        repository.root().join("labels.yml"),
        "schema: 1\nlabels: typo\n",
    )
    .unwrap();
    let report = repository.doctor().unwrap();
    assert!(!report.valid);
    for path in [
        project.path.as_path(),
        cycle.path.as_path(),
        std::path::Path::new("labels.yml"),
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.path.as_deref() == repository.root().join(path).to_str()),
            "missing {path:?}: {:?}",
            report.errors
        );
    }
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == ErrorCode::UnsupportedSchema)
    );
}

#[test]
fn doctor_keeps_inspecting_projects_after_one_malformed_record() {
    let (_temp, repository) = repo();
    let first = create(&repository, PlanningKind::Project, "first");
    let second = create(&repository, PlanningKind::Project, "second");
    fs::write(
        repository.root().join(&first.path),
        "---\nid: [unterminated\n---\n",
    )
    .unwrap();
    let wrong_identity = fs::read_to_string(repository.root().join(&second.path))
        .unwrap()
        .replace("id: second", "id: another");
    fs::write(repository.root().join(&second.path), wrong_identity).unwrap();
    fs::write(
        repository.root().join("projects/unexpected.md"),
        "# invalid layout\n",
    )
    .unwrap();
    let report = repository.doctor().unwrap();
    assert!(!report.valid);
    assert_eq!(report.errors.len(), 3, "{:?}", report.errors);
    assert_eq!(report.checked_records, 3);
}

#[test]
fn bulk_planning_reads_preserve_namespace_and_document_validation() {
    let (_temp, repository) = repo();
    let record = create(&repository, PlanningKind::Project, "exact");
    let path = repository.root().join(&record.path);
    let original = fs::read_to_string(&path).unwrap();
    for replacement in [
        original.replace("id: exact", "id: other"),
        original.replace("schema: 1", "schema: 999"),
        "---\nid: [unterminated\n---\n".into(),
    ] {
        fs::write(&path, replacement).unwrap();
        assert!(repository.list_planning(PlanningKind::Project).is_err());
    }
    fs::write(&path, &original).unwrap();
    let unexpected = repository.root().join("projects/unexpected.md");
    fs::write(&unexpected, "unexpected layout").unwrap();
    assert!(repository.list_planning(PlanningKind::Project).is_err());
    fs::remove_file(unexpected).unwrap();
    assert_eq!(
        repository.list_planning(PlanningKind::Project).unwrap(),
        vec![record]
    );
    assert!(
        repository
            .planning_record(PlanningKind::Project, "EXACT")
            .is_err()
    );
}
