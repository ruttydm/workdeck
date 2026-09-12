#![cfg(unix)]
use assert_cmd::prelude::*;
use std::{fs, process::Command};
use workdeck_pm::*;
#[allow(dead_code)]
#[path = "../../workdeck-pm/tests/support/red_green_fixture.rs"]
mod support;
#[test]
fn cli_qualifies_green_only_and_rejects_dirty_inputs_without_writes() {
    let (root, repo, pair) = support::green_only_fixture("fixed");
    let record: ImportedCheckReportRecord = serde_json::from_value(
        repo.import_check_report(
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
    let input = VerifyImportedCheck {
        attestation: record.record.id,
        expected_attestation: record.content,
        check: "unit".into(),
        candidate: pair.candidate,
        policy: pair.producer_policy,
        expected_policy: pair.expected_producer_policy,
        red_green: None,
    };
    fs::write(
        root.path().join("verify.json"),
        serde_json::to_vec(&input).unwrap(),
    )
    .unwrap();
    let before = repo.operation_history().unwrap();
    let run = || {
        Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(root.path())
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .args(["ci", "verify-imported-check", "verify.json", "--json"])
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["kind"], "ci.verify_imported_check");
    fs::write(root.path().join("src/value"), "dirty\n").unwrap();
    assert!(!run().status.success());
    assert_eq!(repo.operation_history().unwrap(), before);
    fs::write(root.path().join("src/value"), "fixed\n").unwrap();
    let issue = repo.list_issues().unwrap().remove(0);
    let completion = CompleteVerifiedIssue {
        issue: issue.metadata.id.clone(),
        expected_issue: issue.source,
        actor: "fixture".into(),
        authority: CompletionAuthority {
            candidate: input.candidate,
            policy: input.policy,
            expected_policy: input.expected_policy,
            red_green: None,
        },
        checks: vec![CompletionCheckSelection {
            check: input.check,
            attestation: input.attestation,
            expected_attestation: input.expected_attestation,
        }],
        gates: vec![],
    };
    fs::write(
        root.path().join("completion.json"),
        serde_json::to_vec(&completion).unwrap(),
    )
    .unwrap();
    let request = RequestId::new().to_string();
    let complete = |preview: bool| {
        let mut cmd = Command::cargo_bin("workdeck").unwrap();
        cmd.current_dir(root.path())
            .env("XDG_CONFIG_HOME", root.path().join("config"));
        if preview {
            cmd.args(["issue", "done", issue.metadata.id.as_str(), "--dry-run"]);
        } else {
            cmd.args([
                "issue",
                "--request-id",
                &request,
                "done",
                issue.metadata.id.as_str(),
            ]);
        }
        cmd.args(["--verification-file", "completion.json", "--json"])
            .output()
            .unwrap()
    };
    let preview = complete(true);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert_eq!(repo.operation_history().unwrap(), before);
    let done = complete(false);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let done: serde_json::Value = serde_json::from_slice(&done.stdout).unwrap();
    assert_eq!(done["result"]["after"]["metadata"]["status"], "done");
    let retry = complete(false);
    assert!(retry.status.success());
    let retry: serde_json::Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(retry["result"], done["result"]);
}
