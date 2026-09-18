use serde_json::json;
use workdeck_pm::*;
fn input() -> WriteSavedView {
    WriteSavedView {
        id: "ready".into(),
        name: "Ready work".into(),
        query: IssueQuery::default(),
        archived: false,
        expected: None,
    }
}
#[test]
fn saved_predicates_evaluate_live_membership_and_preserve_source_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let input = input();
    let request = RequestId::new();
    let receipt = repo.write_saved_view(&input, &request).unwrap();
    assert!(repo.query_saved_view("ready").unwrap().issues.is_empty());
    repo.create_issue(&CreateIssue::new("New work", ""), &RequestId::new())
        .unwrap();
    let result = repo.query_saved_view("ready").unwrap();
    assert_eq!(result.issues, repo.query_issues(&input.query).unwrap());
    let path = repo.root().join(&result.view.path);
    let original = std::fs::read_to_string(&path).unwrap();
    let edited = original.replace("custom: {}", "custom:\n  untouched: [one, two]")
        + "\nx-tool: useful # retain this comment\n";
    std::fs::write(&path, &edited).unwrap();
    let mut update = input.clone();
    update.expected = Some(result.view.content);
    assert_eq!(
        repo.write_saved_view(&update, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    update.expected = Some(repo.saved_view("ready").unwrap().content);
    update.name = "Renamed".into();
    update.archived = true;
    repo.write_saved_view(&update, &RequestId::new()).unwrap();
    let saved = repo.saved_view("ready").unwrap();
    assert_eq!(saved.document, std::fs::read_to_string(&path).unwrap());
    assert_eq!(saved.content, ContentHash::of(saved.document.as_bytes()));
    assert_eq!(saved.definition.custom["untouched"], json!(["one", "two"]));
    assert_eq!(saved.definition.extra["x-tool"], json!("useful"));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("# retain this comment")
    );
    assert_eq!(repo.write_saved_view(&input, &request).unwrap(), receipt);
    assert_eq!(
        repo.query_saved_view("ready").unwrap().issues.len(),
        1,
        "archiving a definition does not rewrite its predicate"
    );
    assert!(repo.doctor().unwrap().valid);
    let snapshot = repo.export_snapshot().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let root = destination.path().join(".workdeck");
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new()).unwrap();
    let restored = Repository::discover(destination.path()).unwrap();
    assert_eq!(restored.saved_view("ready").unwrap(), saved);
    assert_eq!(restored.query_saved_view("ready").unwrap().issues.len(), 1);
}
#[test]
fn invalid_saved_queries_and_paths_cannot_publish() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let before = repo.export_snapshot().unwrap();
    for id in ["../escape", "UPPER", "", "nested/view"] {
        let mut bad = input();
        bad.id = id.into();
        assert!(repo.write_saved_view(&bad, &RequestId::new()).is_err());
    }
    let mut bad = input();
    bad.query.status = Some("not-a-status".into());
    assert!(repo.write_saved_view(&bad, &RequestId::new()).is_err());
    bad = input();
    bad.query.targets = vec!["same".into(), "same".into()];
    assert!(repo.write_saved_view(&bad, &RequestId::new()).is_err());
    assert_eq!(repo.export_snapshot().unwrap().files, before.files);
    assert!(repo.saved_views().unwrap().is_empty());
}

