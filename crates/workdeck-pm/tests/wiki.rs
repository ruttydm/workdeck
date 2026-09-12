use std::fs;
use tempfile::TempDir;
use workdeck_pm::{Repository, RequestId, preview_snapshot_restore, restore_snapshot};

#[test]
fn plain_markdown_wiki_is_valid_authority_and_roundtrips_in_snapshot() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    fs::create_dir_all(repository.root().join("wiki/architecture")).unwrap();
    // Plain Markdown has no required YAML schema, and scripts remain inert text.
    let body = "---\nnot: [valid YAML\n---\n# Architecture\r\n<script>neverRun()</script>\r\n";
    fs::write(
        repository.root().join("wiki/architecture/overview.md"),
        body,
    )
    .unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    assert!(repository.doctor().unwrap().valid);
    let destination = TempDir::new().unwrap();
    let root = destination.path().join(".workdeck");
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new()).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("wiki/architecture/overview.md")).unwrap(),
        body
    );
}

use workdeck_pm::transactions::{FaultPoint, TransactionStore};
use workdeck_pm::{ContentHash, ErrorCode, PmError, WikiDocument, WriteWiki};

fn input(body: &str, expected: Option<ContentHash>) -> WriteWiki {
    WriteWiki {
        path: "architecture/overview.md".into(),
        body: body.into(),
        expected,
    }
}

