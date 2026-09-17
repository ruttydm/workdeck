use serde_json::json;
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;
use workdeck_pm::{CreateIssue, ErrorCode, IssueRecord, Repository, RequestId, UpdateIssue};

fn request(value: &str) -> RequestId {
    value.parse().unwrap()
}
fn setup() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}
fn label(repository: &Repository, id: &str) {
    let mut input = workdeck_pm::CreatePlanning::new(id);
    input.id = Some(id.into());
    repository
        .create_planning(workdeck_pm::PlanningKind::Label, &input, &RequestId::new())
        .unwrap();
}
fn create(repository: &Repository) -> IssueRecord {
    serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Fix parser", "Keep metadata intact.\n"),
                &request("create-1"),
            )
            .unwrap()
            .result,
    )
    .unwrap()
}

#[test]
fn create_show_update_and_replay_return_real_stable_records() {
    let (_temp, repository) = setup();
    let created = create(&repository);
    assert_eq!(
        created.path.to_string_lossy(),
        format!("issues/{}/item.md", created.metadata.id)
    );
    assert_eq!(repository.list_issues().unwrap().len(), 1);
    assert_eq!(
        repository.show_issue(created.metadata.id.as_str()).unwrap(),
        created
    );
    assert_eq!(create(&repository), created);
    let changed = repository
        .update_issue(
            created.metadata.id.as_str(),
            &created.source,
            &UpdateIssue {
                fields: BTreeMap::from([("title".into(), json!("Fixed parser"))]),
                body: Some("New body\n".into()),
            },
            &request("edit-1"),
        )
        .unwrap();
    let changed: IssueRecord = serde_json::from_value(changed.result).unwrap();
    assert_eq!(changed.metadata.title, "Fixed parser");
    assert_eq!(changed.source.revision.get(), 2);
    assert_eq!(changed.body, "New body\n");
    assert_eq!(
        create(&repository),
        created,
        "retry must return the original outcome even after subsequent edits"
    );
    assert_eq!(
        repository.show_issue(created.metadata.id.as_str()).unwrap(),
        changed
    );
    let error = repository
        .create_issue(
            &CreateIssue::new("Different title", ""),
            &request("create-1"),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::IdempotencyConflict);
}

#[test]
fn invalid_or_stale_updates_never_rewrite_authoritative_source() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let path = repository.root().join(&issue.path);
    let original = fs::read_to_string(&path).unwrap();
    for fields in [
        BTreeMap::from([("title".into(), json!(""))]),
        BTreeMap::from([("id".into(), json!("WD-6"))]),
        BTreeMap::from([("status".into(), json!("nonexistent"))]),
    ] {
        assert!(
            repository
                .update_issue(
                    issue.metadata.id.as_str(),
                    &issue.source,
                    &UpdateIssue { fields, body: None },
                    &RequestId::new()
                )
                .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
    let edited = original.replace("Fix parser", "Direct editor change");
    fs::write(&path, &edited).unwrap();
    let result = repository.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue::default(),
        &request("stale"),
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
}

#[test]
fn completion_rules_apply_to_status_updates_and_explicit_done() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let changed = repository.update_issue(issue.metadata.id.as_str(), &issue.source,
        &UpdateIssue { fields: BTreeMap::from([("acceptance".into(), json!([{"id":"round-trip","description":"Custom metadata survives","checked":false}]))]), body: None }, &request("criteria")).unwrap();
    let issue: IssueRecord = serde_json::from_value(changed.result).unwrap();
    let report = repository
        .completion_report(issue.metadata.id.as_str())
        .unwrap();
    assert!(!report.allowed);
    assert!(
        report
            .reasons
            .iter()
            .any(|reason| reason.contains("round-trip"))
    );
    let before = fs::read(repository.root().join(&issue.path)).unwrap();
    let error = repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: BTreeMap::from([("status".into(), json!("done"))]),
                body: None,
            },
            &request("bypass"),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyBlocked);
    assert_eq!(
        fs::read(repository.root().join(&issue.path)).unwrap(),
        before
    );
    let changed = repository.update_issue(issue.metadata.id.as_str(), &issue.source,
        &UpdateIssue { fields: BTreeMap::from([("acceptance".into(), json!([{"id":"round-trip","description":"Custom metadata survives","checked":true}]))]), body: None }, &request("checked")).unwrap();
    let issue: IssueRecord = serde_json::from_value(changed.result).unwrap();
    let done = repository
        .complete_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            None,
            &request("done"),
        )
        .unwrap();
    let done: IssueRecord = serde_json::from_value(done.result).unwrap();
    assert_eq!(done.metadata.status, "done");
    assert!(done.metadata.completed_at.is_some());
    let reopened = repository
        .reopen_issue(done.metadata.id.as_str(), &done.source, &request("reopen"))
        .unwrap();
    let reopened: IssueRecord = serde_json::from_value(reopened.result).unwrap();
    assert_eq!(reopened.metadata.status, "ready");
    assert!(reopened.metadata.completed_at.is_none());
}

