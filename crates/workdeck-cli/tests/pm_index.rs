use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use workdeck_pm::{CreateIssue, IssueRecord, Repository, RequestId, UpdateIssue};

fn run(root: &Path, args: &[&str]) -> (std::process::ExitStatus, Value) {
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(args)
        .output()
        .unwrap();
    let value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{args:?}: {output:?}"));
    (output.status, value)
}
fn input(root: &Path, name: &str, value: Value) {
    fs::write(root.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn cached_queries_do_not_initialize_and_pagination_rejects_changed_generations() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let repo = Repository::init(root, "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue::new("Original", "Bound body"),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.create_issue(&CreateIssue::new("Second", ""), &RequestId::new())
        .unwrap();
    input(root, "query.json", json!({"kind":"issues","query":{}}));
    let (status, missing) = run(root, &["index", "query", "--input", "query.json", "--json"]);
    assert!(!status.success(), "{missing}");
    assert_eq!(fs::read_dir(repo.root().join(".index")).unwrap().count(), 0);
    let (status, refreshed) = run(root, &["index", "refresh", "--json"]);
    assert!(status.success(), "{refreshed}");
    let (status, first) = run(
        root,
        &[
            "index",
            "query",
            "--input",
            "query.json",
            "--limit",
            "1",
            "--json",
        ],
    );
    assert!(status.success(), "{first}");
    assert_eq!(first["result"]["status"]["state"], "cached");
    assert_eq!(first["result"]["page"]["rows"][0]["title"], "Original");
    assert_eq!(first["source"]["freshness"], "cached");
    assert_eq!(first["source"]["selector"]["kind"], "working_tree");
    let (status, projected) = run(
        root,
        &[
            "index",
            "query",
            "--input",
            "query.json",
            "--fields",
            "page.rows",
            "--compact",
        ],
    );
    assert!(status.success(), "{projected}");
    assert_eq!(projected["source"]["freshness"], "cached");
    assert_eq!(
        projected["source"]["projection"],
        first["source"]["projection"]
    );
    let expected = serde_json::to_string(&first["result"]["page"]["handle"]).unwrap();
    let (status, second) = run(
        root,
        &[
            "index",
            "query",
            "--input",
            "query.json",
            "--offset",
            "1",
            "--limit",
            "1",
            "--expected-query",
            &expected,
            "--json",
        ],
    );
    assert!(status.success(), "{second}");
    assert_eq!(second["result"]["page"]["rows"][0]["title"], "Second");
    input(
        root,
        "token.json",
        first["result"]["page"]["rows"][0]["token"].clone(),
    );
    assert!(run(root, &["index", "show", "--input", "token.json", "--json"]).1["result"]["detail"]["document"].as_str().unwrap().contains("Bound body"));
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([("title".into(), json!("Changed"))]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let (_, old) = run(root, &["index", "query", "--input", "query.json", "--json"]);
    assert_eq!(old["result"]["page"]["rows"][0]["title"], "Original");
    assert_eq!(old["source"]["freshness"], "cached");
    assert!(run(root, &["index", "refresh", "--json"]).0.success());
    let (status, stale) = run(
        root,
        &[
            "index",
            "query",
            "--input",
            "query.json",
            "--offset",
            "1",
            "--expected-query",
            &expected,
            "--json",
        ],
    );
    assert!(!status.success(), "{stale}");
    assert_eq!(stale["error"]["code"], "stale_source");
    assert!(
        !run(root, &["index", "show", "--input", "token.json", "--json"])
            .0
            .success()
    );
}

#[test]
fn board_and_tree_queries_use_shared_bounded_structures_and_discoverable_contracts() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let repo = Repository::init(root, "WD").unwrap();
    for assignee in ["Alice", "Bob"] {
        repo.create_issue(
            &CreateIssue {
                title: assignee.into(),
                body: String::new(),
                fields: std::collections::BTreeMap::from([("assignee".into(), json!(assignee))]),
            },
            &RequestId::new(),
        )
        .unwrap();
    }
    let parent: workdeck_pm::FeatureOutcome = serde_json::from_value(
        repo.create_feature(&workdeck_pm::CreateFeature::new("Zulu"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut child = workdeck_pm::CreateFeature::new("Alpha");
    child
        .fields
        .insert("parent".into(), json!(parent.record.metadata.id));
    repo.create_feature(&child, &RequestId::new()).unwrap();
    input(
        root,
        "board.json",
        json!({"kind":"issues","query":{},"group_by":"assignee"}),
    );
    input(
        root,
        "tree.json",
        json!({"kind":"features","query":{"tree":true}}),
    );
    assert!(run(root, &["index", "refresh", "--json"]).0.success());
    let (status, board) = run(
        root,
        &[
            "index",
            "board",
            "--input",
            "board.json",
            "--columns",
            "2",
            "--rows",
            "1",
            "--json",
        ],
    );
    assert!(status.success(), "{board}");
    assert_eq!(board["result"]["columns"].as_array().unwrap().len(), 2);
    assert_eq!(board["result"]["columns"][1]["group"]["value"], "Bob");
    let (status, tree) = run(root, &["index", "query", "--input", "tree.json", "--json"]);
    assert!(status.success(), "{tree}");
    assert_eq!(tree["result"]["page"]["rows"][0]["title"], "Zulu");
    assert_eq!(tree["result"]["page"]["rows"][1]["tree"]["depth"], 1);
    let (status, schema) = run(root, &["schema", "projection-query", "--json"]);
    assert!(status.success(), "{schema}");
    let (status, commands) = run(root, &["protocol", "render", "commands", "--json"]);
    assert!(status.success(), "{commands}");
    assert!(commands.to_string().contains("index refresh"));
}

#[test]
fn failed_refresh_retains_cached_data_and_read_sources_never_create_storage() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    input(root, "query.json", json!({"kind":"issues","query":{}}));
    assert!(
        !run(root, &["index", "query", "--input", "query.json", "--json"])
            .0
            .success()
    );
    assert!(!root.join(".workdeck").exists() && !root.join(".git").exists());
    let repo = Repository::init(root, "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Retained", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    assert!(run(root, &["index", "refresh", "--json"]).0.success());
    fs::write(
        repo.root()
            .join(format!("issues/{}/item.md", issue.metadata.id)),
        "---\nschema: [invalid\n---\n",
    )
    .unwrap();
    let (status, failed) = run(root, &["index", "refresh", "--json"]);
    assert!(!status.success(), "{failed}");
    let (status, retained) = run(root, &["index", "query", "--input", "query.json", "--json"]);
    assert!(status.success(), "{retained}");
    assert_eq!(retained["result"]["page"]["rows"][0]["title"], "Retained");
    assert_eq!(retained["source"]["freshness"], "cached");
    assert!(
        !run(
            root,
            &[
                "index",
                "query",
                "--input",
                "query.json",
                "--offset",
                "1",
                "--json"
            ]
        )
        .0
        .success()
    );
    let entries = || {
        let mut names = fs::read_dir(repo.root().join(".index"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let before = entries();
    assert!(
        !run(
            root,
            &[
                "index",
                "query",
                "--source",
                "accepted",
                "--input",
                "query.json",
                "--json"
            ]
        )
        .0
        .success()
    );
    assert!(
        !run(
            root,
            &[
                "index",
                "query",
                "--source",
                "proposal",
                "--input",
                "query.json",
                "--json"
            ]
        )
        .0
        .success()
    );
    assert_eq!(entries(), before);
    let (_, capabilities) = run(root, &["capabilities", "--json"]);
    let query = capabilities["result"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["path"] == "index query")
        .unwrap();
    assert_eq!(query["native_planning"]["implemented"], true);
    assert!(
        query["native_planning"]["limitations"]
            .as_array()
            .unwrap()
            .contains(&json!("cached_reads_do_not_validate_current_source"))
    );
}

#[test]
fn cached_handles_and_document_tokens_cannot_cross_repository_checkouts() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    for root in [first.path(), second.path()] {
        let repo = Repository::init(root, "WD").unwrap();
        for name in ["Same title", "Same second title"] {
            repo.create_issue(&CreateIssue::new(name, ""), &RequestId::new())
                .unwrap();
        }
        input(root, "query.json", json!({"kind":"issues","query":{}}));
        assert!(run(root, &["index", "refresh", "--json"]).0.success());
    }
    let (_, page) = run(
        first.path(),
        &["index", "query", "--input", "query.json", "--json"],
    );
    let expected = serde_json::to_string(&page["result"]["page"]["handle"]).unwrap();
    input(
        second.path(),
        "token.json",
        page["result"]["page"]["rows"][0]["token"].clone(),
    );
    let (status, mismatch) = run(
        second.path(),
        &[
            "index",
            "query",
            "--input",
            "query.json",
            "--expected-query",
            &expected,
            "--json",
        ],
    );
    assert!(!status.success(), "{mismatch}");
    assert_eq!(mismatch["error"]["code"], "stale_source");
    let (status, mismatch) = run(
        second.path(),
        &["index", "show", "--input", "token.json", "--json"],
    );
    assert!(!status.success(), "{mismatch}");
    assert_eq!(mismatch["error"]["code"], "stale_source");
}
