use serde_json::json;
use std::collections::BTreeMap;
use workdeck_pm::*;

fn fixture() -> (tempfile::TempDir, Repository, CycleCarryoverRequest) {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path(), "WD").unwrap();
    for id in ["previous", "next"] {
        repo.create_planning(
            PlanningKind::Cycle,
            &CreatePlanning {
                id: Some(id.into()),
                ..CreatePlanning::new(id)
            },
            &RequestId::new(),
        )
        .unwrap();
    }
    (
        directory,
        repo,
        CycleCarryoverRequest {
            from: "previous".into(),
            to: "next".into(),
            issues: vec![],
        },
    )
}
fn issue(repo: &Repository, title: &str) -> IssueRecord {
    serde_json::from_value(
        repo.create_issue(
            &CreateIssue {
                title: title.into(),
                body: "Retain this Markdown\n".into(),
                fields: BTreeMap::from([("cycle".into(), json!("previous"))]),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}
#[test]
fn carryover_moves_only_unfinished_work_and_replays_the_original_batch() {
    let (_directory, repo, request) = fixture();
    let first = issue(&repo, "First");
    let second = issue(&repo, "Second");
    let canceled = issue(&repo, "Canceled");
    repo.mutate_issue(
        canceled.metadata.id.as_str(),
        Some(&canceled.source),
        &IssueMutation::Cancel,
        &RequestId::new(),
    )
    .unwrap();
    let plan = repo.preview_cycle_carryover(&request).unwrap();
    assert_eq!(plan.issues.len(), 2);
    assert_eq!(plan.excluded.len(), 1);
    let id = RequestId::new();
    let receipt = repo
        .apply_cycle_carryover(&request, &plan.fingerprint, &id)
        .unwrap();
    assert_eq!(receipt.changed.len(), 2);
    for original in [first, second] {
        let current = repo.show_issue(original.metadata.id.as_str()).unwrap();
        assert_eq!(current.metadata.cycle.as_deref(), Some("next"));
        assert_eq!(current.metadata.status, original.metadata.status);
        assert_eq!(current.body, original.body);
    }
    assert_eq!(
        repo.show_issue(canceled.metadata.id.as_str())
            .unwrap()
            .metadata
            .cycle
            .as_deref(),
        Some("previous")
    );
    assert_eq!(
        repo.apply_cycle_carryover(&request, &plan.fingerprint, &id)
            .unwrap()
            .operation_id,
        receipt.operation_id
    );
}
#[test]
fn changed_membership_rejects_the_entire_reviewed_carryover() {
    let (_directory, repo, request) = fixture();
    let first = issue(&repo, "First");
    let plan = repo.preview_cycle_carryover(&request).unwrap();
    issue(&repo, "Added after review");
    assert_eq!(
        repo.apply_cycle_carryover(&request, &plan.fingerprint, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repo.show_issue(first.metadata.id.as_str()).unwrap().source,
        first.source
    );
}

#[test]
fn carryover_recovers_the_original_multi_issue_intent_after_interruption() {
    for fault in [
        transactions::FaultPoint::AfterJournal,
        transactions::FaultPoint::AfterChange(0),
        transactions::FaultPoint::BeforeReceipt,
    ] {
        let (_directory, repo, input) = fixture();
        let first = issue(&repo, "First");
        let second = issue(&repo, "Second");
        let plan = repo.preview_cycle_carryover(&input).unwrap();
        let request = RequestId::new();
        let result =
            repo.apply_cycle_carryover_with_faults(&input, &plan.fingerprint, &request, |point| {
                if point == fault {
                    Err(PmError::new(
                        ErrorCode::Io,
                        "injected carryover interruption",
                    ))
                } else {
                    Ok(())
                }
            });
        assert!(result.is_err());
        repo.recover_operations().unwrap();
        let replay = repo
            .apply_cycle_carryover(&input, &plan.fingerprint, &request)
            .unwrap();
        assert_eq!(replay.changed.len(), 2);
        for original in [first, second] {
            let current = repo.show_issue(original.metadata.id.as_str()).unwrap();
            assert_eq!(current.metadata.cycle.as_deref(), Some("next"));
            assert_eq!(
                current.metadata.revision.get(),
                original.metadata.revision.get() + 1
            );
        }
    }
}
#[test]
fn carryover_rejects_other_checkout_plans_and_invalid_explicit_selections() {
    let (_first_directory, first, input) = fixture();
    let original = issue(&first, "First");
    let plan = first.preview_cycle_carryover(&input).unwrap();
    let (_second_directory, second, _) = fixture();
    issue(&second, "First");
    assert_eq!(
        second
            .apply_cycle_carryover(&input, &plan.fingerprint, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let mut duplicate = input.clone();
    duplicate.issues = vec![original.metadata.id.clone(), original.metadata.id.clone()];
    assert_eq!(
        first.preview_cycle_carryover(&duplicate).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    let mut same = input.clone();
    same.to = same.from.clone();
    assert_eq!(
        first.preview_cycle_carryover(&same).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    first
        .mutate_issue(
            original.metadata.id.as_str(),
            Some(&original.source),
            &IssueMutation::Cancel,
            &RequestId::new(),
        )
        .unwrap();
    duplicate.issues.pop();
    assert_eq!(
        first.preview_cycle_carryover(&duplicate).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
}

#[test]
fn completed_and_archived_members_stay_while_explicit_selection_moves_one() {
    let (_directory, repo, mut input) = fixture();
    let selected = issue(&repo, "Selected");
    let other = issue(&repo, "Other unfinished");
    let done = issue(&repo, "Done");
    let archived = issue(&repo, "Archived");
    repo.mutate_issue(
        done.metadata.id.as_str(),
        Some(&done.source),
        &IssueMutation::Complete { manual: None },
        &RequestId::new(),
    )
    .unwrap();
    repo.mutate_issue(
        archived.metadata.id.as_str(),
        Some(&archived.source),
        &IssueMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    let all = repo.preview_cycle_carryover(&input).unwrap();
    assert_eq!(all.issues.len(), 2);
    assert_eq!(all.excluded.len(), 2);
    assert!(
        all.excluded
            .iter()
            .any(|excluded| excluded.reason == "completed")
    );
    assert!(
        all.excluded
            .iter()
            .any(|excluded| excluded.reason == "archived")
    );
    input.issues = vec![selected.metadata.id.clone()];
    let plan = repo.preview_cycle_carryover(&input).unwrap();
    repo.apply_cycle_carryover(&input, &plan.fingerprint, &RequestId::new())
        .unwrap();
    assert_eq!(
        repo.show_issue(other.metadata.id.as_str()).unwrap().source,
        other.source
    );
    assert_eq!(
        repo.show_issue(selected.metadata.id.as_str())
            .unwrap()
            .metadata
            .cycle
            .as_deref(),
        Some("next")
    );
}
#[test]
fn identical_repository_and_issue_ids_in_a_copied_checkout_do_not_share_carryover_authority() {
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let (_directory, repo, input) = fixture();
    let original = issue(&repo, "Same-looking work");
    let plan = repo.preview_cycle_carryover(&input).unwrap();
    let clone = tempfile::tempdir().unwrap();
    copy(repo.root(), &clone.path().join(".workdeck"));
    let other = Repository::discover(clone.path()).unwrap();
    assert_eq!(repo.identity(), other.identity());
    assert_eq!(
        other
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        original.source
    );
    assert_eq!(
        other
            .apply_cycle_carryover(&input, &plan.fingerprint, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn carryover_preview_binds_absent_validation_catalogs_even_when_new_policy_is_equivalent() {
    let (_directory, repo, input) = fixture();
    let original = issue(&repo, "Policy-bound carryover");
    let schema = repo.organization_schema().unwrap();
    assert!(schema.source.is_none());
    let plan = repo.preview_cycle_carryover(&input).unwrap();
    std::fs::write(
        repo.root().join("schema.yml"),
        serde_json::to_vec(&schema.definition).unwrap(),
    )
    .unwrap();
    assert!(repo.organization_schema().unwrap().source.is_some());
    assert_eq!(
        repo.apply_cycle_carryover(&input, &plan.fingerprint, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repo.show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        original.source
    );
}