#[test]
fn saved_view_creation_serializes_and_interrupted_update_recovers_once() {
    use std::sync::{Arc, Barrier};
    use workdeck_pm::transactions::{FaultPoint, TransactionStore};
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads = (0..2)
        .map(|_| {
            let repo = repo.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                repo.write_saved_view(&input(), &RequestId::new())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .next()
            .unwrap()
            .code,
        ErrorCode::Conflict
    );
    let mut update = input();
    update.name = "Recovered view".into();
    update.expected = Some(repo.saved_view("ready").unwrap().content);
    let request = RequestId::new();
    let result = repo.write_saved_view_with_faults(&update, &request, |point| {
        if point == FaultPoint::AfterJournal {
            Err(PmError::new(ErrorCode::Io, "interrupted"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    TransactionStore::open(repo.root())
        .unwrap()
        .recover()
        .unwrap();
    let receipt = repo.write_saved_view(&update, &request).unwrap();
    assert_eq!(
        repo.saved_view("ready").unwrap().definition.name,
        "Recovered view"
    );
    assert_eq!(repo.write_saved_view(&update, &request).unwrap(), receipt);
    assert_eq!(repo.saved_views().unwrap().len(), 1);
}

#[test]
fn forged_saved_view_receipts_cannot_cross_replay_event_snapshot_or_staging_boundaries() {
    for alteration in [
        "name",
        "custom",
        "coherent_source",
        "repository",
        "missing_document",
        "oversized_document",
        "path",
        "before",
        "extra_publication",
        "pretend_noop",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path(), "WD").unwrap();
        let input = input();
        let request = RequestId::new();
        let original = repo.write_saved_view(&input, &request).unwrap();
        let mut forged = original.clone();
        match alteration {
            "name" => forged.result["definition"]["name"] = json!("Forged name"),
            "custom" => forged.result["definition"]["custom"]["forged"] = json!(true),
            "coherent_source" => {
                forged.result["definition"]["name"] = json!("Coherent forged name");
                let document = serde_yaml_ng::to_string(&forged.result["definition"]).unwrap();
                let hash = ContentHash::of(document.as_bytes());
                forged.result["document"] = json!(document);
                forged.result["content"] = json!(hash);
                forged.changed[0].after = Some(hash);
            }
            "repository" => {
                forged.result["definition"]["repository"] = json!(RepositoryId::new());
                let document = serde_yaml_ng::to_string(&forged.result["definition"]).unwrap();
                let hash = ContentHash::of(document.as_bytes());
                forged.result["document"] = json!(document);
                forged.result["content"] = json!(hash);
                forged.changed[0].after = Some(hash);
            }
            "missing_document" => {
                forged.result.as_object_mut().unwrap().remove("document");
            }
            "oversized_document" => {
                let document = format!(
                    "{}\n# {}",
                    forged.result["document"].as_str().unwrap(),
                    "x".repeat(64 * 1024)
                );
                let hash = ContentHash::of(document.as_bytes());
                forged.result["document"] = json!(document);
                forged.result["content"] = json!(hash);
                forged.changed[0].after = Some(hash);
            }
            "path" => {
                forged.result["path"] = json!("views/different.yml");
                forged.changed[0].path = "views/different.yml".into();
            }
            "before" => forged.changed[0].before = Some(ContentHash::of(b"invented before")),
            "extra_publication" => {
                let mut extra = forged.changed[0].clone();
                extra.path = "views/additional.yml".into();
                forged.changed.push(extra);
            }
            "pretend_noop" => forged.changed.clear(),
            _ => unreachable!(),
        }
        let path = repo
            .root()
            .join(format!("operations/{}.yml", original.operation_id));
        let bytes = serde_yaml_ng::to_string(&forged).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            repo.write_saved_view(&input, &request).unwrap_err().code,
            ErrorCode::CorruptStore,
            "replay: {alteration}"
        );
        assert_eq!(
            repo.operation_history().unwrap_err().code,
            ErrorCode::CorruptStore,
            "events: {alteration}"
        );
        assert_eq!(
            repo.export_snapshot().unwrap_err().code,
            ErrorCode::CorruptStore,
            "snapshot: {alteration}"
        );
        #[cfg(unix)]
        assert_eq!(
            repo.stage_operation(&forged).unwrap_err().code,
            ErrorCode::CorruptStore,
            "staging: {alteration}"
        );
        assert_eq!(
            repo.saved_view("ready").unwrap().definition.name,
            input.name
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::read_dir(repo.root().join("operations"))
                .unwrap()
                .count(),
            1
        );
        assert!(!repo.root().join("views/different.yml").exists());
        assert!(!repo.root().join("views/additional.yml").exists());
        std::fs::write(&path, serde_yaml_ng::to_string(&original).unwrap()).unwrap();
        assert_eq!(repo.write_saved_view(&input, &request).unwrap(), original);
    }
}

#[test]
fn saved_view_noop_and_update_proofs_survive_later_edits_and_restoration() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    repo.write_saved_view(&input(), &RequestId::new()).unwrap();
    let mut noop = input();
    noop.expected = Some(repo.saved_view("ready").unwrap().content);
    let noop_request = RequestId::new();
    let no_change = repo.write_saved_view(&noop, &noop_request).unwrap();
    assert!(no_change.changed.is_empty());
    let mut update = noop.clone();
    update.name = "Updated view".into();
    let update_request = RequestId::new();
    let updated = repo.write_saved_view(&update, &update_request).unwrap();
    let mut later = update.clone();
    later.name = "Latest view".into();
    later.expected = Some(repo.saved_view("ready").unwrap().content);
    repo.write_saved_view(&later, &RequestId::new()).unwrap();
    assert_eq!(
        repo.write_saved_view(&noop, &noop_request).unwrap(),
        no_change
    );
    assert_eq!(
        repo.write_saved_view(&update, &update_request).unwrap(),
        updated
    );
    assert_eq!(
        repo.write_saved_view(&noop, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let mut changed_intent = update.clone();
    changed_intent.expected = later.expected.clone();
    assert_eq!(
        repo.write_saved_view(&changed_intent, &update_request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(
        repo.saved_view("ready").unwrap().definition.name,
        "Latest view"
    );
    let snapshot = repo.export_snapshot().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let root = destination.path().join(".workdeck");
    restore_snapshot(&root, &snapshot, None, &RequestId::new()).unwrap();
    let restored = Repository::open_source(&root).unwrap();
    assert_eq!(
        restored.write_saved_view(&noop, &noop_request).unwrap(),
        no_change
    );
    assert_eq!(
        restored.write_saved_view(&update, &update_request).unwrap(),
        updated
    );
    assert_eq!(
        restored.saved_view("ready").unwrap().definition.name,
        "Latest view"
    );
    assert!(restored.operation_history().unwrap().contains(&updated));
}

#[test]
fn historical_unavailable_view_status_is_warned_and_restored_without_rewriting() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let request = RequestId::new();
    let original = repo.write_saved_view(&input(), &request).unwrap();
    let mut record = repo.saved_view("ready").unwrap();
    // A retained predicate may outlive the workflow status it refers to.
    record.definition.query.status = Some("removed-workflow-status".into());
    let bytes = serde_yaml_ng::to_string(&record.definition).unwrap();
    std::fs::write(repo.root().join(&record.path), &bytes).unwrap();
    let report = repo.doctor().unwrap();
    assert!(report.valid);
    assert!(report.warnings.iter().any(|warning| {
        warning.message.contains("removed-workflow-status")
            && warning
                .path
                .as_ref()
                .is_some_and(|path| std::path::Path::new(path).ends_with(&record.path))
    }));
    assert_eq!(
        repo.query_saved_view("ready").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(repo.write_saved_view(&input(), &request).unwrap(), original);
    let snapshot = repo.export_snapshot().unwrap();
    snapshot.validate().unwrap();
    assert!(
        repo.preview_snapshot_import(&snapshot, SnapshotImportMode::Merge)
            .unwrap()
            .allowed,
        "unchanged historical predicates remain valid import input"
    );
    let destination = tempfile::tempdir().unwrap();
    let root = destination.path().join(".workdeck");
    restore_snapshot(&root, &snapshot, None, &RequestId::new()).unwrap();
    let restored = Repository::open_source(&root).unwrap();
    assert!(restored.doctor().unwrap().valid);
    assert!(!restored.doctor().unwrap().warnings.is_empty());
    assert_eq!(
        std::fs::read_to_string(root.join(&record.path)).unwrap(),
        bytes
    );
    assert_eq!(
        restored.query_saved_view("ready").unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn ordinary_import_cannot_author_new_or_changed_unavailable_view_statuses() {
    for existing in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path(), "WD").unwrap();
        repo.write_saved_view(&input(), &RequestId::new()).unwrap();
        let baseline = repo.export_snapshot().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let root = destination.path().join(".workdeck");
        restore_snapshot(&root, &baseline, None, &RequestId::new()).unwrap();
        let target = Repository::open_source(&root).unwrap();
        let before = target.export_snapshot().unwrap().files;
        let mut definition = repo.saved_view("ready").unwrap().definition;
        if !existing {
            definition.id = "new-view".into();
        }
        definition.query.status = Some("missing-status".into());
        let relative = format!("views/{}.yml", definition.id);
        std::fs::write(
            repo.root().join(&relative),
            serde_yaml_ng::to_string(&definition).unwrap(),
        )
        .unwrap();
        let snapshot = repo.export_snapshot().unwrap();
        let mode = if existing {
            SnapshotImportMode::ReplaceMatching
        } else {
            SnapshotImportMode::Merge
        };
        let plan = target.preview_snapshot_import(&snapshot, mode).unwrap();
        assert!(
            !plan.allowed,
            "new or changed unavailable query must match ordinary authoring policy"
        );
        assert!(
            plan.blockers
                .iter()
                .any(|error| error.message.contains("missing-status"))
        );
        assert!(
            target
                .import_snapshot(&snapshot, mode, Some(&plan.fingerprint), &RequestId::new())
                .is_err()
        );
        assert_eq!(target.export_snapshot().unwrap().files, before);
    }
}
