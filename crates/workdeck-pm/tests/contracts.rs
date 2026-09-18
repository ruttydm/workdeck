use std::{collections::BTreeSet, str::FromStr};

use workdeck_pm::{
    ContentHash, ErrorCode, IssueId, QualifiedRef, RepositoryId, Revision, SchemaVersion,
    SourceToken, Workflow, WorkflowCategory,
};

#[test]
fn identities_survive_serialization_without_accepting_paths_or_truncated_ulids() {
    let id = IssueId::new("WD").unwrap();
    assert_eq!(id.as_str().len(), 29);
    assert_eq!(IssueId::from_str(id.as_str()).unwrap(), id);
    assert_eq!(
        serde_json::from_str::<IssueId>(&serde_json::to_string(&id).unwrap()).unwrap(),
        id
    );
    assert_eq!(IssueId::from_str("WD-42").unwrap().as_str(), "WD-42");
    for invalid in [
        "WD-0",
        "WD-01",
        "../WD-1",
        "WD-abc",
        "wd-1",
        "WD-",
        "WD-01ARZ3NDEKTSV4RRFFQ69G5FA",
        "WD-ZZZZZZZZZZZZZZZZZZZZZZZZZZ",
    ] {
        assert!(IssueId::from_str(invalid).is_err(), "accepted {invalid}");
        assert!(serde_json::from_value::<IssueId>(serde_json::json!(invalid)).is_err());
    }
    assert!(IssueId::new("../WD").is_err());
    let ids = (0..10_000)
        .map(|_| IssueId::new("WD").unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 10_000);
}

#[test]
fn qualified_references_do_not_lose_repository_identity() {
    let repository = RepositoryId::new();
    let local = IssueId::from_str("WD-7").unwrap();
    let qualified = QualifiedRef::new(repository.clone(), local.clone());
    assert_eq!(
        QualifiedRef::from_str(&qualified.to_string()).unwrap(),
        qualified
    );
    assert_ne!(qualified, QualifiedRef::new(RepositoryId::new(), local));
    assert!(QualifiedRef::from_str("WD-7").is_err());
    assert!(RepositoryId::from_str("../../repo").is_err());
}

#[test]
fn revision_and_content_are_both_required_for_editor_aware_preconditions() {
    let revision = Revision::new(1).unwrap();
    let original = SourceToken::new(revision, b"title: one\n");
    let edited = SourceToken::new(revision, b"title: two\n");
    assert_ne!(original, edited);
    assert_eq!(original.content, ContentHash::of(b"title: one\n"));
    assert_eq!(revision.next().unwrap().get(), 2);
    assert!(Revision::new(0).is_err());
    assert!(Revision::new(u64::MAX).unwrap().next().is_err());
    assert!(serde_json::from_str::<Revision>("0").is_err());
    assert!(serde_json::from_str::<ContentHash>("\"abc\"").is_err());
}

#[test]
fn schema_and_errors_have_explicit_machine_contracts() {
    assert_eq!(SchemaVersion::CURRENT.get(), 1);
    assert!(serde_json::from_str::<SchemaVersion>("2").is_err());
    assert_eq!(
        serde_json::to_value(ErrorCode::StaleSource).unwrap(),
        "stale_source"
    );
    assert_ne!(ErrorCode::Conflict.exit_code(), 0);
    assert!(ErrorCode::StaleSource.retryable());
    assert!(!ErrorCode::InvalidSchema.retryable());
}

#[test]
fn workflow_categories_distinguish_completed_and_canceled() {
    let workflow = Workflow::default();
    workflow.validate().unwrap();
    assert_eq!(workflow.states.len(), 8);
    assert_eq!(
        workflow.state("done").unwrap().category,
        WorkflowCategory::Completed
    );
    assert_eq!(
        workflow.state("canceled").unwrap().category,
        WorkflowCategory::Canceled
    );
    assert_eq!(workflow.canonical_status("todo").unwrap(), "ready");
    assert_eq!(
        workflow.canonical_status("in-progress").unwrap(),
        "in_progress"
    );
    assert!(workflow.state("missing").is_err());
    assert!(workflow.transition("done", "in_progress").is_err());
    assert!(workflow.transition("ready", "in_progress").is_ok());
    let mut malformed = workflow;
    malformed.states[1].id = malformed.states[0].id.clone();
    assert!(malformed.validate().is_err());
}

#[test]
fn unsupported_source_dialect_and_ci_checks_remain_rejected() {
    // This original future-only fixture uses mode/proposal_ref, not the PM-09
    // remote/proposal_namespace contract. It must remain rejected rather than
    // silently interpreting those fields under the newly implemented protocol.
    let proposal = serde_yaml_ng::from_str::<workdeck_pm::Config>(include_str!(
        "fixtures/source-proposals/config.yml"
    ))
    .unwrap_err();
    assert!(
        proposal.to_string().contains("unknown field `mode`"),
        "{proposal}"
    );
    let checks: workdeck_pm::Config =
        serde_yaml_ng::from_str(include_str!("fixtures/future-checks/config.yml")).unwrap();
    checks.validate().unwrap();
    assert_eq!(
        checks
            .acceptance
            .ensure_supported_for_completion()
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}
