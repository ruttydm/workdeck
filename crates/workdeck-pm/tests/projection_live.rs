#![cfg(unix)]
use std::fs;
use workdeck_pm::{projection::*, *};

fn fixture() -> (tempfile::TempDir, Repository, ProjectionRowToken) {
    let temporary = tempfile::tempdir().unwrap();
    let repository = Repository::init(temporary.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Pinned issue", "Original body"),
            &RequestId::new(),
        )
        .unwrap();
    let token = token(temporary.path(), ProjectionQuery::default());
    (temporary, repository, token)
}
fn token(root: &std::path::Path, query: ProjectionQuery) -> ProjectionRowToken {
    let mut store = ProjectionStore::open(
        root,
        SourceSelector::WorkingTree,
        ProjectionLimits::default(),
    )
    .unwrap();
    let ProjectionRefresh::Published(view) =
        store.refresh(&ProjectionRefreshRequest::default()).unwrap()
    else {
        panic!("new generation required")
    };
    let query = view.query(&query).unwrap();
    view.page(&query, 0, 1).unwrap().rows.remove(0).token
}

#[test]
fn exact_native_record_reopens_but_editor_changes_and_ref_rows_are_rejected() {
    let (_temporary, repository, token) = fixture();
    let native = repository.issue_from_projection(&token).unwrap();
    assert_eq!(native.source.content, token.content);
    assert_eq!(native.metadata.title, "Pinned issue");
    let mut accepted = token.clone();
    accepted.view.source.role = SourceRole::Accepted;
    assert_eq!(
        repository
            .issue_from_projection(&accepted)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let mut wrong_path = token.clone();
    wrong_path.path = "../config.yml".into();
    assert_eq!(
        repository
            .issue_from_projection(&wrong_path)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    fs::write(
        repository.root().join(&token.path),
        repository.issue_markdown(&token.key.id).unwrap() + "\nEditor changed this\n",
    )
    .unwrap();
    assert_eq!(
        repository.issue_from_projection(&token).unwrap_err().code,
        ErrorCode::StaleSource
    );
}

#[test]
fn same_looking_id_and_even_copied_repository_identity_cannot_cross_checkout() {
    let (_temporary, repository, token) = fixture();
    let other = tempfile::tempdir().unwrap();
    let other_repo = Repository::init(other.path(), "WD").unwrap();
    let destination = other_repo.root().join(&token.path);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::copy(repository.root().join(&token.path), destination).unwrap();
    assert_eq!(
        other_repo.issue_from_projection(&token).unwrap_err().code,
        ErrorCode::StaleSource
    );
    fs::copy(
        repository.root().join("config.yml"),
        other_repo.root().join("config.yml"),
    )
    .unwrap();
    let same_identity = Repository::open_source(other_repo.root()).unwrap();
    assert_eq!(same_identity.identity(), repository.identity());
    assert_eq!(
        same_identity
            .issue_from_projection(&token)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repository
            .issue_from_projection(&token)
            .unwrap()
            .metadata
            .title,
        "Pinned issue"
    );
}

#[test]
fn same_directory_spelling_with_replaced_checkout_does_not_retarget_a_row() {
    let (temporary, repository, token) = fixture();
    let held = temporary.path().join("held-planning");
    fs::rename(repository.root(), &held).unwrap();
    let replacement = Repository::init(temporary.path(), "WD").unwrap();
    fs::copy(
        held.join("config.yml"),
        replacement.root().join("config.yml"),
    )
    .unwrap();
    let destination = replacement.root().join(&token.path);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::copy(held.join(&token.path), destination).unwrap();
    let reopened = Repository::open_source(replacement.root()).unwrap();
    assert_eq!(reopened.identity(), repository.identity());
    assert_eq!(
        reopened.issue_from_projection(&token).unwrap_err().code,
        ErrorCode::StaleSource
    );
}

#[test]
fn feature_citation_reopens_exact_native_states_and_rejects_stale_content() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = Repository::init(temporary.path(), "WD").unwrap();
    repository
        .create_feature(&CreateFeature::new("Native capability"), &RequestId::new())
        .unwrap();
    let token = token(
        temporary.path(),
        ProjectionQuery::Features {
            query: Default::default(),
        },
    );
    let native = repository.feature_from_projection(&token).unwrap();
    assert_eq!(native.source.content, token.content);
    assert_eq!(native.metadata.name, "Native capability");
    let mut mismatch = token.clone();
    mismatch.key.id = FeatureId::new().to_string();
    assert_eq!(
        repository
            .feature_from_projection(&mismatch)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    fs::write(
        repository.root().join(&token.path),
        native.document + "\nChanged\n",
    )
    .unwrap();
    assert_eq!(
        repository.feature_from_projection(&token).unwrap_err().code,
        ErrorCode::StaleSource
    );
}

#[test]
fn concurrent_cold_readers_publish_one_complete_ignore_policy() {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path(), "WD").unwrap();
    let start = std::sync::Arc::new(std::sync::Barrier::new(8));
    let readers = (0..8)
        .map(|_| {
            let root = directory.path().to_path_buf();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                ProjectionStore::open(
                    &root,
                    SourceSelector::WorkingTree,
                    ProjectionLimits::default(),
                )
                .unwrap()
            })
        })
        .collect::<Vec<_>>();
    for reader in readers {
        drop(reader.join().unwrap());
    }
    assert_eq!(
        fs::read(directory.path().join(".workdeck/.index/.gitignore")).unwrap(),
        b"*\n"
    );
    assert!(
        !fs::read_dir(directory.path().join(".workdeck/.index"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".ignore-"))
    );
}