#[test]
fn authoring_is_create_only_or_hash_guarded_and_replays_original_receipt() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let create = input("# First\r\n", None);
    let request = RequestId::new();
    let receipt = repository.write_wiki(&create, &request).unwrap();
    let first: WikiDocument = serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(repository.wiki_document(&create.path).unwrap(), first);
    assert_eq!(
        repository
            .write_wiki(&create, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let update = input("# Second\n", Some(first.content_hash.clone()));
    repository.write_wiki(&update, &RequestId::new()).unwrap();
    let replay = repository.write_wiki(&create, &request).unwrap();
    assert_eq!(replay, receipt);
    assert_eq!(
        repository.wiki_document(&create.path).unwrap().body,
        "# Second\n"
    );
    assert_eq!(
        repository
            .write_wiki(&update, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let current = repository.wiki_document(&create.path).unwrap();
    fs::write(repository.root().join(&current.path), "# Direct editor\n").unwrap();
    assert_eq!(
        repository
            .write_wiki(
                &input("overwrite", Some(current.content_hash)),
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repository.wiki_documents().unwrap()[0].body,
        "# Direct editor\n"
    );
    assert_eq!(
        repository
            .write_wiki(&input("different", None), &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
}

#[test]
fn unsafe_paths_and_invalid_content_leave_authority_unchanged() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let before = repository.export_snapshot().unwrap();
    for path in [
        "../escape.md",
        "/absolute.md",
        "a/../../escape.md",
        "CON.md",
        "Guide.md",
        "a.txt",
        "a//b.md",
        "a\\b.md",
    ] {
        let mut write = input("body", None);
        write.path = path.into();
        assert!(
            repository.write_wiki(&write, &RequestId::new()).is_err(),
            "{path}"
        );
    }
    for body in ["embedded\0nul".into(), "x".repeat(2 * 1024 * 1024 + 1)] {
        assert!(
            repository
                .write_wiki(&input(&body, None), &RequestId::new())
                .is_err()
        );
    }
    assert_eq!(repository.export_snapshot().unwrap(), before);
    assert!(!repository.root().join("wiki").exists());
    assert_eq!(
        repository.wiki_document("missing.md").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(!repository.root().join("wiki").exists());
}

#[test]
fn doctor_and_snapshot_reject_malformed_wiki_without_parsing_frontmatter() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    fs::create_dir(repository.root().join("wiki")).unwrap();
    let path = repository.root().join("wiki/invalid.md");
    for bytes in [vec![0xff], vec![0], vec![b'x'; 2 * 1024 * 1024 + 1]] {
        fs::write(&path, bytes).unwrap();
        assert!(!repository.doctor().unwrap().valid);
        assert!(repository.export_snapshot().is_err());
    }
}

#[test]
fn interrupted_wiki_publication_recovers_once_without_blind_overwrite() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let request = RequestId::new();
    let write = input("# Interrupted\n", None);
    let error = repository
        .write_wiki_with_faults(&write, &request, |point| {
            if point == FaultPoint::AfterChange(0) {
                Err(PmError::new(ErrorCode::Io, "injected interruption"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    assert_eq!(
        repository.wiki_document(&write.path).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    let recovered = TransactionStore::open(repository.root())
        .unwrap()
        .recover()
        .unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        repository.write_wiki(&write, &request).unwrap(),
        recovered[0]
    );
    assert_eq!(
        repository.wiki_document(&write.path).unwrap().body,
        write.body
    );
    assert!(repository.doctor().unwrap().valid);
}

#[test]
fn concurrent_same_request_publishes_one_document_and_receipt() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let request = RequestId::new();
    let receipts = std::thread::scope(|scope| {
        let left = scope.spawn(|| repository.write_wiki(&input("concurrent", None), &request));
        let right = scope.spawn(|| repository.write_wiki(&input("concurrent", None), &request));
        (
            left.join().unwrap().unwrap(),
            right.join().unwrap().unwrap(),
        )
    });
    assert_eq!(receipts.0, receipts.1);
    assert_eq!(repository.wiki_documents().unwrap().len(), 1);
    assert_eq!(repository.operation_history().unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn linked_wiki_content_and_directory_collisions_are_rejected() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    fs::create_dir_all(repository.root().join("wiki/architecture/overview.md")).unwrap();
    assert!(
        repository
            .write_wiki(&input("body", None), &RequestId::new())
            .is_err()
    );
    fs::remove_dir(repository.root().join("wiki/architecture/overview.md")).unwrap();
    let outside = temp.path().join("outside.md");
    fs::write(&outside, "external").unwrap();
    symlink(
        &outside,
        repository.root().join("wiki/architecture/overview.md"),
    )
    .unwrap();
    assert_eq!(
        repository
            .wiki_document("architecture/overview.md")
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    assert!(
        repository
            .write_wiki(&input("body", None), &RequestId::new())
            .is_err()
    );
    assert_eq!(fs::read_to_string(outside).unwrap(), "external");
}

#[test]
fn forged_wiki_receipt_body_cannot_be_replayed_or_exported() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let write = input("original", None);
    let request = RequestId::new();
    let receipt = repository.write_wiki(&write, &request).unwrap();
    let mut forged = receipt.clone();
    forged.result["body"] = serde_json::json!("forged body");
    fs::write(
        repository
            .root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&forged).unwrap(),
    )
    .unwrap();
    assert_eq!(
        repository.write_wiki(&write, &request).unwrap_err().code,
        ErrorCode::CorruptStore
    );
    assert!(repository.export_snapshot().is_err());
    assert_eq!(
        repository.wiki_document(&write.path).unwrap().body,
        "original"
    );
}

#[test]
fn self_consistent_wiki_result_forgery_cannot_change_the_original_intent_or_publication_set() {
    use workdeck_pm::transactions::MutationReceipt;
    for alteration in [
        "body_and_hash",
        "redirect_path",
        "invent_before",
        "pretend_noop",
        "extra_publication",
    ] {
        let temp = TempDir::new().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let write = input("Original wiki content\r\n", None);
        let request = RequestId::new();
        let original = repository.write_wiki(&write, &request).unwrap();
        let mut forged: MutationReceipt = original.clone();
        match alteration {
            "body_and_hash" => {
                let body = "Different claimed publication";
                let hash = ContentHash::of(body.as_bytes());
                forged.result["body"] = serde_json::json!(body);
                forged.result["content_hash"] = serde_json::json!(hash);
                forged.changed[0].after = Some(hash);
            }
            "redirect_path" => {
                forged.result["path"] = serde_json::json!("wiki/different.md");
                forged.changed[0].path = "wiki/different.md".into();
            }
            "invent_before" => {
                forged.changed[0].before = Some(ContentHash::of(b"invented old document"))
            }
            "pretend_noop" => forged.changed.clear(),
            "extra_publication" => {
                let mut extra = forged.changed[0].clone();
                extra.path = "wiki/additional.md".into();
                forged.changed.push(extra);
            }
            _ => unreachable!(),
        }
        let path = repository
            .root()
            .join(format!("operations/{}.yml", original.operation_id));
        let bytes = serde_yaml_ng::to_string(&forged).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            repository.write_wiki(&write, &request).unwrap_err().code,
            ErrorCode::CorruptStore,
            "{alteration}"
        );
        assert_eq!(
            repository.operation_history().unwrap_err().code,
            ErrorCode::CorruptStore,
            "events: {alteration}"
        );
        assert_eq!(
            repository.export_snapshot().unwrap_err().code,
            ErrorCode::CorruptStore,
            "snapshot: {alteration}"
        );
        #[cfg(unix)]
        assert_eq!(
            repository.stage_operation(&forged).unwrap_err().code,
            ErrorCode::CorruptStore,
            "staging: {alteration}"
        );
        assert_eq!(
            repository.wiki_document(&write.path).unwrap().body,
            write.body,
            "{alteration}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
        assert_eq!(
            fs::read_dir(repository.root().join("operations"))
                .unwrap()
                .count(),
            1
        );
        assert!(!repository.root().join("wiki/different.md").exists());
        assert!(!repository.root().join("wiki/additional.md").exists());
        fs::write(path, serde_yaml_ng::to_string(&original).unwrap()).unwrap();
        assert_eq!(repository.write_wiki(&write, &request).unwrap(), original);
    }
}

#[test]
fn unchanged_write_receipts_remain_historical_after_later_edits_and_snapshot_restoration() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let first = repository
        .write_wiki(&input("First body", None), &RequestId::new())
        .unwrap();
    let first_record: WikiDocument = serde_json::from_value(first.result).unwrap();
    let no_change = input("First body", Some(first_record.content_hash.clone()));
    let request = RequestId::new();
    let original = repository.write_wiki(&no_change, &request).unwrap();
    assert!(original.changed.is_empty());
    repository
        .write_wiki(
            &input("Later body", Some(first_record.content_hash)),
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(
        repository.write_wiki(&no_change, &request).unwrap(),
        original
    );
    assert_eq!(
        repository.wiki_document(&no_change.path).unwrap().body,
        "Later body"
    );
    assert_eq!(
        repository
            .write_wiki(&no_change, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert!(repository.operation_history().unwrap().contains(&original));
    let snapshot = repository.export_snapshot().unwrap();
    snapshot.validate().unwrap();
    let destination = TempDir::new().unwrap();
    let root = destination.path().join(".workdeck");
    restore_snapshot(&root, &snapshot, None, &RequestId::new()).unwrap();
    let restored = Repository::open_source(&root).unwrap();
    assert_eq!(restored.write_wiki(&no_change, &request).unwrap(), original);
    assert_eq!(
        restored.wiki_document(&no_change.path).unwrap().body,
        "Later body"
    );
    assert!(restored.operation_history().unwrap().contains(&original));
}

#[test]
fn noop_result_forgery_is_rejected_even_when_its_body_and_hash_agree() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    repository
        .write_wiki(&input("First", None), &RequestId::new())
        .unwrap();
    let write = input("First", Some(ContentHash::of(b"First")));
    let request = RequestId::new();
    let original = repository.write_wiki(&write, &request).unwrap();
    assert!(original.changed.is_empty());
    let mut forged = original.clone();
    forged.result["body"] = serde_json::json!("Forged");
    forged.result["content_hash"] = serde_json::json!(ContentHash::of(b"Forged"));
    fs::write(
        repository
            .root()
            .join(format!("operations/{}.yml", original.operation_id)),
        serde_yaml_ng::to_string(&forged).unwrap(),
    )
    .unwrap();
    assert_eq!(
        repository.write_wiki(&write, &request).unwrap_err().code,
        ErrorCode::CorruptStore
    );
    assert_eq!(
        repository.operation_history().unwrap_err().code,
        ErrorCode::CorruptStore
    );
    assert_eq!(
        repository.export_snapshot().unwrap_err().code,
        ErrorCode::CorruptStore
    );
    assert_eq!(repository.wiki_document(&write.path).unwrap().body, "First");
}

#[test]
fn update_replay_keeps_the_original_before_hash_after_later_edits() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    repository
        .write_wiki(&input("First", None), &RequestId::new())
        .unwrap();
    let write = input("Second", Some(ContentHash::of(b"First")));
    let request = RequestId::new();
    let original = repository.write_wiki(&write, &request).unwrap();
    assert_eq!(original.changed[0].before, write.expected);
    repository
        .write_wiki(
            &input("Third", Some(ContentHash::of(b"Second"))),
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(repository.write_wiki(&write, &request).unwrap(), original);
    assert_eq!(repository.wiki_document(&write.path).unwrap().body, "Third");
    assert_eq!(
        repository
            .write_wiki(&input("Second", Some(ContentHash::of(b"Third"))), &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert!(repository.operation_history().unwrap().contains(&original));
    repository.export_snapshot().unwrap().validate().unwrap();
}
