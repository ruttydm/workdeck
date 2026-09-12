use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    sync::{Arc, Barrier},
    thread,
};
use workdeck_pm::*;

fn request(value: &str) -> RequestId {
    value.parse().unwrap()
}
fn repository() -> (tempfile::TempDir, Repository) {
    let root = tempfile::tempdir().unwrap();
    let repository = Repository::init(root.path(), "WD").unwrap();
    (root, repository)
}
fn create(repository: &Repository) -> transactions::MutationReceipt {
    repository
        .create_recorded_session(
            &NewRecordedSession {
                id: Some("session-a".into()),
                title: "Recorded work".into(),
                fields: BTreeMap::new(),
            },
            &request("create-session"),
        )
        .unwrap()
}

#[test]
fn generated_session_ids_and_annotations_replay_after_later_mutations() {
    let (_root, repository) = repository();
    let input = NewRecordedSession {
        id: None,
        title: "Generated".into(),
        fields: BTreeMap::new(),
    };
    let first = repository
        .create_recorded_session(&input, &request("generated"))
        .unwrap();
    let id = first.result["session"]["id"].as_str().unwrap();
    let mutation = SessionMutation::Append {
        field: SessionCollection::Notes,
        text: "Retain this".into(),
    };
    let appended = repository
        .mutate_recorded_session(id, None, &mutation, &request("append"))
        .unwrap();
    repository
        .mutate_recorded_session(
            id,
            None,
            &SessionMutation::Finish { summary: None },
            &request("finish"),
        )
        .unwrap();
    assert_eq!(
        repository
            .create_recorded_session(&input, &request("generated"))
            .unwrap(),
        first
    );
    assert_eq!(
        repository
            .mutate_recorded_session(id, None, &mutation, &request("append"))
            .unwrap(),
        appended
    );
    assert_eq!(
        repository
            .recorded_session(id)
            .unwrap()
            .session
            .handoff_notes,
        vec!["Retain this"]
    );
}