#[test]
fn comments_are_separate_records_and_retried_comments_do_not_duplicate() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let item_before = fs::read(repository.root().join(&issue.path)).unwrap();
    let first = repository
        .add_comment(
            issue.metadata.id.as_str(),
            &issue.source,
            "agent-a",
            "Verified round trips.\n",
            &request("comment"),
        )
        .unwrap();
    let second = repository
        .add_comment(
            issue.metadata.id.as_str(),
            &issue.source,
            "agent-a",
            "Verified round trips.\n",
            &request("comment"),
        )
        .unwrap();
    assert_eq!(first, second);
    let comments = repository.comments(issue.metadata.id.as_str()).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "Verified round trips.\n");
    assert_eq!(comments[0].author, "agent-a");
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
    assert_eq!(
        fs::read(repository.root().join(&issue.path)).unwrap(),
        item_before
    );
    assert!(repository.doctor().unwrap().valid);
}

#[test]
fn removing_requirements_and_closing_in_one_update_is_not_acceptance() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let receipt = repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: BTreeMap::from([(
                    "acceptance".into(),
                    json!([{"id":"required","description":"Original requirement","checked":false}]),
                )]),
                body: None,
            },
            &request("required"),
        )
        .unwrap();
    let issue: IssueRecord = serde_json::from_value(receipt.result).unwrap();
    let result = repository.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: BTreeMap::from([
                ("acceptance".into(), json!([])),
                ("status".into(), json!("done")),
            ]),
            body: None,
        },
        &request("erase-and-close"),
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::PolicyBlocked);
}

#[test]
fn completed_issue_cannot_retain_manual_acceptance_after_body_changes() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let manual = workdeck_pm::ManualAcceptanceInput {
        actor: "reviewer".into(),
        reason: "I checked the described behavior".into(),
    };
    let receipt = repository
        .complete_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            Some(&manual),
            &request("manual"),
        )
        .unwrap();
    let issue: IssueRecord = serde_json::from_value(receipt.result).unwrap();
    let result = repository.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: BTreeMap::new(),
            body: Some("Entirely different behavior".into()),
        },
        &request("change-accepted-body"),
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::PolicyBlocked);
}

#[test]
fn caller_cannot_edit_timestamps_or_claim_completion_evidence() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    for key in [
        "revision",
        "created_at",
        "updated_at",
        "completed_at",
        "canceled_at",
        "manual_acceptance",
        "source",
    ] {
        let error = repository
            .update_issue(
                issue.metadata.id.as_str(),
                &issue.source,
                &UpdateIssue {
                    fields: BTreeMap::from([(key.into(), json!("forged"))]),
                    body: None,
                },
                &RequestId::new(),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{key}");
    }
}

