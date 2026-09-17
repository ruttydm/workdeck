use super::*;
use crate::{ReviewApp, ReviewOptions};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{collections::BTreeMap, path::Path};
use workdeck_pm::{CreateIssue, CreatePlanning, PlanningKind, Repository, RequestId};

fn app(root: &Path) -> ReviewApp {
    let mut app = ReviewApp::new(
        workdeck_diff::changeset_from_patch(
            "",
            "Planning",
            "Planning",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        ),
        ReviewOptions {
            repo: Some(root.into()),
            workbench: Some(WorkbenchOptions::new(root)),
            highlight: false,
            ..ReviewOptions::default()
        },
    );
    super::test_pump::settle_app(&mut app);
    app
}
fn key(app: &mut ReviewApp, code: KeyCode) {
    super::test_pump::settle_app(app);
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    super::test_pump::settle_app(app);
}
fn effects(app: &mut ReviewApp) {
    app.process_workbench_effect(&mut |_, _, _| Err("PM navigation must not reload VCS".into()));
    super::test_pump::settle_app(app);
}
fn seed(repo: &Repository) -> (workdeck_pm::IssueRecord, workdeck_pm::IssueRecord) {
    let mut project = CreatePlanning::new("Project");
    project.id = Some("p".into());
    repo.create_planning(PlanningKind::Project, &project, &RequestId::new())
        .unwrap();
    let mut issues = Vec::new();
    for title in ["Active member", "Archived member"] {
        let issue: workdeck_pm::IssueRecord = serde_json::from_value(
            repo.create_issue(
                &CreateIssue {
                    title: title.into(),
                    body: String::new(),
                    fields: BTreeMap::from([("project".into(), serde_json::json!("p"))]),
                },
                &RequestId::new(),
            )
            .unwrap()
            .result,
        )
        .unwrap();
        issues.push(issue);
    }
    repo.mutate_issue(
        issues[1].metadata.id.as_str(),
        None,
        &workdeck_pm::IssueMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    (issues.remove(0), issues.remove(0))
}

#[test]
fn unavailable_planning_refresh_discovers_external_initialization_without_legacy_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(11));
    effects(&mut app);
    assert!(!directory.path().join(".workdeck").exists());
    let repo = Repository::init(directory.path(), "WD").unwrap();
    seed(&repo);
    key(&mut app, KeyCode::Char('r'));
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert_eq!(
        shell
            .planning
            .selected()
            .map(|record| record.metadata.id.as_str()),
        Some("p")
    );
    assert!(shell.planning.error.is_none(), "{:?}", shell.planning.error);
    assert_eq!(
        shell.controller.repository().unwrap().identity(),
        repo.identity()
    );
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn planning_members_open_without_optional_repository_panel_provider() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path(), "WD").unwrap();
    let (active, _) = seed(&repo);
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(11));
    effects(&mut app);
    assert!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .panels
            .is_none()
    );
    key(&mut app, KeyCode::Enter);
    effects(&mut app);
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert_eq!(shell.tab, WorkbenchTab::Issues, "{:?}", shell.notice);
    assert_eq!(shell.controller.filter().project.as_deref(), Some("p"));
    assert_eq!(
        shell
            .index
            .as_ref()
            .unwrap()
            .page
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .map(|issue| issue.token.key.id.parse::<workdeck_pm::IssueId>().unwrap())
            .collect::<Vec<_>>(),
        vec![active.metadata.id]
    );
    assert!(shell.notice.is_none(), "{:?}", shell.notice);
}

#[test]
fn planning_all_scope_survives_navigation_to_shared_issue_filter() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path(), "WD").unwrap();
    let (active, archived) = seed(&repo);
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(11));
    effects(&mut app);
    key(&mut app, KeyCode::Char('x'));
    key(&mut app, KeyCode::Enter);
    effects(&mut app);
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert_eq!(shell.tab, WorkbenchTab::Issues, "{:?}", shell.notice);
    assert_eq!(
        shell.controller.filter().query().archive,
        workdeck_pm::ArchiveFilter::All
    );
    let ids = shell
        .index
        .as_ref()
        .unwrap()
        .page
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .map(|issue| issue.token.key.id.parse::<workdeck_pm::IssueId>().unwrap())
        .collect::<Vec<_>>();
    assert!(ids.contains(&active.metadata.id));
    assert!(ids.contains(&archived.metadata.id));
}

#[test]
fn planning_refresh_preserves_bound_identity_when_configuration_is_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path(), "WD").unwrap();
    seed(&repo);
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(11));
    effects(&mut app);
    let config = repo.root().join("config.yml");
    let original = std::fs::read_to_string(&config).unwrap();
    let replacement = workdeck_pm::Config::new("WD").unwrap();
    std::fs::write(
        config,
        original.replace(repo.identity().as_str(), replacement.repository.as_str()),
    )
    .unwrap();
    key(&mut app, KeyCode::Char('r'));
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert_eq!(
        shell.controller.repository().unwrap().identity(),
        repo.identity()
    );
    assert_eq!(
        shell.planning.error.as_ref().map(|error| error.code),
        Some(workdeck_pm::ErrorCode::StaleSource)
    );
}
