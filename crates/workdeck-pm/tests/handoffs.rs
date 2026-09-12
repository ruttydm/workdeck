use workdeck_pm::*;

#[test]
fn malformed_handoff_is_independently_diagnosed() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Work", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let directory = repo
        .root()
        .join("issues")
        .join(issue.metadata.id.as_str())
        .join("handoffs");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("bad.md"), "not a handoff").unwrap();
    assert!(
        !repo.doctor().unwrap().valid,
        "handoff authority cannot be invisible to doctor"
    );
}

use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier},
};
use workdeck_pm::transactions::{FaultPoint, TransactionStore};
fn setup() -> (tempfile::TempDir, Repository, IssueRecord, CreateHandoff) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue::new("Continue work", "Acceptance lives here"),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let anchor = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap()
        .anchor;
    let input = CreateHandoff {
        actor: "agent-one".into(),
        anchor,
        body: "Resume from this summary.\n".into(),
        attempted: vec!["Inspected the implementation".into()],
        uncertainties: vec!["Behavior not verified".into()],
        evidence_refs: vec![],
        questions: vec![],
        pending_operations: vec![],
        next_steps: vec!["Review the accepted requirements".into()],
        custom: BTreeMap::from([(
            "legacy".into(),
            json!({"unknown":[null,18446744073709551615u64]}),
        )]),
        extra: BTreeMap::from([("x-provider".into(), json!({"opaque":true}))]),
    };
    (temp, repo, issue, input)
}
#[test]
fn immutable_handoff_preserves_issue_anchor_and_replays_after_source_change() {
    let (_temp, repo, issue, input) = setup();
    let item = std::fs::read(repo.root().join(&issue.path)).unwrap();
    let request = RequestId::new();
    let receipt = repo.create_handoff(&input, &request).unwrap();
    let handoff: HandoffRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(handoff.body, input.body);
    assert_eq!(handoff.metadata.custom, input.custom);
    assert_eq!(handoff.metadata.extra, input.extra);
    assert_eq!(std::fs::read(repo.root().join(&issue.path)).unwrap(), item);
    assert_eq!(
        repo.context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
            .unwrap()
            .anchor,
        input.anchor,
        "handoff does not invalidate its own basis"
    );
    repo.mutate_issue(
        issue.metadata.id.as_str(),
        Some(&issue.source),
        &IssueMutation::Update {
            input: UpdateIssue {
                fields: BTreeMap::from([("title".into(), json!("Changed source"))]),
                body: None,
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    assert_ne!(
        repo.context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
            .unwrap()
            .anchor,
        handoff.metadata.anchor
    );
    assert_eq!(repo.create_handoff(&input, &request).unwrap(), receipt);
    assert_eq!(
        repo.create_handoff(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repo.handoff(&issue.metadata.id, &handoff.metadata.id)
            .unwrap(),
        handoff
    );
}
#[test]
fn question_state_changes_invalidate_handoff_anchor_without_mutating_issue() {
    let (_temp, repo, issue, input) = setup();
    let question = CreateQuestion {
        actor: "author".into(),
        body: "Question".into(),
        subjects: vec![QuestionSubject {
            subject: SubjectRef::Issue(issue.metadata.id.clone()),
            source: issue.source.clone(),
        }],
        requirements: vec![],
        blocks_work: true,
        custom: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let q = serde_json::from_value::<QuestionMutationResult>(
        repo.create_question(&question, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
    .question;
    assert_eq!(
        repo.create_handoff(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let mut next = input;
    next.anchor = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap()
        .anchor;
    next.questions = vec![q.metadata.id.clone()];
    repo.mutate_question(
        &q.metadata.id,
        &q.source,
        &QuestionMutation::Answer {
            actor: "reviewer".into(),
            body: "Decision".into(),
            decision_refs: vec![],
        },
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.create_handoff(&next, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repo.show_issue(issue.metadata.id.as_str()).unwrap().source,
        issue.source
    );
}
#[test]
fn malformed_and_forged_handoff_inputs_never_publish() {
    let (_temp, repo, _issue, input) = setup();
    let before = repo.export_snapshot().unwrap();
    let mut bad = input.clone();
    bad.anchor.fingerprint = ContentHash::of(b"forged");
    assert_eq!(
        repo.create_handoff(&bad, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let mut bad = input.clone();
    bad.extra.insert("verified".into(), json!(true));
    assert!(repo.create_handoff(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.anchor.repository = RepositoryId::new();
    assert!(repo.create_handoff(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.pending_operations = vec![HandoffOperationRef {
        request_id: RequestId::new(),
        operation_id: Some(OperationId::new()),
        receipt_content: None,
    }];
    assert!(repo.create_handoff(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.actor = "bad\nactor".into();
    assert!(repo.create_handoff(&bad, &RequestId::new()).is_err());
    assert_eq!(repo.export_snapshot().unwrap().files, before.files);
}
#[test]
fn global_handoff_identity_and_orphan_records_are_diagnosed() {
    let (_temp, repo, issue, input) = setup();
    let h: HandoffRecord = serde_json::from_value(
        repo.create_handoff(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let other: IssueRecord = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Other", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let path = repo
        .root()
        .join("issues")
        .join(other.metadata.id.as_str())
        .join("handoffs")
        .join(format!("{}.md", h.metadata.id));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = h
        .document
        .replace(issue.metadata.id.as_str(), other.metadata.id.as_str());
    std::fs::write(&path, text).unwrap();
    assert!(!repo.doctor().unwrap().valid);
    assert!(repo.handoffs(&issue.metadata.id).is_err());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(repo.root().join(&issue.path)).unwrap();
    assert!(!repo.doctor().unwrap().valid);
}
#[test]
fn equal_value_handoffs_keep_distinct_identity_but_same_request_deduplicates() {
    let (_temp, repo, issue, input) = setup();
    let barrier = Arc::new(Barrier::new(2));
    let request = RequestId::new();
    let handles = (0..2)
        .map(|_| {
            let repo = repo.clone();
            let input = input.clone();
            let request = request.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                repo.create_handoff(&input, &request)
            })
        })
        .collect::<Vec<_>>();
    let a = handles
        .into_iter()
        .map(|h| h.join().unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(a[0], a[1]);
    let second = repo.create_handoff(&input, &RequestId::new()).unwrap();
    assert_ne!(a[0].operation_id, second.operation_id);
    assert_eq!(repo.handoffs(&issue.metadata.id).unwrap().len(), 2);
}
#[test]
fn source_race_is_rejected_before_journal_and_fault_after_publish_recovers() {
    let (_temp, repo, issue, input) = setup();
    let path = repo.root().join(&issue.path);
    let original = std::fs::read(&path).unwrap();
    let mut changed = false;
    let result = repo.create_handoff_with_faults(&input, &RequestId::new(), |p| {
        if p == FaultPoint::BeforeJournal && !changed {
            changed = true;
            let mut bytes = original.clone();
            bytes.extend_from_slice(b"\nDirect editor change\n");
            std::fs::write(&path, bytes).unwrap();
        }
        Ok(())
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
    assert!(repo.handoffs(&issue.metadata.id).unwrap().is_empty());
    std::fs::write(path, original).unwrap();
    let request = RequestId::new();
    assert!(
        repo.create_handoff_with_faults(&input, &request, |p| if p == FaultPoint::AfterChange(0) {
            Err(PmError::new(ErrorCode::Io, "interrupted"))
        } else {
            Ok(())
        })
        .is_err()
    );
    assert_eq!(
        repo.handoffs(&issue.metadata.id).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    TransactionStore::open(repo.root())
        .unwrap()
        .recover()
        .unwrap();
    let receipt = repo.create_handoff(&input, &request).unwrap();
    assert_eq!(repo.create_handoff(&input, &request).unwrap(), receipt);
    assert_eq!(repo.handoffs(&issue.metadata.id).unwrap().len(), 1);
}
#[test]
fn retirement_retains_handoff_bytes_and_snapshot_restores_historical_context() {
    let (_temp, repo, issue, input) = setup();
    let request = RequestId::new();
    let receipt = repo.create_handoff(&input, &request).unwrap();
    let h: HandoffRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    let target = RetirementTarget::new(RetirementKind::Issue, issue.metadata.id.as_str()).unwrap();
    let plan = repo.retirement_preview(&target).unwrap();
    assert!(plan.allowed);
    repo.retire_record(
        &RetirementInput {
            target,
            expected: Some(plan.source),
            expected_preview: Some(plan.fingerprint),
        },
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(repo.handoff(&issue.metadata.id, &h.metadata.id).unwrap(), h);
    assert_eq!(
        repo.create_handoff(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(repo.create_handoff(&input, &request).unwrap(), receipt);
    let snapshot = repo.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new()).unwrap();
    let restored = Repository::open_source(&root).unwrap();
    assert!(restored.doctor().unwrap().valid);
    assert_eq!(
        restored
            .handoff(&issue.metadata.id, &h.metadata.id)
            .unwrap(),
        h
    );
    assert_eq!(restored.create_handoff(&input, &request).unwrap(), receipt);
}
#[test]
fn immutable_handoff_native_import_rejects_matching_file_overwrite() {
    let (_temp, repo, _issue, input) = setup();
    let h: HandoffRecord = serde_json::from_value(
        repo.create_handoff(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let snapshot = repo.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    restore_snapshot(&root, &snapshot, None, &RequestId::new()).unwrap();
    let imported = Repository::open_source(&root).unwrap();
    std::fs::write(
        root.join(&h.path),
        h.document.replace(&h.body, "Changed historical handoff"),
    )
    .unwrap();
    let changed = imported.export_snapshot().unwrap();
    let plan = repo
        .preview_snapshot_import(&changed, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(!plan.allowed);
    assert!(
        plan.blockers
            .iter()
            .any(|e| e.path.as_ref().is_some_and(|p| p == &h.path)),
        "{:?}",
        plan.blockers
    );
}
#[test]
fn handoff_receipt_proof_rejects_consistent_body_hash_forgery() {
    let (_temp, repo, _issue, input) = setup();
    let mut receipt = repo.create_handoff(&input, &RequestId::new()).unwrap();
    let mut h: HandoffRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    h.document = h.document.replace(&h.body, "Forged verified summary");
    h.body = "Forged verified summary".into();
    h.content = ContentHash::of(h.document.as_bytes());
    receipt.changed[0].after = Some(h.content.clone());
    receipt.result = json!(h);
    std::fs::write(
        repo.root()
            .join("operations")
            .join(format!("{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&receipt).unwrap(),
    )
    .unwrap();
    assert!(repo.create_handoff(&input, &receipt.request_id).is_err());
    assert!(repo.export_snapshot().is_err());
}
#[test]
fn declared_operation_refs_are_exact_and_actor_policy_remains_prospective() {
    let (_temp, repo, _issue, mut input) = setup();
    let operation = repo
        .write_wiki(
            &WriteWiki {
                path: "notes.md".into(),
                body: "Notes".into(),
                expected: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    let path = repo
        .root()
        .join("operations")
        .join(format!("{}.yml", operation.operation_id));
    let content = ContentHash::of(&std::fs::read(&path).unwrap());
    input.pending_operations = vec![HandoffOperationRef {
        request_id: operation.request_id.clone(),
        operation_id: Some(operation.operation_id.clone()),
        receipt_content: Some(content),
    }];
    let mut wrong = input.clone();
    wrong.pending_operations[0].request_id = RequestId::new();
    assert!(repo.create_handoff(&wrong, &RequestId::new()).is_err());
    let request = RequestId::new();
    let receipt = repo.create_handoff(&input, &request).unwrap();
    repo.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    assert!(repo.create_handoff(&input, &RequestId::new()).is_err());
    assert_eq!(repo.create_handoff(&input, &request).unwrap(), receipt);
}
#[cfg(unix)]
#[test]
fn exact_staging_preserves_mixed_index_for_question_and_handoff_receipts() {
    use std::process::Command;
    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let mut c = Command::new("git");
        for (k, _) in std::env::vars_os() {
            if k.to_str().is_some_and(|k| k.starts_with("GIT_")) {
                c.env_remove(k);
            }
        }
        let out = c
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
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
    let (temp, repo, issue, input) = setup();
    git(temp.path(), &["init", "-q"]);
    git(temp.path(), &["config", "user.name", "Fixture"]);
    git(
        temp.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(temp.path().join("notes.txt"), "initial").unwrap();
    git(temp.path(), &["add", "--", "notes.txt"]);
    git(
        temp.path(),
        &["commit", "-q", "--no-gpg-sign", "-m", "fixture"],
    );
    std::fs::write(temp.path().join("notes.txt"), "staged").unwrap();
    git(temp.path(), &["add", "--", "notes.txt"]);
    std::fs::write(temp.path().join("notes.txt"), "unstaged").unwrap();
    let h = repo.create_handoff(&input, &RequestId::new()).unwrap();
    let q = repo
        .create_question(
            &CreateQuestion {
                actor: "author".into(),
                body: "Question".into(),
                subjects: vec![QuestionSubject {
                    subject: SubjectRef::Issue(issue.metadata.id.clone()),
                    source: issue.source,
                }],
                requirements: vec![],
                blocks_work: false,
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            &RequestId::new(),
        )
        .unwrap();
    repo.stage_operation(&h).unwrap();
    repo.stage_operation(&q).unwrap();
    let expected = git(temp.path(), &["diff", "--cached", "--name-only"]);
    assert_eq!(expected.lines().count(), 5);
    assert_eq!(git(temp.path(), &["show", ":notes.txt"]), "staged");
    assert_eq!(
        std::fs::read_to_string(temp.path().join("notes.txt")).unwrap(),
        "unstaged"
    );
    let index = std::fs::read(temp.path().join(".git/index")).unwrap();
    repo.stage_operation(&h).unwrap();
    assert_eq!(
        std::fs::read(temp.path().join(".git/index")).unwrap(),
        index
    );
    let hpath = repo.root().join(&h.changed[0].path);
    std::fs::write(hpath, "Changed source").unwrap();
    assert!(repo.stage_operation(&h).is_err());
    assert_eq!(
        std::fs::read(temp.path().join(".git/index")).unwrap(),
        index
    );
}