#[test]
fn completion_retry_returns_original_receipt_after_workflow_changes() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let first = repository
        .complete_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            None,
            &request("done-stable"),
        )
        .unwrap();
    let path = repository.root().join("config.yml");
    let changed = fs::read_to_string(&path)
        .unwrap()
        .replace("done", "finished");
    fs::write(path, changed).unwrap();
    let second = repository
        .complete_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            None,
            &request("done-stable"),
        )
        .unwrap();
    assert_eq!(first, second);
}

#[test]
fn optional_preconditions_preserve_semantic_retries_and_collection_updates_are_atomic() {
    use workdeck_pm::{IssueCollection, IssueMutation};
    let (_temp, repository) = setup();
    label(&repository, "parser");
    let issue = create(&repository);
    let intent = IssueMutation::Add {
        field: IssueCollection::Labels,
        value: json!("parser"),
    };
    let first = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &intent,
            &request("label-intent"),
        )
        .unwrap();
    let edit = IssueMutation::UpdateAndAdd {
        input: UpdateIssue {
            fields: BTreeMap::from([("title".into(), json!("Parser fixed"))]),
            body: None,
        },
        field: IssueCollection::Commits,
        values: vec![json!("abcdef1234567890"), json!("abcdef1234567890")],
    };
    let second = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &edit,
            &request("atomic-edit"),
        )
        .unwrap();
    let edited: IssueRecord = serde_json::from_value(second.result).unwrap();
    assert_eq!(edited.metadata.title, "Parser fixed");
    assert_eq!(edited.metadata.commits, vec!["abcdef1234567890"]);
    assert_eq!(
        repository
            .mutate_issue(
                issue.metadata.id.as_str(),
                None,
                &intent,
                &request("label-intent")
            )
            .unwrap(),
        first
    );
    assert_eq!(
        repository.show_issue(issue.metadata.id.as_str()).unwrap(),
        edited
    );
}

