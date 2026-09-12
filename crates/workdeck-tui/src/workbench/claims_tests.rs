use super::{claims_workspace::*, *};
use crate::{ReviewApp, ReviewOptions};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    fs,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use workdeck_pm::*;

#[cfg(unix)]
#[allow(dead_code)]
#[path = "../../../workdeck-pm/tests/support/red_green_fixture.rs"]
mod green_support;

fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Claim this work", "Accepted work"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    (directory, repository, issue)
}
fn workspace(repository: &Repository, issue: &IssueRecord) -> ClaimsWorkspace {
    let mut workspace = ClaimsWorkspace::new(
        Some(repository.clone()),
        "local".into(),
        Arc::new(ForegroundRunSignal::default()),
    );
    workspace.open(Some(issue.metadata.id.clone()), None);
    workspace
}
fn finish(workspace: &mut ClaimsWorkspace) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while workspace.busy() {
        assert!(Instant::now() < deadline, "{:?}", workspace.state());
        workspace.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[cfg(unix)]
fn finish_slow(workspace: &mut ClaimsWorkspace) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while workspace.busy() {
        assert!(
            Instant::now() < deadline,
            "verified completion did not join"
        );
        workspace.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn explicit_acquire_renew_and_release_keep_issue_source_and_current_assessment_separate() {
    let (_directory, repository, issue) = fixture();
    let mut workspace = workspace(&repository, &issue);
    assert!(repository.claims().unwrap().is_empty());
    workspace.begin(ClaimAction::Acquire).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    let first = workspace.state().unwrap().outcome.clone().unwrap();
    assert_eq!(
        first.current.as_ref().unwrap().assessment.guarantee,
        ClaimGuarantee::LocalSourceOnly
    );
    assert!(first.requested_token_current);
    workspace.begin(ClaimAction::Renew).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    let renewed = workspace.state().unwrap().outcome.clone().unwrap();
    assert_ne!(renewed.request_id, first.request_id);
    workspace.begin(ClaimAction::Release).unwrap();
    workspace.state_mut().draft.as_mut().unwrap().form.fields[1].value =
        "Finished this foreground session".into();
    workspace.submit().unwrap();
    finish(&mut workspace);
    let released = workspace.state().unwrap().outcome.clone().unwrap();
    assert_eq!(
        released.current.as_ref().unwrap().claim.metadata.state,
        ClaimState::Released
    );
    assert!(!released.may_continue);
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
    assert!(!workspace.signal.is_active());
}

#[test]
fn lost_ack_retries_exact_input_after_later_edits_and_refresh_does_not_rebase_it() {
    let (_directory, repository, issue) = fixture();
    let mut workspace = workspace(&repository, &issue);
    workspace.begin(ClaimAction::Acquire).unwrap();
    workspace
        .submit_with(|repository, input, request| {
            repository.mutate_claim(&input, &request)?;
            Err(PmError::new(
                ErrorCode::Io,
                "acknowledgement lost after committed claim",
            ))
        })
        .unwrap();
    finish(&mut workspace);
    assert_eq!(
        workspace.state().unwrap().error.as_ref().unwrap().code,
        ErrorCode::Io
    );
    let draft = workspace.state().unwrap().draft.as_ref().unwrap();
    let request = draft.request.clone().unwrap();
    let input = draft.input.clone().unwrap();
    let contract = draft.contract.clone();
    let committed = repository.claims().unwrap()[0].clone();
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: Default::default(),
                body: Some("Later accepted work".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    workspace.refresh().unwrap();
    let draft = workspace.state().unwrap().draft.as_ref().unwrap();
    assert_eq!(draft.request.as_ref(), Some(&request));
    assert_eq!(draft.input.as_ref(), Some(&input));
    assert_eq!(draft.contract, contract);
    workspace.submit().unwrap();
    finish(&mut workspace);
    let outcome = workspace.state().unwrap().outcome.as_ref().unwrap();
    assert_eq!(outcome.request_id, request);
    assert_eq!(
        outcome.current.as_ref().unwrap().claim.source,
        committed.claim.source
    );
    assert_eq!(outcome.receipt.as_ref().unwrap().request_id, request);
    workspace.key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    workspace.state_mut().draft.as_mut().unwrap().form.fields[0].value = "another actor".into();
    assert_eq!(
        workspace.submit().unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(
        workspace
            .state()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .request
            .as_ref(),
        Some(&request)
    );
}

#[test]
fn repository_replacement_retains_original_contract_and_request_without_creating_a_claim() {
    let (_directory, repository, issue) = fixture();
    let mut workspace = workspace(&repository, &issue);
    workspace.begin(ClaimAction::Acquire).unwrap();
    let original = workspace
        .state()
        .unwrap()
        .draft
        .as_ref()
        .unwrap()
        .contract
        .clone();
    let mut config = repository.config().unwrap();
    config.repository = RepositoryId::new();
    std::fs::write(
        repository.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    let state = workspace.state().unwrap();
    assert_eq!(state.error.as_ref().unwrap().code, ErrorCode::StaleSource);
    assert_eq!(state.draft.as_ref().unwrap().contract, original);
    assert!(state.draft.as_ref().unwrap().request.is_some());
    assert!(!repository.root().join("claims").exists());
}

#[test]
fn mounted_publication_survives_tabs_defers_quit_and_signals_then_preserves_outcome() {
    let (directory, repository, issue) = fixture();
    let mut app = ReviewApp::new(
        workdeck_diff::changeset_from_patch(
            "",
            "Claims",
            "Claims",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        ),
        ReviewOptions {
            repo: Some(directory.path().into()),
            workbench: Some(WorkbenchOptions::new(directory.path())),
            highlight: false,
            ..ReviewOptions::default()
        },
    );
    for code in [
        KeyCode::F(3),
        KeyCode::Char('i'),
        KeyCode::Char('7'),
        KeyCode::Char('n'),
    ] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        super::test_pump::settle_app(&mut app);
    }
    let (started, ready) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let signal;
    {
        let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
        let workspace = &mut shell.context.claims;
        assert_eq!(workspace.current.as_ref(), Some(&issue.metadata.id));
        signal = workspace.signal.clone();
        workspace
            .submit_with(move |repository, input, request| {
                started.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(5)).unwrap();
                repository.mutate_claim(&input, &request)
            })
            .unwrap();
    }
    ready.recv_timeout(Duration::from_secs(1)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    assert_eq!(
        app.workbench.as_ref().unwrap().lock().unwrap().tab,
        WorkbenchTab::Review
    );
    assert!(signal.interrupt_active());
    assert!(signal.interrupt_active());
    assert!(app.defer_foreground_run_suspend());
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(!app.take_quit_requested());
    assert!(signal.is_active());
    assert!(signal.is_publication());
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while signal.is_active() {
        assert!(Instant::now() < deadline);
        app.poll_workbench();
        std::thread::sleep(Duration::from_millis(5));
    }
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert!(
        shell
            .context
            .claims
            .state()
            .unwrap()
            .outcome
            .as_ref()
            .unwrap()
            .receipt
            .is_some()
    );
    assert_eq!(repository.claims().unwrap().len(), 1);
}

#[test]
fn claims_from_accepted_citation_retain_that_contract_and_proposal_cannot_reuse_it() {
    let (directory, repository, issue) = super::source_tests::shared_fixture();
    let remote = tempfile::tempdir().unwrap();
    super::source_tests::git(remote.path(), &["init", "--bare", "--quiet"]);
    super::source_tests::git(
        directory.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let mut app = ReviewApp::new(
        workdeck_diff::changeset_from_patch(
            "",
            "Claims",
            "Claims",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        ),
        ReviewOptions {
            repo: Some(directory.path().into()),
            workbench: Some(WorkbenchOptions::new(directory.path())),
            highlight: false,
            ..ReviewOptions::default()
        },
    );
    for code in [
        KeyCode::F(3),
        KeyCode::Char('i'),
        KeyCode::Char('6'),
        KeyCode::Char('a'),
        KeyCode::Enter,
        KeyCode::Char('7'),
        KeyCode::Char('n'),
    ] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        super::test_pump::settle_app(&mut app);
    }
    let contract = app
        .workbench
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .context
        .claims
        .state()
        .unwrap()
        .draft
        .as_ref()
        .unwrap()
        .contract
        .clone();
    assert_eq!(contract.issue_source, issue.source);
    assert_eq!(contract.accepted_source.role, SourceRole::Accepted);
    for code in [KeyCode::F(2), KeyCode::F(3)] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        super::test_pump::settle_app(&mut app);
    }
    assert_eq!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .context
            .claims
            .state()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .contract,
        contract
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    for code in [KeyCode::Char('6'), KeyCode::Char('p')] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        super::test_pump::settle_app(&mut app);
    }
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
    for code in [KeyCode::Enter, KeyCode::Enter, KeyCode::Char('7')] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        super::test_pump::settle_app(&mut app);
    }
    let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert!(shell.context.claims.state().unwrap().contract.is_none());
    assert!(shell.context.claims.begin(ClaimAction::Acquire).is_err());
    assert!(!repository.root().join("claims").exists());
}

#[test]
fn explicit_claimed_completion_opens_a_separate_complete_and_release_intent() {
    let (_directory, repository, issue) = fixture();
    let mut workspace = workspace(&repository, &issue);
    workspace.begin(ClaimAction::Acquire).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    workspace.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    assert!(
        workspace
            .form()
            .is_some_and(|form| form.title.contains("Complete and release"))
    );
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
    assert_eq!(
        repository.claims().unwrap()[0].claim.metadata.state,
        ClaimState::Active
    );
}

#[cfg(unix)]
#[test]
fn authenticated_claimed_completion_has_a_first_class_tui_control() {
    let (root, repository, pair) = green_support::green_only_fixture_with_gate("fixed");
    let imported: ImportedCheckReportRecord = serde_json::from_value(
        repository
            .import_check_report(
                &ImportCheckReportRequest {
                    envelope: serde_json::to_string(&pair.green).unwrap(),
                    policy: pair.producer_policy.clone(),
                    expected_policy: pair.expected_producer_policy.clone(),
                    expected_commit: pair.candidate.clone(),
                    actor: "fixture".into(),
                    red_green: None,
                },
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let issue = repository.list_issues().unwrap().remove(0);
    let gate = repository.gates().unwrap().remove(0);
    let report = repository
        .verify_imported_check(&VerifyImportedCheck {
            attestation: imported.record.id.clone(),
            expected_attestation: imported.content.clone(),
            check: "unit".into(),
            candidate: pair.candidate.clone(),
            policy: pair.producer_policy.clone(),
            expected_policy: pair.expected_producer_policy.clone(),
            red_green: None,
        })
        .unwrap();
    let declaration: DeclareEvidence = serde_json::from_value(serde_json::json!({
        "criterion": gate.definition.requirements[0].criterion,
        "subject": {"repository":repository.identity(),"kind":"source","content":report.source.content},
        "producer": report.producer,
        "check": report.report.publication.result.result.checks[0].check,
        "result": {"id":report.report.publication.intent.intent.id,"content":report.report.fingerprint},
        "observed_at": report.report.observed_at,
        "provenance": {"actor":"fixture","reason":"TUI claimed green-only result"},
        "links": [{"kind":"attestation","id":imported.record.id,"content":imported.content}]
    })).unwrap();
    let evidence: EvidenceRecord = serde_json::from_value(
        repository
            .declare_evidence(&declaration, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let authority = CompletionAuthority {
        candidate: pair.candidate,
        policy: pair.producer_policy,
        expected_policy: pair.expected_producer_policy,
        red_green: None,
    };
    let verification = CompleteVerifiedIssue {
        issue: issue.metadata.id.clone(),
        expected_issue: issue.source.clone(),
        actor: "worker".into(),
        authority,
        checks: vec![CompletionCheckSelection {
            check: "unit".into(),
            attestation: imported.record.id,
            expected_attestation: imported.content,
        }],
        gates: vec![CompletionGateSelection {
            gate: gate.definition.id,
            expected_gate: gate.source,
            evidence: ["behavior", "also-required"]
                .into_iter()
                .map(|requirement| GateEvidenceSelection {
                    requirement: requirement.into(),
                    evidence: evidence.reference.id.clone(),
                    expected_evidence: evidence.content.clone(),
                })
                .collect(),
        }],
    };
    fs::write(
        root.path().join("verification.json"),
        serde_json::to_vec(&verification).unwrap(),
    )
    .unwrap();
    let contract = repository.claim_contract(&issue.metadata.id).unwrap();
    repository.save_claim_contract(&contract).unwrap();
    let mut workspace = ClaimsWorkspace::new(
        Some(repository.clone()),
        "worker".into(),
        Arc::new(ForegroundRunSignal::default()),
    );
    workspace.open(Some(issue.metadata.id.clone()), None);
    workspace.begin(ClaimAction::Acquire).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    workspace.begin(ClaimAction::CompleteVerified).unwrap();
    assert!(
        workspace
            .form()
            .is_some_and(|form| form.title.contains("Complete with verification"))
    );
    workspace.state_mut().draft.as_mut().unwrap().form.fields[1].value = "verification.json".into();
    workspace.submit().unwrap();
    finish_slow(&mut workspace);
    let state = workspace.state().unwrap();
    assert!(state.completion_verified);
    assert_eq!(
        state.status.as_ref().unwrap().claim.metadata.state,
        ClaimState::Active
    );
    assert!(!state.completion.as_ref().unwrap().release_recorded);
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .status,
        "done"
    );
    let proof: ClaimedCompletionProof =
        serde_json::from_value(state.completion.as_ref().unwrap().completion.result.clone())
            .unwrap();
    assert!(proof.verification.is_some());
}

#[test]
fn completed_issue_with_lost_release_ack_retains_both_requests_and_retries_only_that_release() {
    let (_directory, repository, issue) = fixture();
    let mut workspace = workspace(&repository, &issue);
    workspace.begin(ClaimAction::Acquire).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    workspace.begin(ClaimAction::Complete).unwrap();
    workspace.state_mut().draft.as_mut().unwrap().form.fields[1].value = "Work is complete".into();
    workspace
        .submit_completion_with(|repository, input, completion, release, reason| {
            repository.complete_claimed_issue_and_release_with_faults(
                &input,
                &completion,
                &release,
                &reason,
                |_| {
                    Err(PmError::new(
                        ErrorCode::Io,
                        "release acknowledgement unavailable",
                    ))
                },
            )
        })
        .unwrap();
    finish(&mut workspace);
    let first = workspace.state().unwrap().completion.clone().unwrap();
    assert!(!first.release_recorded);
    assert!(first.release_error.is_some());
    let completed = repository.show_issue(issue.metadata.id.as_str()).unwrap();
    assert_eq!(completed.metadata.status, "done");
    let draft = workspace.state().unwrap().draft.as_ref().unwrap();
    let completion_request = draft.request.clone().unwrap();
    let release_request = draft.release_request.clone().unwrap();
    assert_ne!(completion_request, release_request);
    workspace.key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    workspace.state_mut().draft.as_mut().unwrap().form.fields[1].value =
        "Changed retry reason".into();
    assert_eq!(
        workspace.submit().unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    workspace.state_mut().draft.as_mut().unwrap().form.fields[1].value = "Work is complete".into();
    workspace.submit().unwrap();
    finish(&mut workspace);
    let final_result = workspace.state().unwrap().completion.as_ref().unwrap();
    assert_eq!(final_result.completion, first.completion);
    assert!(final_result.release_recorded);
    assert_eq!(
        final_result.release.as_ref().unwrap().request_id,
        release_request
    );
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        completed.source
    );
    assert_eq!(
        repository.claims().unwrap()[0].claim.metadata.state,
        ClaimState::Released
    );
}

#[test]
fn fresh_revalidation_action_uses_new_observation_without_rebasing_the_prior_request() {
    let (_directory, repository, issue) = fixture();
    let mut workspace = workspace(&repository, &issue);
    workspace.begin(ClaimAction::Acquire).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    let original = workspace
        .state()
        .unwrap()
        .draft
        .as_ref()
        .unwrap()
        .contract
        .clone();
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: Default::default(),
                body: Some("Changed accepted requirements".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    workspace.refresh().unwrap();
    assert_eq!(
        workspace.state().unwrap().draft.as_ref().unwrap().contract,
        original
    );
    workspace.begin(ClaimAction::Revalidate).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    assert!(
        workspace.state().unwrap().error.is_none(),
        "{:?}",
        workspace.state()
    );
    assert_ne!(
        workspace
            .state()
            .unwrap()
            .outcome
            .as_ref()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .claim
            .metadata
            .contract
            .issue_source,
        original.issue_source
    );
}

#[test]
fn frozen_source_claim_draft_rejects_a_changed_mirrored_remote_and_keeps_its_binding() {
    let (directory, remote, repository, issue) = super::source_action_tests::fixture();
    let mirror = tempfile::tempdir().unwrap();
    super::source_tests::git(
        mirror.path(),
        &[
            "clone",
            "--mirror",
            remote.path().to_str().unwrap(),
            "mirror.git",
        ],
    );
    let mirror_path = mirror.path().join("mirror.git");
    let mut sources = super::sources_workspace::SourcesWorkspace::new(
        directory.path().into(),
        Some(repository.identity().clone()),
    );
    sources.select(SourceSelector::Accepted);
    sources.open_selected().unwrap();
    let inspected = sources.selected_claim().unwrap();
    let binding = inspected.1.clone().unwrap();
    super::source_tests::git(
        directory.path(),
        &["remote", "set-url", "origin", mirror_path.to_str().unwrap()],
    );
    sources.refresh().unwrap();
    assert_eq!(sources.selected_claim().unwrap().1.as_ref(), Some(&binding));
    assert_ne!(
        sources.state().unwrap().view.publication_binding(),
        Some(&binding)
    );
    let mut workspace = ClaimsWorkspace::new(
        Some(repository.clone()),
        "local".into(),
        Arc::new(ForegroundRunSignal::default()),
    );
    workspace.open(Some(issue.metadata.id), Some(Ok(inspected)));
    workspace.begin(ClaimAction::Acquire).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    assert_eq!(
        workspace.state().unwrap().error.as_ref().unwrap().code,
        ErrorCode::StaleSource
    );
    let request = workspace
        .state()
        .unwrap()
        .draft
        .as_ref()
        .unwrap()
        .request
        .clone();
    let _ = workspace.refresh(); // No coordination ref exists; current status is explicitly unknown.
    assert_eq!(
        workspace
            .state()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .binding
            .as_ref(),
        Some(&binding)
    );
    workspace.submit().unwrap();
    finish(&mut workspace);
    assert_eq!(
        workspace.state().unwrap().error.as_ref().unwrap().code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        workspace.state().unwrap().draft.as_ref().unwrap().request,
        request
    );
    assert!(!repository.root().join("claims").exists());
    assert!(
        super::source_tests::git(
            mirror_path.as_path(),
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/workdeck-coordination"
            ]
        )
        .is_empty()
    );
    assert!(
        super::source_tests::git(
            remote.path(),
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/workdeck-coordination"
            ]
        )
        .is_empty()
    );
}

#[test]
fn inspected_local_mutation_does_not_switch_to_shared_publication_after_config_changes() {
    let (_directory, repository, issue) = fixture();
    let mut workspace = workspace(&repository, &issue);
    workspace.begin(ClaimAction::Acquire).unwrap();
    workspace.submit().unwrap();
    finish(&mut workspace);
    workspace.begin(ClaimAction::Renew).unwrap();
    assert!(
        workspace
            .state()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .binding
            .is_none()
    );
    let before = std::fs::read(
        repository
            .root()
            .join("claims")
            .join(format!("{}.yml", issue.metadata.id)),
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
    let operations = std::fs::read_dir(repository.root().join("operations"))
        .unwrap()
        .count();
    workspace.submit().unwrap();
    finish(&mut workspace);
    assert_eq!(
        workspace.state().unwrap().error.as_ref().unwrap().code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(
        std::fs::read(
            repository
                .root()
                .join("claims")
                .join(format!("{}.yml", issue.metadata.id))
        )
        .unwrap(),
        before
    );
    assert_eq!(
        std::fs::read_dir(repository.root().join("operations"))
            .unwrap()
            .count(),
        operations
    );
}
