use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use workdeck_pm::{Repository, RequestId};

fn run(cwd: &Path, args: &[&str]) -> (std::process::ExitStatus, Value) {
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(cwd)
        .env("XDG_CONFIG_HOME", cwd.join("config"))
        .args(args)
        .output()
        .unwrap();
    let value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{args:?}: {output:?}"));
    (output.status, value)
}

#[cfg(unix)]
#[test]
fn reviewed_registration_resolves_only_explicit_mappings_and_replays_after_target_disappears() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    let (status, empty) = run(owner.path(), &["repository", "list", "--json"]);
    assert!(status.success(), "{empty}");
    assert_eq!(empty["result"]["entries"], serde_json::json!([]));
    assert!(!owner_repo.root().join(".local/repositories").exists());
    let (status, plan) = run(
        owner.path(),
        &[
            "repository",
            "inspect",
            "second",
            target.path().to_str().unwrap(),
            "--json",
        ],
    );
    assert!(status.success(), "{plan}");
    assert_eq!(
        plan["result"]["mutation"]["checkout"]["repository"],
        target_repo.identity().as_str()
    );
    assert!(!owner_repo.root().join(".local/repositories").exists());
    fs::write(
        owner.path().join("register.json"),
        serde_json::to_vec(&plan["result"]).unwrap(),
    )
    .unwrap();
    let request = RequestId::new();
    let command = [
        "repository",
        "register",
        "--input",
        "register.json",
        "--request-id",
        request.as_str(),
        "--json",
    ];
    let (status, registered) = run(owner.path(), &command);
    assert!(status.success(), "{registered}");
    let (status, shown) = run(owner.path(), &["repository", "show", "second", "--json"]);
    assert!(status.success(), "{shown}");
    assert_eq!(
        shown["result"]["repository"],
        target_repo.identity().as_str()
    );
    assert!(!target_repo.root().join(".local/repositories").exists());
    fs::remove_dir_all(target_repo.root()).unwrap();
    let (status, missing) = run(owner.path(), &["repository", "show", "second", "--json"]);
    assert!(!status.success(), "{missing}");
    let (status, listed) = run(owner.path(), &["repository", "list", "--json"]);
    assert!(status.success(), "{listed}");
    assert_eq!(listed["result"]["entries"].as_array().unwrap().len(), 1);
    let (status, replay) = run(owner.path(), &command);
    assert!(status.success(), "{replay}");
    assert_eq!(replay["result"]["replayed"], true);
    let revision = listed["result"]["source"]["revision"]
        .as_u64()
        .unwrap()
        .to_string();
    let content = listed["result"]["source"]["content"].as_str().unwrap();
    let removal = RequestId::new();
    let (status, removed) = run(
        owner.path(),
        &[
            "repository",
            "remove",
            "second",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--request-id",
            removal.as_str(),
            "--json",
        ],
    );
    assert!(status.success(), "{removed}");
    assert_eq!(
        run(owner.path(), &["repository", "list", "--json"]).1["result"]["entries"],
        serde_json::json!([])
    );
}

#[test]
fn registry_commands_do_not_initialize_a_missing_planning_source() {
    let temp = tempfile::tempdir().unwrap();
    let (status, value) = run(temp.path(), &["repository", "list", "--json"]);
    assert!(!status.success(), "{value}");
    assert!(value["error"].is_object(), "{value}");
    assert!(!temp.path().join(".workdeck").exists());
    assert!(!temp.path().join(".git").exists());
}