#[test]
fn simultaneous_comments_share_no_item_revision_hotspot() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let handles = (0..3)
        .map(|index| {
            let repository = repository.clone();
            let issue = issue.clone();
            std::thread::spawn(move || {
                repository
                    .add_comment(
                        issue.metadata.id.as_str(),
                        &issue.source,
                        "agent",
                        &format!("Independent comment {index}"),
                        &RequestId::new(),
                    )
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(
        repository
            .comments(issue.metadata.id.as_str())
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
}

#[test]
fn repeating_done_does_not_ignore_newly_required_checks() {
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let receipt = repository
        .complete_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            None,
            &request("old-done"),
        )
        .unwrap();
    let done: IssueRecord = serde_json::from_value(receipt.result).unwrap();
    let mut config = repository.config().unwrap();
    config.acceptance.required_checks.push("unit".into());
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    assert!(
        !repository
            .completion_report(done.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    let result = repository.complete_issue(
        done.metadata.id.as_str(),
        &done.source,
        None,
        &request("new-done"),
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::PolicyBlocked);
}

#[test]
fn unlinking_a_file_by_path_removes_line_qualified_links_and_rejects_invalid_intents() {
    use workdeck_pm::{IssueCollection, IssueMutation};
    let (_temp, repository) = setup();
    let issue = create(&repository);
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Add {
                field: IssueCollection::Files,
                value: json!({"path":"src/lib.rs","line":12,"end_line":18}),
            },
            &request("range-link"),
        )
        .unwrap();
    let result = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Remove {
                field: IssueCollection::Files,
                value: json!({"path":"src/lib.rs"}),
            },
            &request("unlink-path"),
        )
        .unwrap();
    let updated: IssueRecord = serde_json::from_value(result.result).unwrap();
    assert!(updated.metadata.files.is_empty());
    let error = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Remove {
                field: IssueCollection::Files,
                value: json!(42),
            },
            &request("invalid-remove"),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn editor_drafts_preserve_authored_formatting_and_reject_managed_field_changes() {
    use workdeck_pm::IssueMutation;
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let original = repository
        .issue_markdown(issue.metadata.id.as_str())
        .unwrap();
    let edited = original
        .replacen("---\n", "---\n# Reviewed draft\n", 1)
        .replace("Fix parser", "Parser draft")
        .replace("Keep metadata intact.", "Body edited in my editor.");
    let receipt = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &IssueMutation::EditDocument { markdown: edited },
            &request("editor"),
        )
        .unwrap();
    let updated: IssueRecord = serde_json::from_value(receipt.result).unwrap();
    assert_eq!(updated.metadata.title, "Parser draft");
    assert_eq!(updated.body, "Body edited in my editor.\n");
    let source = repository
        .issue_markdown(issue.metadata.id.as_str())
        .unwrap();
    assert!(source.contains("# Reviewed draft\n"));
    assert_eq!(updated.source.revision.get(), 2);
    let forged = source.replace("revision: 2", "revision: 44");
    let error = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            Some(&updated.source),
            &IssueMutation::EditDocument { markdown: forged },
            &request("forged-draft"),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(
        repository
            .issue_markdown(issue.metadata.id.as_str())
            .unwrap(),
        source
    );
    let malformed = "---\ninvalid: [\n---\nbody".into();
    assert!(
        repository
            .mutate_issue(
                issue.metadata.id.as_str(),
                Some(&updated.source),
                &IssueMutation::EditDocument {
                    markdown: malformed
                },
                &request("invalid-draft")
            )
            .is_err()
    );
    assert_eq!(
        repository
            .issue_markdown(issue.metadata.id.as_str())
            .unwrap(),
        source
    );
}

#[test]
fn editor_comment_only_changes_are_preserved_but_unchanged_drafts_do_not_increment_revision() {
    use workdeck_pm::IssueMutation;
    let (_temp, repository) = setup();
    let issue = create(&repository);
    let original = repository
        .issue_markdown(issue.metadata.id.as_str())
        .unwrap();
    let result = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::EditDocument {
                markdown: original.clone(),
            },
            &request("same-draft"),
        )
        .unwrap();
    assert!(result.changed.is_empty());
    let edited = original.replacen("---\n", "---\n# Metadata comment\n", 1);
    let result = repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::EditDocument { markdown: edited },
            &request("comment-draft"),
        )
        .unwrap();
    let updated: IssueRecord = serde_json::from_value(result.result).unwrap();
    assert_eq!(updated.source.revision.get(), 2);
    assert!(
        repository
            .issue_markdown(issue.metadata.id.as_str())
            .unwrap()
            .contains("# Metadata comment")
    );
}

#[test]
fn template_creation_applies_defaults_inside_the_replay_aware_operation() {
    use workdeck_pm::TemplateIssueInput;
    let (_temp, repository) = setup();
    label(&repository, "bug");
    let path = repository.root().join("templates/issues/bug.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "---\nschema: 1\nid: bug\nname: Bug report\ndefaults:\n  priority: high\n  labels: [bug]\n---\n## Reproduction\n").unwrap();
    let input = TemplateIssueInput {
        template: "bug".into(),
        title: "Fix a bug".into(),
        body: None,
        fields: BTreeMap::from([("priority".into(), json!("urgent"))]),
    };
    let first = repository
        .create_issue_from_template(&input, &request("template-create"))
        .unwrap();
    let issue: IssueRecord = serde_json::from_value(first.result.clone()).unwrap();
    assert_eq!(issue.metadata.priority, workdeck_pm::Priority::Urgent);
    assert_eq!(issue.metadata.labels, vec!["bug"]);
    assert_eq!(issue.body, "## Reproduction\n");
    fs::write(&path, "broken template").unwrap();
    assert_eq!(
        repository
            .create_issue_from_template(&input, &request("template-create"))
            .unwrap(),
        first
    );
    assert!(
        repository
            .create_issue_from_template(&input, &request("different-request"))
            .is_err()
    );
    assert_eq!(repository.list_issues().unwrap().len(), 1);
}
