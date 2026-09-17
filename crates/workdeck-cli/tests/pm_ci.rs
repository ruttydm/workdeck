#![cfg(unix)]
use assert_cmd::prelude::*;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use workdeck_pm::{CreateIssue, IssueRecord, Repository, RequestId};
fn run(root: &Path, args: &[&str], index: Option<&Path>) -> Output {
    let mut command = Command::cargo_bin("workdeck").unwrap();
    command
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .env_remove("GIT_INDEX_FILE");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    command.output().unwrap()
}
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
fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    let repo = Repository::init(root.path(), "WD").unwrap();
    let issue = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Staged issue", "Body"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    git(root.path(), &["add", "--", ".workdeck"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=CI Test",
            "-c",
            "user.email=ci@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "base",
        ],
    );
    git(root.path(), &["branch", "ci-base"]);
    (root, repo, issue)
}

#[test]
fn ci_validate_uses_commits_even_when_working_config_and_index_are_broken() {
    let (root, repo, _) = fixture();
    fs::write(repo.root().join("config.yml"), "invalid: [").unwrap();
    git(root.path(), &["add", "--", ".workdeck/config.yml"]);
    let index = fs::read(root.path().join(".git/index")).unwrap();
    fs::remove_file(repo.root().join("config.yml")).unwrap();
    let output = run(
        root.path(),
        &[
            "ci", "validate", "--base", "ci-base", "--head", "HEAD", "--json",
        ],
        None,
    );
    assert!(
        output.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let data = value(&output);
    assert_eq!(data["result"]["valid"], true);
    assert_eq!(data["result"]["basis"], "planning_source_validation");
    assert_eq!(data["result"]["base"], data["result"]["head"]);
    let oid = data["result"]["head"]["commit"].as_str().unwrap();
    let exact = run(
        root.path(),
        &["ci", "validate", "--base", oid, "--head", oid, "--json"],
        None,
    );
    assert!(exact.status.success(), "{:?}", value(&exact));
    assert_eq!(value(&exact)["result"]["head"], data["result"]["head"]);
    assert_eq!(
        data["source"]["repository"],
        serde_json::json!(repo.identity())
    );
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    assert!(!repo.root().join("config.yml").exists());
}
#[test]
fn ci_validate_rejects_contract_weakening_with_structured_baseline_report() {
    let (root, repo, _) = fixture();
    let config = fs::read_to_string(repo.root().join("config.yml")).unwrap();
    assert!(config.contains("require_all_criteria: true"));
    fs::write(
        repo.root().join("config.yml"),
        config.replace("require_all_criteria: true", "require_all_criteria: false"),
    )
    .unwrap();
    git(root.path(), &["add", "--", ".workdeck/config.yml"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=CI Test",
            "-c",
            "user.email=ci@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "weaken",
        ],
    );
    let output = run(
        root.path(),
        &[
            "ci",
            "validate",
            "--base",
            "refs/heads/ci-base",
            "--head",
            "HEAD",
            "--json",
        ],
        None,
    );
    assert!(!output.status.success());
    let data = value(&output);
    assert_eq!(data["ok"], false);
    assert_eq!(data["error"]["code"], "policy_blocked");
    assert_eq!(
        data["error"]["details"]["report"]["contracts"]["review_required"],
        true
    );
    assert_ne!(
        data["source"]["base"]["commit"],
        data["source"]["head"]["commit"]
    );
}
#[test]
fn ci_revision_expressions_fail_without_initializing_planning() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    let output = run(
        root.path(),
        &[
            "ci", "validate", "--base", "HEAD~1", "--head", "HEAD", "--json",
        ],
        None,
    );
    assert!(!output.status.success());
    assert_eq!(value(&output)["error"]["code"], "invalid_input");
    assert!(!root.path().join(".workdeck").exists());
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn ci_catalog_and_schema_expose_validation_without_claiming_execution_or_trust() {
    let root = tempfile::tempdir().unwrap();
    let output = run(root.path(), &["capabilities", "--json"], None);
    assert!(output.status.success());
    let data = value(&output);
    let commands = data["result"]["commands"].as_array().unwrap();
    let ci = commands
        .iter()
        .find(|command| command["path"] == "ci validate")
        .unwrap();
    assert_eq!(ci["native_planning"]["implemented"], true);
    assert_eq!(ci["native_planning"]["requires_initialized_source"], false);
    assert!(
        ci["native_planning"]["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "caller_selected_baseline_not_trusted_ci")
    );
    let check = commands
        .iter()
        .find(|command| command["path"] == "ci check")
        .unwrap();
    assert_eq!(check["native_planning"]["implemented"], true);
    assert_eq!(
        check["native_planning"]["requires_initialized_source"],
        true
    );
    assert!(
        check["native_planning"]["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "revision_bound_feedback_not_ci_qualification")
    );
    let schema = run(
        root.path(),
        &["schema", "ci-validation-report", "--json"],
        None,
    );
    assert!(schema.status.success());
    assert!(value(&schema)["result"]["properties"]["contracts"].is_object());
    assert!(!root.path().join(".workdeck").exists());
}

#[test]
fn ci_validate_reports_subject_acceptance_changes_by_stable_identity() {
    let (root, repo, _) = fixture();
    let mut input = CreateIssue::new("Accepted work", "");
    input.fields.insert(
        "acceptance".into(),
        serde_json::json!([{"id":"complete","description":"All records pass","checked":false}]),
    );
    let issue: IssueRecord =
        serde_json::from_value(repo.create_issue(&input, &RequestId::new()).unwrap().result)
            .unwrap();
    git(root.path(), &["add", "--", ".workdeck"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=CI Test",
            "-c",
            "user.email=ci@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "requirements",
        ],
    );
    git(root.path(), &["branch", "-f", "ci-base", "HEAD"]);
    let path = repo.root().join(&issue.path);
    let original = fs::read_to_string(&path).unwrap();
    assert!(original.contains("All records pass"));
    fs::write(
        &path,
        original.replace("All records pass", "Some records pass"),
    )
    .unwrap();
    git(root.path(), &["add", "--", ".workdeck"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=CI Test",
            "-c",
            "user.email=ci@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "weaker criteria",
        ],
    );
    fs::write(&path, &original).unwrap();
    let output = run(
        root.path(),
        &[
            "ci", "validate", "--base", "ci-base", "--head", "HEAD", "--json",
        ],
        None,
    );
    assert!(!output.status.success());
    let data = value(&output);
    let comparison = &data["error"]["details"]["report"]["contracts"];
    assert_eq!(comparison["review_required"], true);
    assert_eq!(
        comparison["changes"][0]["subject"],
        serde_json::json!({"kind":"issue","id":issue.metadata.id})
    );
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn ci_validate_reports_committed_evaluator_changes_with_unchanged_check_definition() {
    let (root, repo, _) = fixture();
    let definitions = [
        (
            "commands/unit.yml",
            serde_json::json!({
                "schema":1, "repository":repo.identity(), "id":"unit", "name":"Unit",
                "recipe":{"kind":"argv", "argv":[{"kind":"literal", "value":"runner"}]},
                "cwd":".", "inputs":{"trees":["tests"]},
                "tools":[{"name":"runner", "executable":"/bin/false"}]
            }),
        ),
        (
            "checks/unit.yml",
            serde_json::json!({
                "schema":1, "repository":repo.identity(), "id":"unit", "name":"Unit",
                "command":"unit", "expectation":{"kind":"process", "allowed_exit_codes":[0]},
                "evaluator_inputs":{"trees":["tests"]}
            }),
        ),
        (
            "check-profiles/required.yml",
            serde_json::json!({
                "schema":1, "repository":repo.identity(), "id":"required", "name":"Required",
                "checks":["unit"]
            }),
        ),
    ];
    for (path, definition) in definitions {
        let path = repo.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(&definition).unwrap()).unwrap();
    }
    let mut config = repo.config().unwrap();
    config.acceptance.required_profiles = vec!["required".into()];
    fs::write(
        repo.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    fs::create_dir(root.path().join("tests")).unwrap();
    let evaluator = root.path().join("tests/evaluator.py");
    fs::write(&evaluator, "assert value == 42\n").unwrap();
    let commit = || {
        git(root.path(), &["add", "--", ".workdeck", "tests"]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=CI Test",
                "-c",
                "user.email=ci@example.invalid",
                "commit",
                "--quiet",
                "--no-gpg-sign",
                "-m",
                "evaluator",
            ],
        );
    };
    commit();
    git(root.path(), &["branch", "evaluator-base"]);
    let args = [
        "ci",
        "validate",
        "--base",
        "evaluator-base",
        "--head",
        "HEAD",
        "--json",
    ];
    let baseline = run(root.path(), &args, None);
    assert!(baseline.status.success(), "{:?}", value(&baseline));
    let definition = fs::read(repo.root().join("checks/unit.yml")).unwrap();
    fs::write(&evaluator, "assert True\n").unwrap();
    commit();
    fs::write(&evaluator, "assert value == 42\n").unwrap();
    let output = run(root.path(), &args, None);
    assert!(!output.status.success());
    let data = value(&output);
    assert_eq!(data["error"]["code"], "policy_blocked");
    let contracts = &data["error"]["details"]["report"]["contracts"];
    assert_eq!(contracts["review_required"], true);
    assert_eq!(contracts["changes"], serde_json::json!([]));
    let changes = contracts["evaluator_changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["check"], "unit");
    assert_eq!(changes[0]["path"], "tests/evaluator.py");
    assert_ne!(changes[0]["base"]["content"], changes[0]["head"]["content"]);
    assert_eq!(
        fs::read(repo.root().join("checks/unit.yml")).unwrap(),
        definition
    );
    assert_eq!(
        fs::read_to_string(&evaluator).unwrap(),
        "assert value == 42\n"
    );
}

fn execution_fixture(script: &str) -> (tempfile::TempDir, Repository) {
    let (root, repo, _) = fixture();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/input"), "candidate\n").unwrap();
    for (path, value) in [
        (
            "commands/unit.yml",
            serde_json::json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit",
            "recipe":{"kind":"argv","argv":[{"kind":"literal","value":"sh"},{"kind":"literal","value":"-c"},{"kind":"literal","value":script}]},
            "cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],"inputs":{"trees":["src"]},
            "bounds":{"timeout_seconds":2,"stdout_bytes":2048,"stderr_bytes":2048}}),
        ),
        (
            "checks/unit.yml",
            serde_json::json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit",
            "command":"unit","expectation":{"kind":"process","allowed_exit_codes":[0]},"evaluator_inputs":{}}),
        ),
        (
            "check-profiles/unit.yml",
            serde_json::json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit","checks":["unit"]}),
        ),
    ] {
        fs::create_dir_all(repo.root().join(path).parent().unwrap()).unwrap();
        fs::write(repo.root().join(path), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    git(root.path(), &["add", "--", ".workdeck", "src"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=CI Test",
            "-c",
            "user.email=ci@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "execution",
        ],
    );
    (root, repo)
}

#[test]
fn ci_check_executes_a_saved_revision_plan_and_replays_after_source_changes() {
    let (root, _) = execution_fixture(
        "read value < src/input; test \"$value\" = candidate || exit 1; printf x >> sentinel",
    );
    let plan = run(
        root.path(),
        &[
            "ci",
            "plan",
            "--revision",
            "HEAD",
            "--profile",
            "unit",
            "--json",
        ],
        None,
    );
    assert!(plan.status.success(), "{:?}", value(&plan));
    assert!(!root.path().join("sentinel").exists());
    fs::write(root.path().join("ci-plan.json"), &plan.stdout).unwrap();
    let data = value(&plan);
    let fingerprint = data["result"]["binding"]["fingerprint"].as_str().unwrap();
    let args = [
        "ci",
        "check",
        "--plan-file",
        "ci-plan.json",
        "--expected-plan",
        fingerprint,
        "--actor",
        "tester",
        "--request-id",
        "ci-once",
        "--json",
    ];
    let first = run(root.path(), &args, None);
    assert!(first.status.success(), "{:?}", value(&first));
    let first = value(&first);
    assert_eq!(first["result"]["state"], "passed");
    assert_eq!(
        first["result"]["run"]["intent"]["revision"],
        data["result"]["binding"]
    );
    assert_eq!(
        first["result"]["results"]["result"]["basis"],
        "local_feedback"
    );
    fs::write(root.path().join("src/input"), "later\n").unwrap();
    let replay = run(root.path(), &args, None);
    assert_eq!(replay.status.code(), Some(4));
    let replay = value(&replay);
    assert_eq!(replay["result"]["replayed"], true);
    assert_eq!(replay["result"]["assessment"]["state"], "stale");
    assert_eq!(replay["result"]["run"], first["result"]["run"]);
    assert_eq!(fs::read(root.path().join("sentinel")).unwrap(), b"x");
    let id = first["result"]["run"]["intent"]["id"].as_str().unwrap();
    let exported = run(root.path(), &["check", "export", id, "--json"], None);
    assert!(exported.status.success(), "{:?}", value(&exported));
    let exported = value(&exported);
    assert_eq!(exported["result"]["basis"], "local_feedback");
    assert_eq!(exported["result"]["observation"]["state"], "stale");
    assert_eq!(
        exported["result"]["observation"]["historical_state"],
        "passed"
    );
    assert_eq!(
        exported["result"]["publication"]["intent"]["intent"]["revision"],
        data["result"]["binding"]
    );
}

#[test]
fn ci_plan_rejects_dirty_inputs_and_ci_check_returns_failure_status() {
    let (root, _) = execution_fixture("exit 7");
    let args = [
        "ci",
        "plan",
        "--revision",
        "HEAD",
        "--profile",
        "unit",
        "--json",
    ];
    let plan = run(root.path(), &args, None);
    assert!(plan.status.success(), "{:?}", value(&plan));
    let data = value(&plan);
    fs::write(root.path().join("ci-plan.json"), &plan.stdout).unwrap();
    let output = run(
        root.path(),
        &[
            "ci",
            "check",
            "--plan-file",
            "ci-plan.json",
            "--expected-plan",
            data["result"]["binding"]["fingerprint"].as_str().unwrap(),
            "--actor",
            "tester",
            "--request-id",
            "failed-run",
            "--json",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(value(&output)["result"]["state"], "failed");
    fs::write(root.path().join("src/input"), "dirty\n").unwrap();
    let rejected = run(root.path(), &args, None);
    assert!(!rejected.status.success());
    assert_eq!(value(&rejected)["error"]["code"], "stale_source");
}

#[test]
fn ci_validate_rejects_policy_violations_in_an_otherwise_valid_revision() {
    let (directory, repository, _) = fixture();
    let mut schema =
        serde_json::to_value(repository.organization_schema().unwrap().definition).unwrap();
    schema["fields"] = serde_json::json!({"risk": {
        "type":"text", "scopes":["issue"], "required":true
    }});
    fs::write(
        repository.root().join("schema.yml"),
        serde_json::to_string(&schema).unwrap(),
    )
    .unwrap();
    git(directory.path(), &["add", "--", ".workdeck"]);
    git(
        directory.path(),
        &[
            "-c",
            "user.name=CI Test",
            "-c",
            "user.email=ci@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "policy violation",
        ],
    );
    let output = run(
        directory.path(),
        &[
            "ci", "validate", "--base", "HEAD", "--head", "HEAD", "--json",
        ],
        None,
    );
    assert!(!output.status.success());
    let envelope = value(&output);
    assert_eq!(envelope["error"]["code"], "policy_blocked");
    let response = &envelope["error"]["details"]["report"];
    assert_eq!(response["head_report"]["valid"], true, "{response}");
    assert_eq!(response["head_report"]["policy_compliant"], false);
    assert_eq!(response["valid"], false);
}

#[test]
fn ci_authentication_is_discoverable_without_initializing_a_checkout() {
    let directory = tempfile::tempdir().unwrap();
    let output = run(directory.path(), &["ci", "authenticate", "--help"], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--expected-policy"));
    assert!(help.contains("--expected-commit"));
    assert!(!directory.path().join(".workdeck").exists());
}

#[test]
fn ci_authenticates_a_signed_failed_report_offline_and_rejects_policy_substitution() {
    use base64::Engine as _;
    use ed25519_dalek::Signer as _;
    use workdeck_pm::*;
    let (root, repo) = execution_fixture("exit 7");
    let prepared = prepare_ci_check(
        root.path(),
        &CiRevision::Head {},
        &CheckPlanRequest {
            profiles: vec!["unit".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let expected_commit = prepared.binding.source.commit.to_string();
    let result = repo
        .run_ci_check_plan(
            &CiCheckRunRequest {
                input: CheckRunRequest {
                    expected_plan: prepared.plan.fingerprint.clone(),
                    plan: prepared.plan,
                    actor: "fixture".into(),
                },
                expected_binding: prepared.binding.fingerprint.clone(),
                binding: prepared.binding,
            },
            &RequestId::new(),
            &RunControl::default(),
        )
        .unwrap();
    let report = repo.export_check_report(&result.run.intent.id).unwrap();
    let signing = ed25519_dalek::SigningKey::from_bytes(&[43u8; 32]);
    let mut policy = ProducerTrustPolicy {
        schema: SchemaVersion::CURRENT,
        repository: repo.identity().clone(),
        producers: vec![TrustedProducer {
            id: "ci-test".into(),
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
    let fingerprint = policy.fingerprint().unwrap().to_string();
    let payload = serde_json::to_vec(&report).unwrap();
    let mut message = format!(
        "DSSEv1 {} {} {} ",
        CHECK_REPORT_PAYLOAD_TYPE.len(),
        CHECK_REPORT_PAYLOAD_TYPE,
        payload.len()
    )
    .into_bytes();
    message.extend_from_slice(&payload);
    let envelope = SignedCheckReport {
        payload_type: CHECK_REPORT_PAYLOAD_TYPE.into(),
        payload: base64::engine::general_purpose::STANDARD.encode(&payload),
        signatures: vec![ReportSignature {
            keyid: None,
            sig: base64::engine::general_purpose::STANDARD
                .encode(signing.sign(&message).to_bytes()),
        }],
    };
    let offline = tempfile::tempdir().unwrap();
    fs::write(
        offline.path().join("policy.json"),
        serde_json::to_vec(&policy).unwrap(),
    )
    .unwrap();
    fs::write(
        offline.path().join("signed.json"),
        serde_json::to_vec(&envelope).unwrap(),
    )
    .unwrap();
    let inspected = run(
        offline.path(),
        &["ci", "policy", "--policy-file", "policy.json", "--json"],
        None,
    );
    assert!(inspected.status.success(), "{:?}", value(&inspected));
    assert_eq!(value(&inspected)["result"]["fingerprint"], fingerprint);
    let policy_path = offline.path().join("policy.json");
    let signed_path = offline.path().join("signed.json");
    let import_args = [
        "ci",
        "import-report",
        "--report-file",
        signed_path.to_str().unwrap(),
        "--policy-file",
        policy_path.to_str().unwrap(),
        "--expected-policy",
        &fingerprint,
        "--expected-commit",
        &expected_commit,
        "--actor",
        "importer",
        "--request-id",
        "import-once",
        "--json",
    ];
    let imported = run(root.path(), &import_args, None);
    assert!(imported.status.success(), "{:?}", value(&imported));
    let imported = value(&imported);
    let id = imported["result"]["record"]["id"].as_str().unwrap();
    let replay = run(root.path(), &import_args, None);
    assert!(replay.status.success(), "{:?}", value(&replay));
    assert_eq!(value(&replay)["receipt"], imported["receipt"]);
    let listed = run(root.path(), &["ci", "reports", "--json"], None);
    assert!(listed.status.success(), "{:?}", value(&listed));
    assert_eq!(value(&listed)["result"].as_array().unwrap().len(), 1);
    assert_eq!(value(&listed)["result"][0]["id"], id);
    assert!(value(&listed)["result"][0].get("document").is_none());
    assert!(value(&listed)["result"][0].get("input").is_none());
    let retained = run(root.path(), &["ci", "report", id, "--json"], None);
    assert_eq!(value(&retained)["result"], imported["result"]);
    let rechecked = run(
        root.path(),
        &[
            "ci",
            "reauthenticate",
            id,
            "--policy-file",
            policy_path.to_str().unwrap(),
            "--expected-policy",
            &fingerprint,
            "--expected-commit",
            &expected_commit,
            "--json",
        ],
        None,
    );
    assert!(rechecked.status.success(), "{:?}", value(&rechecked));
    assert_eq!(
        value(&rechecked)["result"]["report"]["observation"]["historical_state"],
        "failed"
    );
    drop(repo);
    drop(root);
    let args = [
        "ci",
        "authenticate",
        "--report-file",
        "signed.json",
        "--policy-file",
        "policy.json",
        "--expected-policy",
        &fingerprint,
        "--expected-commit",
        &expected_commit,
        "--json",
    ];
    let output = run(offline.path(), &args, None);
    assert!(output.status.success(), "{:?}", value(&output));
    let authenticated = value(&output);
    assert_eq!(authenticated["result"]["basis"], "authenticated_producer");
    assert_eq!(authenticated["result"]["producer"]["id"], "ci-test");
    assert_eq!(
        authenticated["result"]["report"]["observation"]["historical_state"],
        "failed"
    );
    assert_eq!(authenticated["result"]["report"]["basis"], "local_feedback");
    policy.producers[0].id = "candidate-self-trust".into();
    fs::write(
        offline.path().join("policy.json"),
        serde_json::to_vec(&policy).unwrap(),
    )
    .unwrap();
    let rejected = run(offline.path(), &args, None);
    assert!(!rejected.status.success());
    assert_eq!(value(&rejected)["error"]["code"], "policy_blocked");
    assert!(!offline.path().join(".workdeck").exists());
}

#[test]
fn ci_import_report_exposes_explicit_mutation_and_trust_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let output = run(directory.path(), &["ci", "import-report", "--help"], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    for flag in [
        "--request-id",
        "--actor",
        "--expected-policy",
        "--expected-commit",
    ] {
        assert!(help.contains(flag));
    }
    assert!(!directory.path().join(".workdeck").exists());
}

#[test]
fn ci_validate_baseline_pins_are_paired_and_enforced() {
    let (root, _, _) = fixture();
    let initial = run(
        root.path(),
        &[
            "ci", "validate", "--base", "HEAD", "--head", "HEAD", "--json",
        ],
        None,
    );
    assert!(initial.status.success());
    let data = value(&initial);
    let commit = data["result"]["base"]["commit"].as_str().unwrap();
    let contract = data["result"]["contracts"]["base"]["fingerprint"]
        .as_str()
        .unwrap();
    let args = [
        "ci",
        "validate",
        "--base",
        "ci-base",
        "--head",
        "HEAD",
        "--json",
        "--expected-base-commit",
        commit,
        "--expected-base-contract",
        contract,
    ];
    let pinned = run(root.path(), &args, None);
    assert!(pinned.status.success(), "{:?}", value(&pinned));
    assert_eq!(
        value(&pinned)["result"]["basis"],
        "pinned_baseline_validation"
    );
    assert_eq!(
        value(&pinned)["result"]["baseline_pin"]["contract"],
        contract
    );
    let bad_hash = workdeck_pm::ContentHash::of(b"substituted").to_string();
    let mut wrong = args;
    wrong[10] = &bad_hash;
    let rejected = run(root.path(), &wrong, None);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stdout).contains("baseline does not match"));
    let incomplete = run(root.path(), &args[..9], None);
    assert!(!incomplete.status.success());
    assert!(String::from_utf8_lossy(&incomplete.stderr).contains("--expected-base-contract"));
}

#[test]
fn cli_authenticates_exact_contract_review_and_rejects_wrong_policy() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer, SigningKey};
    use workdeck_pm::*;
    let (root, repo, _) = fixture();
    let validation = ci_validate(
        root.path(),
        &CiValidateRequest {
            base: CiRevision::Head {},
            head: CiRevision::Head {},
        },
    )
    .unwrap();
    let now = chrono::Utc::now();
    let signing = SigningKey::from_bytes(&[45; 32]);
    let policy = ContractReviewPolicy {
        schema: SchemaVersion::CURRENT,
        repository: repo.identity().clone(),
        required_reviewers: vec![ContractReviewer {
            id: "maintainer".into(),
            public_key: STANDARD.encode(signing.verifying_key().to_bytes()),
            not_before: now - chrono::Duration::hours(1),
            expires_at: now + chrono::Duration::hours(1),
        }],
    };
    let baseline = CiBaselinePin {
        commit: validation.base.commit,
        contract: validation.contracts.base.unwrap().fingerprint,
    };
    let approval = CiContractApproval {
        schema: SchemaVersion::CURRENT,
        repository: repo.identity().clone(),
        baseline: baseline.clone(),
        head: validation.head,
        head_contract: validation.contracts.head.unwrap().fingerprint,
        decision: ContractReviewDecision::Approve,
        reviewed_at: now,
        expires_at: now + chrono::Duration::minutes(30),
    };
    let payload = serde_json::to_vec(&approval).unwrap();
    let mut pae = format!(
        "DSSEv1 {} {} {} ",
        CONTRACT_REVIEW_PAYLOAD_TYPE.len(),
        CONTRACT_REVIEW_PAYLOAD_TYPE,
        payload.len()
    )
    .into_bytes();
    pae.extend_from_slice(&payload);
    let envelope = SignedContractReview {
        payload_type: CONTRACT_REVIEW_PAYLOAD_TYPE.into(),
        payload: STANDARD.encode(payload),
        signatures: vec![ReportSignature {
            keyid: None,
            sig: STANDARD.encode(signing.sign(&pae).to_bytes()),
        }],
    };
    fs::write(
        root.path().join("review-policy.json"),
        serde_json::to_vec(&policy).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("review.json"),
        serde_json::to_vec(&envelope).unwrap(),
    )
    .unwrap();
    let inspected = run(
        root.path(),
        &[
            "ci",
            "review-policy",
            "--policy-file",
            "review-policy.json",
            "--json",
        ],
        None,
    );
    assert!(inspected.status.success(), "{:?}", value(&inspected));
    let hash = policy.fingerprint().unwrap().to_string();
    assert_eq!(value(&inspected)["result"]["fingerprint"], hash);
    let commit = baseline.commit.to_string();
    let contract = baseline.contract.to_string();
    let args = [
        "ci",
        "validate-reviewed",
        "--base",
        "HEAD",
        "--head",
        "HEAD",
        "--expected-base-commit",
        &commit,
        "--expected-base-contract",
        &contract,
        "--policy-file",
        "review-policy.json",
        "--expected-policy",
        &hash,
        "--review-file",
        "review.json",
        "--json",
    ];
    let reviewed = run(root.path(), &args, None);
    assert!(reviewed.status.success(), "{:?}", value(&reviewed));
    assert_eq!(
        value(&reviewed)["result"]["admission"]["basis"],
        "authenticated_contract_review"
    );
    assert_eq!(
        value(&reviewed)["result"]["admission"]["reviewers"],
        serde_json::json!(["maintainer"])
    );
    let request = RequestId::new().to_string();
    let imported = run(
        root.path(),
        &[
            "ci",
            "import-review",
            "--review-file",
            "review.json",
            "--policy-file",
            "review-policy.json",
            "--expected-policy",
            &hash,
            "--expected-base-commit",
            &commit,
            "--expected-base-contract",
            &contract,
            "--expected-commit",
            &commit,
            "--actor",
            "importer",
            "--request-id",
            &request,
            "--json",
        ],
        None,
    );
    assert!(imported.status.success(), "{:?}", value(&imported));
    let imported = value(&imported);
    let id = imported["result"]["record"]["id"].as_str().unwrap();
    let listed = run(root.path(), &["ci", "reviews", "--json"], None);
    assert!(listed.status.success());
    assert_eq!(value(&listed)["result"][0]["id"], id);
    assert!(value(&listed)["result"][0].get("input").is_none());
    let read = run(root.path(), &["ci", "review", id, "--json"], None);
    assert!(read.status.success());
    assert_eq!(value(&read)["result"], imported["result"]);
    let reauthenticated = run(
        root.path(),
        &[
            "ci",
            "reauthenticate-review",
            id,
            "--policy-file",
            "review-policy.json",
            "--expected-policy",
            &hash,
            "--expected-base-commit",
            &commit,
            "--expected-base-contract",
            &contract,
            "--expected-commit",
            &commit,
            "--json",
        ],
        None,
    );
    assert!(
        reauthenticated.status.success(),
        "{:?}",
        value(&reauthenticated)
    );
    assert_eq!(value(&reauthenticated)["result"]["valid"], true);
    let coverage = run(
        root.path(),
        &["ci", "review-coverage", "--revision", "HEAD", "--json"],
        None,
    );
    assert!(coverage.status.success(), "{:?}", value(&coverage));
    assert_eq!(
        value(&coverage)["result"]["rows"][0]["state"],
        "historical_match"
    );
    assert_eq!(value(&coverage)["result"]["authenticated"], false);
    let coverage = run(
        root.path(),
        &[
            "ci",
            "review-coverage",
            "--revision",
            "HEAD",
            "--policy-file",
            "review-policy.json",
            "--expected-policy",
            &hash,
            "--expected-base-commit",
            &commit,
            "--expected-base-contract",
            &contract,
            "--json",
        ],
        None,
    );
    assert!(coverage.status.success(), "{:?}", value(&coverage));
    assert_eq!(value(&coverage)["result"]["authenticated"], true);
    assert_eq!(
        value(&coverage)["result"]["rows"][0]["current_reviewers"],
        serde_json::json!(["maintainer"])
    );
    let config_path = root.path().join(".workdeck/config.yml");
    let original_config = std::fs::read(&config_path).unwrap();
    let mut changed_config = original_config.clone();
    changed_config.extend_from_slice(b"\n# uncommitted policy document\n");
    std::fs::write(&config_path, changed_config).unwrap();
    let dirty = run(
        root.path(),
        &[
            "ci",
            "review-coverage",
            "--revision",
            "HEAD",
            "--working-tree",
            "--json",
        ],
        None,
    );
    assert!(dirty.status.success(), "{:?}", value(&dirty));
    assert_eq!(
        value(&dirty)["result"]["working_tree"]["matches_revision"],
        false
    );
    assert_eq!(value(&dirty)["result"]["rows"][0]["state"], "stale");
    let gated = run(
        root.path(),
        &[
            "ci",
            "review-coverage",
            "--revision",
            "HEAD",
            "--working-tree",
            "--policy-file",
            "review-policy.json",
            "--expected-policy",
            &hash,
            "--expected-base-commit",
            &commit,
            "--expected-base-contract",
            &contract,
            "--json",
        ],
        None,
    );
    assert!(!gated.status.success());
    std::fs::write(config_path, original_config).unwrap();
    let bad = ContentHash::of(b"candidate-selected policy").to_string();
    let mut wrong = args;
    wrong[13] = &bad;
    assert!(!run(root.path(), &wrong, None).status.success());
}
