use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
use tempfile::TempDir;
use workdeck_pm::{ContentHash, ErrorCode, IssueRecord, Repository, RequestId};

fn setup() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}

fn write_template(repository: &Repository, id: &str, defaults: Value, body: &str) -> PathBuf {
    let path = repository.root().join(format!("templates/issues/{id}.md"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let header = serde_yaml_ng::to_string(
        &json!({"schema":1,"id":id,"name":format!("{id} template"),"defaults":defaults}),
    )
    .unwrap();
    fs::write(&path, format!("---\n{header}---\n{body}")).unwrap();
    path
}

#[test]
fn absent_template_folder_lists_empty_without_creating_authoritative_files() {
    let (_temp, repository) = setup();
    let config = fs::read(repository.root().join("config.yml")).unwrap();
    assert!(repository.list_issue_templates().unwrap().is_empty());
    assert!(!repository.root().join("templates").exists());
    assert_eq!(
        fs::read(repository.root().join("config.yml")).unwrap(),
        config
    );
    assert_eq!(
        repository.issue_template("missing").unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn list_and_lookup_preserve_exact_body_defaults_source_and_order() {
    let (_temp, repository) = setup();
    write_template(
        &repository,
        "z-bug",
        json!({"priority":"urgent"}),
        "Bug reproduction\n",
    );
    let path = write_template(
        &repository,
        "a-feature",
        json!({
            "priority":"high", "labels":["feature"],
            "custom":{"risk":{"score":3,"reviewers":["Équipe"]}},
            "x-import":{"reason":"legacy template"}
        }),
        "# Résumé\n\nKeep this body exactly.\n",
    );
    let before = fs::read(&path).unwrap();
    let templates = repository.list_issue_templates().unwrap();
    assert_eq!(
        templates
            .iter()
            .map(|template| template.id.as_str())
            .collect::<Vec<_>>(),
        ["a-feature", "z-bug"]
    );
    let loaded = repository.issue_template("a-feature").unwrap();
    assert_eq!(loaded, templates[0]);
    assert_eq!(loaded.body, "# Résumé\n\nKeep this body exactly.\n");
    assert_eq!(loaded.defaults["custom"]["risk"]["score"], 3);
    assert_eq!(loaded.path, PathBuf::from("templates/issues/a-feature.md"));
    assert_eq!(loaded.content, ContentHash::of(&before));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn apply_merges_overrides_deterministically_and_distinguishes_empty_body() {
    let (_temp, repository) = setup();
    write_template(
        &repository,
        "bug",
        json!({"priority":"high","labels":["bug"],"custom":{"owner":"initial"}}),
        "Template body\n",
    );
    let template = repository.issue_template("bug").unwrap();
    let overrides = BTreeMap::from([
        ("priority".into(), json!("urgent")),
        ("custom".into(), json!({"owner":"new"})),
    ]);
    let input = template.apply("User title".into(), None, overrides);
    assert_eq!(input.title, "User title");
    assert_eq!(input.body, "Template body\n");
    assert_eq!(input.fields["priority"], "urgent");
    assert_eq!(input.fields["labels"], json!(["bug"]));
    assert_eq!(input.fields["custom"], json!({"owner":"new"}));
    assert_eq!(template.defaults["priority"], "high");
    assert_eq!(
        template
            .apply("Empty".into(), Some(String::new()), BTreeMap::new())
            .body,
        ""
    );
    assert_eq!(
        template
            .apply(
                "Own body".into(),
                Some("Caller body".into()),
                BTreeMap::new()
            )
            .body,
        "Caller body"
    );
}

#[test]
fn loaded_defaults_produce_a_valid_issue_through_shared_creation() {
    let (_temp, repository) = setup();
    for (kind, id) in [
        (workdeck_pm::PlanningKind::Project, "project-a"),
        (workdeck_pm::PlanningKind::Cycle, "cycle-a"),
        (workdeck_pm::PlanningKind::Label, "bug"),
        (workdeck_pm::PlanningKind::Milestone, "milestone-a"),
    ] {
        let mut input = workdeck_pm::CreatePlanning::new(id);
        input.id = Some(id.into());
        if kind == workdeck_pm::PlanningKind::Milestone {
            input.fields.insert("project".into(), json!("project-a"));
        }
        repository
            .create_planning(kind, &input, &RequestId::new())
            .unwrap();
    }
    write_template(
        &repository,
        "bug",
        json!({
            "status":"todo","priority":"high","assignee":"agent-1",
            "reporter":"reviewer-1","reviewer":"reviewer-2","due_at":"2026-09-10",
            "project":"project-a","cycle":"cycle-a","milestone":"milestone-a",
            "labels":["bug"],"files":[{"path":"src/auth.rs","line":1}],
            "commits":["HEAD~1"],"documents":["docs/auth.md"],
            "acceptance":[{"id":"deep-link","description":"Preserve deep links","checked":false}]
        }),
        "Reproduction details\n",
    );
    let input = repository.issue_template("bug").unwrap().apply(
        "A redirect bug".into(),
        None,
        BTreeMap::new(),
    );
    let receipt = repository.create_issue(&input, &RequestId::new()).unwrap();
    let issue: IssueRecord = serde_json::from_value(receipt.result).unwrap();
    assert_eq!(issue.metadata.status, "ready");
    assert_eq!(issue.metadata.assignee.as_deref(), Some("agent-1"));
    assert_eq!(issue.body, "Reproduction details\n");
    assert!(
        !repository
            .completion_report(issue.metadata.id.as_str())
            .unwrap()
            .allowed
    );
}

#[test]
fn reserved_defaults_cannot_supply_identity_internal_time_or_completion_authority() {
    let (_temp, repository) = setup();
    for (key, value) in [
        ("id", json!("WD-1")),
        ("title", json!("Template-controlled title")),
        ("schema", json!(1)),
        ("revision", json!(2)),
        ("created_at", json!("2026-09-08T10:00:00Z")),
        ("updated_at", json!("2026-09-08T10:00:00Z")),
        ("completed_at", Value::Null),
        ("canceled_at", Value::Null),
        ("manual_acceptance", Value::Null),
        ("archived", json!(true)),
    ] {
        write_template(&repository, "protected", json!({key:value}), "");
        let error = repository.issue_template("protected").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidSchema, "{key}");
        assert!(error.message.contains(key), "{error}");
    }
}

#[test]
fn invalid_default_values_fail_typed_issue_validation() {
    let (_temp, repository) = setup();
    for defaults in [
        json!({"priority":"super-high"}),
        json!({"status":"nonexistent"}),
        json!({"files":[{"path":"../outside.rs"}]}),
        json!({"labels":["duplicate","duplicate"]}),
        json!({"acceptance":[{"id":"criterion","description":"","checked":false}]}),
        json!({"assignee":""}),
        json!({"due_at":"tomorrow"}),
        json!({"features":["FEATURE-1"]}),
        json!({"x-Invalid":true}),
    ] {
        write_template(&repository, "invalid", defaults, "");
        assert!(repository.issue_template("invalid").is_err());
    }
}

#[test]
fn completed_or_canceled_status_defaults_are_not_accepted() {
    let (_temp, repository) = setup();
    for status in ["done", "canceled", "cancelled"] {
        write_template(&repository, "terminal", json!({"status":status}), "");
        assert_eq!(
            repository.issue_template("terminal").unwrap_err().code,
            ErrorCode::PolicyBlocked
        );
    }
}

#[test]
fn null_default_fields_match_creation_removal_semantics() {
    let (_temp, repository) = setup();
    write_template(
        &repository,
        "empty",
        json!({"priority":null,"labels":null,"assignee":null,"custom":null}),
        "",
    );
    let input = repository.issue_template("empty").unwrap().apply(
        "Clear fields".into(),
        None,
        BTreeMap::new(),
    );
    let record: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    assert_eq!(record.metadata.priority, workdeck_pm::Priority::Medium);
    assert!(record.metadata.labels.is_empty());
    assert!(record.metadata.custom.is_empty());
}

#[test]
fn unsafe_template_ids_are_rejected_before_path_resolution() {
    let (_temp, repository) = setup();
    for id in [
        "", "../bug", "/bug", "bug.md", "Bug", "a/b", "a\\b", "con", "lpt1", "a\nname",
    ] {
        let error = repository.issue_template(id).unwrap_err();
        assert!(
            matches!(error.code, ErrorCode::InvalidInput | ErrorCode::UnsafePath),
            "{id}: {error}"
        );
    }
    assert!(!repository.root().join("templates").exists());
}

#[test]
fn filename_identity_unknown_metadata_and_bad_names_are_explicit_errors() {
    let (_temp, repository) = setup();
    let path = write_template(&repository, "bug", json!({}), "");
    for header in [
        "schema: 1\nid: different\nname: Bug\ndefaults: {}\n",
        "schema: 1\nid: bug\nname: ' '\ndefaults: {}\n",
        "schema: 1\nid: bug\nname: \"bad\\nname\"\ndefaults: {}\n",
        "schema: 1\nid: bug\nname: Bug\ndefaults: {}\nreporter: typo\n",
    ] {
        fs::write(&path, format!("---\n{header}---\nBody\n")).unwrap();
        let error = repository.issue_template("bug").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidSchema);
        assert!(
            error
                .path
                .as_deref()
                .unwrap()
                .ends_with("templates/issues/bug.md")
        );
    }
}

#[test]
fn malformed_and_future_schema_templates_are_preserved_with_diagnostics() {
    let (_temp, repository) = setup();
    let path = write_template(&repository, "bug", json!({}), "");
    let malformed = "---\nschema: 1\ndefaults: [\n---\nbody\n";
    fs::write(&path, malformed).unwrap();
    let error = repository.issue_template("bug").unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidSchema);
    assert!(error.line.is_some());
    assert_eq!(fs::read_to_string(&path).unwrap(), malformed);
    let future = "---\nschema: 2\nid: bug\nname: Future\ndefaults: {}\n---\nbody\n";
    fs::write(&path, future).unwrap();
    let error = repository.issue_template("bug").unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedSchema);
    assert!(error.path.as_deref().unwrap().ends_with("bug.md"));
    assert_eq!(fs::read_to_string(&path).unwrap(), future);
}

#[test]
fn nested_template_definitions_are_diagnosed_instead_of_hidden() {
    let (_temp, repository) = setup();
    let path = repository.root().join("templates/issues/nested/bug.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        "---\nschema: 1\nid: bug\nname: Bug\ndefaults: {}\n---\nbody\n",
    )
    .unwrap();
    assert_eq!(
        repository.list_issue_templates().unwrap_err().code,
        ErrorCode::InvalidSchema
    );
}

#[cfg(unix)]
#[test]
fn symlinked_template_files_do_not_expose_outside_content() {
    let (_temp, repository) = setup();
    let outside = TempDir::new().unwrap();
    let path = outside.path().join("private.md");
    fs::write(&path, "Private local fixture\n").unwrap();
    fs::create_dir_all(repository.root().join("templates/issues")).unwrap();
    std::os::unix::fs::symlink(path, repository.root().join("templates/issues/bug.md")).unwrap();
    assert_eq!(
        repository.issue_template("bug").unwrap_err().code,
        ErrorCode::UnsafePath
    );
}
