use std::{fs, path::Path, process::Command};
use tempfile::{TempDir, tempdir};
use workdeck_cli::repository_panels::RepositoryPanels;
use workdeck_tui::workbench::*;

fn repository() -> TempDir {
    let root = tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    root
}
fn provider(root: &Path) -> RepositoryPanels {
    RepositoryPanels::new(root, None, 20).unwrap()
}
fn request(page: PanelPage, query: &str, directory: &str, limit: usize) -> PanelRequest {
    PanelRequest {
        page,
        query: query.into(),
        directory: directory.into(),
        limit,
    }
}

#[test]
fn file_page_browses_direct_children_filters_before_limit_and_respects_gitignore() {
    let root = repository();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("ignored")).unwrap();
    fs::write(root.path().join(".gitignore"), "ignored/\n").unwrap();
    fs::write(root.path().join("ignored/secret.txt"), "hidden").unwrap();
    fs::write(root.path().join("src/main.rs"), "fn panel_symbol() {}\n").unwrap();
    fs::write(root.path().join("top.txt"), "top").unwrap();
    let provider = provider(root.path());
    let all = provider
        .load(&request(PanelPage::Files, "", "", 100))
        .unwrap();
    assert!(
        all.entries
            .iter()
            .any(|row| matches!(&row.target, PanelTarget::Directory { path } if path == "src"))
    );
    assert!(
        !all.entries
            .iter()
            .any(|row| row.label == ".git" || row.label == "ignored" || row.label == "main.rs")
    );
    let selected = provider
        .load(&request(PanelPage::Files, "top", "", 1))
        .unwrap();
    assert_eq!(selected.entries.len(), 1);
    assert_eq!(selected.entries[0].label, "top.txt");
    assert!(!selected.truncated);
    let children = provider
        .load(&request(PanelPage::Files, "", "src", 100))
        .unwrap();
    assert_eq!(
        children.entries[0].target,
        PanelTarget::File {
            path: "src/main.rs".into(),
            line: None
        }
    );
    assert!(
        provider
            .preview(&children.entries[0].target)
            .unwrap()
            .body
            .contains("panel_symbol")
    );
}

#[test]
fn preview_is_bounded_reports_binary_and_rejects_escape_or_git_private_paths() {
    let root = repository();
    fs::write(root.path().join("large.txt"), vec![b'a'; 600 * 1024]).unwrap();
    fs::write(root.path().join("binary.dat"), [0, 1, 2, 3]).unwrap();
    let provider = provider(root.path());
    let preview = provider
        .preview(&PanelTarget::File {
            path: "large.txt".into(),
            line: None,
        })
        .unwrap();
    assert!(preview.truncated);
    assert_eq!(preview.body.len(), 512 * 1024);
    let binary = provider
        .preview(&PanelTarget::File {
            path: "binary.dat".into(),
            line: None,
        })
        .unwrap();
    assert!(binary.binary);
    assert!(binary.body.contains("4 bytes"));
    for path in [
        "../outside.txt",
        "/etc/passwd",
        "src/../outside.txt",
        ".git/config",
        "src\\main.rs",
    ] {
        assert!(
            provider
                .preview(&PanelTarget::File {
                    path: path.into(),
                    line: None
                })
                .is_err(),
            "accepted {path}"
        );
        assert!(
            provider
                .load(&request(PanelPage::Files, "", path, 100))
                .is_err(),
            "accepted directory {path}"
        );
    }
}

#[test]
fn native_search_keeps_custom_workflow_body_metadata_symbols_and_stage_identity() {
    let root = repository();
    fs::write(root.path().join("main.rs"), "fn panel_symbol() {}\n").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "main.rs"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    let repository = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let mut config = repository.config().unwrap();
    config.workflow.states[0].id = "triage-special".into();
    config.workflow.initial = "triage-special".into();
    for state in &mut config.workflow.states {
        for transition in &mut state.transitions {
            if transition == "inbox" {
                *transition = "triage-special".into();
            }
        }
    }
    // JSON is valid YAML and keeps this direct-editor fixture independent of
    // the application's document writer.
    fs::write(
        repository.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let mut input = workdeck_pm::CreateIssue::new("Native parser", "unique-body-evidence");
    input.fields.insert(
        "custom".into(),
        serde_json::json!({"origin":"unique-custom-evidence"}),
    );
    repository
        .create_issue(&input, &"search-create".parse().unwrap())
        .unwrap();
    let provider = provider(root.path());
    for query in [
        "unique-body-evidence",
        "unique-custom-evidence",
        "triage-special",
    ] {
        let result = provider
            .load(&request(PanelPage::Search, query, "", 100))
            .unwrap();
        assert!(
            result
                .entries
                .iter()
                .any(|entry| matches!(entry.target, PanelTarget::Issue { .. })),
            "{query}: {result:?}"
        );
    }
    let symbols = provider
        .load(&request(PanelPage::Search, "panel_symbol", "", 100))
        .unwrap();
    assert!(symbols.entries.iter().any(
        |entry| matches!(&entry.target,PanelTarget::File{path,line:Some(1)} if path=="main.rs")
    ));
    let files = provider
        .load(&request(PanelPage::Search, "main.rs", "", 100))
        .unwrap();
    assert!(
        files
            .entries
            .iter()
            .any(|entry| matches!(entry.target, PanelTarget::Change { staged: true, .. }))
    );
    let ids = files
        .entries
        .iter()
        .map(|entry| &entry.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), files.entries.len());
}

