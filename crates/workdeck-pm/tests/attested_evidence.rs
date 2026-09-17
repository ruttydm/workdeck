#![cfg(unix)]
use workdeck_pm::*;
#[path = "support/red_green_fixture.rs"]
mod support;

#[test]
fn exact_criterion_links_reverify_original_proof_and_reject_changed_or_superseded_claims() {
    let (root, repo, pair) = support::fixture("broken", "fixed", false);
    let review = support::baseline_review(root.path(), &pair);
    let input = ImportCheckReportRequest {
        envelope: serde_json::to_string(&pair.green).unwrap(),
        policy: pair.producer_policy.clone(),
        expected_policy: pair.expected_producer_policy.clone(),
        expected_commit: pair.candidate.clone(),
        actor: "fixture".into(),
        red_green: Some(RetainedRedGreenProof {
            baseline: pair.baseline.clone(),
            check: pair.check.clone(),
            red: pair.red.clone(),
            red_artifact: pair.red_artifact.clone(),
            green_artifact: pair.green_artifact.clone(),
            review: Some(review.clone()),
        }),
    };
    let imported: ImportedCheckReportRecord = serde_json::from_value(
        repo.import_check_report(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let authority = RetainedRedGreenAuthority {
        baseline: pair.baseline,
        candidate: pair.candidate,
        policy: pair.producer_policy.clone(),
        expected_policy: pair.expected_producer_policy,
        review: Some(ReviewCoverageAuthority {
            baseline: review.accepted,
            policy: review.policy,
            expected_policy: review.expected_policy,
        }),
    };
    let verified = repo
        .reauthenticate_imported_red_green(&imported.record.id, &authority)
        .unwrap();
    let report = repo
        .reauthenticate_imported_report(
            &imported.record.id,
            &authority.policy,
            &authority.expected_policy,
            &authority.candidate,
        )
        .unwrap();
    let issue = repo.list_issues().unwrap().remove(0);
    let criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(issue.metadata.id.clone()), "works")
        .unwrap()
        .reference;
    let declaration: DeclareEvidence = serde_json::from_value(serde_json::json!({
        "criterion":criterion, "subject":{"repository":repo.identity(),"kind":"source","content":verified.pair.candidate.content},
        "producer":verified.pair.green_producer,"check":verified.pair.check,
        "result":{"id":verified.pair.green_run,"content":imported.record.admission.report},
        "observed_at":report.report.observed_at,"provenance":{"actor":"fixture","reason":"Exact regression result"},
        "links":[{"kind":"attestation","id":imported.record.id,"content":imported.content}]
    })).unwrap();
    let record: EvidenceRecord = serde_json::from_value(
        repo.declare_evidence(&declaration, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let request = RedGreenEvidenceRequest {
        evidence: record.reference.id.clone(),
        expected_evidence: record.content.clone(),
        authority,
    };
    let before = repo.operation_history().unwrap();
    let result = repo.verify_red_green_evidence(&request).unwrap();
    assert_eq!(result.criterion.reference, criterion);
    assert_eq!(result.attestation, imported.record.id);
    assert_eq!(result.attestation_content, imported.content);
    assert_eq!(repo.operation_history().unwrap(), before);
    // The accepted check requirement still needs completion-policy integration.
    assert!(
        !repo
            .completion_report(issue.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    let mut wrong = request.clone();
    wrong.expected_evidence = ContentHash::of(b"wrong declaration bytes");
    assert!(repo.verify_red_green_evidence(&wrong).is_err());
    for field in [
        "attestation",
        "producer",
        "check",
        "result",
        "time",
        "missing",
        "expired",
    ] {
        let mut bad = declaration.clone();
        match field {
            "attestation" => {
                bad.links = vec![EvidenceLink::Attestation {
                    id: imported.record.id.clone(),
                    content: ContentHash::of(b"wrong stored bytes"),
                }]
            }
            "producer" => bad.producer.definition = ContentHash::of(b"wrong producer definition"),
            "check" => bad.check.definition = ContentHash::of(b"wrong check definition"),
            "result" => bad.result.content = ContentHash::of(b"wrong result"),
            "time" => bad.observed_at -= chrono::Duration::seconds(1),
            "missing" => bad.links.clear(),
            _ => bad.expires_at = Some(bad.observed_at),
        }
        let other: EvidenceRecord = serde_json::from_value(
            repo.declare_evidence(&bad, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        let invalid = RedGreenEvidenceRequest {
            evidence: other.reference.id,
            expected_evidence: other.content,
            authority: request.authority.clone(),
        };
        assert!(repo.verify_red_green_evidence(&invalid).is_err(), "{field}");
    }
    let mut new_issue = CreateIssue::new(
        "Uncommitted criterion owner",
        "Not present in the candidate",
    );
    new_issue.fields.insert("acceptance".into(), serde_json::json!([{"id":"works","description":"Not checked in this commit","checked":true}]));
    let new_issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&new_issue, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut absent = declaration.clone();
    absent.criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(new_issue.metadata.id), "works")
        .unwrap()
        .reference;
    let absent: EvidenceRecord = serde_json::from_value(
        repo.declare_evidence(&absent, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    assert!(
        repo.verify_red_green_evidence(&RedGreenEvidenceRequest {
            evidence: absent.reference.id,
            expected_evidence: absent.content,
            authority: request.authority.clone()
        })
        .is_err()
    );
    let mut duplicate = declaration.clone();
    duplicate.links.extend(declaration.links.clone());
    assert_eq!(
        repo.declare_evidence(&duplicate, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
    repo.update_issue(issue.metadata.id.as_str(), &issue.source, &UpdateIssue {
        fields: std::collections::BTreeMap::from([("acceptance".into(),serde_json::json!([{"id":"works","description":"Changed requirement","checked":true}]))]), body: None,
    }, &RequestId::new()).unwrap();
    assert!(
        repo.verify_red_green_evidence(&request)
            .unwrap_err()
            .message
            .contains("current criterion")
    );
    let current = repo
        .list_issues()
        .unwrap()
        .into_iter()
        .find(|r| r.metadata.id == issue.metadata.id)
        .unwrap();
    repo.update_issue(issue.metadata.id.as_str(), &current.source, &UpdateIssue {
        fields: std::collections::BTreeMap::from([("acceptance".into(),serde_json::json!([{"id":"works","description":"Regression behaves correctly","checked":true}]))]), body: None,
    }, &RequestId::new()).unwrap();
    let path = repo.root().join(&record.path);
    let error = repo
        .verify_red_green_evidence_with_faults(&request, || {
            std::fs::write(&path, format!("{}\n", record.document)).unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    std::fs::write(path, &record.document).unwrap();
    let mut bad = declaration.clone();
    bad.subject.content = ContentHash::of(b"another source");
    let other: EvidenceRecord = serde_json::from_value(
        repo.declare_evidence(&bad, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    wrong.evidence = other.reference.id;
    wrong.expected_evidence = other.content;
    assert!(repo.verify_red_green_evidence(&wrong).is_err());
    bad = declaration.clone();
    bad.supersedes = Some(EvidenceSupersession {
        id: record.reference.id,
        content: record.content,
        reason: "Correction".into(),
    });
    repo.declare_evidence(&bad, &RequestId::new()).unwrap();
    assert!(repo.verify_red_green_evidence(&request).is_err());
}
