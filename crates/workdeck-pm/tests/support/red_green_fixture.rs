#![cfg(unix)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use std::{fs, path::Path, process::Command};
use workdeck_pm::*;

fn git(root: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|k| k.starts_with("GIT_")) {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Red Green",
            "-c",
            "user.email=redgreen@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn commit(root: &Path) {
    git(root, &["add", "--", ".workdeck", "src", "evaluators"]);
    git(
        root,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "fixture"],
    );
}
fn write(repo: &Repository, path: &str, value: serde_json::Value) {
    let path = repo.root().join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
fn run(root: &Path, repo: &Repository) -> (CheckReport, String) {
    let prepared = prepare_ci_check(
        root,
        &CiRevision::Head {},
        &CheckPlanRequest {
            checks: vec!["unit".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let input = CiCheckRunRequest {
        expected_binding: prepared.binding.fingerprint.clone(),
        binding: prepared.binding,
        input: CheckRunRequest {
            expected_plan: prepared.plan.fingerprint.clone(),
            plan: prepared.plan,
            actor: "fixture".into(),
        },
    };
    let outcome = repo
        .run_ci_check_plan(&input, &RequestId::new(), &RunControl::default())
        .unwrap();
    let report = repo.export_check_report(&outcome.run.intent.id).unwrap();
    let artifact = &report.publication.result.result.invocations[0].artifacts[0];
    (
        report.clone(),
        fs::read_to_string(repo.root().join(&artifact.path)).unwrap(),
    )
}
fn seal(report: &CheckReport) -> SignedCheckReport {
    let key = SigningKey::from_bytes(&[67; 32]);
    let bytes = serde_json::to_vec(report).unwrap();
    let mut pae = format!(
        "DSSEv1 {} {} {} ",
        CHECK_REPORT_PAYLOAD_TYPE.len(),
        CHECK_REPORT_PAYLOAD_TYPE,
        bytes.len()
    )
    .into_bytes();
    pae.extend_from_slice(&bytes);
    SignedCheckReport {
        payload_type: CHECK_REPORT_PAYLOAD_TYPE.into(),
        payload: STANDARD.encode(bytes),
        signatures: vec![ReportSignature {
            keyid: None,
            sig: STANDARD.encode(key.sign(&pae).to_bytes()),
        }],
    }
}
#[allow(dead_code)] // Some integration targets use the gate-enabled fixture directly.
pub(super) fn fixture(
    red_mode: &str,
    green_mode: &str,
    change_evaluator: bool,
) -> (tempfile::TempDir, Repository, RedGreenRequest) {
    fixture_with_gate(red_mode, green_mode, change_evaluator, false)
}
pub(super) fn fixture_with_gate(
    red_mode: &str,
    green_mode: &str,
    change_evaluator: bool,
    gate: bool,
) -> (tempfile::TempDir, Repository, RedGreenRequest) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    let repo = Repository::init(root.path(), "WD").unwrap();
    let mut issue = CreateIssue::new("Regression behavior", "Required behavior");
    issue.fields.insert(
        "acceptance".into(),
        json!([{"id":"works","description":"Regression behaves correctly","checked":true}]),
    );
    repo.create_issue(&issue, &RequestId::new()).unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("evaluators")).unwrap();
    fs::write(root.path().join("src/value"), format!("{red_mode}\n")).unwrap();
    fs::write(root.path().join("evaluators/test.sh"), r#"IFS= read -r mode < src/value
case "$mode" in
extended) cases='<testcase classname="Behavior" name="regression"/><testcase classname="Behavior" name="additional"/>'; status=0 ;;
flaky) if [ -e once ]; then cases='<testcase classname="Behavior" name="regression"/>'; status=0; else : > once; cases='<testcase classname="Behavior" name="regression"><failure/></testcase>'; status=1; fi ;;
unrelated) cases='<testcase classname="Behavior" name="regression"/><testcase classname="Behavior" name="unrelated"><failure/></testcase>'; status=1 ;;
crash) cases='<testcase classname="Behavior" name="regression"><failure/></testcase>'; status=2 ;;
fixed) cases='<testcase classname="Behavior" name="regression"/>'; status=0 ;;
renamed) cases='<testcase classname="Behavior" name="replacement"/>'; status=0 ;;
infra) cases='<testcase classname="Behavior" name="regression"><error message="setup failed"/></testcase>'; status=1 ;;
skip) cases='<testcase classname="Behavior" name="regression"><skipped/></testcase>'; status=0 ;;
*) cases='<testcase classname="Behavior" name="regression"><failure message="wrong result"/></testcase>'; status=1 ;;
esac
printf '<testsuite name="unit">%s</testsuite>' "$cases" > "$1"
exit "$status"
"#).unwrap();
    commit(root.path());
    git(root.path(), &["branch", "accepted-before-tests"]);
    write(
        &repo,
        "commands/unit.yml",
        json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit",
        "recipe":{"kind":"argv","argv":[{"kind":"literal","value":"sh"},{"kind":"literal","value":"evaluators/test.sh"},{"kind":"artifact","id":"report"}]},
        "cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],"inputs":{"files":["src/value","evaluators/test.sh"]},
        "artifacts":[{"id":"report","name":"junit.xml","max_bytes":8192,"required":true}]}),
    );
    write(
        &repo,
        "checks/unit.yml",
        json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit","command":"unit",
        "expectation":{"kind":"junit","artifact":"report","suites":["unit"],"minimum_tests":1,"maximum_skipped":0,"allowed_exit_codes":[0]},
        "evaluator_inputs":{"files":["evaluators/test.sh"]},"red_green":{"red_exit_codes":[1],"cases":[{"suites":["unit"],"class_name":"Behavior","name":"regression"}]}}),
    );
    let mut config = repo.config().unwrap();
    config.acceptance.required_checks = vec!["unit".into()];
    fs::write(
        repo.root().join("config.yml"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let now = chrono::Utc::now();
    let producer_policy = ProducerTrustPolicy {
        schema: SchemaVersion::CURRENT,
        repository: repo.identity().clone(),
        producers: vec![TrustedProducer {
            id: "ci".into(),
            public_key: STANDARD
                .encode(SigningKey::from_bytes(&[67; 32]).verifying_key().to_bytes()),
            checks: repo
                .command_catalog()
                .unwrap()
                .checks
                .iter()
                .map(|c| (c.definition.id.clone(), c.content.clone()))
                .collect(),
            not_before: now - chrono::Duration::hours(1),
            expires_at: now + chrono::Duration::hours(1),
        }],
    };
    if gate {
        let issue = repo.list_issues().unwrap().remove(0);
        let criterion = repo
            .resolve_criterion(&CriterionOwner::Issue(issue.metadata.id), "works")
            .unwrap()
            .reference;
        let producer = &producer_policy.producers[0];
        let mut value = serde_json::to_value(producer).unwrap();
        value.sort_all_objects();
        let definition = ContentHash::of(&serde_json::to_vec(&value).unwrap());
        let mut input = CreateGate {
            name: "Regression acceptance".into(),
            description: String::new(),
            requirements: vec![GateRequirement {
                id: "behavior".into(),
                criterion,
                producer: ProducerRef {
                    id: producer.id.clone(),
                    definition,
                },
                check: CheckRef {
                    id: "unit".into(),
                    definition: producer.checks["unit"].clone(),
                },
                max_age_seconds: Some(3600),
            }],
            custom: Default::default(),
            extra: Default::default(),
        };
        let mut second = input.requirements[0].clone();
        second.id = "also-required".into();
        input.requirements.push(second);
        let created: GateMutationResult =
            serde_json::from_value(repo.create_gate(&input, &RequestId::new()).unwrap().result)
                .unwrap();
        let issue = repo.list_issues().unwrap().remove(0);
        repo.update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::from([(
                    "gates".into(),
                    json!([created.gate.definition.id]),
                )]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    }
    commit(root.path());
    let captured = ci_validate(
        root.path(),
        &CiValidateRequest {
            base: CiRevision::Head {},
            head: CiRevision::Head {},
        },
    )
    .unwrap();
    let baseline = CiBaselinePin {
        commit: captured.base.commit,
        contract: captured.contracts.base.unwrap().fingerprint,
    };
    let (red, red_artifact) = run(root.path(), &repo);
    fs::write(root.path().join("src/value"), format!("{green_mode}\n")).unwrap();
    if change_evaluator {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(root.path().join("evaluators/test.sh"))
            .unwrap()
            .write_all(b"\n# changed evaluator\n")
            .unwrap();
    }
    commit(root.path());
    let (green, green_artifact) = run(root.path(), &repo);
    let request = RedGreenRequest {
        candidate: green
            .publication
            .intent
            .intent
            .revision
            .as_ref()
            .unwrap()
            .source
            .commit
            .clone(),
        baseline,
        check: "unit".into(),
        expected_producer_policy: producer_policy.fingerprint().unwrap(),
        producer_policy,
        red: seal(&red),
        green: seal(&green),
        red_artifact,
        green_artifact,
    };
    (root, repo, request)
}

pub(super) fn baseline_review(root: &Path, pair: &RedGreenRequest) -> RedGreenBaselineReview {
    let captured = ci_validate(
        root,
        &CiValidateRequest {
            base: CiRevision::Reference {
                reference: "refs/heads/accepted-before-tests".parse().unwrap(),
            },
            head: CiRevision::Commit {
                oid: pair.baseline.commit.clone(),
            },
        },
    )
    .unwrap();
    assert!(captured.contracts.review_required);
    let accepted = CiBaselinePin {
        commit: captured.base.commit,
        contract: captured.contracts.base.unwrap().fingerprint,
    };
    let now = chrono::Utc::now();
    let key = SigningKey::from_bytes(&[68; 32]);
    let policy = ContractReviewPolicy {
        schema: SchemaVersion::CURRENT,
        repository: captured.head.repository.clone(),
        required_reviewers: vec![ContractReviewer {
            id: "maintainer".into(),
            public_key: STANDARD.encode(key.verifying_key().to_bytes()),
            not_before: now - chrono::Duration::hours(1),
            expires_at: now + chrono::Duration::hours(1),
        }],
    };
    let approval = CiContractApproval {
        schema: SchemaVersion::CURRENT,
        repository: captured.head.repository.clone(),
        baseline: accepted.clone(),
        head: captured.head,
        head_contract: pair.baseline.contract.clone(),
        decision: ContractReviewDecision::Approve,
        reviewed_at: now,
        expires_at: now + chrono::Duration::minutes(30),
    };
    let bytes = serde_json::to_vec(&approval).unwrap();
    let mut pae = format!(
        "DSSEv1 {} {} {} ",
        CONTRACT_REVIEW_PAYLOAD_TYPE.len(),
        CONTRACT_REVIEW_PAYLOAD_TYPE,
        bytes.len()
    )
    .into_bytes();
    pae.extend_from_slice(&bytes);
    RedGreenBaselineReview {
        accepted,
        expected_policy: policy.fingerprint().unwrap(),
        policy,
        envelope: SignedContractReview {
            payload_type: CONTRACT_REVIEW_PAYLOAD_TYPE.into(),
            payload: STANDARD.encode(bytes),
            signatures: vec![ReportSignature {
                keyid: None,
                sig: STANDARD.encode(key.sign(&pae).to_bytes()),
            }],
        },
    }
}

#[allow(dead_code)]
pub(super) fn green_only_fixture(mode: &str) -> (tempfile::TempDir, Repository, RedGreenRequest) {
    let (root, repo, mut pair) = fixture("broken", mode, false);
    let path = repo.root().join("checks/unit.yml");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("red_green");
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    commit(root.path());
    let (report, artifact) = run(root.path(), &repo);
    pair.candidate = report
        .publication
        .intent
        .intent
        .revision
        .as_ref()
        .unwrap()
        .source
        .commit
        .clone();
    pair.green = seal(&report);
    pair.green_artifact = artifact;
    pair.producer_policy.producers[0].checks = repo
        .command_catalog()
        .unwrap()
        .checks
        .iter()
        .map(|c| (c.definition.id.clone(), c.content.clone()))
        .collect();
    pair.expected_producer_policy = pair.producer_policy.fingerprint().unwrap();
    (root, repo, pair)
}

#[allow(dead_code)]
pub(super) fn green_only_fixture_with_gate(
    mode: &str,
) -> (tempfile::TempDir, Repository, RedGreenRequest) {
    let (root, repo, mut pair) = green_only_fixture(mode);
    let issue = repo.list_issues().unwrap().remove(0);
    let criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(issue.metadata.id.clone()), "works")
        .unwrap()
        .reference;
    let producer = &pair.producer_policy.producers[0];
    let mut producer_value = serde_json::to_value(producer).unwrap();
    producer_value.sort_all_objects();
    let definition = ContentHash::of(&serde_json::to_vec(&producer_value).unwrap());
    let check_definition = pair.producer_policy.producers[0].checks["unit"].clone();
    let created: GateMutationResult = serde_json::from_value(
        repo.create_gate(
            &CreateGate {
                name: "Green-only acceptance".into(),
                description: String::new(),
                requirements: vec![
                    GateRequirement {
                        id: "behavior".into(),
                        criterion: criterion.clone(),
                        producer: ProducerRef {
                            id: producer.id.clone(),
                            definition: definition.clone(),
                        },
                        check: CheckRef {
                            id: "unit".into(),
                            definition: check_definition.clone(),
                        },
                        max_age_seconds: Some(3600),
                    },
                    GateRequirement {
                        id: "also-required".into(),
                        criterion,
                        producer: ProducerRef {
                            id: producer.id.clone(),
                            definition,
                        },
                        check: CheckRef {
                            id: "unit".into(),
                            definition: check_definition,
                        },
                        max_age_seconds: Some(3600),
                    },
                ],
                custom: Default::default(),
                extra: Default::default(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([(
                "gates".into(),
                json!([created.gate.definition.id]),
            )]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    commit(root.path());
    let (report, artifact) = run(root.path(), &repo);
    pair.candidate = report
        .publication
        .intent
        .intent
        .revision
        .as_ref()
        .unwrap()
        .source
        .commit
        .clone();
    pair.green = seal(&report);
    pair.green_artifact = artifact;
    pair.producer_policy.producers[0].checks = repo
        .command_catalog()
        .unwrap()
        .checks
        .iter()
        .map(|c| (c.definition.id.clone(), c.content.clone()))
        .collect();
    pair.expected_producer_policy = pair.producer_policy.fingerprint().unwrap();
    (root, repo, pair)
}