#[test]
fn planning_citations_reopen_exact_records_and_reject_wrong_kind_path_and_changed_content() {
    for kind in [
        PlanningKind::Initiative,
        PlanningKind::Project,
        PlanningKind::Milestone,
        PlanningKind::Cycle,
        PlanningKind::Target,
        PlanningKind::Label,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        let mut input = CreatePlanning::new("Indexed planning");
        if kind == PlanningKind::Milestone {
            let mut owner = CreatePlanning::new("Owning project");
            owner.id = Some("owner".into());
            repository
                .create_planning(PlanningKind::Project, &owner, &RequestId::new())
                .unwrap();
            input
                .fields
                .insert("project".into(), serde_json::json!("owner"));
        }
        repository
            .create_planning(kind, &input, &RequestId::new())
            .unwrap();
        let row = token(
            directory.path(),
            ProjectionQuery::Planning {
                query: ProjectionPlanningQuery {
                    kind,
                    query: String::new(),
                    archive: ArchiveFilter::Active,
                    project: None,
                    target: None,
                },
            },
        );
        let native = repository.planning_from_projection(kind, &row).unwrap();
        assert_eq!(native.metadata.name, "Indexed planning");
        assert_eq!(native.source.content, row.content);
        let mut wrong = row.clone();
        wrong.path = "config.yml".into();
        assert_eq!(
            repository
                .planning_from_projection(kind, &wrong)
                .unwrap_err()
                .code,
            ErrorCode::StaleSource
        );
        let mut wrong = row.clone();
        wrong.key.kind = SnapshotKind::Issue;
        assert_eq!(
            repository
                .planning_from_projection(kind, &wrong)
                .unwrap_err()
                .code,
            ErrorCode::StaleSource
        );
        let mut wrong = row.clone();
        wrong.view.source.role = SourceRole::Accepted;
        assert_eq!(
            repository
                .planning_from_projection(kind, &wrong)
                .unwrap_err()
                .code,
            ErrorCode::StaleSource
        );
        let path = repository.root().join(&row.path);
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"\n# external change\n");
        fs::write(path, bytes).unwrap();
        assert_eq!(
            repository
                .planning_from_projection(kind, &row)
                .unwrap_err()
                .code,
            ErrorCode::StaleSource
        );
    }
}

#[test]
fn empty_label_registry_is_inventory_but_not_a_planning_label() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    fs::write(
        repository.root().join("labels.yml"),
        "schema: 1\nlabels: []\n",
    )
    .unwrap();
    let mut store = ProjectionStore::open(
        directory.path(),
        SourceSelector::WorkingTree,
        ProjectionLimits::default(),
    )
    .unwrap();
    let ProjectionRefresh::Published(view) =
        store.refresh(&ProjectionRefreshRequest::default()).unwrap()
    else {
        panic!("new view expected")
    };
    let query = ProjectionQuery::Planning {
        query: ProjectionPlanningQuery {
            kind: PlanningKind::Label,
            query: String::new(),
            archive: ArchiveFilter::All,
            project: None,
            target: None,
        },
    };
    assert_eq!(view.query(&query).unwrap().total, 0);
    let inventory = view
        .query(&ProjectionQuery::Records {
            family: Some(SnapshotKind::Labels),
            query: String::new(),
        })
        .unwrap();
    assert_eq!(
        inventory.total, 1,
        "empty registry remains inspectable as a source document"
    );
    repository
        .create_planning(
            PlanningKind::Label,
            &CreatePlanning::new("Real label"),
            &RequestId::new(),
        )
        .unwrap();
    let ProjectionRefresh::Published(updated) =
        store.refresh(&ProjectionRefreshRequest::default()).unwrap()
    else {
        panic!("updated view expected")
    };
    let handle = updated.query(&query).unwrap();
    assert_eq!(handle.total, 1);
    let row = updated.page(&handle, 0, 1).unwrap().rows.remove(0);
    assert_eq!(
        repository
            .planning_from_projection(PlanningKind::Label, &row.token)
            .unwrap()
            .metadata
            .name,
        "Real label"
    );
    assert_eq!(
        view.query(&query).unwrap().total,
        0,
        "old inspected generation remains immutable"
    );
}
