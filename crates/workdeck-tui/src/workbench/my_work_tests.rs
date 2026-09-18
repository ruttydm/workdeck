use super::*;
use std::{
    fs,
    time::{Duration, Instant},
};
use workdeck_pm::{CreateIssue, RequestId, SourceSelector, registry::*};

fn settle(view: &mut MyWorkWorkspace) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        view.poll();
        if view.is_idle() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "My work reader did not settle: {:?}",
            view.error
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn key(view: &mut MyWorkWorkspace, code: KeyCode) {
    view.key(KeyEvent::new(code, KeyModifiers::NONE));
    settle(view);
}
fn issue(repository: &Repository, title: &str, body: &str) -> workdeck_pm::IssueRecord {
    let mut input = CreateIssue::new(title, body);
    input
        .fields
        .insert("assignee".into(), serde_json::json!("local"));
    serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}
fn register(owner: &Repository, alias: &str, root: &std::path::Path) {
    let store = RegistryStore::open(owner).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: inspect_checkout(alias, root, SourceSelector::WorkingTree).unwrap(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
}
fn screen(view: &mut MyWorkWorkspace, width: u16) -> String {
    let area = Rect::new(0, 0, width, 30);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    view.render(area, &mut buffer, &crate::resolve_theme(None, None, &[]));
    buffer.content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn same_issue_ids_open_only_their_registered_checkout_and_changed_mapping_is_rejected() {
    let owner = tempfile::tempdir().unwrap();
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let left_repo = Repository::init(left.path(), "WD").unwrap();
    let right_repo = Repository::init(right.path(), "WD").unwrap();
    let original = issue(&left_repo, "Same looking ID", "Left exact source");
    let destination = right_repo.root().join(&original.path);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(
        destination,
        fs::read_to_string(left_repo.root().join(&original.path))
            .unwrap()
            .replace("Left exact source", "Right exact source"),
    )
    .unwrap();
    register(&owner_repo, "a", left.path());
    register(&owner_repo, "b", right.path());
    let mut view = MyWorkWorkspace::new(
        owner.path().to_owned(),
        Ok(owner_repo.clone()),
        "local".into(),
    );
    settle(&mut view);
    let report = view.report.as_ref().unwrap();
    assert_eq!(report.rows.len(), 2);
    assert_eq!(
        report.rows[0].row.token.key.id,
        report.rows[1].row.token.key.id
    );
    assert_ne!(
        report.rows[0].row.token.key.repository,
        report.rows[1].row.token.key.repository
    );
    key(&mut view, KeyCode::Enter);
    assert!(
        view.opened
            .as_ref()
            .unwrap()
            .1
            .document
            .as_ref()
            .unwrap()
            .contains("Left exact source")
    );
    key(&mut view, KeyCode::Down);
    key(&mut view, KeyCode::Enter);
    let opened = view.opened.clone().unwrap();
    assert_eq!(opened.0.alias, "b");
    assert!(
        opened
            .1
            .document
            .as_ref()
            .unwrap()
            .contains("Right exact source")
    );
    let store = RegistryStore::open(&owner_repo).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Remove { alias: "b".into() },
            },
            &RequestId::new(),
        )
        .unwrap();
    register(&owner_repo, "b", left.path());
    key(&mut view, KeyCode::Enter);
    assert!(view.error.as_ref().unwrap().contains("changed"));
    assert_eq!(view.opened.as_ref().unwrap(), &opened);
    assert_eq!(
        view.report.as_ref().unwrap().rows[1]
            .row
            .token
            .key
            .repository,
        *right_repo.identity()
    );
}

#[test]
fn malformed_and_missing_sources_stay_explicit_and_keep_the_opened_excerpt() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    let record = issue(
        &target_repo,
        "Retained assignment",
        "Retained exact excerpt",
    );
    register(&owner_repo, "secondary", target.path());
    let mut view = MyWorkWorkspace::new(owner.path().to_owned(), Ok(owner_repo), "local".into());
    settle(&mut view);
    key(&mut view, KeyCode::Enter);
    let opened = view.opened.clone();
    fs::write(target_repo.root().join(record.path), "malformed issue").unwrap();
    key(&mut view, KeyCode::Char('r'));
    assert!(!view.report.as_ref().unwrap().all_sources_available);
    assert!(view.report.as_ref().unwrap().sources[0].error.is_some());
    assert_eq!(view.opened, opened);
    // A narrow split shows only the beginning of the document; use the real
    // excerpt navigation to reach its body, independently of list selection.
    screen(&mut view, 62);
    view.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::SHIFT));
    for width in [62, 160] {
        let text = screen(&mut view, width);
        assert!(text.contains("partial"), "{text}");
        assert!(text.contains("Retained exact excerpt"), "{text}");
    }
    fs::rename(target_repo.root(), target.path().join("saved-planning")).unwrap();
    key(&mut view, KeyCode::Char('r'));
    assert_eq!(view.report.as_ref().unwrap().known_total, 0);
    assert!(!view.report.as_ref().unwrap().all_sources_available);
    assert_eq!(view.opened, opened);
    key(&mut view, KeyCode::Char('s'));
    assert!(screen(&mut view, 62).contains("unavailable/stale"));
    assert!(!target.path().join(".workdeck").exists());
}

