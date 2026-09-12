#![cfg(unix)]
use serde_json::json;
use std::{fs, path::Path, process::Command};
use workdeck_pm::*;

fn git(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
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
}
fn commit(root: &Path) {
    git(root, &["add", "--", ".workdeck", "src"]);
    git(
        root,
        &[
            "-c",
            "user.name=CI Test",
            "-c",
            "user.email=ci@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "fixture",
        ],
    );
}
fn setup(script: &str) -> (tempfile::TempDir, Repository, CiCheckRunRequest) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    let repo = Repository::init(root.path(), "WD").unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/input"), "candidate\n").unwrap();
    for (path, value) in [
        (
            "commands/unit.yml",
            json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit",
            "recipe":{"kind":"argv","argv":[{"kind":"literal","value":"sh"},{"kind":"literal","value":"-c"},{"kind":"literal","value":script}]},
            "cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],"inputs":{"trees":["src"]},
            "bounds":{"timeout_seconds":2,"stdout_bytes":2048,"stderr_bytes":2048}}),
        ),
        (
            "checks/unit.yml",
            json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit",
            "command":"unit","expectation":{"kind":"process","allowed_exit_codes":[0]},"evaluator_inputs":{}}),
        ),
    ] {
        fs::create_dir_all(repo.root().join(path).parent().unwrap()).unwrap();
        fs::write(repo.root().join(path), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    commit(root.path());
    let source = ci_validate(
        root.path(),
        &CiValidateRequest {
            base: CiRevision::Head {},
            head: CiRevision::Head {},
        },
    )
    .unwrap()
    .head;
    let plan = repo
        .check_plan(&CheckPlanRequest {
            checks: vec!["unit".into()],
            ..Default::default()
        })
        .unwrap();
    let binding = bind_ci_check_plan(root.path(), &source, &plan).unwrap();
    let input = CiCheckRunRequest {
        expected_binding: binding.fingerprint.clone(),
        binding,
        input: CheckRunRequest {
            expected_plan: plan.fingerprint.clone(),
            plan,
            actor: "tester".into(),
        },
    };
    (root, repo, input)
}

#[test]
fn revision_binding_is_durable_before_spawn_and_replays_original_run() {
    let (root, repo, input) =
        setup("read value < src/input; test \"$value\" = candidate; printf x >> sentinel");
    let request = RequestId::new();
    let control = RunControl::default();
    let result = repo
        .run_ci_check_plan_with_faults(&input, &request, &control, |point| {
            if point == RunFaultPoint::BeforeSpawn {
                assert!(!root.path().join("sentinel").exists());
                let runs = repo.check_results(&RunQuery::default()).unwrap();
                assert_eq!(runs.len(), 1);
                assert_eq!(runs[0].run.intent.revision.as_ref(), Some(&input.binding));
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(result.state, RunState::Passed);
    assert_eq!(
        result.results.as_ref().unwrap().result.basis,
        "local_feedback"
    );
    fs::write(root.path().join("src/input"), "later\n").unwrap();
    let replay = repo.run_ci_check_plan(&input, &request, &control).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.run, result.run);
    assert_eq!(replay.results, result.results);
    assert_eq!(replay.assessment.state, RunState::Stale);
    assert_eq!(fs::read(root.path().join("sentinel")).unwrap(), b"x");
}

#[test]
fn wrong_or_forged_binding_cannot_reserve_or_spawn() {
    let (root, repo, input) = setup("printf x >> sentinel");
    let control = RunControl::default();
    let mut wrong = input.clone();
    wrong.expected_binding = ContentHash::of(b"wrong");
    assert_eq!(
        repo.run_ci_check_plan(&wrong, &RequestId::new(), &control)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert!(control.cleanup_complete());
    let mut forged = input.clone();
    forged.binding.inputs[0].entries.clear();
    assert_eq!(
        repo.run_ci_check_plan(&forged, &RequestId::new(), &control)
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
    assert!(repo.check_results(&RunQuery::default()).unwrap().is_empty());
    assert!(!root.path().join("sentinel").exists());
}

#[test]
fn revision_and_ordinary_local_requests_cannot_alias() {
    let (root, repo, input) = setup("printf x >> sentinel");
    let request = RequestId::new();
    repo.run_ci_check_plan(&input, &request, &RunControl::default())
        .unwrap();
    assert_eq!(
        repo.run_check_plan(&input.input, &request, &RunControl::default())
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let other = RequestId::new();
    let ordinary = repo
        .run_check_plan(&input.input, &other, &RunControl::default())
        .unwrap();
    assert!(ordinary.run.intent.revision.is_none());
    assert!(
        serde_json::to_value(&ordinary.run.intent)
            .unwrap()
            .get("revision")
            .is_none()
    );
    assert_eq!(
        repo.run_ci_check_plan(&input, &other, &RunControl::default())
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(fs::read(root.path().join("sentinel")).unwrap(), b"xx");
}

#[test]
fn interrupted_revision_run_recovers_without_a_second_execution() {
    for point in [
        RunFaultPoint::AfterIntent,
        RunFaultPoint::AfterResultJournal,
    ] {
        let (root, repo, input) = setup("printf x >> sentinel");
        let request = RequestId::new();
        let result = repo.run_ci_check_plan_with_faults(
            &input,
            &request,
            &RunControl::default(),
            |current| {
                if current == point {
                    Err(PmError::new(ErrorCode::Io, "lost acknowledgement"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
        let recovered = repo
            .run_ci_check_plan(&input, &request, &RunControl::default())
            .unwrap();
        assert!(recovered.replayed);
        assert_eq!(recovered.run.intent.revision.as_ref(), Some(&input.binding));
        if point == RunFaultPoint::AfterIntent {
            assert_eq!(recovered.state, RunState::Unknown);
            assert!(!root.path().join("sentinel").exists());
        } else {
            assert_eq!(recovered.state, RunState::Passed);
            assert_eq!(fs::read(root.path().join("sentinel")).unwrap(), b"x");
        }
        assert!(repo.doctor().unwrap().valid);
    }
}

#[test]
fn revision_input_change_before_spawn_never_runs_the_process() {
    let (root, repo, input) = setup("printf x >> sentinel");
    let result = repo
        .run_ci_check_plan_with_faults(&input, &RequestId::new(), &RunControl::default(), |point| {
            if point == RunFaultPoint::BeforeSpawn {
                fs::write(root.path().join("src/input"), "changed").unwrap();
            }
            Ok(())
        })
        .unwrap();
    assert_ne!(result.state, RunState::Passed);
    assert!(!root.path().join("sentinel").exists());
    assert_eq!(result.run.intent.revision.as_ref(), Some(&input.binding));
}

#[test]
fn cancellation_and_during_run_input_changes_cannot_qualify_a_revision() {
    let (root, repo, input) = setup("printf changed > src/input");
    let result = repo
        .run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
        .unwrap();
    assert_eq!(result.state, RunState::Stale);
    assert_eq!(result.results.unwrap().result.basis, "local_feedback");
    drop(root);
    let (root, repo, input) = setup("printf x >> sentinel");
    let control = RunControl::default();
    let result = repo
        .run_ci_check_plan_with_faults(&input, &RequestId::new(), &control, |point| {
            if point == RunFaultPoint::BeforeSpawn {
                control.cancel();
            }
            Ok(())
        })
        .unwrap();
    assert_ne!(result.state, RunState::Passed);
    assert!(control.cleanup_complete());
    assert!(!root.path().join("sentinel").exists());
}

#[test]
fn prepared_ci_check_rejects_dirty_issue_contracts_and_omitted_evaluator_declarations() {
    let (root, repo, _) = setup("exit 0");
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue::new("Subject", "Accepted body"),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let original = fs::read_to_string(repo.root().join(&issue.path)).unwrap();
    commit(root.path());
    let request = CheckPlanRequest {
        checks: vec!["unit".into()],
        issue: Some(issue.metadata.id.to_string()),
        ..Default::default()
    };
    prepare_ci_check(root.path(), &CiRevision::Head {}, &request).unwrap();
    fs::write(
        repo.root().join(&issue.path),
        format!("{original}\nDirty body\n"),
    )
    .unwrap();
    assert_eq!(
        prepare_ci_check(root.path(), &CiRevision::Head {}, &request)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    fs::write(repo.root().join(&issue.path), &original).unwrap();
    let path = repo.root().join("checks/unit.yml");
    let mut definition: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    definition
        .as_object_mut()
        .unwrap()
        .remove("evaluator_inputs");
    fs::write(path, serde_json::to_vec(&definition).unwrap()).unwrap();
    commit(root.path());
    assert!(repo.check_plan(&request).is_ok());
    assert_eq!(
        prepare_ci_check(root.path(), &CiRevision::Head {}, &request)
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
}

#[test]
fn supplied_ci_run_cannot_bypass_evaluator_declaration_admission() {
    let (root, repo, _) = setup("printf x >> sentinel");
    let path = repo.root().join("checks/unit.yml");
    let mut definition: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    definition
        .as_object_mut()
        .unwrap()
        .remove("evaluator_inputs");
    fs::write(path, serde_json::to_vec(&definition).unwrap()).unwrap();
    commit(root.path());
    let source = ci_validate(
        root.path(),
        &CiValidateRequest {
            base: CiRevision::Head {},
            head: CiRevision::Head {},
        },
    )
    .unwrap()
    .head;
    let plan = repo
        .check_plan(&CheckPlanRequest {
            checks: vec!["unit".into()],
            ..Default::default()
        })
        .unwrap();
    let binding = bind_ci_check_plan(root.path(), &source, &plan).unwrap();
    let input = CiCheckRunRequest {
        expected_binding: binding.fingerprint.clone(),
        binding,
        input: CheckRunRequest {
            expected_plan: plan.fingerprint.clone(),
            plan,
            actor: "tester".into(),
        },
    };
    assert_eq!(
        repo.run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert!(!root.path().join("sentinel").exists());
}

#[test]
fn supplied_ci_binding_cannot_bypass_invalid_committed_planning() {
    let (root, repo, _) = setup("printf x >> sentinel");
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Broken source", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    fs::write(repo.root().join(&issue.path), "---\ninvalid: [\n---\n").unwrap();
    commit(root.path());
    let validation = ci_validate(
        root.path(),
        &CiValidateRequest {
            base: CiRevision::Head {},
            head: CiRevision::Head {},
        },
    )
    .unwrap();
    assert!(!validation.valid);
    let plan = repo
        .check_plan(&CheckPlanRequest {
            checks: vec!["unit".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        bind_ci_check_plan(root.path(), &validation.head, &plan)
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert!(!root.path().join("sentinel").exists());
}

#[test]
fn exported_report_preserves_revision_provenance_and_survives_checkout_removal() {
    let (root, repo, input) = setup("exit 0");
    let outcome = repo
        .run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
        .unwrap();
    let report = repo.export_check_report(&outcome.run.intent.id).unwrap();
    assert_eq!(report.basis, VerificationBasis::LocalFeedback);
    assert_eq!(
        report.publication.intent.intent.revision.as_ref(),
        Some(&input.binding)
    );
    assert_eq!(report.publication.result.result.checks.len(), 1);
    assert_eq!(report.observation.state, RunState::Passed);
    let bytes = serde_json::to_vec(&report).unwrap();
    drop(repo);
    drop(root);
    assert_eq!(CheckReport::from_json(&bytes).unwrap(), report);
    let mut changed = report.clone();
    changed.publication.intent.intent.input.actor = "forged".into();
    assert!(changed.validate().is_err());
    let mut document = serde_json::to_value(&report).unwrap();
    document["basis"] = json!("trusted_ci");
    assert!(CheckReport::from_json(&serde_json::to_vec(&document).unwrap()).is_err());
}

#[test]
fn report_export_preserves_failed_and_stale_results_and_rejects_unfinished_runs() {
    for script in ["exit 0", "exit 7"] {
        let (root, repo, input) = setup(script);
        let result = repo
            .run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
            .unwrap();
        let report = repo.export_check_report(&result.run.intent.id).unwrap();
        assert_eq!(report.observation.state, result.state);
        fs::write(root.path().join("src/input"), "changed").unwrap();
        let stale = repo.export_check_report(&result.run.intent.id).unwrap();
        assert_eq!(stale.observation.state, RunState::Stale);
        assert_eq!(stale.observation.historical_state, result.state);
        assert_eq!(stale.publication, report.publication);
        assert_ne!(stale.fingerprint, report.fingerprint);
        stale.validate().unwrap();
        fs::remove_file(
            repo.root()
                .join(".local/runs")
                .join(result.run.intent.id.as_str())
                .join("terminal.yml"),
        )
        .unwrap();
        let unavailable = repo.export_check_report(&result.run.intent.id).unwrap();
        assert_eq!(unavailable.observation.state, RunState::Unknown);
        assert_eq!(unavailable.observation.historical_state, result.state);
        assert_eq!(unavailable.publication, report.publication);
        unavailable.validate().unwrap();
    }
    let (_root, repo, input) = setup("exit 0");
    let attempt = repo.run_ci_check_plan_with_faults(
        &input,
        &RequestId::new(),
        &RunControl::default(),
        |point| {
            if point == RunFaultPoint::AfterIntent {
                Err(PmError::new(ErrorCode::Io, "interrupted"))
            } else {
                Ok(())
            }
        },
    );
    assert!(attempt.is_err());
    let runs = repo.check_results(&RunQuery::default()).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        repo.export_check_report(&runs[0].run.intent.id)
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
}

fn rehash_report(report: &mut CheckReport) {
    let mut value = serde_json::to_value(&*report).unwrap();
    value.as_object_mut().unwrap().remove("fingerprint");
    value.sort_all_objects();
    report.fingerprint = ContentHash::of(&serde_json::to_vec(&value).unwrap());
}

#[test]
fn report_validation_checks_proof_relationships_after_hash_recomputation() {
    let (_root, repo, input) = setup("exit 7");
    let first = repo
        .run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
        .unwrap();
    let first = repo.export_check_report(&first.run.intent.id).unwrap();
    let second = repo
        .run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
        .unwrap();
    let second = repo.export_check_report(&second.run.intent.id).unwrap();
    let mut changed = first.clone();
    changed.receipts = second.receipts;
    rehash_report(&mut changed);
    assert!(
        changed
            .validate()
            .unwrap_err()
            .message
            .contains("different records")
    );
    let mut changed = first.clone();
    changed.observation.state = RunState::Passed;
    rehash_report(&mut changed);
    assert!(
        changed
            .validate()
            .unwrap_err()
            .message
            .contains("contradicts")
    );
    let mut changed = first;
    changed.receipts.pop();
    rehash_report(&mut changed);
    assert!(
        changed
            .validate()
            .unwrap_err()
            .message
            .contains("requires reservation")
    );
}

fn signed_report_fixture(script: &str) -> (CheckReport, ProducerTrustPolicy, SignedCheckReport) {
    use base64::Engine as _;
    let (_root, repo, input) = setup(script);
    let outcome = repo
        .run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
        .unwrap();
    let report = repo.export_check_report(&outcome.run.intent.id).unwrap();
    // Synthetic deterministic test key; never used outside temporary fixtures.
    let signing = ed25519_dalek::SigningKey::from_bytes(&[37u8; 32]);
    let policy = ProducerTrustPolicy {
        schema: SchemaVersion::CURRENT,
        repository: report.publication.intent.intent.repository.clone(),
        producers: vec![TrustedProducer {
            id: "test-ci".into(),
            public_key: base64::engine::general_purpose::STANDARD
                .encode(signing.verifying_key().to_bytes()),
            checks: report
                .publication
                .result
                .result
                .checks
                .iter()
                .map(|c| (c.check.id.clone(), c.check.definition.clone()))
                .collect(),
            not_before: report.observed_at - chrono::Duration::hours(1),
            expires_at: report.observed_at + chrono::Duration::hours(1),
        }],
    };
    let envelope = seal_test_report(&report);
    (report, policy, envelope)
}

fn seal_test_report(report: &CheckReport) -> SignedCheckReport {
    use base64::Engine as _;
    use ed25519_dalek::Signer as _;
    let signing = ed25519_dalek::SigningKey::from_bytes(&[37u8; 32]);
    let payload = serde_json::to_vec_pretty(report).unwrap();
    let mut message = format!(
        "DSSEv1 {} {} {} ",
        CHECK_REPORT_PAYLOAD_TYPE.len(),
        CHECK_REPORT_PAYLOAD_TYPE,
        payload.len()
    )
    .into_bytes();
    message.extend_from_slice(&payload);
    SignedCheckReport {
        payload_type: CHECK_REPORT_PAYLOAD_TYPE.into(),
        payload: base64::engine::general_purpose::STANDARD.encode(&payload),
        signatures: vec![ReportSignature {
            keyid: Some("an-untrusted-hint".into()),
            sig: base64::engine::general_purpose::STANDARD
                .encode(signing.sign(&message).to_bytes()),
        }],
    }
}

#[test]
fn producer_authentication_preserves_signed_bytes_failed_results_and_ignores_identity_hints() {
    use base64::Engine as _;
    for script in ["exit 0", "exit 7"] {
        let (report, policy, mut envelope) = signed_report_fixture(script);
        let commit = &report
            .publication
            .intent
            .intent
            .revision
            .as_ref()
            .unwrap()
            .source
            .commit;
        let fingerprint = policy.fingerprint().unwrap();
        let result =
            authenticate_check_report(&envelope, &policy, &fingerprint, commit, report.observed_at)
                .unwrap();
        assert_eq!(
            result.basis,
            ProducerAuthenticationBasis::AuthenticatedProducer
        );
        assert_eq!(result.producer.id, "test-ci");
        assert_eq!(result.report, report);
        assert_eq!(result.report.basis, VerificationBasis::LocalFeedback);
        let payload = base64::engine::general_purpose::STANDARD
            .decode(&envelope.payload)
            .unwrap();
        assert_eq!(result.payload, ContentHash::of(&payload));
        envelope.payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload);
        envelope.signatures[0].keyid = Some("forged-producer-name".into());
        let signature = base64::engine::general_purpose::STANDARD
            .decode(&envelope.signatures[0].sig)
            .unwrap();
        envelope.signatures[0].sig =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);
        assert_eq!(
            authenticate_check_report(&envelope, &policy, &fingerprint, commit, report.observed_at)
                .unwrap()
                .producer
                .id,
            "test-ci"
        );
    }
}

#[test]
fn producer_authentication_rejects_tampering_wrong_policy_source_key_scope_and_expiry() {
    use base64::Engine as _;
    let (report, policy, envelope) = signed_report_fixture("exit 0");
    let commit = &report
        .publication
        .intent
        .intent
        .revision
        .as_ref()
        .unwrap()
        .source
        .commit;
    let pin = policy.fingerprint().unwrap();
    let now = report.observed_at;
    let rejected = |envelope: &SignedCheckReport,
                    policy: &ProducerTrustPolicy,
                    pin: &ContentHash,
                    commit: &GitOid,
                    time| {
        assert!(authenticate_check_report(envelope, policy, pin, commit, time).is_err());
    };
    rejected(
        &envelope,
        &policy,
        &ContentHash::of(b"candidate policy"),
        commit,
        now,
    );
    rejected(
        &envelope,
        &policy,
        &pin,
        &"1111111111111111111111111111111111111111".parse().unwrap(),
        now,
    );
    rejected(
        &envelope,
        &policy,
        &pin,
        commit,
        now + chrono::Duration::hours(2),
    );
    rejected(
        &envelope,
        &policy,
        &pin,
        commit,
        now - chrono::Duration::seconds(1),
    );
    let mut other_repository = policy.clone();
    other_repository.repository = RepositoryId::new();
    rejected(
        &envelope,
        &other_repository,
        &other_repository.fingerprint().unwrap(),
        commit,
        now,
    );
    let mut changed = envelope.clone();
    let mut payload = base64::engine::general_purpose::STANDARD
        .decode(&changed.payload)
        .unwrap();
    payload.push(b' '); // Semantically identical JSON still requires a new signature.
    changed.payload = base64::engine::general_purpose::STANDARD.encode(payload);
    rejected(&changed, &policy, &pin, commit, now);
    changed = envelope.clone();
    changed.payload_type = "application/json".into();
    rejected(&changed, &policy, &pin, commit, now);
    let mut policy = policy.clone();
    policy.producers[0]
        .checks
        .insert("unit".into(), ContentHash::of(b"other check"));
    rejected(
        &envelope,
        &policy,
        &policy.fingerprint().unwrap(),
        commit,
        now,
    );
    policy.producers[0].public_key = base64::engine::general_purpose::STANDARD.encode(
        ed25519_dalek::SigningKey::from_bytes(&[99u8; 32])
            .verifying_key()
            .to_bytes(),
    );
    rejected(
        &envelope,
        &policy,
        &policy.fingerprint().unwrap(),
        commit,
        now,
    );
}

#[test]
fn producer_policy_rejects_duplicate_key_aliases_and_unknown_authority_fields() {
    let (_report, mut policy, envelope) = signed_report_fixture("exit 0");
    let mut alias = policy.producers[0].clone();
    alias.id = "another-producer".into();
    policy.producers.push(alias);
    assert!(policy.fingerprint().is_err());
    let mut value = serde_json::to_value(&envelope).unwrap();
    value["trusted"] = json!(true);
    assert!(SignedCheckReport::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn signed_local_only_run_does_not_gain_revision_bound_producer_admission() {
    let (_root, repo, input) = setup("exit 0");
    let outcome = repo
        .run_check_plan(&input.input, &RequestId::new(), &RunControl::default())
        .unwrap();
    let report = repo.export_check_report(&outcome.run.intent.id).unwrap();
    assert!(report.publication.intent.intent.revision.is_none());
    let (_other_report, mut policy, _) = signed_report_fixture("exit 0");
    policy.repository = repo.identity().clone();
    policy.producers[0].checks = report
        .publication
        .result
        .result
        .checks
        .iter()
        .map(|c| (c.check.id.clone(), c.check.definition.clone()))
        .collect();
    let envelope = seal_test_report(&report);
    let error = authenticate_check_report(
        &envelope,
        &policy,
        &policy.fingerprint().unwrap(),
        &input.binding.source.commit,
        report.observed_at,
    )
    .unwrap_err();
    assert!(error.message.contains("revision-bound"), "{error:?}");
}

#[test]
fn producer_authentication_bounds_multi_signature_hashing_work() {
    use base64::Engine as _;
    let (report, mut policy, mut envelope) = signed_report_fixture("exit 0");
    let template = policy.producers[0].clone();
    policy.producers = (1u8..=16)
        .map(|seed| {
            let mut producer = template.clone();
            producer.id = format!("producer-{seed}");
            producer.public_key = base64::engine::general_purpose::STANDARD.encode(
                ed25519_dalek::SigningKey::from_bytes(&[seed; 32])
                    .verifying_key()
                    .to_bytes(),
            );
            producer
        })
        .collect();
    envelope.payload = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 5 * 1024 * 1024]);
    envelope.signatures = vec![envelope.signatures[0].clone(); 8];
    let error = authenticate_check_report(
        &envelope,
        &policy,
        &policy.fingerprint().unwrap(),
        &report
            .publication
            .intent
            .intent
            .revision
            .as_ref()
            .unwrap()
            .source
            .commit,
        report.observed_at,
    )
    .unwrap_err();
    assert!(error.message.contains("workload"), "{error:?}");
}

fn import_report_fixture() -> (tempfile::TempDir, Repository, ImportCheckReportRequest) {
    let (root, repo, input) = setup("exit 7");
    let result = repo
        .run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
        .unwrap();
    let report = repo.export_check_report(&result.run.intent.id).unwrap();
    let (_, mut policy, _) = signed_report_fixture("exit 0");
    policy.repository = repo.identity().clone();
    policy.producers[0].checks = report
        .publication
        .result
        .result
        .checks
        .iter()
        .map(|c| (c.check.id.clone(), c.check.definition.clone()))
        .collect();
    let envelope = format!(
        " \n{}\n ",
        serde_json::to_string_pretty(&seal_test_report(&report)).unwrap()
    );
    let request = ImportCheckReportRequest {
        red_green: None,
        expected_policy: policy.fingerprint().unwrap(),
        policy,
        expected_commit: input.binding.source.commit,
        envelope,
        actor: "importer".into(),
    };
    (root, repo, request)
}

#[test]
fn imported_report_retains_exact_envelope_replays_and_exports_with_receipt_authority() {
    let (_root, repo, input) = import_report_fixture();
    let request = RequestId::new();
    let receipt = repo.import_check_report(&input, &request).unwrap();
    let record: ImportedCheckReportRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(record.record.input.envelope, input.envelope);
    assert_eq!(record.record.admission.historical_state, RunState::Failed);
    assert_eq!(repo.import_check_report(&input, &request).unwrap(), receipt);
    assert_eq!(repo.imported_check_reports().unwrap(), vec![record.clone()]);
    assert_eq!(
        repo.imported_check_report(&record.record.id).unwrap(),
        record
    );
    assert!(repo.doctor().unwrap().valid);
    let snapshot = repo.export_snapshot().unwrap();
    snapshot.validate().unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.kind == SnapshotKind::Attestation && file.path == record.path)
    );
    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join(".workdeck");
    let preview = preview_snapshot_restore(&destination, &snapshot).unwrap();
    assert!(preview.allowed, "{:?}", preview.blockers);
    restore_snapshot(
        &destination,
        &snapshot,
        Some(&preview.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    let restored = Repository::open_source(&destination).unwrap();
    assert_eq!(
        restored.imported_check_report(&record.record.id).unwrap(),
        record
    );
    restored
        .reauthenticate_imported_report(
            &record.record.id,
            &input.policy,
            &input.expected_policy,
            &input.expected_commit,
        )
        .unwrap();
    let authenticated = repo
        .reauthenticate_imported_report(
            &record.record.id,
            &input.policy,
            &input.expected_policy,
            &input.expected_commit,
        )
        .unwrap();
    assert_eq!(
        authenticated.report.observation.historical_state,
        RunState::Failed
    );
    let mut changed = input.clone();
    changed.actor = "another-importer".into();
    assert_eq!(
        repo.import_check_report(&changed, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
}

#[test]
fn imported_report_recovery_has_one_record_and_one_receipt_after_each_fault() {
    use workdeck_pm::transactions::FaultPoint;
    for point in [
        FaultPoint::BeforeJournal,
        FaultPoint::AfterJournal,
        FaultPoint::AfterChange(0),
        FaultPoint::BeforeReceipt,
        FaultPoint::AfterReceipt,
    ] {
        let (_root, repo, input) = import_report_fixture();
        let request = RequestId::new();
        assert!(
            repo.import_check_report_with_faults(&input, &request, |actual| {
                if actual == point {
                    Err(PmError::new(ErrorCode::Io, "interrupted import"))
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        repo.recover_operations().unwrap();
        let receipt = repo.import_check_report(&input, &request).unwrap();
        let reports = repo.imported_check_reports().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].record.request_id, request);
        assert_eq!(repo.import_check_report(&input, &request).unwrap(), receipt);
        assert_eq!(
            repo.operation_history()
                .unwrap()
                .iter()
                .filter(|r| r.operation == "evidence.import_report")
                .count(),
            1
        );
        assert!(repo.doctor().unwrap().valid);
    }
}

#[test]
fn imported_report_missing_or_edited_source_or_receipt_never_passes_doctor() {
    for mutation in ["delete", "edit", "receipt"] {
        let (_root, repo, input) = import_report_fixture();
        let receipt = repo.import_check_report(&input, &RequestId::new()).unwrap();
        let record: ImportedCheckReportRecord =
            serde_json::from_value(receipt.result.clone()).unwrap();
        let path = repo.root().join(&record.path);
        match mutation {
            "delete" => fs::remove_file(&path).unwrap(),
            "edit" => fs::write(&path, format!("{} ", record.document)).unwrap(),
            _ => fs::remove_file(
                repo.root()
                    .join("operations")
                    .join(format!("{}.yml", receipt.operation_id)),
            )
            .unwrap(),
        }
        let doctor = repo.doctor().unwrap();
        assert!(!doctor.valid, "{mutation}");
        assert!(
            doctor
                .errors
                .iter()
                .any(|error| error.path.as_deref() == record.path.to_str()),
            "corrupt attestation diagnostics must identify the affected file: {:?}",
            doctor.errors
        );
        assert!(repo.imported_check_report(&record.record.id).is_err());
        assert!(
            repo.reauthenticate_imported_report(
                &record.record.id,
                &input.policy,
                &input.expected_policy,
                &input.expected_commit
            )
            .is_err()
        );
    }
}

#[test]
fn imported_historical_admission_does_not_override_a_new_policy_or_refresh_on_replay() {
    use base64::Engine as _;
    let (_root, repo, input) = import_report_fixture();
    let request = RequestId::new();
    let receipt = repo.import_check_report(&input, &request).unwrap();
    let record: ImportedCheckReportRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    let mut revoked = input.policy.clone();
    revoked.producers[0].public_key = base64::engine::general_purpose::STANDARD.encode(
        ed25519_dalek::SigningKey::from_bytes(&[98u8; 32])
            .verifying_key()
            .to_bytes(),
    );
    assert!(
        repo.reauthenticate_imported_report(
            &record.record.id,
            &revoked,
            &revoked.fingerprint().unwrap(),
            &input.expected_commit
        )
        .is_err()
    );
    assert_eq!(repo.import_check_report(&input, &request).unwrap(), receipt);
    assert_eq!(
        repo.imported_check_report(&record.record.id).unwrap(),
        record
    );
}

#[test]
fn imported_report_rejects_invalid_admission_without_publishing() {
    let (_root, repo, input) = import_report_fixture();
    let before = repo.operation_history().unwrap();
    let mut wrong_policy = input.clone();
    wrong_policy.expected_policy = ContentHash::of(b"untrusted replacement");
    let mut wrong_source = input.clone();
    wrong_source.expected_commit = "1111111111111111111111111111111111111111".parse().unwrap();
    let mut invalid_actor = input.clone();
    invalid_actor.actor = " ".into();
    for invalid in [wrong_policy, wrong_source, invalid_actor] {
        assert!(
            repo.import_check_report(&invalid, &RequestId::new())
                .is_err()
        );
    }
    assert!(!repo.root().join("attestations").exists());
    assert!(repo.imported_check_reports().unwrap().is_empty());
    assert_eq!(repo.operation_history().unwrap(), before);
}

#[test]
fn concurrent_import_request_reserves_one_attestation() {
    let (_root, repo, input) = import_report_fixture();
    let request = RequestId::new();
    let barrier = std::sync::Barrier::new(2);
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            repo.import_check_report(&input, &request)
        });
        let second = scope.spawn(|| {
            barrier.wait();
            repo.import_check_report(&input, &request)
        });
        (
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap(),
        )
    });
    assert_eq!(first, second);
    assert_eq!(repo.imported_check_reports().unwrap().len(), 1);
}

#[test]
fn imported_report_activity_has_import_time_actor_and_historical_status() {
    use workdeck_pm::projection::*;
    let (root, repo, input) = import_report_fixture();
    let receipt = repo.import_check_report(&input, &RequestId::new()).unwrap();
    let record: ImportedCheckReportRecord = serde_json::from_value(receipt.result).unwrap();
    let mut store = ProjectionStore::open(
        root.path(),
        SourceSelector::WorkingTree,
        ProjectionLimits::default(),
    )
    .unwrap();
    let ProjectionRefresh::Published(view) =
        store.refresh(&ProjectionRefreshRequest::default()).unwrap()
    else {
        panic!("new projection required")
    };
    let query = view
        .query(&ProjectionQuery::Activity {
            query: ProjectionActivityQuery {
                kinds: vec![SnapshotKind::Attestation],
                after: Some(record.record.imported_at - chrono::Duration::seconds(1)),
                ..Default::default()
            },
        })
        .unwrap();
    let page = view.page(&query, 0, 10).unwrap();
    assert_eq!(
        page.rows.len(),
        1,
        "imported attestation must be visible in its dated activity window"
    );
    let row = &page.rows[0];
    assert_eq!(row.token.content, record.content);
    assert_eq!(row.status.as_deref(), Some("failed"));
    assert_eq!(row.assignee.as_deref(), Some("importer"));
    assert!(row.title.contains("test-ci"));
}
