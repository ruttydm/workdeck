#![cfg(unix)]
use std::{fs, path::Path};
use workdeck_pm::projection::ProjectionQuery;
use workdeck_pm::*;

fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let receipt = repo
        .create_issue(
            &CreateIssue::new("Original", "projection body"),
            &RequestId::new(),
        )
        .unwrap();
    let issue = serde_json::from_value(receipt.result).unwrap();
    (temp, repo, issue)
}
fn store(root: &Path) -> ProjectionStore {
    ProjectionStore::open(
        root,
        SourceSelector::WorkingTree,
        ProjectionLimits::default(),
    )
    .unwrap()
}
fn publish(store: &mut ProjectionStore) -> Box<ProjectionReadView> {
    match store.refresh(&ProjectionRefreshRequest::default()).unwrap() {
        ProjectionRefresh::Published(view) => view,
        other => panic!("expected fresh publication: {other:?}"),
    }
}
fn copy(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn checkpoint_load_and_index_deletion_rebuild_equivalent_generation() {
    let (temp, _, _) = fixture();
    let first = publish(&mut store(temp.path()));
    let loaded = store(temp.path()).load().unwrap().unwrap();
    assert_eq!(first.id(), loaded.id());
    assert_eq!(first.counts(), loaded.counts());
    fs::remove_dir_all(temp.path().join(".workdeck/.index")).unwrap();
    let rebuilt = publish(&mut store(temp.path()));
    assert_eq!(first.id(), rebuilt.id());
}

#[test]
fn unchanged_local_refresh_reuses_source_without_republishing() {
    let (temp, _, _) = fixture();
    let mut index = store(temp.path());
    let first = publish(&mut index);
    match index.refresh(&ProjectionRefreshRequest::default()).unwrap() {
        ProjectionRefresh::Unchanged(id) => assert_eq!(&id, first.id()),
        other => panic!("expected unchanged refresh, got {other:?}"),
    }
    assert_eq!(index.status().state, ProjectionState::Current);
}

#[test]
fn retained_reader_survives_refresh_and_malformed_source_keeps_last_good() {
    let (temp, repo, issue) = fixture();
    let mut store = store(temp.path());
    let old = publish(&mut store);
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([(
                "title".into(),
                serde_json::json!("Changed"),
            )]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let next = publish(&mut store);
    assert_ne!(old.id().generation, next.id().generation);
    let query = projection::ProjectionQuery::default();
    let old_query = old.query(&query).unwrap();
    let next_query = next.query(&query).unwrap();
    assert_eq!(
        old.page(&old_query, 0, 1).unwrap().rows[0].title,
        "Original"
    );
    assert_eq!(
        next.page(&next_query, 0, 1).unwrap().rows[0].title,
        "Changed"
    );
    let old_id = old.id().clone();
    fs::write(repo.root().join(&issue.path), "broken source").unwrap();
    assert!(store.refresh(&ProjectionRefreshRequest::default()).is_err());
    assert_eq!(store.status().state, ProjectionState::Stale);
    assert_eq!(store.status().view.as_ref(), Some(next.id()));
    assert_eq!(old.id(), &old_id);
    assert_eq!(store.load().unwrap().unwrap().id(), next.id());
}

#[test]
fn copied_checkpoint_is_not_adopted_in_identical_content_checkout() {
    let (temp, _, _) = fixture();
    let original = publish(&mut store(temp.path()));
    let other = tempfile::tempdir().unwrap();
    copy(
        &temp.path().join(".workdeck"),
        &other.path().join(".workdeck"),
    );
    assert!(store(other.path()).load().unwrap().is_none());
    let rebuilt = publish(&mut store(other.path()));
    assert_eq!(rebuilt.id().source.content, original.id().source.content);
    assert_ne!(rebuilt.id().slot, original.id().slot);
}

#[test]
fn interruption_before_publication_preserves_checkpoint_and_reader() {
    let (temp, _, _) = fixture();
    let mut store = store(temp.path());
    let first = publish(&mut store);
    let result = store.refresh_with_faults(&ProjectionRefreshRequest { rebuild: true }, |point| {
        if point == ProjectionFaultPoint::BeforePublish {
            Err(PmError::new(ErrorCode::Io, "simulated interruption"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert_eq!(store.load().unwrap().unwrap().id(), first.id());
}

fn checkpoint(root: &Path, view: &ProjectionReadView) -> std::path::PathBuf {
    root.join(".workdeck/.index")
        .join(view.id().slot.as_str())
        .join("checkpoint.bin")
}

#[test]
fn malformed_checkpoint_rebuilds_and_sqlite_creates_no_sidecars() {
    let (temp, _, _) = fixture();
    let mut index = store(temp.path());
    let first = publish(&mut index);
    let path = checkpoint(temp.path(), &first);
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(index.load().unwrap().is_none());
    assert!(!index.status().diagnostics.is_empty());
    let rebuilt = publish(&mut index);
    assert_eq!(first.id(), rebuilt.id());
    let names = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert!(
        names
            .iter()
            .all(|name| matches!(name.as_str(), "checkpoint.bin" | "writer.lock")),
        "{names:?}"
    );
}

#[test]
fn cache_paths_reject_fifo_symlink_and_replaced_directory() {
    use std::os::unix::fs::symlink;
    let (temp, _, _) = fixture();
    let mut index = store(temp.path());
    let first = publish(&mut index);
    let path = checkpoint(temp.path(), &first);
    fs::remove_file(&path).unwrap();
    symlink(temp.path().join(".workdeck/config.yml"), &path).unwrap();
    assert_eq!(index.load().unwrap_err().code, ErrorCode::UnsafePath);
    fs::remove_file(&path).unwrap();
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let start = std::time::Instant::now();
    assert_eq!(index.load().unwrap_err().code, ErrorCode::UnsafePath);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    fs::remove_file(path).unwrap();
    let original = temp.path().join(".workdeck/.index");
    fs::rename(&original, temp.path().join(".workdeck/index-retained")).unwrap();
    fs::create_dir(&original).unwrap();
    assert_eq!(index.load().unwrap_err().code, ErrorCode::StaleSource);
}

#[test]
fn concurrent_refresh_cas_keeps_the_newer_generation() {
    let (temp, repo, issue) = fixture();
    let mut first = store(temp.path());
    let old = publish(&mut first);
    let mut second = store(temp.path());
    let mut winner = None;
    let result = first
        .refresh_with_faults(&ProjectionRefreshRequest { rebuild: true }, |point| {
            if point == ProjectionFaultPoint::BeforePublish {
                repo.update_issue(
                    issue.metadata.id.as_str(),
                    &issue.source,
                    &UpdateIssue {
                        fields: std::collections::BTreeMap::from([(
                            "title".into(),
                            serde_json::json!("Concurrent winner"),
                        )]),
                        body: None,
                    },
                    &RequestId::new(),
                )
                .unwrap();
                winner = Some(publish(&mut second));
            }
            Ok(())
        })
        .unwrap();
    let winner = winner.unwrap();
    match result {
        ProjectionRefresh::Superseded(id) => assert_eq!(&id, winner.id()),
        other => panic!("{other:?}"),
    }
    assert_ne!(old.id(), winner.id());
    assert_eq!(first.load().unwrap().unwrap().id(), winner.id());
}

#[test]
fn source_edit_during_projection_never_publishes_a_mixed_generation() {
    for changed_at in [
        ProjectionFaultPoint::AfterCapture,
        ProjectionFaultPoint::AfterProject,
        ProjectionFaultPoint::BeforePublish,
        ProjectionFaultPoint::AfterCheckpointWritten,
    ] {
        let (temp, repo, issue) = fixture();
        let mut index = store(temp.path());
        let first = publish(&mut index);
        let before = fs::read(checkpoint(temp.path(), &first)).unwrap();
        let result =
            index.refresh_with_faults(&ProjectionRefreshRequest { rebuild: true }, |point| {
                if point == changed_at {
                    repo.update_issue(
                        issue.metadata.id.as_str(),
                        &issue.source,
                        &UpdateIssue {
                            fields: std::collections::BTreeMap::from([(
                                "title".into(),
                                serde_json::json!("External editor"),
                            )]),
                            body: None,
                        },
                        &RequestId::new(),
                    )
                    .unwrap();
                }
                Ok(())
            });
        assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
        assert_eq!(fs::read(checkpoint(temp.path(), &first)).unwrap(), before);
    }
}

#[test]
fn panic_after_writing_candidate_cleans_it_and_preserves_last_good() {
    let (temp, _, _) = fixture();
    let mut index = store(temp.path());
    let first = publish(&mut index);
    let path = checkpoint(temp.path(), &first);
    let before = fs::read(&path).unwrap();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = index.refresh_with_faults(&ProjectionRefreshRequest { rebuild: true }, |point| {
            assert_ne!(
                point,
                ProjectionFaultPoint::AfterCheckpointWritten,
                "simulated worker panic"
            );
            Ok(())
        });
    }));
    assert!(caught.is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(fs::read_dir(path.parent().unwrap()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".candidate-")
    }));
    assert_eq!(index.load().unwrap().unwrap().id(), first.id());
}

#[test]
fn bounded_database_failure_never_publishes_an_incomplete_checkpoint() {
    let (temp, _, _) = fixture();
    let mut index = ProjectionStore::open(
        temp.path(),
        SourceSelector::WorkingTree,
        ProjectionLimits {
            max_database_bytes: 4096,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(index.refresh(&ProjectionRefreshRequest::default()).is_err());
    assert!(index.load().unwrap().is_none());
    assert_eq!(index.status().state, ProjectionState::Error);
}

#[test]
fn cached_inspection_never_creates_repairs_or_refreshes_projection_storage() {
    let (temp, repo, issue) = fixture();
    // Initialization creates this empty disposable directory. Remove it to
    // exercise a genuinely absent cache rather than assume it starts absent.
    fs::remove_dir(repo.root().join(".index")).unwrap();
    assert!(
        ProjectionStore::open_cached(
            temp.path(),
            SourceSelector::WorkingTree,
            ProjectionLimits::default()
        )
        .is_err()
    );
    assert!(!repo.root().join(".index").exists());
    let mut writer = store(temp.path());
    let original = publish(&mut writer);
    let mut reader = ProjectionStore::open_cached(
        temp.path(),
        SourceSelector::WorkingTree,
        ProjectionLimits::default(),
    )
    .unwrap();
    let loaded = reader.load().unwrap().unwrap();
    assert_eq!(loaded.id(), original.id());
    assert_eq!(reader.status().state, ProjectionState::Cached);
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([(
                "title".into(),
                serde_json::json!("Changed"),
            )]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        reader
            .refresh(&ProjectionRefreshRequest::default())
            .is_err()
    );
    let still_cached = reader.load().unwrap().unwrap();
    assert_eq!(still_cached.id(), original.id());
    let query = still_cached.query(&ProjectionQuery::default()).unwrap();
    assert_eq!(
        still_cached.page(&query, 0, 1).unwrap().rows[0].title,
        "Original"
    );
    let ignore = repo.root().join(".index/.gitignore");
    fs::remove_file(&ignore).unwrap();
    assert!(
        ProjectionStore::open_cached(
            temp.path(),
            SourceSelector::WorkingTree,
            ProjectionLimits::default()
        )
        .is_err()
    );
    assert!(
        !ignore.exists(),
        "cached inspection must not repair the ignore policy"
    );
}

#[test]
fn old_projection_schema_requires_rebuild_and_never_reports_empty_new_predicates() {
    let (temp, repo, issue) = fixture();
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([(
                "reviewer".into(),
                serde_json::json!("Ada"),
            )]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let first = publish(&mut store(temp.path()));
    let path = checkpoint(temp.path(), &first);
    let bytes = fs::read(&path).unwrap();
    let size = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let mut header: serde_json::Value = serde_json::from_slice(&bytes[16..16 + size]).unwrap();
    header["view"]["schema"] = serde_json::json!(5);
    let header = serde_json::to_vec(&header).unwrap();
    let mut old = bytes[..8].to_vec();
    old.extend_from_slice(&(header.len() as u64).to_le_bytes());
    old.extend_from_slice(&header);
    old.extend_from_slice(&bytes[16 + size..]);
    fs::write(&path, &old).unwrap();
    let mut cached = ProjectionStore::open_cached(
        temp.path(),
        SourceSelector::WorkingTree,
        ProjectionLimits::default(),
    )
    .unwrap();
    assert!(cached.load().unwrap().is_none());
    assert!(!cached.status().diagnostics.is_empty());
    assert_eq!(fs::read(&path).unwrap(), old);
    let rebuilt = publish(&mut store(temp.path()));
    let query = ProjectionQuery::Issues {
        query: IssueQuery {
            reviewer: Some("Ada".into()),
            ..Default::default()
        },
        group_by: None,
    };
    assert_eq!(rebuilt.query(&query).unwrap().total, 1);
    assert_eq!(rebuilt.id().schema, 6);
}
