use std::fs;
use tempfile::TempDir;
use workdeck_pm::{
    CreateIssue, ErrorCode, IssueRecord, Repository, RepositoryId, RequestId, UpdateIssue,
};

#[test]
fn opened_source_rejects_replaced_repository_identity_for_reads_writes_and_retries() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let request = RequestId::new();
    let input = CreateIssue::new("Pinned source", "Original body\n");
    let receipt = repository.create_issue(&input, &request).unwrap();
    assert_eq!(
        receipt.repository.as_ref(),
        Some(&repository.config().unwrap().repository)
    );
    let issue: IssueRecord = serde_json::from_value(receipt.result).unwrap();
    let item_path = repository.root().join(&issue.path);
    let original = fs::read(&item_path).unwrap();
    let config_path = repository.root().join("config.yml");
    let old_id = repository.config().unwrap().repository;
    let new_id = RepositoryId::new();
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace(old_id.as_str(), new_id.as_str()),
    )
    .unwrap();
    let update = UpdateIssue {
        body: Some("Changed body\n".into()),
        ..Default::default()
    };
    assert_eq!(
        repository
            .update_issue(
                issue.metadata.id.as_str(),
                &issue.source,
                &update,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repository.create_issue(&input, &request).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repository.config().unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(fs::read(item_path).unwrap(), original);
    let reopened = Repository::open_source(repository.root()).unwrap();
    assert_eq!(reopened.config().unwrap().repository, new_id);
    assert_eq!(
        reopened.create_issue(&input, &request).unwrap_err().code,
        ErrorCode::StaleSource
    );
}
