use crate::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn fixture() -> (tempfile::TempDir, Repository) {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .mutate_user(
            "worker",
            None,
            &UserMutation::Create {
                user: UserDefinition::new("Worker"),
            },
            &RequestId::new(),
        )
        .unwrap();
    repository
        .set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    (directory, repository)
}
fn document(path: &Path, metadata: &impl serde::Serialize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "---\n{}---\nScale contract\n",
            serde_yaml_ng::to_string(metadata).unwrap()
        ),
    )
    .unwrap();
}
fn features(repository: &Repository, count: usize) -> (PathBuf, FeatureMetadata) {
    let timestamp = chrono::Utc::now();
    let mut last = None;
    for index in 0..count {
        let metadata: FeatureMetadata = serde_json::from_value(serde_json::json!({
            "schema":1,"repository":repository.identity(),"id":format!("FEAT-{index:026}"),
            "revision":1,"name":format!("Feature {index:05}"),"created_at":timestamp,
            "updated_at":timestamp,"lead":"worker"
        }))
        .unwrap();
        let path = PathBuf::from(format!("features/{}.md", metadata.id));
        document(&repository.root().join(&path), &metadata);
        last = Some((path, metadata));
    }
    last.unwrap()
}
#[test]
fn registered_policy_does_not_reparse_history_for_every_subject() {
    let (_directory, repository) = fixture();
    features(&repository, 80);
    crate::retirement::take_retirement_parse_count();
    let report = repository.organization_compliance().unwrap();
    let parsed = crate::retirement::take_retirement_parse_count();
    assert!(report.compliant);
    assert_eq!(report.checked_records, 80);
    assert!(
        parsed <= 2,
        "two receipts were parsed {parsed} times for 80 subjects"
    );
}
#[test]
fn registered_policy_audits_all_forty_thousand_features() {
    let (_directory, repository) = fixture();
    let (path, mut last) = features(&repository, 40_000);
    let report = repository.organization_compliance().unwrap();
    assert!(report.compliant);
    assert_eq!(report.checked_records, 40_000);
    last.lead = Some("unregistered".into());
    document(&repository.root().join(&path), &last);
    let report = repository.organization_compliance().unwrap();
    assert!(!report.compliant);
    assert_eq!(report.checked_records, 40_000);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].path, path);
}
#[test]
fn registered_policy_audits_all_ten_thousand_issues() {
    let (_directory, repository) = fixture();
    let config = repository.config().unwrap();
    let timestamp = chrono::Utc::now();
    let mut last = None;
    for index in 0..10_000 {
        let mut metadata = IssueMetadata::new(&config, "Scale issue", timestamp).unwrap();
        metadata.id = format!("WD-{index:026}").parse().unwrap();
        metadata.assignee = Some("worker".into());
        let path = PathBuf::from(format!("issues/{}/item.md", metadata.id));
        document(&repository.root().join(&path), &metadata);
        last = Some((path, metadata));
    }
    let report = repository.organization_compliance().unwrap();
    assert!(report.compliant);
    assert_eq!(report.checked_records, 10_000);
    let (path, mut last) = last.unwrap();
    last.assignee = Some("unregistered".into());
    document(&repository.root().join(&path), &last);
    let report = repository.organization_compliance().unwrap();
    assert!(!report.compliant);
    assert_eq!(report.checked_records, 10_000);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].path, path);
}
