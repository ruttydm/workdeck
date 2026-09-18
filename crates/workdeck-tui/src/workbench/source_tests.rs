use super::*;
use crate::{ReviewApp, ReviewOptions};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};

fn app(root: &std::path::Path) -> ReviewApp {
    ReviewApp::new(
        workdeck_diff::changeset_from_patch(
            "",
            "Sources",
            "Sources",
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
    )
}
fn key(app: &mut ReviewApp, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    super::test_pump::settle_app(app);
}
fn screen(app: &ReviewApp, width: u16) -> String {
    let area = Rect::new(3, 2, width, 30);
    let mut buffer = Buffer::empty(Rect::new(0, 0, width + 6, 34));
    assert!(app.render_workbench_body(area, &mut buffer));
    buffer.content.iter().map(|cell| cell.symbol()).collect()
}
#[test]
fn mounted_sources_entry_does_not_initialize_and_keeps_native_review_available() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('i'));
    key(&mut app, KeyCode::Char('6'));
    let text = screen(&app, 110);
    assert!(text.contains("Planning sources"), "{text}");
    assert!(
        text.contains("no fetch") || text.contains("No fetch"),
        "{text}"
    );
    assert!(!directory.path().join(".workdeck").exists());
    assert!(!directory.path().join(".agents").exists());
    key(&mut app, KeyCode::F(2));
    assert_eq!(
        app.workbench.as_ref().unwrap().lock().unwrap().tab,
        WorkbenchTab::Review
    );
}

pub(super) fn git(root: &std::path::Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
pub(super) fn shared_fixture() -> (
    tempfile::TempDir,
    workdeck_pm::Repository,
    workdeck_pm::IssueRecord,
) {
    use workdeck_pm::*;
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "--quiet", "-b", "main"]);
    git(directory.path(), &["config", "user.name", "Source tests"]);
    git(
        directory.path(),
        &["config", "user.email", "sources@example.invalid"],
    );
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let mut config = repository.config().unwrap();
    config.sources = Some(SharedSources {
        remote: "origin".into(),
        accepted_ref: "refs/heads/main".parse().unwrap(),
        coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
        proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
    });
    std::fs::write(
        repository.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Accepted title", "Accepted body"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    git(directory.path(), &["add", "--", ".workdeck"]);
    git(
        directory.path(),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "accepted fixture",
        ],
    );
    git(
        directory.path(),
        &["checkout", "--quiet", "-b", "workdeck-proposals/demo"],
    );
    let updated = repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::from([(
                    "title".into(),
                    serde_json::json!("Proposal title"),
                )]),
                body: Some("Proposal body".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    let proposed: IssueRecord = serde_json::from_value(updated.result).unwrap();
    git(directory.path(), &["add", "--", ".workdeck"]);
    git(
        directory.path(),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "proposal fixture",
        ],
    );
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &proposed.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::from([(
                    "title".into(),
                    serde_json::json!("Uncommitted title"),
                )]),
                body: Some("Uncommitted body".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    (directory, repository, issue)
}

#[test]
fn accepted_and_proposal_citations_open_immutable_bytes_in_mounted_narrow_and_wide_views() {
    let (directory, repository, issue) = shared_fixture();
    let before_index = std::fs::read(directory.path().join(".git/index")).unwrap();
    let before_issue = std::fs::read(repository.root().join(&issue.path)).unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('i'));
    key(&mut app, KeyCode::Char('6'));
    key(&mut app, KeyCode::Char('a'));
    key(&mut app, KeyCode::Enter);
    for width in [62, 112] {
        let text = screen(&app, width);
        assert!(text.contains("Accepted title"), "{text}");
        assert!(!text.contains("Uncommitted body"), "{text}");
        assert!(text.contains("Accepted"), "{text}");
    }
    {
        let shell = app.workbench.as_ref().unwrap().lock().unwrap();
        let state = shell.context.sources.state().unwrap();
        assert_eq!(state.opened.as_ref().unwrap().issue.body, "Accepted body");
        assert_eq!(
            state.view.observation.identity.role,
            workdeck_pm::SourceRole::Accepted
        );
        assert_eq!(
            shell
                .context
                .sources
                .selected_contract()
                .unwrap()
                .accepted_source,
            state.view.observation.identity
        );
    }
    key(&mut app, KeyCode::Char('p'));
    app.workbench
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .context
        .sources
        .proposal_form
        .as_mut()
        .unwrap()
        .fields[0]
        .value = "refs/heads/workdeck-proposals/demo".into();
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Enter);
    let text = screen(&app, 112);
    assert!(text.contains("Proposal title"), "{text}");
    assert_eq!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .context
            .sources
            .state()
            .unwrap()
            .opened
            .as_ref()
            .unwrap()
            .issue
            .body,
        "Proposal body"
    );
    assert_eq!(
        std::fs::read(directory.path().join(".git/index")).unwrap(),
        before_index
    );
    assert_eq!(
        std::fs::read(repository.root().join(&issue.path)).unwrap(),
        before_issue
    );
}

