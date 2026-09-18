#![cfg(unix)]
use serde_json::json;
use std::fs;
use workdeck_pm::*;

fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    fs::write(directory.path().join("source.txt"), "unchanged source").unwrap();
    let issue = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Check feedback", "Accepted scope"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    for (path, value) in [
        (
            "commands/unit.yml",
            json!({
                "schema":1,"repository":repository.identity(),"id":"unit","name":"Unit",
                "recipe":{"kind":"shell","interpreter":"sh","script":"printf '<testsuite name=\"unit\" tests=\"1\" failures=\"1\"><testcase name=\"contract\"><failure message=\"stale request rejected\"/></testcase></testsuite>' > \"$1\"; printf raw-output-marker; exit 1","args":[{"kind":"artifact","id":"report"}]},
                "cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],"inputs":{"files":["source.txt"]},
                "bounds":{"timeout_seconds":5,"stdout_bytes":1024,"stderr_bytes":1024},
                "artifacts":[{"id":"report","name":"unit.xml","max_bytes":4096,"required":true}]
            }),
        ),
        (
            "checks/unit.yml",
            json!({
                "schema":1,"repository":repository.identity(),"id":"unit","name":"Unit","command":"unit",
                "expectation":{"kind":"junit","artifact":"report","suites":["unit"],"minimum_tests":1,"maximum_skipped":null,"allowed_exit_codes":[0]}
            }),
        ),
    ] {
        let path = repository.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
    (directory, repository, issue)
}
fn run(repository: &Repository, issue: &IssueRecord) -> RunOutcome {
    let plan = repository
        .check_plan(&CheckPlanRequest {
            issue: Some(issue.metadata.id.to_string()),
            checks: vec!["unit".into()],
            ..Default::default()
        })
        .unwrap();
    repository
        .run_check_plan(
            &CheckRunRequest {
                expected_plan: plan.fingerprint.clone(),
                plan,
                actor: "local".into(),
            },
            &RequestId::new(),
            &RunControl::default(),
        )
        .unwrap()
}
fn summaries(packet: &ContextPacket) -> Vec<&ContextCheckRun> {
    packet
        .sections
        .iter()
        .flat_map(|section| &section.entries)
        .filter_map(|entry| match &entry.content {
            ContextContent::CheckRun { summary } => Some(summary),
            _ => None,
        })
        .collect()
}

#[test]
fn context_includes_actionable_local_failure_without_logs_and_keeps_requirements_stable() {
    let (_directory, repository, issue) = fixture();
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let before = repository.context(&request).unwrap();
    let outcome = run(&repository, &issue);
    let after = repository.context(&request).unwrap();
    assert_eq!(before.anchor.requirements, after.anchor.requirements);
    assert_ne!(before.anchor.fingerprint, after.anchor.fingerprint);
    let summaries = summaries(&after);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, outcome.run.intent.id);
    assert_eq!(summaries[0].state, RunState::Failed);
    assert!(summaries[0].failures.iter().any(|failure| {
        failure
            .diagnostic
            .message
            .contains("stale request rejected")
    }));
    assert!(
        !serde_json::to_string(&after)
            .unwrap()
            .contains("raw-output-marker")
    );
    assert!(
        after
            .sections
            .iter()
            .flat_map(|section| &section.entries)
            .any(|entry| {
                matches!(entry.content, ContextContent::CheckRun { .. })
                    && entry.citations.iter().any(|citation| {
                        citation.source.as_ref()
                            == outcome.results.as_ref().map(|record| &record.content)
                    })
            })
    );
    let actions = repository
        .next_actions(&NextActionRequest::new(issue.metadata.id.as_str()))
        .unwrap();
    assert!(
        actions
            .actions
            .iter()
            .any(|action| action.kind == NextActionKind::RunChecks && action.available)
    );
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
}

#[test]
fn missing_artifacts_change_context_anchor_and_cannot_remain_current_feedback() {
    let (_directory, repository, issue) = fixture();
    let outcome = run(&repository, &issue);
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let before = repository.context(&request).unwrap();
    let artifact = &outcome.results.as_ref().unwrap().result.invocations[0].artifacts[0].path;
    fs::remove_file(repository.root().join(artifact)).unwrap();
    let after = repository.context(&request).unwrap();
    assert_ne!(before.anchor.fingerprint, after.anchor.fingerprint);
    assert_eq!(before.anchor.requirements, after.anchor.requirements);
    assert_eq!(summaries(&after)[0].state, RunState::Unknown);
    assert!(
        summaries(&after)[0]
            .artifacts
            .iter()
            .any(|artifact| artifact.availability == ArtifactAvailability::Missing)
    );
    assert_eq!(
        repository
            .context(&ContextRequest {
                expected_context: Some(before.anchor.fingerprint),
                ..request
            })
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn context_counts_omitted_history_and_keeps_small_packets_within_their_real_byte_budget() {
    let (_directory, repository, issue) = fixture();
    for _ in 0..7 {
        run(&repository, &issue);
    }
    let packet = repository
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    let section = packet
        .sections
        .iter()
        .find(|section| section.kind == ContextSectionKind::Checks)
        .unwrap();
    assert_eq!(section.total, 7);
    assert_eq!(section.entries.len(), 5);
    assert_eq!(section.omitted, 2);
    assert!(
        section
            .omission_reasons
            .iter()
            .any(|reason| reason == "older_runs_not_assessed")
    );
    let budget = packet.budget.minimum_bytes + 128;
    let small = repository
        .context(&ContextRequest::new(issue.metadata.id.as_str(), budget))
        .unwrap();
    assert!(serde_json::to_vec(&small).unwrap().len() <= budget);
    assert!(
        small
            .sections
            .iter()
            .find(|section| section.kind == ContextSectionKind::Checks)
            .unwrap()
            .omitted
            >= 2
    );
}

#[test]
fn artifact_changes_during_capture_fail_instead_of_publishing_mixed_context() {
    let (_directory, repository, issue) = fixture();
    let outcome = run(&repository, &issue);
    let artifact = repository
        .root()
        .join(&outcome.results.as_ref().unwrap().result.invocations[0].artifacts[0].path);
    let error = repository
        .context_with_faults(
            &ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024),
            |_| {
                fs::remove_file(&artifact).unwrap();
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
}
