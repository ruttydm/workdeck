use serde_json::json;
use std::{collections::BTreeMap, path::Path};
use workdeck_pm::transactions::{FaultPoint, TransactionStore};
use workdeck_pm::*;
fn setup() -> (tempfile::TempDir, Repository, GateRecord, DeclareEvidence) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let mut create = CreateIssue::new("Subject", "");
    create.fields.insert(
        "acceptance".into(),
        json!([{"id":"works","description":"Works","checked":true}]),
    );
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&create, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(issue.metadata.id), "works")
        .unwrap()
        .reference;
    let producer = ProducerRef {
        id: "test-runner".into(),
        definition: ContentHash::of(b"producer"),
    };
    let check = CheckRef {
        id: "suite".into(),
        definition: ContentHash::of(b"suite"),
    };
    let gate = CreateGate {
        name: "Gate".into(),
        description: "".into(),
        requirements: vec![GateRequirement {
            id: "works".into(),
            criterion: criterion.clone(),
            producer: producer.clone(),
            check: check.clone(),
            max_age_seconds: Some(60),
        }],
        custom: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let gate = serde_json::from_value::<GateMutationResult>(
        repo.create_gate(&gate, &RequestId::new()).unwrap().result,
    )
    .unwrap()
    .gate;
    let input = DeclareEvidence {
        criterion,
        subject: ExactSubject {
            repository: repo.identity().clone(),
            kind: ExactSubjectKind::Source,
            content: ContentHash::of(b"tree"),
        },
        producer,
        check,
        result: ResultRef {
            id: "result-1".into(),
            content: ContentHash::of(b"result"),
        },
        observed_at: chrono::Utc::now() - chrono::Duration::seconds(1),
        expires_at: None,
        provenance: DeclaredProvenance {
            actor: "agent".into(),
            reason: "Imported runner reference".into(),
        },
        links: vec![
            EvidenceLink::Source {
                link: SourceLink {
                    path: "src/main.rs".into(),
                    line: Some(1),
                    end_line: None,
                },
            },
            EvidenceLink::Url {
                url: "https://example.test/runs/1".into(),
            },
        ],
        supersedes: None,
        custom: BTreeMap::from([("unknown".into(), json!({"retained":null}))]),
        extra: BTreeMap::from([("x-source".into(), json!("retain"))]),
    };
    (temp, repo, gate, input)
}
fn record(repo: &Repository, input: &DeclareEvidence) -> EvidenceRecord {
    serde_json::from_value(
        repo.declare_evidence(input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}
fn assess(
    repo: &Repository,
    gate: &GateRecord,
    input: &DeclareEvidence,
    as_of: Timestamp,
) -> GateAssessment {
    repo.assess_gate(&GateAssessmentRequest {
        gate: gate.definition.id.clone(),
        subject: input.subject.clone(),
        as_of,
    })
    .unwrap()
}
fn reasons(assessment: &GateAssessment) -> Vec<&str> {
    assessment
        .requirements
        .iter()
        .flat_map(|r| &r.conditions)
        .map(|c| c.reason_code.as_str())
        .collect()
}
#[test]
fn exact_matching_declarations_never_claim_verified_success_and_roundtrip() {
    let (_d, repo, gate, input) = setup();
    let entry = record(&repo, &input);
    let report = assess(&repo, &gate, &input, chrono::Utc::now());
    assert_eq!(report.state, ConditionState::Unknown);
    assert!(reasons(&report).contains(&"producer_unadmitted"));
    assert!(reasons(&report).contains(&"evidence_declared_only"));
    assert!(repo.doctor().unwrap().valid);
    let snapshot = repo.export_snapshot().unwrap();
    let encoded = snapshot.to_jsonl().unwrap();
    let snapshot = decode_snapshot(encoded.as_bytes()).unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new()).unwrap();
    let restored = Repository::open_source(&root).unwrap();
    assert_eq!(restored.evidence(&entry.reference.id).unwrap(), entry);
    assert_eq!(
        restored
            .assess_gate(&GateAssessmentRequest {
                gate: gate.definition.id,
                subject: input.subject,
                as_of: report.as_of
            })
            .unwrap(),
        report
    );
}
#[test]
fn every_mismatch_and_expiry_is_explicit_and_never_green() {
    for (field, code) in [
        ("subject", "evidence_subject_mismatch"),
        ("criterion", "evidence_definition_mismatch"),
        ("producer", "evidence_producer_mismatch"),
        ("check", "evidence_check_mismatch"),
        ("age", "evidence_stale"),
        ("expiry", "evidence_stale"),
    ] {
        let (_d, repo, gate, good) = setup();
        let mut input = good.clone();
        match field {
            "subject" => input.subject.content = ContentHash::of(b"other"),
            "criterion" => input.criterion.definition = ContentHash::of(b"changed"),
            "producer" => input.producer.definition = ContentHash::of(b"changed"),
            "check" => input.check.definition = ContentHash::of(b"changed"),
            "age" => input.observed_at -= chrono::Duration::seconds(120),
            "expiry" => input.expires_at = Some(input.observed_at),
            _ => unreachable!(),
        }
        record(&repo, &input);
        let report = assess(&repo, &gate, &good, chrono::Utc::now());
        assert_eq!(report.state, ConditionState::Unknown);
        assert!(
            reasons(&report).contains(&code),
            "{field}: {:?}",
            reasons(&report)
        );
    }
}
#[test]
fn supersession_is_explicit_no_forks_and_historical_as_of_is_exact() {
    let (_d, repo, gate, input) = setup();
    let request = RequestId::new();
    let receipt = repo.declare_evidence(&input, &request).unwrap();
    let first: EvidenceRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    let mut amend = input.clone();
    amend.result.id = "result-2".into();
    amend.supersedes = Some(EvidenceSupersession {
        id: first.reference.id.clone(),
        content: first.content.clone(),
        reason: "Correct result pointer".into(),
    });
    let second = record(&repo, &amend);
    assert_eq!(
        repo.evidence_references(&EvidenceQuery::default()).unwrap(),
        vec![second.clone()]
    );
    assert_eq!(
        repo.evidence_references(&EvidenceQuery {
            include_superseded: true,
            ..Default::default()
        })
        .unwrap()
        .len(),
        2
    );
    let before = assess(&repo, &gate, &input, first.reference.recorded_at);
    assert_eq!(
        before.requirements[0].evidence,
        vec![first.reference.id.clone()]
    );
    let after = assess(&repo, &gate, &input, second.reference.recorded_at);
    assert_eq!(
        after.requirements[0].evidence,
        vec![second.reference.id.clone()]
    );
    assert_eq!(
        repo.declare_evidence(&amend, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(repo.declare_evidence(&input, &request).unwrap(), receipt);
    assert_eq!(
        std::fs::read_to_string(repo.root().join(first.path)).unwrap(),
        first.document
    );
    let third = record(&repo, &input);
    assert_ne!(
        third.reference.id, first.reference.id,
        "equal values with a new request are independent declarations"
    );
}
#[test]
fn unsafe_and_authority_injecting_inputs_have_no_side_effect() {
    let (_d, repo, _gate, input) = setup();
    let before = repo.export_snapshot().unwrap();
    for field in [
        "passed",
        "verified",
        "producer_trusted",
        "recorded_at",
        "id",
    ] {
        let mut bad = input.clone();
        bad.extra.insert(field.into(), json!(true));
        assert!(repo.declare_evidence(&bad, &RequestId::new()).is_err());
    }
    for url in [
        "file:///secret",
        "https://user:pass@example.test",
        "https://example.test/a b",
    ] {
        let mut bad = input.clone();
        bad.links = vec![EvidenceLink::Url { url: url.into() }];
        assert!(repo.declare_evidence(&bad, &RequestId::new()).is_err());
    }
    let mut bad = input.clone();
    bad.subject.repository = RepositoryId::new();
    assert!(repo.declare_evidence(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.observed_at += chrono::Duration::days(1);
    assert!(repo.declare_evidence(&bad, &RequestId::new()).is_err());
    assert_eq!(repo.export_snapshot().unwrap().files, before.files);
}
#[test]
fn concurrent_supersession_serializes_and_interruption_recovers_once() {
    let (_d, repo, _gate, input) = setup();
    let old = record(&repo, &input);
    let mut amend = input.clone();
    amend.supersedes = Some(EvidenceSupersession {
        id: old.reference.id,
        content: old.content,
        reason: "Correction".into(),
    });
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|_| {
            let repo = repo.clone();
            let input = amend.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                repo.declare_evidence(&input, &RequestId::new())
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .any(|r| r.as_ref().is_err_and(|e| e.code == ErrorCode::Conflict))
    );
    let request = RequestId::new();
    let failure = repo.declare_evidence_with_faults(&input, &request, |point| {
        if point == FaultPoint::AfterJournal {
            Err(PmError::new(ErrorCode::Io, "interrupted"))
        } else {
            Ok(())
        }
    });
    assert!(failure.is_err());
    assert_eq!(
        repo.evidence_references(&EvidenceQuery::default())
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    TransactionStore::open(repo.root())
        .unwrap()
        .recover()
        .unwrap();
    let receipt = repo.declare_evidence(&input, &request).unwrap();
    assert_eq!(repo.declare_evidence(&input, &request).unwrap(), receipt);
    assert!(repo.doctor().unwrap().valid);
}
#[test]
fn malformed_records_and_forged_receipt_proofs_are_diagnosed() {
    let (_d, repo, _gate, input) = setup();
    let request = RequestId::new();
    let receipt = repo.declare_evidence(&input, &request).unwrap();
    let mut entry: EvidenceRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    entry.reference.declaration.provenance.reason = "forged".into();
    let mut forged = receipt.clone();
    forged.result = json!(entry);
    let operation = repo
        .root()
        .join("operations")
        .join(format!("{}.yml", receipt.operation_id));
    std::fs::write(&operation, serde_yaml_ng::to_string(&forged).unwrap()).unwrap();
    assert!(repo.declare_evidence(&input, &request).is_err());
    assert!(repo.export_snapshot().is_err());
    std::fs::write(&operation, serde_yaml_ng::to_string(&receipt).unwrap()).unwrap();
    std::fs::write(
        repo.root().join(Path::new("evidence/bad.yml")),
        "schema: 99\n",
    )
    .unwrap();
    assert!(!repo.doctor().unwrap().valid);
}
#[test]
fn evidence_policy_requires_new_fields_but_retains_historical_identity_debt() {
    use std::collections::BTreeSet;
    let (_d, repo, _gate, mut input) = setup();
    let historical = record(&repo, &input);
    let schema = SchemaChange {
        fields: BTreeMap::from([(
            "risk".into(),
            CustomFieldDefinition {
                field_type: CustomFieldType::Text,
                scopes: BTreeSet::from([CustomScope::Evidence]),
                required: true,
                archived: false,
                options: vec![],
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
        )]),
        ..Default::default()
    };
    let plan = repo.preview_schema_change(&schema).unwrap();
    assert!(
        plan.allowed,
        "immutable historical records become explicit policy debt"
    );
    assert!(
        plan.compliance
            .violations
            .iter()
            .any(|v| v.historical && v.path == historical.path)
    );
    repo.apply_schema_change(&schema, Some(&plan.fingerprint), &RequestId::new())
        .unwrap();
    assert_eq!(
        repo.declare_evidence(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    input.custom.insert("risk".into(), json!("high"));
    let entry = record(&repo, &input);
    assert_eq!(
        entry.reference.declaration.custom["unknown"],
        json!({"retained":null})
    );
    repo.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    assert_eq!(
        repo.declare_evidence(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert!(repo.evidence(&historical.reference.id).is_ok());
}
#[test]
fn evidence_source_membership_race_never_publishes_an_unchecked_supersession() {
    let (_d, repo, _gate, input) = setup();
    let old = record(&repo, &input);
    let mut amendment = input;
    amendment.supersedes = Some(EvidenceSupersession {
        id: old.reference.id.clone(),
        content: old.content,
        reason: "corrected".into(),
    });
    let path = repo.root().join(&old.path);
    let before = std::fs::read_to_string(&path).unwrap();
    let result = repo.declare_evidence_with_faults(&amendment, &RequestId::new(), |point| {
        if point == FaultPoint::BeforeJournal {
            std::fs::write(&path, format!("{before}\n# changed after validation\n")).unwrap();
        }
        Ok(())
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
    assert_eq!(
        repo.evidence_references(&EvidenceQuery {
            include_superseded: true,
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
}
#[test]
fn evidence_exact_staging_preserves_other_index_content_and_rejects_old_source() {
    use std::process::Command;
    let (temp, repo, _gate, input) = setup();
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
                command.env_remove(key);
            }
        }
        let output = command
            .current_dir(temp.path())
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    git(&["init", "--quiet"]);
    std::fs::write(temp.path().join("unrelated"), "staged").unwrap();
    git(&["add", "--", "unrelated"]);
    std::fs::write(temp.path().join("unrelated"), "unstaged").unwrap();
    let receipt = repo.declare_evidence(&input, &RequestId::new()).unwrap();
    let entry: EvidenceRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    repo.stage_operation(&receipt).unwrap();
    repo.stage_operation(&receipt).unwrap();
    assert_eq!(git(&["show", ":unrelated"]), b"staged");
    let path = format!(".workdeck/{}", entry.path.display());
    assert_eq!(
        git(&["show", &format!(":{path}")]),
        entry.document.as_bytes()
    );
    let index = std::fs::read(temp.path().join(".git/index")).unwrap();
    std::fs::write(
        repo.root().join(entry.path),
        format!("{}\n# later edit\n", entry.document),
    )
    .unwrap();
    assert_eq!(
        repo.stage_operation(&receipt).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        std::fs::read(temp.path().join(".git/index")).unwrap(),
        index
    );
}
#[test]
fn direct_supersession_forks_are_invalid_and_native_import_cannot_replace_evidence() {
    let (_d, repo, _gate, input) = setup();
    let first = record(&repo, &input);
    let original = repo.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    for file in &original.files {
        let path = root.join(&file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.content).unwrap();
    }
    let imported = Repository::open_source(&root).unwrap();
    let mut changed = first.reference.clone();
    changed.declaration.result.id = "edited-in-place".into();
    std::fs::write(
        root.join(&first.path),
        serde_yaml_ng::to_string(&changed).unwrap(),
    )
    .unwrap();
    let snapshot = imported.export_snapshot().unwrap();
    let plan = repo
        .preview_snapshot_import(&snapshot, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(
        !plan.allowed,
        "immutable evidence requires an explicit supersession record"
    );
    let mut amendment = input;
    amendment.supersedes = Some(EvidenceSupersession {
        id: first.reference.id,
        content: first.content,
        reason: "Correction".into(),
    });
    let second = record(&repo, &amendment);
    let mut fork = second.reference;
    fork.id = EvidenceId::new();
    let path = repo
        .root()
        .join("evidence")
        .join(format!("{}.yml", fork.id));
    std::fs::write(path, serde_yaml_ng::to_string(&fork).unwrap()).unwrap();
    assert!(!repo.doctor().unwrap().valid);
    assert!(repo.export_snapshot().is_err());
}
#[test]
fn malformed_typed_query_criteria_do_not_silently_return_empty_history() {
    let (_d, repo, _gate, input) = setup();
    record(&repo, &input);
    let mut criterion = input.criterion;
    criterion.owner = CriterionOwner::Milestone("../escape".into());
    assert!(
        repo.evidence_references(&EvidenceQuery {
            criterion: Some(criterion),
            ..Default::default()
        })
        .is_err()
    );
}
