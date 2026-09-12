#![cfg(unix)]
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use workdeck_pm::*;

fn write(repo: &Repository, path: &str, value: &Value) {
    let path = repo.root().join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_yaml_ng::to_string(value).unwrap()).unwrap();
}
fn setup(script: &str, report: bool) -> (tempfile::TempDir, Repository, CheckRunRequest) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/input"), "first").unwrap();
    let mut argv = vec![
        json!({"kind":"literal","value":"sh"}),
        json!({"kind":"literal","value":"-c"}),
        json!({"kind":"literal","value":script}),
        json!({"kind":"literal","value":"fixture"}),
    ];
    let artifacts = if report {
        argv.push(json!({"kind":"artifact","id":"report"}));
        json!([{"id":"report","name":"report.xml","max_bytes":8192,"required":true}])
    } else {
        json!([])
    };
    write(
        &repo,
        "commands/fixture.yml",
        &json!({"schema":1,"repository":repo.identity(),"id":"fixture","name":"Fixture","recipe":{"kind":"argv","argv":argv},"cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],"inputs":{"trees":["src"]},"artifacts":artifacts,"bounds":{"timeout_seconds":2,"stdout_bytes":2048,"stderr_bytes":2048}}),
    );
    let plan = if report {
        write(
            &repo,
            "checks/unit.yml",
            &json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit","command":"fixture","expectation":{"kind":"junit","artifact":"report","suites":["unit"],"minimum_tests":1,"allowed_exit_codes":[0]}}),
        );
        repo.check_plan(&CheckPlanRequest {
            checks: vec!["unit".into()],
            ..Default::default()
        })
        .unwrap()
    } else {
        repo.command_plan(&CommandPlanRequest {
            command: "fixture".into(),
            arguments: BTreeMap::new(),
        })
        .unwrap()
    };
    let input = CheckRunRequest {
        expected_plan: plan.fingerprint.clone(),
        plan,
        actor: "tester".into(),
    };
    (temp, repo, input)
}
fn request(value: &str) -> RequestId {
    value.parse().unwrap()
}
fn lost(point: RunFaultPoint, wanted: RunFaultPoint) -> Result<()> {
    if point == wanted {
        Err(PmError::new(ErrorCode::Io, "fixture lost acknowledgement"))
    } else {
        Ok(())
    }
}
const PASS: &str = "printf '<testsuite name=\"unit\" tests=\"1\" failures=\"0\" errors=\"0\" skipped=\"0\"><testcase name=\"one\"/></testsuite>' > \"$1\"; printf x >> sentinel";

#[test]
fn canonical_intent_precedes_spawn_and_replay_never_duplicates_execution() {
    let (temp, repo, input) = setup("printf x >> sentinel", false);
    let control = RunControl::default();
    let result = repo
        .run_check_plan_with_faults(&input, &request("once"), &control, |point| {
            if point == RunFaultPoint::BeforeSpawn {
                assert!(!temp.path().join("sentinel").exists());
                let records = repo.check_results(&RunQuery::default()).unwrap();
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].state, RunState::Running);
                assert!(repo.root().join(&records[0].run.path).is_file());
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(result.state, RunState::Passed);
    assert!(control.cleanup_complete());
    assert_eq!(result.receipts.len(), 2);
    fs::write(temp.path().join("src/input"), "later").unwrap();
    let replay = repo
        .run_check_plan(&input, &request("once"), &control)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.run, result.run);
    assert_eq!(replay.results, result.results);
    assert_eq!(replay.assessment.state, RunState::Stale);
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
    let mut changed = input.clone();
    changed.actor = "someone-else".into();
    assert_eq!(
        repo.run_check_plan(&changed, &request("once"), &control)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
}
#[test]
fn interrupted_intent_is_unknown_and_retry_does_not_spawn() {
    let (temp, repo, input) = setup("printf x >> sentinel", false);
    let control = RunControl::default();
    let error = repo
        .run_check_plan_with_faults(&input, &request("intent-only"), &control, |point| {
            lost(point, RunFaultPoint::AfterIntent)
        })
        .unwrap_err();
    assert!(error.details.is_some());
    assert!(control.cleanup_complete());
    let replay = repo
        .run_check_plan(&input, &request("intent-only"), &control)
        .unwrap();
    assert_eq!(replay.state, RunState::Unknown);
    assert!(replay.results.is_none());
    assert!(!temp.path().join("sentinel").exists());
}
#[test]
fn terminal_journal_and_lost_publication_ack_recover_without_respawn() {
    for point in [
        RunFaultPoint::AfterResultJournal,
        RunFaultPoint::AfterResultPublication,
    ] {
        let (temp, repo, input) = setup(PASS, true);
        let control = RunControl::default();
        repo.run_check_plan_with_faults(&input, &request("recover"), &control, |actual| {
            lost(actual, point)
        })
        .unwrap_err();
        assert!(control.cleanup_complete());
        let replay = repo
            .run_check_plan(&input, &request("recover"), &control)
            .unwrap();
        assert_eq!(replay.state, RunState::Passed);
        assert!(replay.replayed);
        assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
        assert_eq!(
            replay.results.as_ref().unwrap().result.checks[0]
                .report
                .counts
                .passed,
            1
        );
    }
}
#[test]
fn process_and_report_and_current_artifacts_are_independent_dimensions() {
    let (_temp, repo, input) = setup(&format!("{PASS}; exit 9"), true);
    let outcome = repo
        .run_check_plan(&input, &request("failed-process"), &RunControl::default())
        .unwrap();
    assert_eq!(outcome.state, RunState::Failed);
    let result = outcome.results.unwrap();
    assert_eq!(result.result.checks[0].report.state, ReportState::Passed);
    assert_eq!(result.result.invocations[0].process.exit_code, Some(9));
    let (_temp, repo, input) = setup("exit 0", true);
    let outcome = repo
        .run_check_plan(&input, &request("missing"), &RunControl::default())
        .unwrap();
    assert_eq!(outcome.state, RunState::Unknown);
    assert_eq!(
        outcome.results.as_ref().unwrap().result.checks[0]
            .report
            .state,
        ReportState::Unknown
    );
    let (_temp, repo, input) = setup(PASS, true);
    let outcome = repo
        .run_check_plan(&input, &request("missing-after"), &RunControl::default())
        .unwrap();
    assert_eq!(outcome.state, RunState::Passed);
    let artifact = &outcome.results.as_ref().unwrap().result.invocations[0].artifacts[0];
    fs::remove_file(repo.root().join(&artifact.path)).unwrap();
    let current = repo.check_status(&outcome.run.intent.id).unwrap();
    assert_eq!(current.assessment.historical_state, RunState::Passed);
    assert_eq!(current.state, RunState::Unknown);
    assert_eq!(current.results, outcome.results);
}
#[test]
fn source_change_before_spawn_rejects_execution_after_durable_reservation() {
    let (temp, repo, input) = setup("printf x >> sentinel", false);
    let outcome = repo
        .run_check_plan_with_faults(&input, &request("stale"), &RunControl::default(), |point| {
            if point == RunFaultPoint::BeforeSpawn {
                fs::write(temp.path().join("src/new"), "new").unwrap();
            }
            Ok(())
        })
        .unwrap();
    assert_ne!(outcome.state, RunState::Passed);
    assert!(!temp.path().join("sentinel").exists());
    assert_eq!(
        outcome.results.as_ref().unwrap().result.invocations[0]
            .process
            .termination,
        ProcessTermination::NotRun
    );
}
#[test]
fn failed_final_journal_is_ambiguous_without_invented_result() {
    let (temp, repo, input) = setup("printf x >> sentinel", false);
    repo.run_check_plan_with_faults(&input, &request("lost"), &RunControl::default(), |point| {
        lost(point, RunFaultPoint::BeforeResultJournal)
    })
    .unwrap_err();
    let outcome = repo
        .run_check_plan(&input, &request("lost"), &RunControl::default())
        .unwrap();
    assert_eq!(outcome.state, RunState::Unknown);
    assert!(outcome.results.is_none());
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
}
#[test]
fn held_foreground_execution_releases_pm_writer_and_request_race_spawns_once() {
    let (temp, repo, input) = setup("printf x >> sentinel", false);
    let other = repo.clone();
    let retry = input.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        other.run_check_plan_with_faults(
            &retry,
            &request("compete"),
            &RunControl::default(),
            |point| {
                if point == RunFaultPoint::AfterSpawn {
                    ready_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                Ok(())
            },
        )
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let now = chrono::Utc::now();
    repo.create_issue(
        &CreateIssue::new("Concurrent mutation", ""),
        &request("issue-during-run"),
    )
    .unwrap();
    let replay = repo
        .run_check_plan(&input, &request("compete"), &RunControl::default())
        .unwrap();
    assert_eq!(replay.state, RunState::Running);
    assert!(replay.replayed);
    release_tx.send(()).unwrap();
    let first = worker.join().unwrap().unwrap();
    assert!(first.run.intent.recorded_at <= now);
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
}
#[test]
fn direct_forged_result_cannot_override_failed_process_or_receipt() {
    let (_temp, repo, input) = setup("exit 4", false);
    let outcome = repo
        .run_check_plan(&input, &request("proof"), &RunControl::default())
        .unwrap();
    assert_eq!(outcome.state, RunState::Failed);
    let record = outcome.results.unwrap();
    let mut value: Value = serde_yaml_ng::from_str(&record.document).unwrap();
    value["state"] = json!("passed");
    fs::write(
        repo.root().join(record.path),
        serde_yaml_ng::to_string(&value).unwrap(),
    )
    .unwrap();
    assert_eq!(
        repo.check_status(&outcome.run.intent.id).unwrap_err().code,
        ErrorCode::InvalidSchema
    );
}
#[test]
fn different_requests_do_not_deduplicate_equal_valued_execution() {
    let (temp, repo, input) = setup("printf x >> sentinel", false);
    let first = repo
        .run_check_plan(&input, &request("first"), &RunControl::default())
        .unwrap();
    let second = repo
        .run_check_plan(&input, &request("second"), &RunControl::default())
        .unwrap();
    assert_ne!(first.run.intent.id, second.run.intent.id);
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"xx");
    assert_eq!(repo.check_results(&RunQuery::default()).unwrap().len(), 2);
    assert!(Path::new(&first.run.intent.invocations[0].argv[0]).is_absolute());
}
#[test]
fn changed_inputs_during_child_execution_remain_historical_feedback_only() {
    let (temp, repo, input) = setup(PASS, true);
    let result = repo
        .run_check_plan_with_faults(
            &input,
            &request("changed-during"),
            &RunControl::default(),
            |point| {
                if point == RunFaultPoint::AfterSpawn {
                    fs::write(temp.path().join("src/input"), "changed during process").unwrap();
                }
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(result.state, RunState::Stale);
    let retained = result.results.unwrap();
    assert!(!retained.result.invocations[0].inputs_unchanged);
    assert_eq!(retained.result.checks[0].report.state, ReportState::Passed);
    assert_ne!(retained.result.state, RunState::Passed);
}
#[test]
fn unsafe_artifacts_and_missing_local_proof_never_qualify_retained_pass() {
    let (_temp, repo, input) = setup(PASS, true);
    let result = repo
        .run_check_plan(&input, &request("artifact-source"), &RunControl::default())
        .unwrap();
    let artifact = repo
        .root()
        .join(&result.results.as_ref().unwrap().result.invocations[0].artifacts[0].path);
    let original = fs::read(&artifact).unwrap();
    fs::remove_file(&artifact).unwrap();
    std::os::unix::fs::symlink("/dev/zero", &artifact).unwrap();
    let invalid = repo.check_status(&result.run.intent.id).unwrap();
    assert_eq!(invalid.state, RunState::Unknown);
    assert_eq!(
        invalid.assessment.artifacts[0].availability,
        ArtifactAvailability::Unsafe
    );
    fs::remove_file(&artifact).unwrap();
    fs::write(artifact, original).unwrap();
    fs::remove_file(
        repo.root()
            .join(".local/runs")
            .join(result.run.intent.id.as_str())
            .join("terminal.yml"),
    )
    .unwrap();
    let invalid = repo.check_status(&result.run.intent.id).unwrap();
    assert_eq!(invalid.state, RunState::Unknown);
    assert_eq!(invalid.assessment.historical_state, RunState::Passed);
}
#[test]
fn literal_arguments_and_explicit_environment_cannot_expand_into_ambient_shell_input() {
    let (temp, repo, _) = setup("printf '%s|%s' \"$1\" \"${HOME-unset}\"", false);
    let path = repo.root().join("commands/fixture.yml");
    let mut definition: Value = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    let literal = "$(touch INJECTED); 日本語 spaces";
    definition["recipe"]["argv"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"literal","value":literal}));
    write(&repo, "commands/fixture.yml", &definition);
    let plan = repo
        .command_plan(&CommandPlanRequest {
            command: "fixture".into(),
            arguments: BTreeMap::new(),
        })
        .unwrap();
    let input = CheckRunRequest {
        expected_plan: plan.fingerprint.clone(),
        plan,
        actor: "tester".into(),
    };
    let result = repo
        .run_check_plan(&input, &request("literal"), &RunControl::default())
        .unwrap();
    assert_eq!(result.state, RunState::Passed);
    let log = &result.results.as_ref().unwrap().result.invocations[0]
        .process
        .stdout;
    assert_eq!(
        fs::read_to_string(repo.root().join(&log.path)).unwrap(),
        format!("{literal}|unset")
    );
    assert!(!temp.path().join("INJECTED").exists());
}
#[test]
fn unbounded_aggregate_output_request_is_rejected_before_reservation() {
    let (_temp, repo, _) = setup("exit 0", false);
    let path = repo.root().join("commands/fixture.yml");
    let mut definition: Value = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    definition["artifacts"]=json!((0..3).map(|index|json!({"id":format!("large{index}"),"name":format!("large{index}.txt"),"max_bytes":32*1024*1024,"required":false})).collect::<Vec<_>>());
    write(&repo, "commands/fixture.yml", &definition);
    let plan = repo
        .command_plan(&CommandPlanRequest {
            command: "fixture".into(),
            arguments: BTreeMap::new(),
        })
        .unwrap();
    let input = CheckRunRequest {
        expected_plan: plan.fingerprint.clone(),
        plan,
        actor: "tester".into(),
    };
    assert_eq!(
        repo.run_check_plan(&input, &request("too-large"), &RunControl::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert!(!repo.root().join("runs").exists());
}
#[test]
fn doctor_detects_missing_authoritative_run_files_from_retained_receipts() {
    let (_temp, repo, input) = setup("exit 0", false);
    let result = repo
        .run_check_plan(
            &input,
            &request("retained-authority"),
            &RunControl::default(),
        )
        .unwrap();
    fs::remove_dir_all(repo.root().join("runs").join(result.run.intent.id.as_str())).unwrap();
    let doctor = repo.doctor().unwrap();
    assert!(
        !doctor.valid,
        "execution receipt authority cannot silently disappear when the run directory is removed"
    );
}
#[test]
fn interrupted_result_transaction_blocks_readers_and_recovers_without_respawn() {
    let (temp, repo, input) = setup(PASS, true);
    let control = RunControl::default();
    let error = repo
        .run_check_plan_with_faults(&input, &request("partial-publication"), &control, |point| {
            lost(point, RunFaultPoint::AfterResultFile)
        })
        .unwrap_err();
    assert!(error.details.is_some());
    assert!(control.cleanup_complete());
    assert_eq!(
        repo.check_results(&RunQuery::default()).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    let pending = repo.pending_operations().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].receipt.operation, "execution.finish");
    repo.recover_operations().unwrap();
    let replay = repo
        .run_check_plan(&input, &request("partial-publication"), &control)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.state, RunState::Passed);
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
}
#[test]
fn timeout_and_cancel_provide_actionable_reasons_and_acknowledged_cleanup() {
    let (_temp, repo, input) = setup("/bin/sleep 8", false);
    let control = RunControl::default();
    let timeout = repo
        .run_check_plan(&input, &request("timeout"), &control)
        .unwrap();
    assert_eq!(timeout.state, RunState::Unknown);
    assert!(control.cleanup_complete());
    assert!(
        timeout
            .assessment
            .reason_codes
            .iter()
            .any(|reason| reason == "process_timeout")
    );
    let canceled = repo
        .run_check_plan_with_faults(&input, &request("cancel"), &control, |point| {
            if point == RunFaultPoint::AfterSpawn {
                control.force_cancel();
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(canceled.state, RunState::Canceled);
    assert!(control.cleanup_complete());
    assert!(
        canceled
            .assessment
            .reason_codes
            .iter()
            .any(|reason| reason == "process_canceled")
    );
}