#[cfg(unix)]
#[test]
fn my_work_pages_qualified_rows_and_reports_unavailable_sources_with_nonzero_exit() {
    use workdeck_pm::{CreateIssue, SourceSelector, registry::*};
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    for title in ["First assignment", "Second assignment"] {
        let mut input = CreateIssue::new(title, "Context");
        input
            .fields
            .insert("assignee".into(), serde_json::json!("agent-a"));
        target_repo.create_issue(&input, &RequestId::new()).unwrap();
    }
    let store = RegistryStore::open(&owner_repo).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: inspect_checkout(
                        "target",
                        target.path(),
                        SourceSelector::WorkingTree,
                    )
                    .unwrap(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let command = [
        "repository",
        "my-work",
        "--assignee",
        "agent-a",
        "--repository",
        "target",
        "--limit",
        "1",
        "--json",
        "--no-input",
    ];
    let (status, first) = run(owner.path(), &command);
    assert!(status.success(), "{first}");
    assert_eq!(first["result"]["known_total"], 2);
    assert_eq!(first["result"]["rows"][0]["alias"], "target");
    assert_eq!(
        first["result"]["rows"][0]["row"]["token"]["key"]["repository"],
        target_repo.identity().as_str()
    );
    let cursor = first["result"]["next_cursor"].as_str().unwrap();
    let mut next = command.to_vec();
    next.extend(["--cursor", cursor]);
    let (status, second) = run(owner.path(), &next);
    assert!(status.success(), "{second}");
    assert_ne!(
        first["result"]["rows"][0]["row"]["token"]["key"],
        second["result"]["rows"][0]["row"]["token"]["key"]
    );
    assert!(second["result"]["next_cursor"].is_null());
    fs::remove_dir_all(target_repo.root()).unwrap();
    let (status, unavailable) = run(owner.path(), &command);
    assert_eq!(status.code(), Some(4), "{unavailable}");
    assert_eq!(unavailable["result"]["all_sources_available"], false);
    assert!(unavailable["result"]["sources"][0]["error"].is_object());
    assert!(unavailable["result"]["sources"][0]["matches"].is_null());
    assert!(!target_repo.root().exists());
}

#[test]
fn my_work_cli_exposes_review_and_explicit_time_overdue_facets() {
    use workdeck_pm::{CreateIssue, SourceSelector, registry::*};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let store = RegistryStore::open(&repository).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: inspect_checkout(
                        "self",
                        directory.path(),
                        SourceSelector::WorkingTree,
                    )
                    .unwrap(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    for (title, fields) in [
        (
            "Review request",
            serde_json::json!({"reviewer":"Ada", "status":"in_review"}),
        ),
        (
            "Overdue assignment",
            serde_json::json!({"assignee":"Ada", "due_at":"2026-09-09"}),
        ),
    ] {
        let mut input = CreateIssue::new(title, "Contract");
        input.fields = serde_json::from_value(fields).unwrap();
        repository.create_issue(&input, &RequestId::new()).unwrap();
    }
    let (status, review) = run(
        directory.path(),
        &[
            "repository",
            "my-work",
            "--assignee",
            "Ada",
            "--facet",
            "review-requested",
            "--json",
        ],
    );
    assert!(status.success(), "{review}");
    assert_eq!(review["result"]["facet"], "review_requested");
    assert_eq!(
        review["result"]["rows"][0]["row"]["title"],
        "Review request"
    );
    let (status, overdue) = run(
        directory.path(),
        &[
            "repository",
            "my-work",
            "--assignee",
            "Ada",
            "--facet",
            "overdue",
            "--as-of",
            "2026-09-10T00:00:00Z",
            "--json",
        ],
    );
    assert!(status.success(), "{overdue}");
    assert_eq!(
        overdue["result"]["rows"][0]["row"]["title"],
        "Overdue assignment"
    );
    assert_eq!(overdue["result"]["as_of"], "2026-09-10T00:00:00Z");
    let (status, missing) = run(
        directory.path(),
        &[
            "repository",
            "my-work",
            "--assignee",
            "Ada",
            "--facet",
            "overdue",
            "--json",
        ],
    );
    assert!(!status.success(), "{missing}");
    assert!(missing.to_string().contains("as_of"));
}

#[test]
fn blocked_cli_reports_source_bound_prerequisite_reasons() {
    use workdeck_pm::{CreateIssue, IssueRecord, SourceSelector, registry::*};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let prerequisite: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Prerequisite", "Contract"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let mut input = CreateIssue::new("Blocked task", "Contract");
    input.fields = serde_json::from_value(
        serde_json::json!({"assignee":"Ada", "prerequisites":[prerequisite.metadata.id]}),
    )
    .unwrap();
    repository.create_issue(&input, &RequestId::new()).unwrap();
    let store = RegistryStore::open(&repository).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: inspect_checkout(
                        "self",
                        directory.path(),
                        SourceSelector::WorkingTree,
                    )
                    .unwrap(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let (status, report) = run(
        directory.path(),
        &[
            "repository",
            "my-work",
            "--assignee",
            "Ada",
            "--facet",
            "blocked",
            "--json",
        ],
    );
    assert!(status.success(), "{report}");
    assert_eq!(report["result"]["known_total"], 1);
    assert_eq!(
        report["result"]["rows"][0]["evidence"]["readiness"]["conditions"][0]["reason_code"],
        "incomplete_prerequisite"
    );
    assert_eq!(
        report["result"]["sources"][0]["evidence_sources"][0]["identity"],
        report["result"]["rows"][0]["row"]["token"]["view"]["source"]
    );
}

#[test]
fn claimed_cli_exposes_observation_scope_without_requiring_issue_assignment() {
    use workdeck_pm::{
        AcquireClaim, ClaimRequest, CreateIssue, IssueRecord, SourceSelector, registry::*,
    };
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Claimed task", "Contract"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    repository
        .mutate_local_claim(
            &ClaimRequest::Acquire {
                input: Box::new(AcquireClaim {
                    actor: "Ada".into(),
                    contract: repository.local_claim_contract(&issue.metadata.id).unwrap(),
                    ttl_seconds: None,
                    recovery: None,
                }),
            },
            &RequestId::new(),
        )
        .unwrap();
    let store = RegistryStore::open(&repository).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: inspect_checkout(
                        "self",
                        directory.path(),
                        SourceSelector::WorkingTree,
                    )
                    .unwrap(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let as_of = chrono::Utc::now().to_rfc3339();
    let (status, report) = run(
        directory.path(),
        &[
            "repository",
            "my-work",
            "--assignee",
            "Ada",
            "--facet",
            "claimed",
            "--as-of",
            &as_of,
            "--json",
        ],
    );
    assert!(status.success(), "{report}");
    assert_eq!(report["result"]["known_total"], 1);
    assert_eq!(
        report["result"]["rows"][0]["evidence"]["claim"]["assessment"]["guarantee"],
        "local_source_only"
    );
    assert_eq!(
        report["result"]["rows"][0]["evidence"]["selected_requirements_match"],
        true
    );
    let (status, report) = run(
        directory.path(),
        &[
            "repository",
            "my-work",
            "--assignee",
            "Ada",
            "--facet",
            "claimed",
            "--json",
        ],
    );
    assert!(!status.success(), "{report}");
    assert!(report.to_string().contains("as_of"));
}
