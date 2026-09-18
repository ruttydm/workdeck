use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, process::Command};
use workdeck_pm::*;
#[test]
fn cycle_carryover_previews_then_applies_and_replays_exact_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let repo = Repository::init(root, "WD").unwrap();
    for id in ["old", "new"] {
        repo.create_planning(
            PlanningKind::Cycle,
            &CreatePlanning {
                id: Some(id.into()),
                ..CreatePlanning::new(id)
            },
            &RequestId::new(),
        )
        .unwrap();
    }
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue {
                title: "CLI carryover".into(),
                body: "".into(),
                fields: BTreeMap::from([("cycle".into(), json!("old"))]),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let run = |extra: &[&str]| {
        let output = Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .args(["cycle", "carryover", "old", "new", "--json", "--no-input"])
            .args(extra)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let preview = run(&[]);
    assert_eq!(
        repo.show_issue(issue.metadata.id.as_str()).unwrap().source,
        issue.source
    );
    assert_eq!(preview["result"]["issues"].as_array().unwrap().len(), 1);
    let fingerprint = preview["result"]["fingerprint"].as_str().unwrap();
    let id = RequestId::new().to_string();
    let receipt = run(&["--expected-preview", fingerprint, "--request-id", &id]);
    assert_eq!(
        repo.show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .cycle
            .as_deref(),
        Some("new")
    );
    assert_eq!(
        run(&["--expected-preview", fingerprint, "--request-id", &id]),
        receipt
    );
}
