use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;
use workdeck_pm::{
    AttachmentInput, AttachmentRecord, ContentHash, CreateIssue, ErrorCode, IssueRecord,
    MAX_ATTACHMENT_BYTES, Repository, RequestId, UpdateIssue,
};

fn setup() -> (TempDir, Repository, IssueRecord) {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Attachment fixture", "Preserve source."),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    (temp, repository, issue)
}

fn input(bytes: &[u8]) -> AttachmentInput {
    AttachmentInput {
        name: "sample data.bin".into(),
        content: bytes.into(),
        media_type: Some("application/octet-stream".into()),
        actor: "fixture-author".into(),
    }
}

#[test]
fn binary_attachment_round_trips_without_rewriting_issue_or_revision() {
    let (_temp, repository, issue) = setup();
    let original = fs::read(repository.root().join(&issue.path)).unwrap();
    let input = input(&[0, 255, 254, 128, 13, 10, 0]);
    let receipt = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &input,
            &RequestId::new(),
        )
        .unwrap();
    let attachment: AttachmentRecord = serde_json::from_value(receipt.result).unwrap();
    assert_eq!(attachment.content_hash, ContentHash::of(&input.content));
    assert_eq!(attachment.size, input.content.len() as u64);
    assert_eq!(attachment.issue, issue.metadata.id);
    assert!(attachment.id.as_str().starts_with("ATT-"));
    assert_eq!(
        attachment.path.to_string_lossy(),
        format!(
            "issues/{}/attachments/{}/metadata.yml",
            issue.metadata.id, attachment.id
        )
    );
    assert_eq!(
        attachment.content_path.to_string_lossy(),
        format!(
            "issues/{}/attachments/{}/content/sample data.bin",
            issue.metadata.id, attachment.id
        )
    );
    assert_eq!(receipt.changed.len(), 2);
    assert!(
        receipt
            .changed
            .iter()
            .all(|change| change.path != issue.path)
    );
    assert_eq!(
        repository
            .read_attachment(issue.metadata.id.as_str(), attachment.id.as_str())
            .unwrap(),
        input.content
    );
    assert_eq!(
        repository
            .list_attachments(issue.metadata.id.as_str())
            .unwrap(),
        [attachment]
    );
    assert_eq!(
        fs::read(repository.root().join(&issue.path)).unwrap(),
        original
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
fn replay_precedes_lookup_and_changed_payload_is_an_idempotency_conflict() {
    let (_temp, repository, issue) = setup();
    let request = RequestId::new();
    let original = input(b"original");
    let first = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &original,
            &request,
        )
        .unwrap();
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: BTreeMap::from([("title".into(), json!("Changed after attachment"))]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    let replay = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &original,
            &request,
        )
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(
        repository
            .list_attachments(issue.metadata.id.as_str())
            .unwrap()
            .len(),
        1
    );
    let error = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &input(b"different"),
            &request,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::IdempotencyConflict);
    let error = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &original,
            &RequestId::new(),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
}