#[test]
fn direct_editor_changes_are_guarded_and_unrelated_toml_bytes_survive_updates() {
    let (_root, repository) = repository();
    create(&repository);
    let first = repository.recorded_session("session-a").unwrap();
    let path = repository.root().join(&first.path);
    fs::write(&path,"# Producer metadata\nid = 'session-a'\ntitle = 'Before' # keep title comment\ncustom = { phase = 'design' }\n").unwrap();
    let mutation = SessionMutation::Update {
        fields: BTreeMap::from([("title".into(), json!("After"))]),
    };
    assert_eq!(
        repository
            .mutate_recorded_session(
                "session-a",
                Some(&first.source),
                &mutation,
                &request("stale")
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    repository
        .mutate_recorded_session("session-a", None, &mutation, &request("update"))
        .unwrap();
    let bytes = fs::read_to_string(&path).unwrap();
    assert!(bytes.starts_with("# Producer metadata\nid = 'session-a'\n"));
    assert!(bytes.contains("# keep title comment"));
    assert!(bytes.contains("custom = { phase = 'design' }\n"));
}

#[test]
fn concurrent_annotations_serialize_without_lost_notes() {
    let (_root, repository) = repository();
    create(&repository);
    let barrier = Arc::new(Barrier::new(2));
    let jobs = (0..2)
        .map(|number| {
            let root = repository.root().to_owned();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let repository = Repository::open_source(&root).unwrap();
                barrier.wait();
                repository
                    .mutate_recorded_session(
                        "session-a",
                        None,
                        &SessionMutation::Append {
                            field: SessionCollection::Notes,
                            text: format!("Note {number}"),
                        },
                        &request(&format!("append-{number}")),
                    )
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for job in jobs {
        job.join().unwrap();
    }
    let mut notes = repository
        .recorded_session("session-a")
        .unwrap()
        .session
        .handoff_notes;
    notes.sort();
    assert_eq!(notes, vec!["Note 0", "Note 1"]);
}

#[test]
fn forged_or_orphan_session_retirement_is_a_doctor_error() {
    let (_root, repository) = repository();
    create(&repository);
    repository
        .mutate_recorded_session(
            "session-a",
            None,
            &SessionMutation::Delete,
            &request("retire"),
        )
        .unwrap();
    assert!(repository.doctor().unwrap().valid);
    let marker = repository
        .root()
        .join("imported-history/deleted-sessions/session-a.yml");
    let original = fs::read_to_string(&marker).unwrap();
    fs::write(
        &marker,
        original.replace("request_id: retire", "request_id: fabricated"),
    )
    .unwrap();
    assert!(
        repository.recorded_session("session-a").is_err(),
        "forged retirement proof was accepted"
    );
    assert!(!repository.doctor().unwrap().valid);
    fs::write(&marker, original).unwrap();
    fs::remove_file(repository.root().join("imported-sessions/session-a.toml")).unwrap();
    assert!(
        !repository.doctor().unwrap().valid,
        "orphan retirement was ignored"
    );
}

#[test]
fn doctor_reports_bad_history_without_rewriting_it() {
    let (_root, repository) = repository();
    fs::create_dir_all(repository.root().join("imported-sessions")).unwrap();
    let path = repository.root().join("imported-sessions/broken.toml");
    fs::write(&path, "id = 'broken'\ntitle = []\n").unwrap();
    let before = fs::read(&path).unwrap();
    let report = repository.doctor().unwrap();
    assert!(!report.valid, "doctor omitted supported historical records");
    assert!(report.errors.iter().any(|error| {
        error
            .path
            .as_ref()
            .is_some_and(|path| path.contains("broken.toml"))
    }));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn removed_session_files_do_not_release_case_insensitive_id_reservations() {
    let (_root, repository) = repository();
    create(&repository);
    fs::remove_file(repository.root().join("imported-sessions/session-a.toml")).unwrap();
    let result = repository.create_recorded_session(
        &NewRecordedSession {
            id: Some("SESSION-A".into()),
            title: "Alias reuse".into(),
            fields: BTreeMap::new(),
        },
        &request("reuse-case-alias"),
    );
    assert!(
        result.is_err(),
        "case alias reused an identity reserved by an operation"
    );
    assert!(
        !repository
            .root()
            .join("imported-sessions/SESSION-A.toml")
            .exists()
    );
}

#[test]
fn removing_retirement_marker_cannot_make_a_session_writable_again() {
    let (_root, repository) = repository();
    let original = create(&repository);
    repository
        .mutate_recorded_session(
            "session-a",
            None,
            &SessionMutation::Delete,
            &request("retire"),
        )
        .unwrap();
    fs::remove_file(
        repository
            .root()
            .join("imported-history/deleted-sessions/session-a.yml"),
    )
    .unwrap();
    assert!(
        repository.recorded_session("session-a").is_err(),
        "missing retirement marker resurrected history"
    );
    assert!(!repository.doctor().unwrap().valid);
    assert!(
        repository
            .mutate_recorded_session(
                "session-a",
                None,
                &SessionMutation::Finish { summary: None },
                &request("finish-retired")
            )
            .is_err()
    );
    assert_eq!(
        repository
            .create_recorded_session(
                &NewRecordedSession {
                    id: Some("session-a".into()),
                    title: "Recorded work".into(),
                    fields: BTreeMap::new()
                },
                &request("create-session")
            )
            .unwrap(),
        original
    );
}

#[test]
fn forged_retirement_and_annotation_results_are_rejected_on_replay() {
    for delete in [false, true] {
        let (_root, repository) = repository();
        create(&repository);
        let mutation = if delete {
            SessionMutation::Delete
        } else {
            SessionMutation::Update {
                fields: BTreeMap::from([("title".into(), json!("Expected title"))]),
            }
        };
        let receipt = repository
            .mutate_recorded_session("session-a", None, &mutation, &request("mutate-once"))
            .unwrap();
        let path = repository
            .root()
            .join(format!("operations/{}.yml", receipt.operation_id));
        let mut forged = receipt;
        forged.result["session"]["title"] = json!("Forged title");
        fs::write(&path, serde_yaml_ng::to_string(&forged).unwrap()).unwrap();
        assert!(
            repository
                .mutate_recorded_session("session-a", None, &mutation, &request("mutate-once"))
                .is_err(),
            "forged returned result accepted (delete={delete})"
        );
    }
}

#[test]
fn appending_files_preserves_existing_nested_metadata_and_toml_comments() {
    let (_root, repository) = repository();
    create(&repository);
    let path = repository.root().join("imported-sessions/session-a.toml");
    fs::write(&path, "id = 'session-a'\ntitle = 'Recorded work'\ncustom = { published = 2026-09-09T12:00:00Z }\n\n[[touched_files]] # keep producer comment\npath = 'src/old.rs'\nchange_type = 'modified'\nproducer = { revision = 7, tags = ['keep'] }\n").unwrap();
    repository
        .mutate_recorded_session(
            "session-a",
            None,
            &SessionMutation::AddFile {
                path: "src/new.rs".into(),
                change_type: "added".into(),
            },
            &request("add-file"),
        )
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("producer = { revision = 7, tags = ['keep'] }"),
        "lost nested producer metadata: {text}"
    );
    assert!(
        text.contains("# keep producer comment"),
        "lost existing file comment: {text}"
    );
    assert!(text.contains("custom = { published = 2026-09-09T12:00:00Z }"));
}

#[test]
fn import_rejects_case_colliding_records_atomically() {
    let (_root, repository) = repository();
    let sessions: Vec<RecordedSession> = serde_json::from_value(json!([
        {"id":"One","title":"First"}, {"id":"one","title":"Case alias"}
    ]))
    .unwrap();
    assert!(
        repository
            .import_recorded_sessions(&sessions, &request("collision-import"))
            .is_err()
    );
    assert!(repository.recorded_sessions().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn unsafe_history_source_is_checked_before_replay_without_rechecking_record_content() {
    use std::os::unix::fs::symlink;
    let (_root, repository) = repository();
    let receipt = create(&repository);
    let sessions = repository.root().join("imported-sessions");
    let retained = repository.root().join("retained-for-test");
    fs::rename(&sessions, &retained).unwrap();
    symlink(&retained, &sessions).unwrap();
    let result = repository.create_recorded_session(
        &NewRecordedSession {
            id: Some("session-a".into()),
            title: "Recorded work".into(),
            fields: BTreeMap::new(),
        },
        &request("create-session"),
    );
    assert!(result.is_err(), "unsafe source was bypassed by replay");
    fs::remove_file(&sessions).unwrap();
    fs::rename(&retained, &sessions).unwrap();
    fs::write(
        sessions.join("session-a.toml"),
        "malformed historical target",
    )
    .unwrap();
    assert_eq!(
        repository
            .create_recorded_session(
                &NewRecordedSession {
                    id: Some("session-a".into()),
                    title: "Recorded work".into(),
                    fields: BTreeMap::new()
                },
                &request("create-session")
            )
            .unwrap(),
        receipt
    );
}

#[test]
fn nested_import_metadata_and_retirement_replay_survive_later_source_edits() {
    let (_root, repository) = repository();
    let input: RecordedSession = toml::from_str("id = 'imported'\ntitle = 'Incoming'\ncustom = { published = 2026-09-09T12:00:00Z }\n[[touched_files]]\npath = 'src/lib.rs'\nproducer = { revision = 3 }\n").unwrap();
    let imported = repository
        .import_recorded_sessions(std::slice::from_ref(&input), &request("nested-import"))
        .unwrap();
    assert_eq!(
        imported.result[0]["session"]["touched_files"][0]["producer"]["revision"],
        3
    );
    let retired = repository
        .mutate_recorded_session(
            "imported",
            None,
            &SessionMutation::Delete,
            &request("retire-import"),
        )
        .unwrap();
    assert!(retired.result.get("retirement").is_some());
    assert!(repository.recorded_session("imported").unwrap().retired);
    fs::write(
        repository.root().join("imported-sessions/imported.toml"),
        "malformed source after retirement",
    )
    .unwrap();
    assert_eq!(
        repository
            .mutate_recorded_session(
                "imported",
                None,
                &SessionMutation::Delete,
                &request("retire-import")
            )
            .unwrap(),
        retired
    );
    assert_eq!(
        repository
            .import_recorded_sessions(&[input], &request("nested-import"))
            .unwrap(),
        imported
    );
    assert!(repository.recorded_sessions().is_err());
}

#[test]
fn inline_file_and_annotation_array_comments_survive_append() {
    let (_root, repository) = repository();
    create(&repository);
    let path = repository.root().join("imported-sessions/session-a.toml");
    fs::write(&path, "id = 'session-a'\ntitle = 'Recorded work'\ntouched_files = [{ path = 'old', producer = { tag = 'keep' } }] # file note\nhandoff_notes = [\n  'original', # note comment\n]\n").unwrap();
    repository
        .mutate_recorded_session(
            "session-a",
            None,
            &SessionMutation::AddFile {
                path: "new".into(),
                change_type: "added".into(),
            },
            &request("inline-add"),
        )
        .unwrap();
    repository
        .mutate_recorded_session(
            "session-a",
            None,
            &SessionMutation::Append {
                field: SessionCollection::Notes,
                text: "next".into(),
            },
            &request("inline-note"),
        )
        .unwrap();
    let text = fs::read_to_string(path).unwrap();
    assert!(
        text.contains("{ path = 'old', producer = { tag = 'keep' } }"),
        "{text}"
    );
    assert!(text.contains("# file note"), "{text}");
    assert!(text.contains("'original', # note comment"), "{text}");
    assert_eq!(
        repository
            .recorded_session("session-a")
            .unwrap()
            .session
            .handoff_notes,
        ["original", "next"]
    );
}

#[test]
fn invalid_and_pathological_imports_publish_no_receipt_or_records() {
    let (_root, repository) = repository();
    let base: RecordedSession =
        serde_json::from_value(json!({"id":"one","title":"Title"})).unwrap();
    let before =
        fs::read_dir(repository.root().join("operations")).map_or(0, |items| items.count());
    let mut nonfinite = base.clone();
    nonfinite
        .extra
        .insert("nonfinite".into(), toml::Value::Float(f64::NAN));
    let mut deep = base.clone();
    let mut value = toml::Value::String("leaf".into());
    for _ in 0..70 {
        value = toml::Value::Array(vec![value]);
    }
    deep.extra.insert("deep".into(), value);
    for (number, input) in [vec![nonfinite], vec![deep], vec![base; 10_001]]
        .iter()
        .enumerate()
    {
        assert!(
            repository
                .import_recorded_sessions(input, &request(&format!("invalid-{number}")))
                .is_err()
        );
    }
    assert!(repository.recorded_sessions().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(repository.root().join("operations")).map_or(0, |items| items.count()),
        before
    );
}

#[test]
fn immutable_history_snapshot_validates_source_paths_identity_and_proofs_without_filesystem_access()
{
    let (_root, repository) = repository();
    let created = create(&repository);
    let mut files = BTreeMap::from([
        (
            "config.yml".into(),
            fs::read(repository.root().join("config.yml")).unwrap(),
        ),
        (
            "imported-sessions/session-a.toml".into(),
            fs::read(repository.root().join("imported-sessions/session-a.toml")).unwrap(),
        ),
        (
            format!("operations/{}.yml", created.operation_id).into(),
            serde_yaml_ng::to_string(&created).unwrap().into_bytes(),
        ),
    ]);
    let nonexistent = repository.root().join("does-not-exist");
    assert_eq!(
        validate_recorded_sessions_snapshot(&nonexistent, repository.identity(), &files)
            .unwrap()
            .len(),
        1
    );
    assert!(!nonexistent.exists());
    let other = RepositoryId::new();
    assert_eq!(
        validate_recorded_sessions_snapshot(&nonexistent, &other, &files)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let bytes = files
        .remove(std::path::Path::new("imported-sessions/session-a.toml"))
        .unwrap();
    files.insert("imported-sessions/./session-a.toml".into(), bytes);
    assert!(
        validate_recorded_sessions_snapshot(&nonexistent, repository.identity(), &files).is_err()
    );
}

#[test]
fn malformed_toml_reports_source_location_without_writing() {
    let (_root, repository) = repository();
    create(&repository);
    let path = repository.root().join("imported-sessions/session-a.toml");
    fs::write(&path, "id = 'session-a'\ntitle = [\n").unwrap();
    let before = fs::read(&path).unwrap();
    let error = repository.recorded_session("session-a").unwrap_err();
    assert!(error.path.as_ref().unwrap().contains("session-a.toml"));
    assert!(error.line.is_some());
    assert!(error.column.is_some());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn same_kind_receipt_replays_must_match_create_import_and_append_inputs() {
    for action in ["create", "import", "append"] {
        let (_root, repository) = repository();
        let input = NewRecordedSession {
            id: Some("one".into()),
            title: "Original".into(),
            fields: BTreeMap::new(),
        };
        let imported: RecordedSession =
            serde_json::from_value(json!({"id":"one","title":"Original"})).unwrap();
        let mutation = SessionMutation::Append {
            field: SessionCollection::Notes,
            text: "Original note".into(),
        };
        let operation = || match action {
            "create" => repository.create_recorded_session(&input, &request("action")),
            "import" => repository
                .import_recorded_sessions(std::slice::from_ref(&imported), &request("action")),
            _ => repository.mutate_recorded_session("one", None, &mutation, &request("action")),
        };
        if action == "append" {
            repository
                .create_recorded_session(&input, &request("create"))
                .unwrap();
        }
        let mut receipt = operation().unwrap();
        match action {
            "create" => receipt.result["session"]["title"] = json!("Forged"),
            "import" => receipt.result[0]["session"]["title"] = json!("Forged"),
            _ => receipt.result["session"]["handoff_notes"] = json!(["Forged"]),
        }
        fs::write(
            repository
                .root()
                .join(format!("operations/{}.yml", receipt.operation_id)),
            serde_yaml_ng::to_string(&receipt).unwrap(),
        )
        .unwrap();
        assert_eq!(
            operation().unwrap_err().code,
            ErrorCode::CorruptStore,
            "{action}"
        );
    }
}

#[test]
fn history_replay_bounds_empty_directory_traversal_before_receipt_lookup() {
    let (_root, repository) = repository();
    let receipt = create(&repository);
    let sessions = repository.root().join("imported-sessions");
    for index in 0..10_000 {
        fs::create_dir(sessions.join(format!("empty-{index}"))).unwrap();
    }
    let input = NewRecordedSession {
        id: Some("session-a".into()),
        title: "Recorded work".into(),
        fields: BTreeMap::new(),
    };
    let error = repository
        .create_recorded_session(&input, &request("create-session"))
        .expect_err("replay ignored unbounded empty directory traversal");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        fs::read_dir(repository.root().join("operations"))
            .unwrap()
            .count(),
        1
    );
    assert!(
        repository
            .root()
            .join(format!("operations/{}.yml", receipt.operation_id))
            .exists()
    );
}
