use super::*;
use crate::{ReviewApp, ReviewOptions};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use workdeck_pm::{RequestId, SourceSelector, registry::*};

#[derive(Debug)]
struct Panels(std::path::PathBuf);
impl RepositoryPanelProvider for Panels {
    fn source(&self) -> RepositoryPanelSource {
        RepositoryPanelSource {
            root: self.0.clone(),
            identity: self.0.display().to_string(),
        }
    }
    fn load(&self, _: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
        Err(PanelError::new("fixture collection"))
    }
    fn preview(&self, _: &PanelTarget) -> Result<PanelPreview, PanelError> {
        Err(PanelError::new("fixture preview"))
    }
}
fn key(app: &mut ReviewApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, modifiers));
    super::test_pump::settle_app(app);
}
fn screen(app: &ReviewApp) -> String {
    let area = ratatui::layout::Rect::new(0, 0, 160, 34);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    assert!(app.render_workbench_body(area, &mut buffer));
    buffer.content.iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn accepted_planning_switch_browses_immutable_rows_and_returns_to_the_native_draft() {
    let (directory, repository, issue) = source_tests::shared_fixture();
    let root = directory.path().canonicalize().unwrap();
    let store = RegistryStore::open(&repository).unwrap();
    let accepted = inspect_checkout("accepted", &root, SourceSelector::Accepted).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: accepted.clone(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let proposal = inspect_checkout(
        "proposal",
        &root,
        SourceSelector::Proposal {
            reference: "refs/heads/workdeck-proposals/demo".parse().unwrap(),
        },
    )
    .unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register { checkout: proposal },
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
            "source",
            "source",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        )
    };
    let mut app = ReviewApp::new(
        changes(),
        ReviewOptions {
            repo: Some(root.clone()),
            command_cwd: Some(root.clone()),
            workbench: Some(WorkbenchOptions::new(&root)),
            repository_panels: Some(Arc::new(Panels(root.clone()))),
            review_input: Some(input),
            ..Default::default()
        },
    );
    super::test_pump::settle_app(&mut app);
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    app.handle_paste("Native draft survives");
    key(&mut app, KeyCode::F(9), KeyModifiers::SHIFT);
    key(&mut app, KeyCode::Char('s'), KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('o'), KeyModifiers::NONE);
    let mut reload =
        |app: &mut ReviewApp, input: &workdeck_core::CliInput, cwd: &std::path::Path| {
            app.session_commit_dynamic_reload(
                crate::DynamicReviewLoad {
                    input: input.clone(),
                    changeset: changes(),
                    replacement_extensions: None,
                    replacement_vcs_catalog: None,
                    host_options: crate::DynamicReviewHostOptions {
                        repo_root: Some(root.clone()),
                        command_cwd: cwd.to_owned(),
                        repository_panels: Some(Some(Arc::new(Panels(root.clone())))),
                        ..Default::default()
                    },
                },
                &Default::default(),
            )
            .map(|_| ())
        };
    app.process_workbench_effect(&mut reload);
    super::test_pump::settle_app(&mut app);
    let text = screen(&app);
    assert!(text.contains("Read-only planning"), "{text}");
    assert!(text.contains("Accepted title"), "{text}");
    assert!(!text.contains("Uncommitted title"), "{text}");
    assert!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .controller
            .repository()
            .is_err()
    );
    let bytes = std::fs::read(repository.root().join(&issue.path)).unwrap();
    let issue_count = repository.list_issues().unwrap().len();
    let head = source_tests::git(&root, &["rev-parse", "HEAD"]);
    let index = std::fs::read(root.join(".git/index")).unwrap();
    for code in [
        KeyCode::Char('n'),
        KeyCode::Char('e'),
        KeyCode::Char('a'),
        KeyCode::Char('c'),
        KeyCode::F(4),
    ] {
        key(&mut app, code, KeyModifiers::NONE);
        app.process_workbench_effect(&mut reload);
    }
    key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(
        std::fs::read(repository.root().join(&issue.path)).unwrap(),
        bytes
    );
    assert_eq!(source_tests::git(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(repository.list_issues().unwrap().len(), issue_count);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(screen(&app).contains("Accepted body"));
    key(&mut app, KeyCode::F(9), KeyModifiers::SHIFT);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('o'), KeyModifiers::NONE);
    app.process_workbench_effect(&mut reload);
    super::test_pump::settle_app(&mut app);
    assert!(screen(&app).contains("Proposal title"));
    key(&mut app, KeyCode::F(9), KeyModifiers::SHIFT);
    key(&mut app, KeyCode::Char('h'), KeyModifiers::NONE);
    app.process_workbench_effect(&mut reload);
    super::test_pump::settle_app(&mut app);
    assert!(screen(&app).contains("Native draft survives"));
    key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(
        repository
            .list_issues()
            .unwrap()
            .iter()
            .any(|issue| issue.metadata.title == "Native draft survives")
    );
    app.shutdown_foreground_run().unwrap();
}