#[test]
fn recorded_sessions_are_visible_and_searchable_without_executing_annotations() {
    let root = repository();
    let repository = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    fs::create_dir(repository.root().join("imported-sessions")).unwrap();
    let record = "id='session-1'\ntitle='Historical implementation'\nagent='external-tool'\nstatus='running'\ncommands_run=['touch MUST_NOT_EXIST']\nsummary='historical-needle'\n";
    fs::write(
        repository.root().join("imported-sessions/session-1.toml"),
        record,
    )
    .unwrap();
    let provider = provider(root.path());
    let list = provider
        .load(&request(PanelPage::Agents, "", "", 100))
        .unwrap();
    assert_eq!(list.entries.len(), 1);
    assert!(list.summary.contains("historical"));
    let preview = provider.preview(&list.entries[0].target).unwrap();
    assert!(preview.body.contains("annotations"));
    assert!(preview.body.contains("touch MUST_NOT_EXIST"));
    let search = provider
        .load(&request(PanelPage::Search, "historical-needle", "", 100))
        .unwrap();
    assert!(
        search
            .entries
            .iter()
            .any(|entry| matches!(&entry.target,PanelTarget::AgentSession{id} if id=="session-1"))
    );
    assert!(!root.path().join("MUST_NOT_EXIST").exists());
    assert!(!repository.root().join("agents").exists());
}

#[test]
fn retired_native_history_disappears_from_agents_search_and_stale_targets() {
    let root = repository();
    let repository = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let request_id = |value: &str| value.parse::<workdeck_pm::RequestId>().unwrap();
    repository
        .create_recorded_session(
            &workdeck_pm::NewRecordedSession {
                id: Some("session-retired".into()),
                title: "retired-needle".into(),
                fields: Default::default(),
            },
            &request_id("provider-create"),
        )
        .unwrap();
    let provider = provider(root.path());
    let before = provider
        .load(&request(PanelPage::Agents, "", "", 100))
        .unwrap();
    assert_eq!(before.entries.len(), 1);
    let target = before.entries[0].target.clone();
    repository
        .mutate_recorded_session(
            "session-retired",
            None,
            &workdeck_pm::SessionMutation::Delete,
            &request_id("provider-retire"),
        )
        .unwrap();
    assert!(
        repository
            .root()
            .join("imported-sessions/session-retired.toml")
            .exists()
    );
    assert!(
        provider
            .load(&request(PanelPage::Agents, "", "", 100))
            .unwrap()
            .entries
            .is_empty()
    );
    let search = provider
        .load(&request(PanelPage::Search, "retired-needle", "", 100))
        .unwrap();
    assert!(
        !search
            .entries
            .iter()
            .any(|entry| matches!(entry.target, PanelTarget::AgentSession { .. }))
    );
    assert!(provider.preview(&target).is_err());
    let marker = repository
        .root()
        .join("imported-history/deleted-sessions/session-retired.yml");
    let bytes = fs::read_to_string(&marker).unwrap();
    fs::write(&marker, bytes.replace("provider-retire", "forged-retire")).unwrap();
    assert!(
        provider
            .load(&request(PanelPage::Agents, "", "", 100))
            .is_err()
    );
    assert!(
        provider
            .load(&request(PanelPage::Files, "", "", 100))
            .is_ok()
    );
    fs::remove_file(&marker).unwrap();
    assert!(
        provider
            .load(&request(PanelPage::Agents, "", "", 100))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn native_history_rejects_special_sessions_markers_and_receipts_promptly() {
    for relative in [
        "imported-sessions/pipe.toml",
        "imported-history/deleted-sessions/pipe.yml",
        "operations/OP-01ARZ3NDEKTSV4RRFFQ69G5FAV.yml",
    ] {
        let root = repository();
        let repository = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
        let path = repository.root().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let pipe = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        let bound = provider(root.path());
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sender
                .send(bound.load(&request(PanelPage::Agents, "", "", 100)))
                .ok();
        });
        let result = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("recorded history inspection blocked on a FIFO");
        assert!(
            result.is_err(),
            "accepted special history source {relative}"
        );
    }
}