#[test]
fn pagination_is_generation_bound_and_invalid_filter_keeps_the_last_report_and_input() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    for title in ["First", "Second", "Third"] {
        issue(&target_repo, title, "Body");
    }
    register(&owner_repo, "secondary", target.path());
    let mut view = MyWorkWorkspace::new(owner.path().to_owned(), Ok(owner_repo), "local".into());
    view.input.limit = 1;
    view.refresh();
    settle(&mut view);
    let first = view.report.as_ref().unwrap().rows[0].row.token.clone();
    key(&mut view, KeyCode::Char(']'));
    let second = view.report.as_ref().unwrap().rows[0].row.token.clone();
    assert_ne!(first.key, second.key);
    let cursor = view.input.cursor.clone();
    issue(&target_repo, "Later", "New generation");
    key(&mut view, KeyCode::Char('['));
    assert!(view.error.as_ref().unwrap().contains("generation changed"));
    assert_eq!(view.input.cursor, cursor);
    assert_eq!(view.report.as_ref().unwrap().rows[0].row.token, second);
    key(&mut view, KeyCode::Char('r'));
    assert!(view.input.cursor.is_none());
    assert_eq!(view.report.as_ref().unwrap().known_total, 4);
    key(&mut view, KeyCode::Char('/'));
    view.form.as_mut().unwrap().fields[0].value.clear();
    view.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    settle(&mut view);
    assert!(view.error.is_some());
    assert_eq!(view.form.as_ref().unwrap().fields[0].value, "");
    assert_eq!(view.report.as_ref().unwrap().known_total, 4);
    view.paste("nobody");
    view.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    settle(&mut view);
    assert!(view.form.is_none());
    assert_eq!(view.report.as_ref().unwrap().known_total, 0);
    assert!(view.report.as_ref().unwrap().all_sources_available);
}

#[test]
fn uninitialized_owner_is_visible_and_never_creates_planning_git_or_registry() {
    let root = tempfile::tempdir().unwrap();
    let mut view = MyWorkWorkspace::new(
        root.path().to_owned(),
        Repository::discover(root.path()),
        "local".into(),
    );
    settle(&mut view);
    assert!(view.report.is_none());
    assert!(view.error.is_some());
    assert!(screen(&mut view, 62).contains("My work"));
    key(&mut view, KeyCode::Char('r'));
    assert!(!root.path().join(".workdeck").exists());
    assert!(!root.path().join(".git").exists());
}

#[test]
fn source_selection_survives_registry_insertion_before_the_selected_mapping() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    Repository::init(target.path(), "WD").unwrap();
    register(&owner_repo, "b", target.path());
    register(&owner_repo, "c", target.path());
    let mut view = MyWorkWorkspace::new(
        owner.path().to_owned(),
        Ok(owner_repo.clone()),
        "local".into(),
    );
    settle(&mut view);
    key(&mut view, KeyCode::Char('s'));
    key(&mut view, KeyCode::Down);
    let selected = view.report.as_ref().unwrap().sources[view.source_selected]
        .checkout
        .clone();
    register(&owner_repo, "a", target.path());
    key(&mut view, KeyCode::Char('r'));
    assert_eq!(
        view.report.as_ref().unwrap().sources[view.source_selected].checkout,
        selected
    );
}

#[test]
fn unavailable_owner_can_be_explicitly_refreshed_after_external_initialization() {
    let root = tempfile::tempdir().unwrap();
    let mut view = MyWorkWorkspace::new(
        root.path().to_owned(),
        Repository::discover(root.path()),
        "local".into(),
    );
    settle(&mut view);
    assert!(view.report.is_none());
    let owner = Repository::init(root.path(), "WD").unwrap();
    key(&mut view, KeyCode::Char('r'));
    assert!(view.report.is_some(), "{:?}", view.error);
    assert_eq!(view.report.as_ref().unwrap().known_total, 0);
    assert!(!owner.root().join(".local/repositories").exists());
}

