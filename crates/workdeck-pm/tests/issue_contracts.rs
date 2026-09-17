use serde_json::json;
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;
use workdeck_pm::{
    Config, CreateIssue, ErrorCode, IssueMetadata, IssueMutation, IssueRecord, Repository,
    RequestId, UpdateIssue, WorkflowCategory, WorkflowState,
};

fn fixture() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}

fn create(repository: &Repository) -> IssueRecord {
    let mut input = CreateIssue::new("Lifecycle", "Description\n");
    input.fields.insert("due_at".into(), json!("2026-09-30"));
    serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}

fn write_config(repository: &Repository, config: &Config) {
    config.validate().unwrap();
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(config).unwrap(),
    )
    .unwrap();
}

#[test]
fn cancellation_and_reopening_preserve_due_dates_and_manage_terminal_times() {
    let (_temp, repository) = fixture();
    let original = create(&repository);
    let request = RequestId::new();
    let receipt = repository
        .mutate_issue(
            original.metadata.id.as_str(),
            Some(&original.source),
            &IssueMutation::Cancel,
            &request,
        )
        .unwrap();
    assert_eq!(receipt.operation, "issue.cancel");
    let canceled: IssueRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(canceled.metadata.status, "canceled");
    assert_eq!(
        canceled.metadata.canceled_at,
        Some(canceled.metadata.updated_at)
    );
    assert!(canceled.metadata.completed_at.is_none());
    assert_eq!(canceled.metadata.due_at, original.metadata.due_at);
    assert_eq!(canceled.metadata.created_at, original.metadata.created_at);
    assert!(
        !repository
            .completion_report(original.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    let noop = repository
        .mutate_issue(
            original.metadata.id.as_str(),
            None,
            &IssueMutation::Cancel,
            &RequestId::new(),
        )
        .unwrap();
    assert!(noop.changed.is_empty());
    assert_eq!(noop.result, receipt.result);
    let reopened: IssueRecord = serde_json::from_value(
        repository
            .mutate_issue(
                original.metadata.id.as_str(),
                Some(&canceled.source),
                &IssueMutation::Reopen,
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    assert_eq!(reopened.metadata.status, "ready");
    assert!(reopened.metadata.canceled_at.is_none());
    assert!(reopened.metadata.completed_at.is_none());
    assert_eq!(reopened.metadata.due_at, original.metadata.due_at);
    assert_eq!(
        repository
            .mutate_issue(
                original.metadata.id.as_str(),
                Some(&original.source),
                &IssueMutation::Cancel,
                &request
            )
            .unwrap(),
        receipt
    );
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap(),
        reopened
    );
}

#[test]
fn cancellation_resolves_configured_category_inside_replay_and_does_not_choose_ambiguously() {
    let (_temp, repository) = fixture();
    let mut config = repository.config().unwrap();
    for state in &mut config.workflow.states {
        if state.id == "canceled" {
            state.id = "stopped".into();
        }
        for target in &mut state.transitions {
            if target == "canceled" {
                *target = "stopped".into();
            }
        }
    }
    write_config(&repository, &config);
    let original = create(&repository);
    let request = RequestId::new();
    let receipt = repository
        .mutate_issue(
            original.metadata.id.as_str(),
            None,
            &IssueMutation::Cancel,
            &request,
        )
        .unwrap();
    assert_eq!(receipt.result["metadata"]["status"], "stopped");
    repository
        .mutate_issue(
            original.metadata.id.as_str(),
            None,
            &IssueMutation::Reopen,
            &RequestId::new(),
        )
        .unwrap();
    config.workflow.states.push(WorkflowState {
        id: "aborted".into(),
        name: "Aborted".into(),
        category: WorkflowCategory::Canceled,
        transitions: Vec::new(),
    });
    config
        .workflow
        .states
        .iter_mut()
        .find(|state| state.id == "ready")
        .unwrap()
        .transitions
        .push("aborted".into());
    write_config(&repository, &config);
    let before = fs::read(repository.root().join(&original.path)).unwrap();
    assert_eq!(
        repository
            .mutate_issue(
                original.metadata.id.as_str(),
                None,
                &IssueMutation::Cancel,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::AmbiguousReference
    );
    assert_eq!(
        fs::read(repository.root().join(&original.path)).unwrap(),
        before
    );
    assert_eq!(
        repository
            .mutate_issue(
                original.metadata.id.as_str(),
                None,
                &IssueMutation::Cancel,
                &request
            )
            .unwrap(),
        receipt
    );
    config.workflow.states.push(WorkflowState {
        id: "canceled".into(),
        name: "Canceled".into(),
        category: WorkflowCategory::Canceled,
        transitions: Vec::new(),
    });
    config
        .workflow
        .states
        .iter_mut()
        .find(|state| state.id == "ready")
        .unwrap()
        .transitions
        .push("canceled".into());
    write_config(&repository, &config);
    assert_eq!(
        repository
            .mutate_issue(
                original.metadata.id.as_str(),
                None,
                &IssueMutation::Cancel,
                &RequestId::new()
            )
            .unwrap()
            .result["metadata"]["status"],
        "canceled"
    );
}

#[test]
fn no_cancel_target_or_a_forbidden_transition_never_changes_source() {
    let (_temp, repository) = fixture();
    let original = create(&repository);
    let completed = repository
        .mutate_issue(
            original.metadata.id.as_str(),
            None,
            &IssueMutation::Complete { manual: None },
            &RequestId::new(),
        )
        .unwrap();
    let before = fs::read(repository.root().join(&original.path)).unwrap();
    assert_eq!(
        repository
            .mutate_issue(
                original.metadata.id.as_str(),
                None,
                &IssueMutation::Cancel,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(
        fs::read(repository.root().join(&original.path)).unwrap(),
        before
    );
    assert!(completed.result["metadata"]["completed_at"].is_string());
    repository
        .mutate_issue(
            original.metadata.id.as_str(),
            None,
            &IssueMutation::Reopen,
            &RequestId::new(),
        )
        .unwrap();
    let mut config = repository.config().unwrap();
    config
        .workflow
        .states
        .retain(|state| state.category != WorkflowCategory::Canceled);
    for state in &mut config.workflow.states {
        state.transitions.retain(|target| target != "canceled");
    }
    write_config(&repository, &config);
    let before = fs::read(repository.root().join(&original.path)).unwrap();
    assert_eq!(
        repository
            .mutate_issue(
                original.metadata.id.as_str(),
                None,
                &IssueMutation::Cancel,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
    assert_eq!(
        fs::read(repository.root().join(&original.path)).unwrap(),
        before
    );
}

#[test]
fn invalid_due_dates_preserve_source_and_exact_instants_can_be_removed() {
    let (_temp, repository) = fixture();
    let original = create(&repository);
    let before = fs::read(repository.root().join(&original.path)).unwrap();
    for due in ["2026-02-29", "2026-9-3", "2026-09-30T12:00:00", "tomorrow"] {
        let mutation = IssueMutation::Update {
            input: UpdateIssue {
                fields: BTreeMap::from([("due_at".into(), json!(due))]),
                body: None,
            },
        };
        assert!(
            repository
                .mutate_issue(
                    original.metadata.id.as_str(),
                    None,
                    &mutation,
                    &RequestId::new()
                )
                .is_err()
        );
        assert_eq!(
            fs::read(repository.root().join(&original.path)).unwrap(),
            before
        );
    }
    for due in [json!("2026-09-30T14:00:00+02:00"), serde_json::Value::Null] {
        let mutation = IssueMutation::Update {
            input: UpdateIssue {
                fields: BTreeMap::from([("due_at".into(), due.clone())]),
                body: None,
            },
        };
        let changed = repository
            .mutate_issue(
                original.metadata.id.as_str(),
                None,
                &mutation,
                &RequestId::new(),
            )
            .unwrap();
        assert_eq!(changed.result["metadata"]["due_at"], due);
    }
}

#[test]
fn ambiguous_short_ids_fail_on_reads_and_mutations_while_exact_ids_win() {
    let (_temp, repository) = fixture();
    let config = repository.config().unwrap();
    for id in ["WD-1234", "WD-1235"] {
        let mut metadata = IssueMetadata::new(&config, id, chrono::Utc::now()).unwrap();
        metadata.id = id.parse().unwrap();
        let directory = repository.root().join(format!("issues/{id}"));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("item.md"),
            format!(
                "---\n{}---\nBody\n",
                serde_yaml_ng::to_string(&metadata).unwrap()
            ),
        )
        .unwrap();
    }
    assert_eq!(
        repository
            .show_issue("WD-1234")
            .unwrap()
            .metadata
            .id
            .as_str(),
        "WD-1234"
    );
    let before = repository.list_issues().unwrap();
    assert_eq!(
        repository.show_issue("WD-123").unwrap_err().code,
        ErrorCode::AmbiguousReference
    );
    assert_eq!(
        repository
            .mutate_issue("WD-123", None, &IssueMutation::Cancel, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::AmbiguousReference
    );
    assert_eq!(repository.list_issues().unwrap(), before);
    assert_eq!(
        fs::read_dir(repository.root().join("operations"))
            .unwrap()
            .count(),
        0
    );
}