#[test]
fn proposal_features_planning_activity_filters_and_excerpts_stay_in_the_captured_source() {
    use workdeck_pm::{CreateFeature, CreatePlanning, FeatureOutcome, PlanningKind};
    let (directory, repository, _) = source_tests::shared_fixture();
    let root = directory.path().canonicalize().unwrap();
    let feature: FeatureOutcome = serde_json::from_value(
        repository
            .create_feature(
                &CreateFeature::new("Captured parent feature"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let mut child = CreateFeature::new("Captured child feature");
    child.fields.insert(
        "parent".into(),
        serde_json::json!(feature.record.metadata.id),
    );
    repository
        .create_feature(&child, &RequestId::new())
        .unwrap();
    repository
        .create_planning(
            PlanningKind::Project,
            &CreatePlanning::new("Captured project"),
            &RequestId::new(),
        )
        .unwrap();
    source_tests::git(&root, &["add", ".workdeck/features", ".workdeck/projects"]);
    source_tests::git(
        &root,
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "Captured hierarchy",
        ],
    );
    repository
        .create_feature(
            &CreateFeature::new("Uncommitted feature only"),
            &RequestId::new(),
        )
        .unwrap();
    let binding = inspect_checkout(
        "proposal",
        &root,
        SourceSelector::Proposal {
            reference: "refs/heads/workdeck-proposals/demo".parse().unwrap(),
        },
    )
    .unwrap();
    let mut shell = WorkbenchShell::open_readonly(WorkbenchOptions::new(&root), binding).unwrap();
    fn settle(shell: &mut WorkbenchShell) {
        super::test_pump::settle_shell(shell);
    }
    fn view(shell: &mut WorkbenchShell, width: u16) -> String {
        let area = ratatui::layout::Rect::new(0, 0, width, 34);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        shell.readonly.as_mut().unwrap().render(
            area,
            &mut buffer,
            &crate::resolve_theme(None, None, &[]),
        );
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }
    // A newly opened tab shares the already captured source, even if its ref moves.
    settle(&mut shell);
    source_tests::git(&root, &["add", ".workdeck/features"]);
    source_tests::git(
        &root,
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "Advance proposal after opening",
        ],
    );
    shell.key(KeyEvent::new(KeyCode::F(11), KeyModifiers::SHIFT));
    settle(&mut shell);
    let text = view(&mut shell, 160);
    assert!(text.contains("Captured parent feature"), "{text}");
    assert!(!text.contains("Uncommitted feature only"));
    shell.key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    settle(&mut shell);
    let text = view(&mut shell, 160);
    assert!(text.contains("▾ Captured parent feature"), "{text}");
    assert!(text.contains("Captured child feature"));
    shell.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    shell.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    settle(&mut shell);
    assert!(!view(&mut shell, 160).contains("Captured child feature"));
    shell.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    settle(&mut shell);
    assert!(view(&mut shell, 160).contains("Captured child feature"));
    shell.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    shell.paste("parent");
    shell.key(KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE));
    settle(&mut shell);
    assert!(view(&mut shell, 62).contains("Captured project"));
    shell.key(KeyEvent::new(KeyCode::F(11), KeyModifiers::SHIFT));
    assert!(view(&mut shell, 160).contains("Filter captured planning"));
    assert!(view(&mut shell, 160).contains("parent"));
    shell.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    settle(&mut shell);
    assert!(!view(&mut shell, 160).contains("Captured child feature"));
    shell.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    settle(&mut shell);
    assert!(view(&mut shell, 160).contains("features/"));
    shell.key(KeyEvent::new(KeyCode::F(12), KeyModifiers::SHIFT));
    settle(&mut shell);
    assert!(view(&mut shell, 160).contains("Read-only planning · Activity"));
    shell.key(KeyEvent::new(KeyCode::F(11), KeyModifiers::SHIFT));
    assert!(view(&mut shell, 160).contains("features/"));
    shell.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    shell.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    shell.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    shell.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    settle(&mut shell);
    assert!(view(&mut shell, 160).contains("Uncommitted feature only"));
    assert!(shell.controller.repository().is_err());
    assert!(shell.controller.drafts().is_empty());
    shell.readonly.as_mut().unwrap().begin_shutdown();
}

#[test]
fn ref_reader_rejects_copied_authority_keeps_its_excerpt_and_recovers_only_the_original_binding() {
    let (directory, repository, _) = source_tests::shared_fixture();
    let root = directory.path().canonicalize().unwrap();
    let binding = inspect_checkout("accepted", &root, SourceSelector::Accepted).unwrap();
    let mut shell = WorkbenchShell::open_readonly(WorkbenchOptions::new(&root), binding).unwrap();
    fn view(shell: &mut WorkbenchShell) -> String {
        let area = ratatui::layout::Rect::new(0, 0, 160, 34);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        shell.readonly.as_mut().unwrap().render(
            area,
            &mut buffer,
            &crate::resolve_theme(None, None, &[]),
        );
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }
    super::test_pump::settle_shell(&mut shell);
    shell.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    super::test_pump::settle_shell(&mut shell);
    assert!(view(&mut shell).contains("Accepted body"));
    let original = repository.root().with_file_name("saved-planning");
    std::fs::rename(repository.root(), &original).unwrap();
    std::fs::create_dir(repository.root()).unwrap();
    std::fs::copy(
        original.join("config.yml"),
        repository.root().join("config.yml"),
    )
    .unwrap();
    shell.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    super::test_pump::settle_shell(&mut shell);
    let text = view(&mut shell);
    assert!(text.contains("stale retained view"), "{text}");
    assert!(text.contains("Accepted body"), "{text}");
    assert!(!repository.root().join(".index").exists());
    std::fs::remove_dir_all(repository.root()).unwrap();
    std::fs::rename(original, repository.root()).unwrap();
    shell.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    super::test_pump::settle_shell(&mut shell);
    let text = view(&mut shell);
    assert!(text.contains("Accepted body"));
    assert!(
        !text.contains("stale retained view"),
        "restored original source stayed in a worker error: {text}"
    );
    shell.readonly.as_mut().unwrap().begin_shutdown();
}
