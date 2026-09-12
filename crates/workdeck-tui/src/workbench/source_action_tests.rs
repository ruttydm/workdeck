use super::{source_actions::*, *};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use workdeck_pm::*;

fn git(root: &std::path::Path, args: &[&str]) -> String {
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
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().into()
}
pub(super) fn fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Repository,
    IssueRecord,
) {
    let directory = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "-b", "main"]);
    git(remote.path(), &["init", "--bare"]);
    git(directory.path(), &["config", "user.name", "Source actions"]);
    git(
        directory.path(),
        &["config", "user.email", "source@example.invalid"],
    );
    git(
        directory.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Accepted task", "Accepted body"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
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
    std::fs::write(directory.path().join("code.txt"), "accepted application").unwrap();
    git(directory.path(), &["add", "."]);
    git(directory.path(), &["commit", "-m", "accepted fixture"]);
    git(directory.path(), &["push", "origin", "main"]);
    (directory, remote, repository, issue)
}
fn actions(repository: &Repository) -> SourceActions {
    SourceActions::new(
        Some(repository.clone()),
        Arc::new(ForegroundRunSignal::default()),
    )
}
#[track_caller]
fn finish(actions: &mut SourceActions) {
    let caller = std::panic::Location::caller();
    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(8);
    while actions.busy() {
        assert!(
            Instant::now() < deadline,
            "source action at {caller} exceeded its wait after {:?}: {:?}",
            started.elapsed(),
            actions.error
        );
        actions.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn fetch_and_sync_execute_only_reviewed_config_and_keep_original_request_after_errors() {
    let (directory, _remote, repository, _issue) = fixture();
    let mut actions = actions(&repository);
    let index = std::fs::read(directory.path().join(".git/index")).unwrap();
    let head = git(directory.path(), &["rev-parse", "HEAD"]);
    actions.inspect_refresh(false).unwrap();
    assert!(actions.request.is_none());
    assert!(actions.outcome.is_none());
    std::fs::write(
        repository.root().join("config.yml"),
        format!(
            "{}\n# changed after inspection\n",
            std::fs::read_to_string(repository.root().join("config.yml")).unwrap()
        ),
    )
    .unwrap();
    actions.execute().unwrap();
    finish(&mut actions);
    assert_eq!(actions.error.as_ref().unwrap().code, ErrorCode::StaleSource);
    let request = actions.request.clone().unwrap();
    assert_eq!(
        actions.inspect_refresh(true).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    actions.execute().unwrap();
    finish(&mut actions);
    assert_eq!(actions.request.as_ref(), Some(&request));
    actions.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    actions.inspect_refresh(false).unwrap();
    actions.execute().unwrap();
    finish(&mut actions);
    assert!(
        matches!(actions.outcome, Some(SourceActionOutcome::Refresh(_))),
        "{:?}",
        actions.error
    );
    actions.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    actions.inspect_refresh(true).unwrap();
    actions.execute().unwrap();
    finish(&mut actions);
    assert!(
        matches!(actions.outcome, Some(SourceActionOutcome::Refresh(_))),
        "{:?}",
        actions.error
    );
    assert_eq!(
        std::fs::read(directory.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(git(directory.path(), &["rev-parse", "HEAD"]), head);
}

#[test]
fn proposal_publication_status_and_fresh_session_resume_keep_original_plan_and_dirty_index() {
    let (directory, remote, repository, issue) = fixture();
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: Default::default(),
                body: Some("Proposed body".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    std::fs::write(directory.path().join("code.txt"), "staged application").unwrap();
    git(directory.path(), &["add", "code.txt"]);
    std::fs::write(directory.path().join("code.txt"), "unstaged application").unwrap();
    let index = std::fs::read(directory.path().join(".git/index")).unwrap();
    let plan = repository
        .preview_proposal(&sources::ProposalRequest {
            reference: "refs/heads/workdeck-proposals/ui".parse().unwrap(),
            title: "Review planning".into(),
        })
        .unwrap();
    let mut actions = actions(&repository);
    actions.review = Some(ReviewedSourceAction::Proposal(Box::new(plan.clone())));
    actions.execute().unwrap();
    finish(&mut actions);
    let request = actions.request.clone().unwrap();
    let Some(SourceActionOutcome::Proposal(outcome)) = &actions.outcome else {
        panic!("{:?}", actions.error);
    };
    assert_eq!(outcome.state, PublicationState::Confirmed);
    assert_eq!(outcome.plan, plan);
    assert_eq!(
        git(
            remote.path(),
            &["show", "refs/heads/workdeck-proposals/ui:code.txt"]
        ),
        "accepted application"
    );
    actions.inspect_request(false).unwrap();
    finish(&mut actions);
    let mut resumed = SourceActions::new(
        Some(repository.clone()),
        Arc::new(ForegroundRunSignal::default()),
    );
    resumed.request = Some(request.clone());
    resumed.inspect_request(true).unwrap();
    finish(&mut resumed);
    let Some(SourceActionOutcome::Proposal(outcome)) = &resumed.outcome else {
        panic!("{:?}", resumed.error);
    };
    assert_eq!(outcome.request_id, request);
    assert_eq!(outcome.plan, plan);
    assert!(outcome.replayed);
    assert_eq!(
        std::fs::read(directory.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("code.txt")).unwrap(),
        "unstaged application"
    );
}