#[test]
fn names_actor_and_media_type_are_validated_before_writes() {
    let (_temp, repository, issue) = setup();
    for name in [
        "",
        ".",
        "..",
        "../escape",
        "/absolute",
        "folder/file",
        "folder\\file",
        "x:y",
        "CON",
        "NUL.txt",
        "com1.bin",
        "LPT9.log",
        "file.",
        "file ",
        "a\nb",
        "a\0b",
        "<file>",
        "a?b",
        "a|b",
        "a*b",
        "a\"b",
    ] {
        let mut candidate = input(b"bytes");
        candidate.name = name.into();
        assert_eq!(
            repository
                .attach_issue(
                    issue.metadata.id.as_str(),
                    None,
                    &candidate,
                    &RequestId::new()
                )
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput,
            "{name:?}"
        );
    }
    for actor in ["", "   ", "author\nother"] {
        let mut candidate = input(b"bytes");
        candidate.actor = actor.into();
        assert_eq!(
            repository
                .attach_issue(
                    issue.metadata.id.as_str(),
                    None,
                    &candidate,
                    &RequestId::new()
                )
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    for media_type in ["", "text/plain\r\nx-evil: x", "text", "text/", "/plain"] {
        let mut candidate = input(b"bytes");
        candidate.media_type = Some(media_type.into());
        assert_eq!(
            repository
                .attach_issue(
                    issue.metadata.id.as_str(),
                    None,
                    &candidate,
                    &RequestId::new()
                )
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    assert!(
        !repository
            .root()
            .join(format!("issues/{}/attachments", issue.metadata.id))
            .exists()
    );
}

#[test]
fn listing_inspects_metadata_without_loading_payload_but_explicit_read_verifies_bytes() {
    let (_temp, repository, issue) = setup();
    let receipt = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            None,
            &input(b"safe bytes"),
            &RequestId::new(),
        )
        .unwrap();
    let record: AttachmentRecord = serde_json::from_value(receipt.result).unwrap();
    fs::write(repository.root().join(&record.content_path), b"different").unwrap();
    assert_eq!(
        repository
            .list_attachments(issue.metadata.id.as_str())
            .unwrap()
            .as_slice(),
        std::slice::from_ref(&record)
    );
    assert_eq!(
        repository
            .read_attachment(issue.metadata.id.as_str(), record.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::CorruptStore
    );
    fs::remove_file(repository.root().join(&record.content_path)).unwrap();
    assert_eq!(
        repository
            .list_attachments(issue.metadata.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::CorruptStore
    );
}

#[test]
fn metadata_cannot_redirect_to_another_issue_or_undeclared_file() {
    let (_temp, repository, issue) = setup();
    let record: AttachmentRecord = serde_json::from_value(
        repository
            .attach_issue(
                issue.metadata.id.as_str(),
                None,
                &input(b"bytes"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let path = repository.root().join(&record.path);
    let original = fs::read_to_string(&path).unwrap();
    for (field, value) in [
        ("issue", json!("WD-1")),
        ("content_path", json!("../../outside")),
        ("path", json!("issues/wrong/metadata.yml")),
        ("schema", json!(2)),
        ("unexpected", json!("field")),
    ] {
        let mut metadata: serde_json::Value = serde_yaml_ng::from_str(&original).unwrap();
        metadata[field] = value;
        fs::write(&path, serde_yaml_ng::to_string(&metadata).unwrap()).unwrap();
        assert!(
            repository
                .list_attachments(issue.metadata.id.as_str())
                .is_err(),
            "{field}"
        );
    }
    fs::write(&path, original).unwrap();
    let extra = repository
        .root()
        .join(record.path.parent().unwrap())
        .join("unknown.txt");
    fs::write(extra, b"unknown").unwrap();
    assert_eq!(
        repository
            .list_attachments(issue.metadata.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
}

#[cfg(unix)]
#[test]
fn symlink_payload_is_not_followed_on_list_or_read() {
    use std::os::unix::fs::symlink;
    let (temp, repository, issue) = setup();
    let record: AttachmentRecord = serde_json::from_value(
        repository
            .attach_issue(
                issue.metadata.id.as_str(),
                None,
                &input(b"bytes"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let outside = temp.path().join("outside.bin");
    fs::write(&outside, b"outside").unwrap();
    let path = repository.root().join(&record.content_path);
    fs::remove_file(&path).unwrap();
    symlink(&outside, &path).unwrap();
    assert_eq!(
        repository
            .list_attachments(issue.metadata.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    assert_eq!(
        repository
            .read_attachment(issue.metadata.id.as_str(), record.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    assert_eq!(fs::read(outside).unwrap(), b"outside");
}

#[test]
fn maximum_attachment_is_supported_and_oversize_inputs_and_payloads_are_rejected() {
    let (_temp, repository, issue) = setup();
    let boundary = input(&vec![0xff; MAX_ATTACHMENT_BYTES]);
    let record: AttachmentRecord = serde_json::from_value(
        repository
            .attach_issue(
                issue.metadata.id.as_str(),
                None,
                &boundary,
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    assert_eq!(
        repository
            .read_attachment(issue.metadata.id.as_str(), record.id.as_str())
            .unwrap(),
        boundary.content
    );
    let mut oversized = boundary;
    oversized.content.push(0);
    assert_eq!(
        repository
            .attach_issue(
                issue.metadata.id.as_str(),
                None,
                &oversized,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    let file = fs::OpenOptions::new()
        .write(true)
        .open(repository.root().join(&record.content_path))
        .unwrap();
    file.set_len(MAX_ATTACHMENT_BYTES as u64 + 1).unwrap();
    assert_eq!(
        repository
            .read_attachment(issue.metadata.id.as_str(), record.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
}

#[test]
fn repeated_named_attachments_keep_distinct_identity_without_staling_the_issue() {
    let (_temp, repository, issue) = setup();
    let mut records = Vec::new();
    for content in [b"one".as_slice(), b"two".as_slice(), b"".as_slice()] {
        let receipt = repository
            .attach_issue(
                issue.metadata.id.as_str(),
                Some(&issue.source),
                &input(content),
                &RequestId::new(),
            )
            .unwrap();
        records.push(serde_json::from_value::<AttachmentRecord>(receipt.result).unwrap());
    }
    assert_eq!(
        records
            .iter()
            .map(|record| record.id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(
        repository
            .list_attachments(issue.metadata.id.as_str())
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
    assert!(
        repository
            .read_attachment(issue.metadata.id.as_str(), records[2].id.as_str())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn oversized_metadata_is_rejected_before_parsing() {
    let (_temp, repository, issue) = setup();
    let record: AttachmentRecord = serde_json::from_value(
        repository
            .attach_issue(
                issue.metadata.id.as_str(),
                None,
                &input(b"bytes"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(repository.root().join(&record.path))
        .unwrap()
        .set_len(64 * 1024 + 1)
        .unwrap();
    assert_eq!(
        repository
            .list_attachments(issue.metadata.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
}