#[test]
fn malformed_planning_is_an_error_for_planning_search_while_files_remain_usable() {
    let root = repository();
    let repository = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    fs::write(root.path().join("ordinary.txt"), "visible").unwrap();
    fs::write(repository.root().join("config.yml"), "broken: [").unwrap();
    let provider = provider(root.path());
    assert!(
        provider
            .load(&request(PanelPage::Files, "ordinary", "", 100))
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.label == "ordinary.txt")
    );
    assert!(
        provider
            .load(&request(PanelPage::Search, "anything", "", 100))
            .is_err()
    );
    assert!(
        provider
            .load(&request(PanelPage::Agents, "", "", 100))
            .is_err()
    );
}

#[test]
fn snapshot_comparison_uses_captured_bytes_after_the_source_is_replaced() {
    let root = repository();
    let path = root.path().join("main.rs");
    fs::write(&path, "original snapshot\n").unwrap();
    let bytes = RepositoryPanels::read_review_file(root.path(), Path::new("main.rs")).unwrap();
    fs::remove_file(&path).unwrap();
    let comparison = workdeck_vcs::load_file_comparison_from_bytes(
        root.path(),
        Path::new("main.rs"),
        Path::new("main.rs"),
        &bytes,
        &bytes,
    )
    .unwrap();
    assert!(format!("{comparison:?}").contains("original snapshot"));
    assert!(!path.exists());
}

