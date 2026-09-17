#![cfg(unix)]

//! A bounded, synthetic user/agent journey for the standalone PM surface.
//!
//! The fixture deliberately uses a temporary Git repository and bare remote.  It
//! proves that the native files, CLI preconditions, check runner, claim receipt,
//! policy transitions, and proposal publication can be composed without touching
//! a developer checkout or a real backlog.

use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use workdeck_pm::{Repository, SharedSources};

fn git(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_INDEX_FILE")
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"))
}

fn git_ok(root: &Path, args: &[&str]) {
    let output = git(root, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit(root: &Path, message: &str) {
    git_ok(root, &["add", "--", ".workdeck", "src"]);
    git_ok(
        root,
        &[
            "-c",
            "user.name=Workdeck dogfood",
            "-c",
            "user.email=dogfood@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            message,
        ],
    );
}

fn run(root: &Path, args: &[String]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .args(args)
        .output()
        .unwrap()
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|value| (*value).to_owned()).collect()
}

fn success(root: &Path, args: &[String]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["api_version"], 1, "{value}");
    assert_eq!(value["ok"], true, "{value}");
    value
}

fn source(value: &Value) -> (String, String) {
    (
        value["result"]["source"]["revision"].to_string(),
        value["result"]["source"]["content"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
}

fn repository(root: &Path) -> Repository {
    Repository::open_source(&root.join(".workdeck")).unwrap()
}

fn write_check_catalog(root: &Path, repository: &Repository) {
    fs::create_dir_all(root.join("src")).unwrap();
    for (path, value) in [
        (
            ".workdeck/commands/unit.yml",
            json!({
                "schema": 1,
                "repository": repository.identity(),
                "id": "unit",
                "name": "Dogfood unit check",
                "recipe": {"kind": "argv", "argv": [
                    {"kind": "literal", "value": "sh"},
                    {"kind": "literal", "value": "-c"},
                    {"kind": "literal", "value": "test \"$(cat src/fixture.txt)\" = implemented; printf checked > check-sentinel"}
                ]},
                "cwd": ".",
                "tools": [{"name": "sh", "executable": "/bin/sh"}],
                "inputs": {"files": ["src/fixture.txt"]},
                "bounds": {"timeout_seconds": 3, "stdout_bytes": 2048, "stderr_bytes": 2048}
            }),
        ),
        (
            ".workdeck/checks/unit.yml",
            json!({
                "schema": 1,
                "repository": repository.identity(),
                "id": "unit",
                "name": "Dogfood unit check",
                "command": "unit",
                "expectation": {"kind": "process", "allowed_exit_codes": [0]},
                "evaluator_inputs": {"files": ["src/fixture.txt"]}
            }),
        ),
        (
            ".workdeck/check-profiles/quick.yml",
            json!({
                "schema": 1,
                "repository": repository.identity(),
                "id": "quick",
                "name": "Dogfood quick profile",
                "checks": ["unit"]
            }),
        ),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
}

#[test]
fn synthetic_agent_journey_can_claim_check_complete_publish_and_resume() {
    let root = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    let root_path = root.path();

    git_ok(root_path, &["init", "--quiet", "-b", "main"]);
    git_ok(root_path, &["config", "user.name", "Workdeck dogfood"]);
    git_ok(
        root_path,
        &["config", "user.email", "dogfood@example.invalid"],
    );
    git_ok(remote.path(), &["init", "--quiet", "--bare"]);
    git_ok(
        root_path,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );

    success(root_path, &strings(&["init", "--prefix", "WD", "--json"]));
    let root_repository = repository(root_path);

    // Migration is an explicit read-only step on a fresh source.  The source
    // directory is empty, so the command must leave its legacy files absent.
    fs::create_dir_all(root_path.join(".agents/workdeck")).unwrap();
    let migration = success(
        root_path,
        &strings(&["migrate", "legacy", "--dry-run", "--json"]),
    );
    assert_eq!(migration["kind"], "migration_preview");
    assert!(
        fs::read_dir(root_path.join(".agents/workdeck"))
            .unwrap()
            .next()
            .is_none()
    );

    let project = success(
        root_path,
        &strings(&[
            "project",
            "create",
            "Dogfood project",
            "--id",
            "dogfood-project",
            "--exit-criterion",
            "shipped=The fixture is accepted",
            "--request-id",
            "project-create",
            "--json",
        ]),
    );
    let milestone = success(
        root_path,
        &strings(&[
            "milestone",
            "create",
            "Dogfood milestone",
            "--id",
            "dogfood-milestone",
            "--project",
            "dogfood-project",
            "--outcome",
            "shipped=The fixture is checked",
            "--request-id",
            "milestone-create",
            "--json",
        ]),
    );
    fs::write(
        root_path.join("feature.json"),
        serde_json::to_vec(&json!({
            "name": "Dogfood capability",
            "body": "The capability is exercised by the synthetic journey.\n",
            "fields": {
                "decision": "accepted",
                "criteria": [{"id": "works", "description": "The fixture works"}]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let feature = success(
        root_path,
        &strings(&[
            "feature",
            "create",
            "--from-json",
            "feature.json",
            "--request-id",
            "feature-create",
            "--json",
        ]),
    );
    let feature_id = feature["result"]["record"]["metadata"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    fs::write(
        root_path.join("issue.json"),
        serde_json::to_vec(&json!({
            "title": "Implement the dogfood fixture",
            "body": "A complete synthetic implementation journey.\n",
            "fields": {
                "project": "dogfood-project",
                "milestone": "dogfood-milestone",
                "features": [feature_id]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let issue = success(
        root_path,
        &strings(&[
            "issue",
            "create",
            "--from-json",
            "issue.json",
            "--request-id",
            "issue-create",
            "--json",
        ]),
    );
    let issue_id = issue["result"]["metadata"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (issue_revision, issue_content) = source(&issue);

    let selected = success(
        root_path,
        &strings(&["issue", "next", "--project", "dogfood-project", "--json"]),
    );
    assert_eq!(selected["result"]["selected"]["issue"], issue_id);
    let context = success(
        root_path,
        &strings(&[
            "context", "--issue", &issue_id, "--budget", "12000", "--json",
        ]),
    );
    assert_eq!(context["result"]["anchor"]["issue"], issue_id);

    // Establish an accepted, committed source before enabling shared
    // coordination.  The fixture starts with a baseline input; the candidate
    // implementation is committed after the claim is acquired.
    write_check_catalog(root_path, &root_repository);
    fs::write(root_path.join("src/fixture.txt"), "baseline\n").unwrap();
    commit(root_path, "dogfood planning baseline");
    git_ok(root_path, &["push", "--quiet", "origin", "HEAD:main"]);

    let mut config = root_repository.config().unwrap();
    config.sources = Some(SharedSources {
        remote: "origin".into(),
        accepted_ref: "refs/heads/main".parse().unwrap(),
        coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
        proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
    });
    fs::write(
        root_repository.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    commit(root_path, "dogfood shared source configuration");
    git_ok(root_path, &["push", "--quiet", "origin", "HEAD:main"]);
    let source_status_output = run(
        root_path,
        &strings(&["source", "status", "--json", "--no-input"]),
    );
    let source_status: Value = serde_json::from_slice(&source_status_output.stdout).unwrap();
    // A missing coordination ref is reported as a nonzero diagnostic while
    // the status document still exposes the reviewed remote binding needed by
    // the first shared claim operation.
    assert_eq!(source_status["ok"], true, "{source_status}");
    let shared_binding = source_status["result"]["binding"]
        .as_str()
        .unwrap()
        .to_owned();

    let contract = success(
        root_path,
        &strings(&["claim", "contract", &issue_id, "--json"]),
    );
    let contract_hash = contract["result"]["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    let claim_request = "dogfood-claim-acquire";
    let acquired = success(
        root_path,
        &strings(&[
            "claim",
            "acquire",
            "--contract",
            &contract_hash,
            "--expected-contract",
            &contract_hash,
            "--actor",
            "dogfood-agent",
            "--expected-binding",
            &shared_binding,
            "--request-id",
            claim_request,
            "--json",
            "--no-input",
        ]),
    );
    assert_eq!(acquired["result"]["may_continue"], true);
    let claim = &acquired["result"]["current"]["claim"];
    let claim_token = claim["metadata"]["token"].as_str().unwrap().to_owned();
    let claim_generation = claim["metadata"]["generation"].to_string();
    let claim_content = claim["source"]["content"].as_str().unwrap().to_owned();

    // Implementation is a concrete fixture file.  Commit it before the
    // revision-bound CI plan so the plan and execution refer to one immutable
    // candidate source.
    fs::write(root_path.join("src/fixture.txt"), "implemented\n").unwrap();
    commit(root_path, "dogfood fixture implementation");

    let plan = success(
        root_path,
        &strings(&[
            "ci",
            "plan",
            "--revision",
            "HEAD",
            "--profile",
            "quick",
            "--issue",
            &issue_id,
            "--json",
        ]),
    );
    fs::write(
        root_path.join("ci-plan.json"),
        serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
    let binding = plan["result"]["binding"]["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    let check = success(
        root_path,
        &strings(&[
            "ci",
            "check",
            "--plan-file",
            "ci-plan.json",
            "--expected-plan",
            &binding,
            "--actor",
            "dogfood-agent",
            "--request-id",
            "dogfood-check",
            "--json",
        ]),
    );
    assert_eq!(check["result"]["state"], "passed");
    assert_eq!(
        fs::read(root_path.join("check-sentinel")).unwrap(),
        b"checked"
    );

    let review = success(
        root_path,
        &strings(&[
            "ci",
            "review-coverage",
            "--revision",
            "HEAD",
            "--subject",
            &format!("issue:{issue_id}"),
            "--json",
        ]),
    );
    assert_eq!(review["kind"], "ci.review_coverage");
    assert_eq!(review["result"]["authenticated"], false);

    let completed = success(
        root_path,
        &strings(&[
            "claim",
            "complete",
            &issue_id,
            "--token",
            &claim_token,
            "--generation",
            &claim_generation,
            "--expected-content",
            &claim_content,
            "--actor",
            "dogfood-agent",
            "--request-id",
            "dogfood-claim-complete",
            "--contract",
            &contract_hash,
            "--expected-contract",
            &contract_hash,
            "--expected-issue-revision",
            &issue_revision,
            "--expected-issue-content",
            &issue_content,
            "--expected-binding",
            &shared_binding,
            "--json",
            "--no-input",
        ]),
    );
    assert_eq!(completed["kind"], "claimed_completion_receipt");
    assert_eq!(
        success(root_path, &strings(&["issue", "show", &issue_id, "--json"]))["result"]["metadata"]
            ["status"],
        "done"
    );

    let feature_source = success(
        root_path,
        &strings(&["feature", "show", &feature_id, "--json"]),
    );
    let (feature_revision, feature_content) = source(&feature_source);
    success(
        root_path,
        &strings(&[
            "feature",
            "promote",
            &feature_id,
            "--to",
            "specified",
            "--actor",
            "dogfood-agent",
            "--reason",
            "Accepted the fixture criteria",
            "--expected-revision",
            &feature_revision,
            "--expected-content",
            &feature_content,
            "--request-id",
            "feature-specified",
            "--json",
        ]),
    );
    let feature_source = success(
        root_path,
        &strings(&["feature", "show", &feature_id, "--json"]),
    );
    let (feature_revision, feature_content) = source(&feature_source);
    success(
        root_path,
        &strings(&[
            "feature",
            "promote",
            &feature_id,
            "--to",
            "implemented",
            "--actor",
            "dogfood-agent",
            "--reason",
            "Accepted the completed fixture",
            "--expected-revision",
            &feature_revision,
            "--expected-content",
            &feature_content,
            "--request-id",
            "feature-implemented",
            "--json",
        ]),
    );

    let (milestone_revision, milestone_content) = source(&milestone);
    success(
        root_path,
        &strings(&[
            "milestone",
            "complete",
            "dogfood-milestone",
            "--actor",
            "dogfood-agent",
            "--reason",
            "Accepted the checked fixture",
            "--expected-revision",
            &milestone_revision,
            "--expected-content",
            &milestone_content,
            "--request-id",
            "milestone-complete",
            "--json",
        ]),
    );
    let (project_revision, project_content) = source(&project);
    let assessed = success(
        root_path,
        &strings(&["project", "assess", "dogfood-project", "--json"]),
    );
    assert_eq!(assessed["result"]["allowed"], false);
    assert_eq!(assessed["result"]["basis"], "declared");
    success(
        root_path,
        &strings(&[
            "project",
            "complete",
            "dogfood-project",
            "--actor",
            "dogfood-agent",
            "--reason",
            "Accepted the complete dogfood journey",
            "--expected-revision",
            &project_revision,
            "--expected-content",
            &project_content,
            "--request-id",
            "project-complete",
            "--json",
        ]),
    );

    // Proposal publication is explicit and local to the fixture's bare remote.
    // Claims remain coordination state; accepted planning and implementation
    // files are the only candidate content published by the proposal engine.
    commit(root_path, "dogfood accepted planning state");
    let preview = success(
        root_path,
        &strings(&[
            "source",
            "proposal",
            "preview",
            "--ref",
            "refs/heads/workdeck-proposals/dogfood",
            "--title",
            "Dogfood proposal",
            "--json",
        ]),
    );
    let plan_hash = preview["result"]["plan"]["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    let published = success(
        root_path,
        &strings(&[
            "source",
            "proposal",
            "publish",
            "--plan",
            &plan_hash,
            "--expected-plan",
            &plan_hash,
            "--request-id",
            "dogfood-publication",
            "--json",
            "--no-input",
        ]),
    );
    assert_eq!(published["result"]["state"], "confirmed");
    assert_eq!(
        success(
            root_path,
            &strings(&[
                "source",
                "proposal",
                "resume",
                "--request-id",
                "dogfood-publication",
                "--json",
            ])
        )["result"]["state"],
        "confirmed"
    );

    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_parent.path().join("resume");
    git_ok(
        clone_parent.path(),
        &[
            "clone",
            "--quiet",
            "--branch",
            "workdeck-proposals/dogfood",
            remote.path().to_str().unwrap(),
            clone_path.to_str().unwrap(),
        ],
    );
    let resumed = success(
        &clone_path,
        &strings(&[
            "context", "--issue", &issue_id, "--budget", "12000", "--json",
        ]),
    );
    assert_eq!(resumed["result"]["anchor"]["issue"], issue_id);
    assert_eq!(
        success(
            &clone_path,
            &strings(&["issue", "show", &issue_id, "--json"])
        )["result"]["metadata"]["status"],
        "done"
    );
    assert!(repository(&clone_path).doctor().unwrap().valid);
}
