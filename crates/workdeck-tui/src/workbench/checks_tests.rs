use super::checks_workspace::*;
use super::*;
use crate::{ReviewApp, ReviewOptions};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};
use serde_json::json;
use std::{
    fs,
    time::{Duration, Instant},
};
use workdeck_pm::*;

fn fixture(script: &str) -> (tempfile::TempDir, Repository, IssueRecord) {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    fs::write(directory.path().join("source.txt"), "source one").unwrap();
    let issue = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Check this task", "Accepted requirements"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    write(
        &repository,
        "commands/test.yml",
        json!({
            "schema":1,"repository":repository.identity(),"id":"test","name":"Test command",
            "recipe":{"kind":"shell","interpreter":"sh","script":script,"args":[]},
            "cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],
            "inputs":{"files":["source.txt"]},"bounds":{"timeout_seconds":10,"stdout_bytes":1024,"stderr_bytes":1024},
            "effects":[{"kind":"write","path":"ran.txt"}]
        }),
    );
    write(
        &repository,
        "checks/unit.yml",
        json!({
            "schema":1,"repository":repository.identity(),"id":"unit","name":"Unit checks",
            "command":"test","expectation":{"kind":"process","allowed_exit_codes":[0]}
        }),
    );
    write(
        &repository,
        "check-profiles/quick.yml",
        json!({
            "schema":1,"repository":repository.identity(),"id":"quick","name":"Quick checks","checks":["unit"]
        }),
    );
    (directory, repository, issue)
}
fn write(repository: &Repository, path: &str, value: serde_json::Value) {
    let path = repository.root().join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // JSON is valid YAML; tests need no second authoring/parser implementation.
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
fn workspace(repository: &Repository, issue: &IssueRecord) -> ChecksWorkspace {
    let mut workspace = ChecksWorkspace::new(Some(repository.clone()), "local".into());
    workspace.open(Some(issue.metadata.id.clone()));
    workspace
}
fn finish(workspace: &mut ChecksWorkspace) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while workspace.running_key.is_some() {
        assert!(
            Instant::now() < deadline,
            "foreground run did not finish: {:?}",
            workspace.state()
        );
        workspace.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn discovery_and_planning_are_inert_and_stale_run_preserves_the_original_request() {
    let (directory, repository, issue) = fixture("printf ran >> ran.txt");
    let mut workspace = workspace(&repository, &issue);
    assert!(workspace.state().unwrap().error.is_none());
    assert!(matches!(
        workspace.selected_row().unwrap().target,
        Some(CheckTarget::Profile(_))
    ));
    workspace.plan_selected().unwrap();
    let plan = workspace
        .state()
        .unwrap()
        .plan
        .as_ref()
        .unwrap()
        .plan
        .clone();
    assert!(!directory.path().join("ran.txt").exists());
    fs::write(directory.path().join("source.txt"), "changed input").unwrap();
    workspace.refresh().unwrap();
    assert_eq!(workspace.state().unwrap().plan.as_ref().unwrap().plan, plan);
    workspace.begin_run().unwrap();
    finish(&mut workspace);
    let state = workspace.state().unwrap();
    assert_eq!(state.error.as_ref().unwrap().code, ErrorCode::StaleSource);
    let request = state.plan.as_ref().unwrap().request.clone();
    assert!(request.is_some());
    assert!(!directory.path().join("ran.txt").exists());
    workspace.begin_run().unwrap();
    finish(&mut workspace);
    assert_eq!(
        workspace.state().unwrap().plan.as_ref().unwrap().request,
        request
    );
    assert!(!directory.path().join("ran.txt").exists());
    assert!(
        !workspace.signal.is_active(),
        "pre-spawn rejection must acknowledge cleanup"
    );
}

#[test]
fn explicit_profile_run_exposes_failure_filters_and_preserves_issue_source() {
    let (directory, repository, issue) =
        fixture("printf ran >> ran.txt; printf failure >&2; exit 7");
    let mut workspace = workspace(&repository, &issue);
    workspace.plan_selected().unwrap();
    workspace.begin_run().unwrap();
    finish(&mut workspace);
    assert!(
        workspace.state().unwrap().error.is_none(),
        "{:?}",
        workspace.state()
    );
    assert_eq!(fs::read(directory.path().join("ran.txt")).unwrap(), b"ran");
    assert_eq!(
        workspace.state().unwrap().runs[0].assessment.state,
        RunState::Failed
    );
    let rows = workspace.rows();
    assert!(
        rows.iter()
            .any(|row| row.title.contains("check unit") && row.detail.contains("exit Some(7)"))
    );
    workspace.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
    assert!(!workspace.rows().is_empty());
    workspace.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
    assert!(
        workspace.rows().is_empty(),
        "passed filter must not hide failure as a passed check"
    );
    let fresh = repository.show_issue(issue.metadata.id.as_str()).unwrap();
    assert_eq!(fresh.source, issue.source);
    assert_eq!(fresh.metadata.status, issue.metadata.status);
}

#[test]
fn refresh_does_not_rebind_a_replaced_repository_or_accept_changed_catalog_rows() {
    let (_directory, repository, issue) = fixture("printf ran >> ran.txt");
    let mut workspace = workspace(&repository, &issue);
    let path = repository.root().join("check-profiles/quick.yml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, original.replace("Quick checks", "Changed checks")).unwrap();
    assert_eq!(
        workspace.plan_selected().unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert!(workspace.state().unwrap().plan.is_none());
    workspace.refresh().unwrap();
    workspace.plan_selected().unwrap();
    let plan = workspace
        .state()
        .unwrap()
        .plan
        .as_ref()
        .unwrap()
        .plan
        .clone();
    let path = repository.root().join("config.yml");
    let config = fs::read_to_string(&path).unwrap();
    fs::write(
        path,
        config.replace(repository.identity().as_str(), RepositoryId::new().as_str()),
    )
    .unwrap();
    assert_eq!(
        workspace.refresh().unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(workspace.state().unwrap().plan.as_ref().unwrap().plan, plan);
    workspace.begin_run().unwrap();
    finish(&mut workspace);
    assert_eq!(
        workspace.state().unwrap().error.as_ref().unwrap().code,
        ErrorCode::StaleSource
    );
}

#[test]
fn mounted_checks_preserve_plan_across_tabs_and_render_allocated_narrow_and_wide_rects() {
    let (directory, _repository, _issue) = fixture("printf ran >> ran.txt");
    let mut app = ReviewApp::new(
        workdeck_diff::changeset_from_patch(
            "",
            "Review",
            "Review",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        ),
        ReviewOptions {
            repo: Some(directory.path().into()),
            workbench: Some(WorkbenchOptions::new(directory.path())),
            highlight: false,
            ..Default::default()
        },
    );
    for code in [
        KeyCode::F(3),
        KeyCode::Char('i'),
        KeyCode::Char('5'),
        KeyCode::Char('p'),
    ] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    super::test_pump::settle_app(&mut app);
    let plan = app
        .workbench
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .context
        .checks
        .state()
        .unwrap()
        .plan
        .as_ref()
        .unwrap()
        .plan
        .clone();
    app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE));
    for width in [78, 180] {
        let area = Rect::new(3, 2, width, 30);
        let mut buffer = Buffer::empty(Rect::new(0, 0, width + 6, 34));
        assert!(app.render_workbench_body(area, &mut buffer));
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Inspected check plan"), "{text}");
        assert!(text.contains("local feedback"), "{text}");
        assert_eq!(buffer[(0, 0)].symbol(), " ");
    }
    assert_eq!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .context
            .checks
            .state()
            .unwrap()
            .plan
            .as_ref()
            .unwrap()
            .plan,
        plan
    );
    assert!(!directory.path().join("ran.txt").exists());
}

#[test]
fn lost_acknowledgement_reuses_original_plan_request_after_later_source_edits() {
    let (directory, repository, issue) = fixture("printf ran >> ran.txt");
    let mut workspace = workspace(&repository, &issue);
    workspace.plan_selected().unwrap();
    workspace
        .begin_run_using(|repository, input, request, control| {
            repository.run_check_plan_with_faults(&input, &request, &control, |point| {
                if point == RunFaultPoint::AfterResultPublication {
                    Err(PmError::new(
                        ErrorCode::Io,
                        "simulated lost acknowledgement",
                    ))
                } else {
                    Ok(())
                }
            })
        })
        .unwrap();
    finish(&mut workspace);
    assert_eq!(
        workspace.state().unwrap().error.as_ref().unwrap().code,
        ErrorCode::Io
    );
    let request = workspace
        .state()
        .unwrap()
        .plan
        .as_ref()
        .unwrap()
        .request
        .clone();
    assert_eq!(fs::read(directory.path().join("ran.txt")).unwrap(), b"ran");
    fs::write(directory.path().join("source.txt"), "a later edit").unwrap();
    workspace.refresh().unwrap();
    workspace.begin_run().unwrap();
    finish(&mut workspace);
    assert!(
        workspace.state().unwrap().error.is_none(),
        "{:?}",
        workspace.state()
    );
    assert_eq!(
        workspace.state().unwrap().plan.as_ref().unwrap().request,
        request
    );
    assert!(workspace.state().unwrap().runs[0].replayed);
    assert_eq!(fs::read(directory.path().join("ran.txt")).unwrap(), b"ran");
}

#[test]
fn completion_is_published_to_its_original_task_and_cancellation_acknowledges_cleanup() {
    let (directory, repository, issue) = fixture("printf started > ran.txt; /bin/sleep 30");
    let other: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&CreateIssue::new("Another task", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut workspace = workspace(&repository, &issue);
    workspace.plan_selected().unwrap();
    workspace.begin_run().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !directory.path().join("ran.txt").exists() {
        assert!(Instant::now() < deadline, "process did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    workspace.open(Some(other.metadata.id.clone()));
    assert!(workspace.state().unwrap().runs.is_empty());
    assert!(workspace.signal.interrupt_active());
    finish(&mut workspace);
    assert!(
        workspace.state().unwrap().runs.is_empty(),
        "the other task must not receive this result"
    );
    workspace.open(Some(issue.metadata.id.clone()));
    assert_eq!(
        workspace.state().unwrap().runs[0].assessment.state,
        RunState::Canceled
    );
    assert!(!workspace.signal.is_active());
    assert!(
        workspace.state().unwrap().runs[0]
            .results
            .as_ref()
            .unwrap()
            .result
            .invocations[0]
            .process
            .cleanup_complete
    );
}

#[test]
fn missing_local_proof_cannot_leave_historical_checks_in_the_passed_filter() {
    let (_directory, repository, issue) = fixture("printf completed");
    let mut workspace = workspace(&repository, &issue);
    workspace.plan_selected().unwrap();
    workspace.begin_run().unwrap();
    finish(&mut workspace);
    let outcome = &workspace.state().unwrap().runs[0];
    assert_eq!(outcome.assessment.state, RunState::Passed);
    let log = &outcome.results.as_ref().unwrap().result.invocations[0]
        .process
        .stdout
        .path;
    fs::remove_file(repository.root().join(log)).unwrap();
    workspace.refresh().unwrap();
    assert_eq!(
        workspace.state().unwrap().runs[0].assessment.state,
        RunState::Unknown
    );
    workspace.state_mut().filter = ResultFilter::Passed;
    assert!(
        workspace.rows().is_empty(),
        "historical pass must not masquerade as current feedback"
    );
    workspace.state_mut().filter = ResultFilter::Attention;
    let check = workspace
        .rows()
        .into_iter()
        .find(|row| row.id.starts_with("result:"))
        .unwrap();
    assert!(check.title.starts_with("Unknown"));
    assert!(check.detail.contains("Historical check: Passed"));
}

#[test]
fn filtering_out_scrolled_results_reveals_the_empty_state_in_narrow_and_wide_views() {
    let (_directory, repository, issue) = fixture("exit 1");
    let mut workspace = workspace(&repository, &issue);
    workspace.plan_selected().unwrap();
    workspace.begin_run().unwrap();
    finish(&mut workspace);
    assert_eq!(
        workspace.state().unwrap().runs[0].assessment.state,
        RunState::Failed
    );
    workspace.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    workspace.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
    assert_eq!(
        workspace.state().unwrap().scroll[2],
        10,
        "same selected result retains its position"
    );
    workspace.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
    assert!(workspace.rows().is_empty());
    for width in [78, 180] {
        let area = Rect::new(0, 0, width, 34);
        let mut buffer = Buffer::empty(area);
        workspace.render(area, &mut buffer, &ReviewOptions::default().theme);
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(
            text.contains("No matching results"),
            "empty explanation was scrolled out at width {width}: {text}"
        );
    }
}

#[test]
fn refreshed_context_opens_its_exact_completed_run_without_reexecution() {
    let (directory, repository, issue) = fixture("printf ran >> ran.txt; exit 1");
    let mut context =
        super::context_workspace::ContextWorkspace::new(Some(repository.clone()), "local".into());
    context.open(Some(issue.metadata.id.clone()));
    for key in ['5', 'p', 'x'] {
        context.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
    }
    finish(&mut context.checks);
    let id = context.checks.state().unwrap().runs[0]
        .run
        .intent
        .id
        .clone();
    for key in ['1', 'r'] {
        context.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
    }
    assert_eq!(
        context.selected_row().unwrap().id,
        "summary",
        "refresh preserves the inspected row when earlier feedback is added"
    );
    let mut found = false;
    for _ in 0..80 {
        if context.selected_row().is_some_and(|row| {
            matches!(
                row.target,
                Some(super::context_workspace::RowTarget::CheckRun(_))
            )
        }) {
            found = true;
            break;
        }
        context.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    }
    assert!(
        found,
        "run summary missing from refreshed context: error={:?} rows={:?} source={:?}",
        context.state().unwrap().error,
        context
            .rows()
            .iter()
            .map(|row| (&row.id, &row.title))
            .collect::<Vec<_>>(),
        context.checks.state().unwrap().runs[0]
            .run
            .intent
            .input
            .plan
            .issue
    );
    let (handled, effect) = context.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(handled && effect.is_none());
    assert_eq!(
        context.state().unwrap().section,
        super::context_workspace::Section::Checks
    );
    assert_eq!(context.checks.state().unwrap().runs[0].run.intent.id, id);
    assert_eq!(fs::read(directory.path().join("ran.txt")).unwrap(), b"ran");
}

#[test]
fn inactive_checkout_checks_keep_their_source_are_polled_and_join_at_shutdown() {
    use std::sync::Arc;
    use workdeck_pm::registry::*;
    #[derive(Debug)]
    struct Panels(std::path::PathBuf);
    impl RepositoryPanelProvider for Panels {
        fn source(&self) -> RepositoryPanelSource {
            RepositoryPanelSource {
                root: self.0.clone(),
                identity: self.0.display().to_string(),
            }
        }
        fn load(&self, _: &PanelRequest) -> std::result::Result<PanelSnapshot, PanelError> {
            Err(PanelError::new("fixture collection"))
        }
        fn preview(&self, _: &PanelTarget) -> std::result::Result<PanelPreview, PanelError> {
            Err(PanelError::new("fixture preview"))
        }
    }
    let (directory, repository, _) = fixture(
        "while [ ! -f .workdeck/.local/release-check ]; do sleep 0.01; done; printf owner > ran.txt",
    );
    let root = directory.path().canonicalize().unwrap();
    let target = tempfile::tempdir().unwrap();
    Repository::init(target.path(), "WD").unwrap();
    let store = RegistryStore::open(&repository).unwrap();
    let mapping = inspect_checkout("target", target.path(), SourceSelector::WorkingTree).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: mapping.clone(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let input = workdeck_core::CliInput::Vcs(workdeck_core::VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: Default::default(),
    });
    let changes = || {
        workdeck_diff::changeset_from_patch(
            "",
            "checks",
            "checks",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        )
    };
    let mut app = ReviewApp::new(
        changes(),
        ReviewOptions {
            repo: Some(root.clone()),
            workbench: Some(WorkbenchOptions::new(&root)),
            repository_panels: Some(Arc::new(Panels(root))),
            review_input: Some(input),
            ..Default::default()
        },
    );
    let signal = {
        let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
        shell.context.checks.open(None);
        shell.context.checks.plan_selected().unwrap();
        shell.context.checks.begin_run().unwrap();
        shell.context.checks.signal.clone()
    };
    assert!(signal.is_active());
    app.workbench_registered_checkout(
        store.prepare_navigation(&mapping).unwrap(),
        &mut |app, input, root| {
            app.session_commit_dynamic_reload(
                crate::DynamicReviewLoad {
                    input: input.clone(),
                    changeset: changes(),
                    replacement_extensions: None,
                    replacement_vcs_catalog: None,
                    host_options: crate::DynamicReviewHostOptions {
                        command_cwd: root.to_owned(),
                        repo_root: Some(root.to_owned()),
                        registry_navigation: Some(store.prepare_navigation(&mapping).unwrap()),
                        repository_panels: Some(Some(Arc::new(Panels(root.to_owned())))),
                        ..Default::default()
                    },
                },
                &Default::default(),
            )
            .map(|_| ())
        },
    )
    .unwrap();
    assert!(app.defer_foreground_run_suspend());
    let release = repository.root().join(".local/release-check");
    fs::create_dir_all(release.parent().unwrap()).unwrap();
    fs::write(&release, "release").unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while signal.is_active() {
        app.poll_workbench();
        assert!(
            Instant::now() < deadline,
            "inactive checkout check was not polled"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        fs::read(directory.path().join("ran.txt")).unwrap(),
        b"owner"
    );
    assert!(!target.path().join("ran.txt").exists());
    fs::remove_file(release).unwrap();
    {
        let mut shell = app.workbench_checkouts.retained[0].shell.lock().unwrap();
        assert_eq!(
            shell.context.checks.state().unwrap().runs[0]
                .assessment
                .state,
            RunState::Passed
        );
        shell
            .context
            .checks
            .key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE));
        shell
            .context
            .checks
            .key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        shell.context.checks.plan_selected().unwrap();
        shell.context.checks.begin_run().unwrap();
    }
    assert!(signal.is_active());
    app.shutdown_foreground_run().unwrap();
    assert!(!signal.is_active());
    assert!(app.workbench_checkouts.retained.is_empty());
    assert!(!target.path().join("ran.txt").exists());
}