#[test]
fn planning_identity_replacement_does_not_rebind_an_existing_provider() {
    let root = repository();
    let first = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let provider = provider(root.path());
    let identity = provider.source();
    let mut config = first.config().unwrap();
    config.repository = workdeck_pm::RepositoryId::new();
    fs::write(
        first.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let failure = provider
        .load(&request(PanelPage::Agents, "", "", 100))
        .unwrap_err();
    assert!(failure.message.contains("planning authority changed"));
    assert_eq!(provider.source(), identity);
    assert!(
        provider
            .load(&request(PanelPage::Files, "", "", 100))
            .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn symlinks_fifo_and_replaced_roots_cannot_redirect_preview_or_navigation() {
    use std::os::unix::fs::symlink;
    let root = repository();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret"), "outside-data").unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    symlink(outside.path().join("secret"), root.path().join("link")).unwrap();
    let pipe = std::ffi::CString::new(root.path().join("pipe").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    let provider = provider(root.path());
    for path in ["escape/secret", "link", "pipe"] {
        assert!(
            provider
                .preview(&PanelTarget::File {
                    path: path.into(),
                    line: None
                })
                .is_err()
        );
        assert!(RepositoryPanels::read_review_file(root.path(), Path::new(path)).is_err());
    }
    assert!(
        provider
            .load(&request(PanelPage::Files, "", "escape", 100))
            .is_err()
    );
    let enclosing = tempdir().unwrap();
    fs::create_dir(enclosing.path().join("bound")).unwrap();
    let bound = RepositoryPanels::new(enclosing.path().join("bound"), None, 20).unwrap();
    fs::rename(enclosing.path().join("bound"), enclosing.path().join("old")).unwrap();
    fs::create_dir(enclosing.path().join("bound")).unwrap();
    assert!(bound.load(&request(PanelPage::Files, "", "", 100)).is_err());
}

fn legacy_fixture(root: &Path, name: &str, title: &str) {
    fs::create_dir_all(root.join(name).join("issues")).unwrap();
    fs::write(root.join(name).join("issues/WD-1.toml"),format!("key='WD-1'\ntitle='{title}'\ncreated_at='2026-01-01T00:00:00Z'\nupdated_at='2026-01-01T00:00:00Z'\n")).unwrap();
}
fn select_legacy(root: &Path, name: &str) {
    fs::create_dir_all(root.join(".workdeck")).unwrap();
    fs::write(
        root.join(".workdeck/config.toml"),
        format!("[paths]\ndata_dir='{name}'\n"),
    )
    .unwrap();
}
fn issue_target() -> PanelTarget {
    PanelTarget::Issue { id: "WD-1".into() }
}

#[test]
fn changing_selected_legacy_path_cannot_rebind_a_provider_or_its_saved_targets() {
    for read_before_change in [false, true] {
        let root = repository();
        legacy_fixture(root.path(), "legacy-a", "First source");
        legacy_fixture(root.path(), "legacy-b", "Second source");
        select_legacy(root.path(), "legacy-a");
        let bound = provider(root.path());
        let identity = bound.source();
        if read_before_change {
            assert!(
                bound
                    .preview(&issue_target())
                    .unwrap()
                    .body
                    .contains("First source")
            );
        }
        select_legacy(root.path(), "legacy-b");
        assert!(
            bound.preview(&issue_target()).is_err(),
            "legacy source was rebound"
        );
        assert_eq!(bound.source(), identity);
        assert!(bound.load(&request(PanelPage::Files, "", "", 100)).is_ok());
        assert!(
            provider(root.path())
                .preview(&issue_target())
                .unwrap()
                .body
                .contains("Second source")
        );
    }
}

#[test]
fn legacy_directory_replacement_removal_and_new_authority_require_reopening() {
    for mode in ["replace", "remove", "remove-markers"] {
        let root = repository();
        legacy_fixture(root.path(), "legacy", "Original");
        select_legacy(root.path(), "legacy");
        let bound = provider(root.path());
        assert!(bound.preview(&issue_target()).is_ok());
        if mode == "remove-markers" {
            fs::remove_dir_all(root.path().join("legacy/issues")).unwrap();
        } else {
            fs::rename(root.path().join("legacy"), root.path().join("old-legacy")).unwrap();
            if mode == "replace" {
                legacy_fixture(root.path(), "legacy", "Replacement");
            }
        }
        assert!(
            bound
                .load(&request(PanelPage::Agents, "", "", 100))
                .is_err(),
            "accepted {mode}"
        );
        assert!(bound.load(&request(PanelPage::Files, "", "", 100)).is_ok());
    }
    let root = repository();
    let bound = provider(root.path());
    legacy_fixture(root.path(), "legacy", "New authority");
    select_legacy(root.path(), "legacy");
    assert!(bound.preview(&issue_target()).is_err());
    assert!(provider(root.path()).preview(&issue_target()).is_ok());
}

#[test]
fn native_directory_replacement_with_same_repository_id_does_not_rebind() {
    let root = repository();
    let first = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let bound = provider(root.path());
    let config = fs::read(first.root().join("config.yml")).unwrap();
    fs::rename(first.root(), root.path().join("old-workdeck")).unwrap();
    fs::create_dir(root.path().join(".workdeck")).unwrap();
    fs::write(root.path().join(".workdeck/config.yml"), config).unwrap();
    assert!(
        bound
            .load(&request(PanelPage::Agents, "", "", 100))
            .is_err()
    );
    assert!(bound.load(&request(PanelPage::Files, "", "", 100)).is_ok());
    assert!(
        provider(root.path())
            .load(&request(PanelPage::Agents, "", "", 100))
            .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn dangling_history_directory_symlink_is_an_error_not_empty_history() {
    let root = repository();
    let native = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    std::os::unix::fs::symlink(
        root.path().join("absent"),
        native.root().join("imported-sessions"),
    )
    .unwrap();
    let bound = provider(root.path());
    assert!(
        bound
            .load(&request(PanelPage::Agents, "", "", 100))
            .is_err()
    );
    assert!(bound.load(&request(PanelPage::Files, "", "", 100)).is_ok());
}

#[test]
fn history_bounds_all_directory_entries_including_unrecognized_files() {
    let root = repository();
    let native = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let history = native.root().join("imported-sessions");
    fs::create_dir(&history).unwrap();
    for index in 0..10_001 {
        fs::write(history.join(format!("ignored-{index}.txt")), []).unwrap();
    }
    let failure = provider(root.path())
        .load(&request(PanelPage::Agents, "", "", 100))
        .unwrap_err();
    assert!(failure.message.contains("10,000"));
}

#[test]
fn explicit_relative_external_legacy_roots_are_pinned_without_changing_intent() {
    let enclosing = tempdir().unwrap();
    let root = enclosing.path().join("project");
    git2::Repository::init(&root).unwrap();
    legacy_fixture(enclosing.path(), "legacy", "Explicit sibling source");
    select_legacy(&root, "../legacy");
    let bound = provider(&root);
    assert!(
        bound
            .preview(&issue_target())
            .unwrap()
            .body
            .contains("Explicit sibling source")
    );
    assert!(bound.source().identity.contains("legacy:"));
    fs::rename(
        enclosing.path().join("legacy"),
        enclosing.path().join("old-legacy"),
    )
    .unwrap();
    legacy_fixture(enclosing.path(), "legacy", "Replaced sibling");
    assert!(bound.preview(&issue_target()).is_err());
}
