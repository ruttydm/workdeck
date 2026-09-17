use serde_json::json;
use std::collections::BTreeMap;
use workdeck_pm::*;

fn project(repo: &Repository, fields: serde_json::Value) -> PlanningRecord {
    serde_json::from_value(
        repo.create_planning(
            PlanningKind::Project,
            &CreatePlanning {
                id: None,
                name: "Policy project".into(),
                body: "Project body".into(),
                fields: serde_json::from_value(fields).unwrap(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}

fn issue(repo: &Repository, fields: serde_json::Value) -> IssueRecord {
    serde_json::from_value(
        repo.create_issue(
            &CreateIssue {
                title: "Policy work".into(),
                body: "A bounded implementation".into(),
                fields: serde_json::from_value(fields).unwrap(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}

fn acceptance() -> PolicyAcceptance {
    PolicyAcceptance {
        actor: "policy-agent".into(),
        reason: "The retained work and exit criteria were reviewed.".into(),
    }
}

#[test]
fn project_policy_is_source_bound_and_requires_completed_members_and_acceptance() {
    let temporary = tempfile::tempdir().unwrap();
    let repo = Repository::init(temporary.path(), "WD").unwrap();
    let project = project(
        &repo,
        json!({"exit_criteria":[{"id":"shipped","description":"The project is shipped"}]}),
    );
    let work = issue(&repo, json!({"project":project.metadata.id.clone()}));
    let blocked = repo
        .assess_planning_policy(PlanningKind::Project, &project.metadata.id)
        .unwrap();
    assert!(!blocked.allowed);
    assert!(blocked.conditions.iter().any(|condition| {
        condition.reason_code == "issue_incomplete" || condition.reason_code == "criterion_declared"
    }));
    let before = repo.show_issue(work.metadata.id.as_str()).unwrap();
    repo.complete_issue(
        work.metadata.id.as_str(),
        &before.source,
        None,
        &RequestId::new(),
    )
    .unwrap();
    let still_declared = repo
        .assess_planning_policy(PlanningKind::Project, &project.metadata.id)
        .unwrap();
    assert!(
        !still_declared.allowed,
        "a declaration is not acceptance evidence"
    );
    let accepted = acceptance();
    let completed = repo
        .complete_planning(
            PlanningKind::Project,
            &project.metadata.id,
            &project_source(&repo, PlanningKind::Project, &project.metadata.id),
            &accepted,
            &RequestId::new(),
        )
        .unwrap();
    let record: PlanningRecord = serde_json::from_value(completed.result).unwrap();
    assert_eq!(record.metadata.status.as_deref(), Some("done"));
    assert_eq!(
        repo.planning_record(PlanningKind::Project, &project.metadata.id)
            .unwrap()
            .metadata
            .status
            .as_deref(),
        Some("done")
    );
}

fn project_source(repo: &Repository, kind: PlanningKind, id: &str) -> SourceToken {
    repo.planning_record(kind, id).unwrap().source
}

#[test]
fn feature_promotion_advances_one_stage_and_requires_issue_completion() {
    let temporary = tempfile::tempdir().unwrap();
    let repo = Repository::init(temporary.path(), "WD").unwrap();
    let feature: FeatureRecord = serde_json::from_value(
        repo.create_feature(
            &CreateFeature {
                name: "Policy capability".into(),
                body: "Capability body".into(),
                fields: BTreeMap::from([
                    ("decision".into(), json!("accepted")),
                    (
                        "criteria".into(),
                        json!([{"id":"contract","description":"The capability contract holds"}]),
                    ),
                ]),
                directory: None,
            },
            &RequestId::new(),
        )
        .unwrap()
        .result["record"]
            .clone(),
    )
    .unwrap();
    let work = issue(&repo, json!({"features":[feature.metadata.id.clone()]}));
    assert!(
        !repo
            .assess_feature_maturity(
                &feature.metadata.id.to_string(),
                FeatureMaturity::Implemented
            )
            .unwrap()
            .allowed
    );
    repo.promote_feature(
        feature.metadata.id.as_str(),
        &feature.source,
        FeatureMaturity::Specified,
        &acceptance(),
        &RequestId::new(),
    )
    .unwrap();
    let specified = repo.feature(feature.metadata.id.as_str()).unwrap();
    let blocked = repo
        .assess_feature_maturity(feature.metadata.id.as_str(), FeatureMaturity::Implemented)
        .unwrap();
    assert!(!blocked.allowed);
    let before = repo.show_issue(work.metadata.id.as_str()).unwrap();
    repo.complete_issue(
        work.metadata.id.as_str(),
        &before.source,
        None,
        &RequestId::new(),
    )
    .unwrap();
    let receipt = repo
        .promote_feature(
            feature.metadata.id.as_str(),
            &specified.source,
            FeatureMaturity::Implemented,
            &acceptance(),
            &RequestId::new(),
        )
        .unwrap();
    let promoted: FeatureOutcome = serde_json::from_value(receipt.result).unwrap();
    assert_eq!(
        promoted.record.metadata.maturity,
        FeatureMaturity::Implemented
    );
    assert!(repo.doctor().unwrap().valid);
}
