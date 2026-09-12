#![cfg(unix)]
use super::*;
use crate::{ReviewApp, ReviewOptions};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ed25519_dalek::{Signer, SigningKey};
use ratatui::{buffer::Buffer, layout::Rect};
use std::{fs, path::Path, process::Command};
use workdeck_pm::*;
fn git(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|s| s.starts_with("GIT_")) {
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
            "user.name=Review Test",
            "-c",
            "user.email=review@example.invalid",
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
fn key(app: &mut ReviewApp, code: KeyCode) {
    super::test_pump::settle_app(app);
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    super::test_pump::settle_app(app);
}
fn screen(app: &ReviewApp, width: u16) -> String {
    let area = Rect::new(0, 0, width, 40);
    let mut buffer = Buffer::empty(area);
    assert!(app.render_workbench_body(area, &mut buffer));
    buffer.content.iter().map(|c| c.symbol()).collect()
}
#[test]
fn mounted_context_distinguishes_historical_review_and_changed_head() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    let repo = Repository::init(root.path(), "WD").unwrap();
    let mut input = CreateIssue::new("Review context", "Requirements");
    input.fields.insert(
        "acceptance".into(),
        serde_json::json!([{"id":"tested","description":"Verified behavior","checked":false}]),
    );
    repo.create_issue(&input, &RequestId::new()).unwrap();
    git(root.path(), &["add", "--", ".workdeck"]);
    git(
        root.path(),
        &["commit", "--quiet", "--no-gpg-sign", "-m", "baseline"],
    );
    let validation = ci_validate(
        root.path(),
        &CiValidateRequest {
            base: CiRevision::Head {},
            head: CiRevision::Head {},
        },
    )
    .unwrap();
    let baseline = CiBaselinePin {
        commit: validation.base.commit,
        contract: validation.contracts.base.unwrap().fingerprint,
    };
    let now = chrono::Utc::now();
    let signing = SigningKey::from_bytes(&[59; 32]);
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
    let approval = CiContractApproval {
        schema: SchemaVersion::CURRENT,
        repository: repo.identity().clone(),
        baseline: baseline.clone(),
        head: validation.head.clone(),
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
    repo.import_contract_review(
        &ImportContractReviewRequest {
            envelope: serde_json::to_string(&envelope).unwrap(),
            expected_policy: policy.fingerprint().unwrap(),
            policy: policy.clone(),
            baseline: baseline.clone(),
            expected_commit: validation.head.commit,
            actor: "importer".into(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let mut app = ReviewApp::new(
        workdeck_diff::changeset_from_patch(
            "",
            "Review context",
            "Review context",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        ),
        ReviewOptions {
            repo: Some(root.path().into()),
            workbench: Some(WorkbenchOptions::new(root.path())),
            highlight: false,
            ..ReviewOptions::default()
        },
    );
    super::test_pump::settle_app(&mut app);
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('i'));
    {
        let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
        let row = shell
            .context
            .rows()
            .into_iter()
            .find(|r| r.id.starts_with("contract-review:"))
            .unwrap();
        assert!(row.body.contains("HistoricalMatch"));
        assert!(row.body.contains("Current authenticated reviewers: none"));
        shell.context.state_mut().selected[0] = Some(row.id);
    }
    assert!(screen(&app, 120).contains("Historical signed contract review"));
    assert!(screen(&app, 55).contains("Contract review"));
    let operations_before = repo.operation_history().unwrap();
    key(&mut app, KeyCode::Char('v'));
    {
        let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
        assert!(
            shell.context.form().is_some(),
            "v must expose explicit review authority inputs"
        );
        for value in [
            serde_json::to_string(&policy).unwrap(),
            policy.fingerprint().unwrap().to_string(),
            baseline.commit.to_string(),
            baseline.contract.to_string(),
        ] {
            shell.context.paste(&value);
            shell
                .context
                .key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(
            shell.context.form().is_none(),
            "{:?}",
            shell.context.state().unwrap().error
        );
        let row = shell
            .context
            .rows()
            .into_iter()
            .find(|r| r.id == "review-auth:summary")
            .unwrap();
        assert!(row.title.contains("Authenticated"), "{row:?}");
        assert!(row.body.contains("maintainer"));
    }
    assert!(screen(&app, 120).contains("Authenticated"));
    assert!(screen(&app, 55).contains("Review authentication"));
    // A well-formed but wrong external policy pin is not current authority.
    key(&mut app, KeyCode::Char('v'));
    {
        let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
        shell
            .context
            .key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        shell
            .context
            .paste(&ContentHash::of(b"wrong policy").to_string());
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(shell.context.rows()[0].title.contains("Not authenticated"));
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        shell
            .context
            .paste(&policy.fingerprint().unwrap().to_string());
        for _ in 0..3 {
            shell
                .context
                .key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(shell.context.rows()[0].title.contains("· Authenticated"));
    }
    // Editing or failing an authority request must erase the previous success.
    key(&mut app, KeyCode::Char('v'));
    {
        let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        shell.context.paste("invalid JSON");
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(shell.context.form().is_some());
        assert!(shell.context.state().unwrap().error.is_some());
        shell
            .context
            .key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(shell.context.rows()[0].title.contains("Not authenticated"));
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        assert_eq!(
            shell.context.form().unwrap().fields[0].value,
            "invalid JSON"
        );
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        shell
            .context
            .paste(&serde_json::to_string(&policy).unwrap());
        shell
            .context
            .key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(shell.context.rows()[0].title.contains("· Authenticated"));
        let current = shell.context.current.clone();
        shell.context.open(None);
        assert!(
            !shell
                .context
                .rows()
                .iter()
                .any(|r| r.id == "review-auth:summary")
        );
        shell.context.open(current);
        assert!(shell.context.rows()[0].title.contains("· Authenticated"));
    }
    let config_path = repo.root().join("config.yml");
    let config = fs::read(&config_path).unwrap();
    fs::write(&config_path, "invalid: [").unwrap();
    key(&mut app, KeyCode::Char('r'));
    assert!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .context
            .rows()[0]
            .title
            .contains("Not authenticated")
    );
    fs::write(config_path, config).unwrap();
    key(&mut app, KeyCode::Char('r'));
    assert!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .context
            .rows()[0]
            .title
            .contains("· Authenticated")
    );
    assert_eq!(repo.operation_history().unwrap(), operations_before);
    fs::write(root.path().join("new.txt"), "new candidate").unwrap();
    git(root.path(), &["add", "--", "new.txt"]);
    git(
        root.path(),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "later candidate",
        ],
    );
    key(&mut app, KeyCode::Char('r'));
    let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
    let row = shell
        .context
        .rows()
        .into_iter()
        .find(|r| r.id.starts_with("contract-review:"))
        .unwrap();
    assert!(row.body.contains("Stale"), "{}", row.body);
    assert!(row.body.contains("review_targets_different_candidate"));
    let current = shell
        .context
        .rows()
        .into_iter()
        .find(|r| r.id == "review-auth:summary")
        .unwrap();
    assert!(current.title.contains("Not authenticated"), "{current:?}");
    shell
        .context
        .key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
    shell
        .context
        .key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert!(shell.context.form().is_none());
    assert!(
        !shell
            .context
            .rows()
            .iter()
            .any(|r| r.id == "review-auth:summary")
    );
}
