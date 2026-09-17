use serde_json::json;
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;
use workdeck_pm::{
    Config, ContentHash, CreateIssue, ErrorCode, IssueMetadata, IssueRecord, Repository, RequestId,
};

fn imported_metadata(config: &Config) -> IssueMetadata {
    let now = "2026-09-01T10:00:00Z".parse().unwrap();
    let mut value =
        serde_json::to_value(IssueMetadata::new(config, "Historical done", now).unwrap()).unwrap();
    value["status"] = json!("done");
    value["imported_completion"] = json!({"source_path":"issues/WD-3.toml", "source_content":ContentHash::of(b"legacy done"), "imported_at":"2026-09-08T10:00:00Z"});
    serde_json::from_value(value).unwrap()
}

#[test]
fn imported_done_preserves_unknown_completion_time_without_verified_acceptance() {
    let config = Config::new("WD").unwrap();
    let metadata = imported_metadata(&config);
    metadata.validate(&config).unwrap();
    assert!(metadata.completed_at.is_none());
    assert!(metadata.manual_acceptance.is_none());
}

#[test]
fn normal_create_cannot_forge_import_provenance_and_reopen_retains_superseded_history() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let metadata = imported_metadata(&repository.config().unwrap());
    let value = serde_json::to_value(&metadata).unwrap()["imported_completion"].clone();
    let request = CreateIssue {
        title: "Forge history".into(),
        body: String::new(),
        fields: BTreeMap::from([("imported_completion".into(), value)]),
    };
    assert_eq!(
        repository
            .create_issue(&request, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    let path = repository
        .root()
        .join(format!("issues/{}/item.md", metadata.id));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "---\n{}---\nHistorical body\n",
            serde_yaml_ng::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let original = repository.show_issue(metadata.id.as_str()).unwrap();
    assert_eq!(
        repository
            .completion_report(metadata.id.as_str())
            .unwrap()
            .basis,
        "imported"
    );
    let reopened: IssueRecord = serde_json::from_value(
        repository
            .reopen_issue(metadata.id.as_str(), &original.source, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let value = serde_json::to_value(&reopened.metadata).unwrap();
    assert!(value["imported_completion"]["superseded_at"].is_string());
    assert_eq!(
        value["imported_completion"]["source_path"],
        "issues/WD-3.toml"
    );
    assert_eq!(
        repository
            .completion_report(metadata.id.as_str())
            .unwrap()
            .basis,
        "declared"
    );
    let completed: IssueRecord = serde_json::from_value(
        repository
            .complete_issue(
                metadata.id.as_str(),
                &reopened.source,
                None,
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    assert!(completed.metadata.completed_at.is_some());
    assert_eq!(
        repository
            .completion_report(metadata.id.as_str())
            .unwrap()
            .basis,
        "declared"
    );
}