#[test]
fn a_click_from_an_old_frame_cannot_open_a_refreshed_source_row() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    let record = issue(&target_repo, "Clicked source", "Original inspected bytes");
    register(&owner_repo, "secondary", target.path());
    let mut view = MyWorkWorkspace::new(owner.path().to_owned(), Ok(owner_repo), "local".into());
    settle(&mut view);
    key(&mut view, KeyCode::Enter);
    let original = view.opened.clone();
    screen(&mut view, 160);
    let old_hit = view.hits[0].0;
    target_repo
        .update_issue(
            record.metadata.id.as_str(),
            &record.source,
            &workdeck_pm::UpdateIssue {
                fields: Default::default(),
                body: Some("New source bytes".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    key(&mut view, KeyCode::Char('r'));
    assert_ne!(
        view.report.as_ref().unwrap().rows[0].row.token,
        original.as_ref().unwrap().1.row.token
    );
    let click = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: old_hit.x,
        row: old_hit.y,
        modifiers: KeyModifiers::NONE,
    };
    view.mouse(&click);
    settle(&mut view);
    assert_eq!(
        view.opened, original,
        "A click on an obsolete frame must not open a newer source"
    );
    screen(&mut view, 160);
    view.mouse(&click);
    settle(&mut view);
    assert!(
        view.opened
            .as_ref()
            .unwrap()
            .1
            .document
            .as_ref()
            .unwrap()
            .contains("New source bytes")
    );
}

#[test]
fn facets_show_review_requests_and_overdue_work_without_losing_opened_source() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    issue(&repository, "Assigned task", "Retained assignment excerpt");
    for (title, fields) in [
        (
            "Requested review",
            serde_json::json!({"reviewer":"local","assignee":"other","status":"in_review"}),
        ),
        (
            "Overdue task",
            serde_json::json!({"assignee":"local","due_at":"2000-01-01"}),
        ),
    ] {
        let mut input = CreateIssue::new(title, "Exact source");
        input.fields = serde_json::from_value(fields).unwrap();
        repository.create_issue(&input, &RequestId::new()).unwrap();
    }
    register(&repository, "local", directory.path());
    let mut view = MyWorkWorkspace::new(
        directory.path().to_owned(),
        Ok(repository.clone()),
        "local".into(),
    );
    settle(&mut view);
    key(&mut view, KeyCode::Enter);
    let opened = view.opened.as_ref().unwrap().1.clone();
    key(&mut view, KeyCode::Char('2'));
    assert_eq!(
        view.report.as_ref().unwrap().facet,
        MyWorkFacet::ReviewRequested
    );
    assert!(screen(&mut view, 62).contains("Requested review"));
    assert_eq!(view.opened.as_ref().unwrap().1.row.token, opened.row.token);
    key(&mut view, KeyCode::Char('3'));
    assert_eq!(view.report.as_ref().unwrap().facet, MyWorkFacet::Overdue);
    assert!(screen(&mut view, 160).contains("Overdue task"));
    let time = view.report.as_ref().unwrap().as_of.unwrap().to_rfc3339();
    assert!(screen(&mut view, 62).contains(&time));
    key(&mut view, KeyCode::Char('1'));
    assert_eq!(view.report.as_ref().unwrap().facet, MyWorkFacet::Assigned);
    view.begin_shutdown();
}

#[test]
fn blocked_facet_explains_the_selected_source_without_overwriting_an_opened_excerpt() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let prerequisite = issue(&repository, "Prerequisite task", "Original excerpt");
    let mut input = CreateIssue::new("Blocked task", "Dependent excerpt");
    input.fields = serde_json::from_value(
        serde_json::json!({"assignee":"local", "prerequisites":[prerequisite.metadata.id]}),
    )
    .unwrap();
    repository.create_issue(&input, &RequestId::new()).unwrap();
    register(&repository, "self", directory.path());
    let mut view =
        MyWorkWorkspace::new(directory.path().to_owned(), Ok(repository), "local".into());
    settle(&mut view);
    key(&mut view, KeyCode::Enter);
    let opened = view.opened.clone();
    key(&mut view, KeyCode::Char('4'));
    assert_eq!(view.report.as_ref().unwrap().facet, MyWorkFacet::Blocked);
    assert_eq!(view.report.as_ref().unwrap().known_total, 1);
    key(&mut view, KeyCode::Char('e'));
    for width in [62, 160] {
        let text = screen(&mut view, width);
        assert!(text.contains("Work evidence"), "{text}");
        assert!(text.contains("incomplete_prerequisite"), "{text}");
    }
    assert_eq!(view.opened, opened);
    key(&mut view, KeyCode::Char('e'));
    assert!(screen(&mut view, 160).contains("Original excerpt"));
    view.begin_shutdown();
}

#[test]
fn claimed_facet_uses_the_claim_actor_and_explains_local_observation_scope() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue: workdeck_pm::IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Claimed without assignment", "Contract"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    repository
        .mutate_local_claim(
            &workdeck_pm::ClaimRequest::Acquire {
                input: Box::new(workdeck_pm::AcquireClaim {
                    actor: "local".into(),
                    contract: repository.local_claim_contract(&issue.metadata.id).unwrap(),
                    ttl_seconds: None,
                    recovery: None,
                }),
            },
            &RequestId::new(),
        )
        .unwrap();
    register(&repository, "self", directory.path());
    let mut view =
        MyWorkWorkspace::new(directory.path().to_owned(), Ok(repository), "local".into());
    settle(&mut view);
    assert_eq!(view.report.as_ref().unwrap().known_total, 0);
    key(&mut view, KeyCode::Char('5'));
    assert_eq!(view.report.as_ref().unwrap().facet, MyWorkFacet::Claimed);
    assert_eq!(view.report.as_ref().unwrap().known_total, 1);
    key(&mut view, KeyCode::Char('e'));
    for width in [62, 160] {
        let text = screen(&mut view, width);
        assert!(text.contains("local source only"), "{text}");
        assert!(text.contains("Usable"), "{text}");
    }
    view.begin_shutdown();
}
