use serde_json::json;
use std::{fs, path::PathBuf};
use tempfile::TempDir;
use workdeck_pm::{
    CommentRecord, CreateIssue, ErrorCode, IssueId, IssueRecord, Repository, RequestId,
};

fn setup() -> (TempDir, Repository, IssueRecord, CommentRecord) {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Parser", "Description"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let receipt = repository
        .add_comment(
            issue.metadata.id.as_str(),
            &issue.source,
            "agent",
            "Preserve this body.\n",
            &RequestId::new(),
        )
        .unwrap();
    let comment = serde_json::from_value(receipt.result["comment"].clone()).unwrap();
    (temp, repository, issue, comment)
}

fn rewrite_comment(
    repository: &Repository,
    comment: &CommentRecord,
    field: &str,
    value: serde_json::Value,
    body: &str,
) {
    let mut header = json!({
        "schema":comment.schema,"id":comment.id,"issue":comment.issue,
        "author":comment.author,"created_at":comment.created_at,"revision":comment.revision,
    });
    header[field] = value;
    let yaml = serde_yaml_ng::to_string(&header).unwrap();
    fs::write(
        repository.root().join(&comment.path),
        format!("---\n{yaml}---\n{body}"),
    )
    .unwrap();
}

#[test]
fn unknown_and_derived_comment_frontmatter_fields_are_rejected() {
    let (_temp, repository, issue, comment) = setup();
    for field in ["unknown_typo", "body", "path"] {
        rewrite_comment(
            &repository,
            &comment,
            field,
            json!("injected"),
            &comment.body,
        );
        let error = repository.comments(issue.metadata.id.as_str()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidSchema, "{field}: {error}");
        assert!(
            error
                .path
                .unwrap()
                .ends_with(&comment.path.to_string_lossy().to_string())
        );
    }
}

#[test]
fn comment_reads_and_writes_reject_empty_bodies_and_control_character_authors() {
    let (_temp, repository, issue, comment) = setup();
    let item_before = fs::read(repository.root().join(&issue.path)).unwrap();
    for (author, body) in [
        ("", "body"),
        ("agent\u{1b}[31m", "body"),
        ("agent\nname", "body"),
        ("agent", " \n\t"),
    ] {
        let error = repository
            .add_comment(
                issue.metadata.id.as_str(),
                &issue.source,
                author,
                body,
                &RequestId::new(),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        rewrite_comment(&repository, &comment, "author", json!(author), body);
        assert_eq!(
            repository
                .comments(issue.metadata.id.as_str())
                .unwrap_err()
                .code,
            ErrorCode::InvalidSchema,
        );
    }
    assert_eq!(
        fs::read(repository.root().join(&issue.path)).unwrap(),
        item_before
    );
    assert_eq!(
        fs::read_dir(repository.root().join("operations"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn comment_layout_rejects_nested_files_wrong_extensions_and_wrong_record_kinds() {
    for variant in ["nested", "extension", "kind", "unexpected"] {
        let (_temp, repository, issue, comment) = setup();
        let original = repository.root().join(&comment.path);
        let directory = original.parent().unwrap();
        match variant {
            "nested" => {
                fs::create_dir(directory.join("nested")).unwrap();
                fs::rename(
                    &original,
                    directory.join("nested").join(original.file_name().unwrap()),
                )
                .unwrap();
            }
            "extension" => {
                fs::rename(&original, original.with_extension("txt")).unwrap();
            }
            "kind" => {
                let wrong_id = IssueId::new("WD").unwrap();
                rewrite_comment(&repository, &comment, "id", json!(wrong_id), &comment.body);
                fs::rename(&original, directory.join(format!("{wrong_id}.md"))).unwrap();
            }
            "unexpected" => {
                fs::write(directory.join("README"), "unexpected record").unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            repository
                .comments(issue.metadata.id.as_str())
                .unwrap_err()
                .code,
            ErrorCode::InvalidSchema,
            "{variant}",
        );
    }
}

#[test]
fn unsupported_issue_and_comment_schemas_keep_their_machine_category() {
    let (_temp, repository, issue, comment) = setup();
    rewrite_comment(&repository, &comment, "schema", json!(2), &comment.body);
    let error = repository.comments(issue.metadata.id.as_str()).unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedSchema);
    assert_eq!(
        PathBuf::from(error.path.unwrap()),
        repository.root().join(&comment.path)
    );
    let item = repository.root().join(&issue.path);
    let raw = fs::read_to_string(&item)
        .unwrap()
        .replace("schema: 1", "schema: 2");
    fs::write(&item, &raw).unwrap();
    let error = repository
        .show_issue(issue.metadata.id.as_str())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedSchema);
    assert_eq!(PathBuf::from(error.path.unwrap()), item);
    assert_eq!(fs::read_to_string(&item).unwrap(), raw);
}

#[test]
fn valid_comment_reads_preserve_unicode_body_source_and_record_identity() {
    let (_temp, repository, issue, comment) = setup();
    let body = "# Review 🐦\n\nKeep **Markdown**, tabs\tand trailing spaces.  \n";
    rewrite_comment(&repository, &comment, "author", json!("Reviewer É"), body);
    let read = repository.comments(issue.metadata.id.as_str()).unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].author, "Reviewer É");
    assert_eq!(read[0].body, body);
    assert_eq!(read[0].id, comment.id);
    assert_eq!(read[0].path, comment.path);
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
}
