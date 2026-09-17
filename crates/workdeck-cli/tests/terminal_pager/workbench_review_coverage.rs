//! Signed-review context through the actual normal-startup terminal executable.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use workdeck_pm::*;
fn select_review(session: &mut Session) {
    let mut frame = session.wait(|text| text.lines().any(|line| line.contains('›')));
    for _ in 0..60 {
        let selected = frame
            .lines()
            .find(|line| line.contains('›'))
            .unwrap()
            .to_owned();
        if selected.contains("Contract review") {
            return;
        }
        session.write(b"j");
        frame = session.wait(|text| {
            text.lines()
                .find(|line| line.contains('›'))
                .is_some_and(|line| line != selected)
        });
    }
    panic!("review row not reachable: {frame}");
}
fn retain(repo: &Repository) -> ReviewCoverageAuthority {
    let validation = ci_validate(
        repo.root().parent().unwrap(),
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
    let signing = SigningKey::from_bytes(&[61; 32]);
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
    ReviewCoverageAuthority {
        expected_policy: policy.fingerprint().unwrap(),
        policy,
        baseline,
    }
}
#[test]
fn signed_review_context_stays_historical_and_marks_a_later_head_stale() {
    for width in [78, 140] {
        let (root, repo) = repository();
        let mut input = CreateIssue::new("Review coverage task", "Accepted requirement");
        input.fields.insert("acceptance".into(), serde_json::json!([{"id":"reviewed","description":"Reviewed behavior","checked":false}]));
        repo.create_issue(&input, &RequestId::new()).unwrap();
        git(root.path(), &["add", "--", ".workdeck"]);
        git(
            root.path(),
            &["commit", "--quiet", "--no-gpg-sign", "-m", "reviewed task"],
        );
        let authority = retain(&repo);
        let before = repo.operation_history().unwrap();
        let mut session = startup(root.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORi");
        session.wait(|text| text.contains("Task context") && text.contains("Review coverage task"));
        select_review(&mut session);
        session.wait(|text| {
            text.contains("HistoricalMatch") && text.contains("Historical signed contract review")
        });
        session.write(b"v");
        session.wait(|text| text.contains("Authenticate task review"));
        for value in [
            serde_json::to_string(&authority.policy).unwrap(),
            authority.expected_policy.to_string(),
            authority.baseline.commit.to_string(),
            authority.baseline.contract.to_string(),
        ] {
            session.write(format!("\x1b[200~{value}\x1b[201~\t").as_bytes());
        }
        session.write(b"\x13");
        session.wait(|text| {
            text.contains("Review authentication") && text.contains("· Authenticated")
        });
        let config = repo.root().join("config.yml");
        let original = fs::read(&config).unwrap();
        let mut dirty = original.clone();
        dirty.extend_from_slice(b"\n# working policy edit\n");
        fs::write(&config, dirty).unwrap();
        session.write(b"r");
        session.wait(|text| text.contains("Task context") && text.contains("Not authenticated"));
        fs::write(config, original).unwrap();
        session.write(b"r");
        session.wait(|text| {
            text.contains("Review authentication") && text.contains("· Authenticated")
        });
        fs::write(root.path().join("later.txt"), "later candidate\n").unwrap();
        git(root.path(), &["add", "--", "later.txt"]);
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
        session.write(b"r");
        session.wait(|text| text.contains("Task context") && text.contains("Not authenticated"));
        assert_eq!(repo.operation_history().unwrap(), before);
    }
}