#[test]
fn refresh_preserves_opened_citation_and_source_replacement_preserves_prior_slot() {
    use workdeck_pm::*;
    let (directory, repository, issue) = shared_fixture();
    let mut workspace = super::sources_workspace::SourcesWorkspace::new(
        directory.path().into(),
        Some(repository.identity().clone()),
    );
    workspace.select(SourceSelector::Accepted);
    workspace.open_selected().unwrap();
    let before = workspace.state().unwrap().opened.clone().unwrap();
    let proposed = git(
        directory.path(),
        &["rev-parse", "refs/heads/workdeck-proposals/demo"],
    );
    git(
        directory.path(),
        &["update-ref", "refs/heads/main", &proposed],
    );
    workspace.refresh().unwrap();
    assert_eq!(
        workspace.state().unwrap().selected.as_ref(),
        Some(&issue.metadata.id)
    );
    assert_eq!(
        workspace.state().unwrap().opened.as_ref().unwrap().document,
        before.document
    );
    assert_ne!(
        workspace.state().unwrap().view.observation.identity,
        before.observation.identity
    );
    assert_eq!(
        workspace.selected_contract().unwrap().accepted_source,
        before.observation.identity
    );
    assert_eq!(workspace.state().unwrap().issues[0].title, "Proposal title");
    workspace.open_selected().unwrap();
    assert_eq!(
        workspace
            .state()
            .unwrap()
            .opened
            .as_ref()
            .unwrap()
            .issue
            .body,
        "Proposal body"
    );
    workspace.select(SourceSelector::WorkingTree);
    workspace.open_selected().unwrap();
    let original = workspace.state().unwrap().view.observation.clone();
    let mut config = repository.config().unwrap();
    config.repository = RepositoryId::new();
    std::fs::write(
        repository.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    assert!(workspace.refresh().is_err());
    assert_eq!(workspace.state().unwrap().view.observation, original);
    assert_eq!(
        workspace
            .state()
            .unwrap()
            .opened
            .as_ref()
            .unwrap()
            .issue
            .body,
        "Uncommitted body"
    );
}

#[test]
fn proposal_reference_draft_survives_review_tab_and_reentry() {
    let (directory, _, _) = shared_fixture();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('i'));
    key(&mut app, KeyCode::Char('6'));
    key(&mut app, KeyCode::Char('p'));
    app.workbench
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .context
        .sources
        .paste("draft-suffix");
    let original = app
        .workbench
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .context
        .sources
        .proposal_form
        .as_ref()
        .unwrap()
        .fields[0]
        .value
        .clone();
    key(&mut app, KeyCode::F(2));
    key(&mut app, KeyCode::F(3));
    assert_eq!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .context
            .sources
            .proposal_form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        original
    );
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn mounted_claims_entry_inspects_without_acquiring_or_initializing() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('i'));
    key(&mut app, KeyCode::Char('7'));
    let text = screen(&app, 112);
    assert!(text.contains("Work claims"), "{text}");
    assert!(text.contains("explicit"), "{text}");
    assert!(!directory.path().join(".workdeck").exists());
    key(&mut app, KeyCode::F(2));
    assert_eq!(
        app.workbench.as_ref().unwrap().lock().unwrap().tab,
        WorkbenchTab::Review
    );
}

#[test]
fn mounted_source_operations_entry_is_explicit_and_does_not_initialize() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    for code in [
        KeyCode::F(3),
        KeyCode::Char('i'),
        KeyCode::Char('6'),
        KeyCode::Char('o'),
    ] {
        key(&mut app, code);
    }
    let text = screen(&app, 112);
    assert!(text.contains("Source operations"), "{text}");
    assert!(text.contains("No plan is selected"), "{text}");
    assert!(!directory.path().join(".workdeck").exists());
    assert!(!directory.path().join(".agents").exists());
}
