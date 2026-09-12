use serde_json::json;
use std::collections::BTreeMap;
use workdeck_pm::*;
fn setup() -> (tempfile::TempDir, Repository, IssueRecord, CreateGate) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let mut create = CreateIssue::new("A checked declaration", "");
    create.fields.insert(
        "acceptance".into(),
        json!([{"id":"works","description":"The behavior works","checked":true}]),
    );
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&create, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(issue.metadata.id.clone()), "works")
        .unwrap()
        .reference;
    let input = CreateGate {
        name: "Release".into(),
        description: "".into(),
        requirements: vec![GateRequirement {
            id: "behavior".into(),
            criterion,
            producer: ProducerRef {
                id: "runner".into(),
                definition: ContentHash::of(b"runner v1"),
            },
            check: CheckRef {
                id: "check".into(),
                definition: ContentHash::of(b"check v1"),
            },
            max_age_seconds: Some(60),
        }],
        custom: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    (temp, repo, issue, input)
}
fn created(repo: &Repository, input: &CreateGate) -> GateRecord {
    serde_json::from_value::<GateMutationResult>(
        repo.create_gate(input, &RequestId::new()).unwrap().result,
    )
    .unwrap()
    .gate
}
#[test]
fn gate_and_evidence_are_first_class_doctor_and_snapshot_records() {
    let (_temp, repo, _issue, input) = setup();
    let gate = created(&repo, &input);
    let snapshot = repo
        .export_snapshot()
        .expect("gate is recognized snapshot authority");
    let destination = tempfile::tempdir().unwrap();
    let root = destination.path().join(".workdeck");
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new()).unwrap();
    assert_eq!(
        Repository::open_source(&root)
            .unwrap()
            .gate(&gate.definition.id)
            .unwrap(),
        gate
    );
    std::fs::write(
        repo.root().join("gates/bad.yml"),
        "schema: 1\nname: malformed\n",
    )
    .unwrap();
    assert!(
        !repo.doctor().unwrap().valid,
        "malformed gate cannot be invisible to doctor"
    );
}
#[test]
fn checked_criteria_are_unknown_and_unchecked_criteria_are_unsatisfied() {
    let (_temp, repo, issue, input) = setup();
    let gate = created(&repo, &input);
    let request = GateAssessmentRequest {
        gate: gate.definition.id.clone(),
        subject: ExactSubject {
            repository: repo.identity().clone(),
            kind: ExactSubjectKind::Source,
            content: ContentHash::of(b"tree"),
        },
        as_of: chrono::Utc::now(),
    };
    let result = repo.assess_gate(&request).unwrap();
    assert_eq!(result.state, ConditionState::Unknown);
    assert_eq!(result.subject_origin, "caller_declared");
    assert!(
        result.requirements[0]
            .conditions
            .iter()
            .any(|c| c.reason_code == "evidence_missing")
    );
    let owner = CriterionOwner::Issue(issue.metadata.id.clone());
    let old = repo.resolve_criterion(&owner, "works").unwrap();
    repo.mutate_issue(
        issue.metadata.id.as_str(),
        Some(&issue.source),
        &IssueMutation::Update {
            input: UpdateIssue {
                fields: BTreeMap::from([(
                    "acceptance".into(),
                    json!([{"id":"works","description":"The behavior works","checked":false}]),
                )]),
                body: None,
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.resolve_criterion(&owner, "works").unwrap().reference,
        old.reference,
        "checking does not redefine criterion identity"
    );
    assert_eq!(
        repo.assess_gate(&request).unwrap().state,
        ConditionState::Unsatisfied
    );
}
#[test]
fn gate_mutation_preserves_bytes_and_replays_original_result() {
    let (_temp, repo, _issue, input) = setup();
    let gate = created(&repo, &input);
    let path = repo.root().join(&gate.path);
    let text = std::fs::read_to_string(&path).unwrap() + "x-tool: keep # preserve comment\n";
    std::fs::write(&path, text).unwrap();
    let original = repo.gate(&gate.definition.id).unwrap();
    let mutation = GateMutation::Update {
        fields: BTreeMap::from([("name".into(), json!("Changed"))]),
    };
    let request = RequestId::new();
    assert_eq!(
        repo.mutate_gate(&gate.definition.id, &gate.source, &mutation, &request)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let receipt = repo
        .mutate_gate(&gate.definition.id, &original.source, &mutation, &request)
        .unwrap();
    let updated = repo.gate(&gate.definition.id).unwrap();
    assert!(updated.document.contains("# preserve comment"));
    repo.mutate_gate(
        &updated.definition.id,
        &updated.source,
        &GateMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.mutate_gate(&gate.definition.id, &original.source, &mutation, &request)
            .unwrap(),
        receipt
    );
}
#[test]
fn empty_or_managed_gate_inputs_and_stale_pins_cannot_publish() {
    let (_temp, repo, issue, input) = setup();
    let before = repo.export_snapshot().unwrap();
    let mut bad = input.clone();
    bad.requirements.clear();
    assert!(repo.create_gate(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.requirements.push(bad.requirements[0].clone());
    assert!(repo.create_gate(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.requirements[0].criterion.definition = ContentHash::of(b"stale");
    assert!(repo.create_gate(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.extra.insert("verified".into(), json!(true));
    assert!(repo.create_gate(&bad, &RequestId::new()).is_err());
    assert_eq!(repo.export_snapshot().unwrap().files, before.files);
    let gate = created(&repo, &input);
    let old = gate.clone();
    repo.mutate_issue(
        issue.metadata.id.as_str(),
        Some(&issue.source),
        &IssueMutation::Update {
            input: UpdateIssue {
                fields: BTreeMap::from([(
                    "acceptance".into(),
                    json!([{"id":"works","description":"Changed definition","checked":true}]),
                )]),
                body: None,
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    let report = repo
        .assess_gate(&GateAssessmentRequest {
            gate: gate.definition.id.clone(),
            subject: ExactSubject {
                repository: repo.identity().clone(),
                kind: ExactSubjectKind::Source,
                content: ContentHash::of(b"tree"),
            },
            as_of: chrono::Utc::now(),
        })
        .unwrap();
    assert!(
        report.requirements[0]
            .conditions
            .iter()
            .any(|c| c.reason_code == "criterion_definition_changed")
    );
    repo.mutate_gate(
        &old.definition.id,
        &old.source,
        &GateMutation::Update {
            fields: BTreeMap::from([("name".into(), json!("Rename retains stale historical pin"))]),
        },
        &RequestId::new(),
    )
    .unwrap();
}
#[test]
fn gate_retirement_keeps_archive_distinction_and_blocks_live_incoming_refs() {
    let (_temp, repo, issue, input) = setup();
    let gate = created(&repo, &input);
    let target = RetirementTarget::new(RetirementKind::Issue, issue.metadata.id.as_str()).unwrap();
    let preview = repo.retirement_preview(&target).unwrap();
    assert!(!preview.allowed);
    assert!(
        preview
            .record_blockers
            .iter()
            .any(|b| b.kind == RetirementKind::Gate)
    );
    repo.mutate_gate(
        &gate.definition.id,
        &gate.source,
        &GateMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        !repo.retirement_preview(&target).unwrap().allowed,
        "archiving preserves authored references"
    );
    let archived = repo.gate(&gate.definition.id).unwrap();
    let target = RetirementTarget::new(RetirementKind::Gate, gate.definition.id.as_str()).unwrap();
    repo.retire_record(&RetirementInput::new(target), &RequestId::new())
        .unwrap();
    let retired = repo.gate(&gate.definition.id).unwrap();
    assert!(retired.retirement.is_some());
    assert_eq!(
        retired.source.revision,
        archived.source.revision.next().unwrap()
    );
    assert_eq!(
        repo.mutate_gate(
            &retired.definition.id,
            &retired.source,
            &GateMutation::Archive { archived: false },
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    assert!(repo.doctor().unwrap().valid);
    repo.export_snapshot().unwrap();
}
#[test]
fn schema_activation_scans_gate_and_evidence_domains_without_recursive_policy_reads() {
    use std::collections::BTreeSet;
    let (_temp, repo, _issue, input) = setup();
    let gate = created(&repo, &input);
    let field = CustomFieldDefinition {
        field_type: CustomFieldType::Text,
        scopes: BTreeSet::from([CustomScope::Gate, CustomScope::Evidence]),
        required: true,
        archived: false,
        options: vec![],
        custom: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let schema = SchemaChange {
        fields: BTreeMap::from([("risk".into(), field)]),
        ..Default::default()
    };
    let blocked = repo.preview_schema_change(&schema).unwrap();
    assert!(!blocked.allowed);
    repo.mutate_gate(
        &gate.definition.id,
        &gate.source,
        &GateMutation::PatchCustom {
            patch: CustomPatch {
                set: BTreeMap::from([("risk".into(), json!("high"))]),
                unset: vec![],
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    let plan = repo.preview_schema_change(&schema).unwrap();
    assert!(plan.allowed, "{:?}", plan);
    let path = repo.root().join(&gate.path);
    let changed = std::fs::read_to_string(&path).unwrap() + "x-race: visible\n";
    std::fs::write(&path, changed).unwrap();
    assert_eq!(
        repo.apply_schema_change(&schema, Some(&plan.fingerprint), &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    repo.apply_schema_change(&schema, None, &RequestId::new())
        .unwrap();
    assert!(repo.organization_compliance().unwrap().compliant);
    let mut invalid = input;
    invalid.name = "Missing risk".into();
    assert_eq!(
        repo.create_gate(&invalid, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
}
#[test]
fn gate_creation_source_race_and_fault_leave_no_false_success() {
    use workdeck_pm::transactions::{FaultPoint, TransactionStore};
    let (_temp, repo, issue, input) = setup();
    let path = repo.root().join(&issue.path);
    let text = std::fs::read_to_string(&path).unwrap();
    let request = RequestId::new();
    let result = repo.write_gate_with_faults(
        &GateRequest::Create {
            input: input.clone(),
        },
        &request,
        |point| {
            if point == FaultPoint::BeforeJournal {
                std::fs::write(&path, format!("{text}\nDirect source edit\n")).unwrap();
            }
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
    assert!(repo.gates().unwrap().is_empty());
    let request = RequestId::new();
    let intent = GateRequest::Create { input };
    assert!(
        repo.write_gate_with_faults(&intent, &request, |point| {
            if point == FaultPoint::AfterJournal {
                Err(PmError::new(ErrorCode::Io, "interrupted"))
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert_eq!(repo.gates().unwrap_err().code, ErrorCode::RecoveryRequired);
    TransactionStore::open(repo.root())
        .unwrap()
        .recover()
        .unwrap();
    let receipt = repo
        .write_gate_with_faults(&intent, &request, |_| Ok(()))
        .unwrap();
    assert_eq!(
        repo.write_gate_with_faults(&intent, &request, |_| Ok(()))
            .unwrap(),
        receipt
    );
    assert_eq!(repo.gates().unwrap().len(), 1);
}
#[test]
fn native_import_cannot_bypass_gate_revision_and_managed_fields() {
    let (_temp, repo, _issue, input) = setup();
    let gate = created(&repo, &input);
    let original = repo.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    for file in &original.files {
        let path = root.join(&file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.content).unwrap();
    }
    let imported = Repository::open_source(&root).unwrap();
    let mut changed = gate.definition.clone();
    changed.name = "Bypass revision".into();
    std::fs::write(
        root.join(&gate.path),
        serde_yaml_ng::to_string(&changed).unwrap(),
    )
    .unwrap();
    let snapshot = imported.export_snapshot().unwrap();
    let plan = repo
        .preview_snapshot_import(&snapshot, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(
        !plan.allowed,
        "a native gate replacement must advance revision"
    );
}
#[test]
fn gate_receipt_proof_rejects_self_consistent_unauthorized_output_and_noop_replays() {
    let (_temp, repo, _issue, input) = setup();
    let receipt = repo.create_gate(&input, &RequestId::new()).unwrap();
    let mut result: GateMutationResult = serde_json::from_value(receipt.result.clone()).unwrap();
    let gate = result.gate.clone();
    let noop = repo
        .mutate_gate(
            &gate.definition.id,
            &gate.source,
            &GateMutation::Update {
                fields: BTreeMap::from([("name".into(), json!(input.name))]),
            },
            &RequestId::new(),
        )
        .unwrap();
    assert!(noop.changed.is_empty());
    assert_eq!(
        serde_json::from_value::<GateMutationResult>(noop.result)
            .unwrap()
            .gate,
        gate
    );
    result.gate.definition.name = "Forged but internally consistent".into();
    result.gate.document = serde_yaml_ng::to_string(&result.gate.definition).unwrap();
    result.gate.source.content = ContentHash::of(result.gate.document.as_bytes());
    let mut forged = receipt.clone();
    forged.result = json!(result);
    forged.changed[0].after = Some(result.gate.source.content);
    let path = repo
        .root()
        .join("operations")
        .join(format!("{}.yml", receipt.operation_id));
    std::fs::write(&path, serde_yaml_ng::to_string(&forged).unwrap()).unwrap();
    assert!(repo.create_gate(&input, &receipt.request_id).is_err());
    assert!(repo.export_snapshot().is_err());
    std::fs::write(path, serde_yaml_ng::to_string(&receipt).unwrap()).unwrap();
    assert!(repo.export_snapshot().is_ok());
}
#[test]
fn criterion_owners_are_typed_and_feature_milestone_declarations_stay_unknown() {
    let (_temp, repo, _issue, mut input) = setup();
    let mut feature = CreateFeature::new("Capability");
    feature.fields.insert(
        "criteria".into(),
        json!([{"id":"works","description":"Works"}]),
    );
    let feature: FeatureOutcome = serde_json::from_value(
        repo.create_feature(&feature, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    repo.create_planning(
        PlanningKind::Project,
        &CreatePlanning {
            id: Some("capability".into()),
            name: "Capability".into(),
            body: String::new(),
            fields: BTreeMap::from([(
                "exit_criteria".into(),
                json!([{"id":"works","description":"Works"}]),
            )]),
        },
        &RequestId::new(),
    )
    .unwrap();
    repo.create_planning(
        PlanningKind::Milestone,
        &CreatePlanning {
            id: Some("Release_One".into()),
            name: "Release".into(),
            body: "".into(),
            fields: BTreeMap::from([
                ("project".into(), json!("capability")),
                (
                    "outcomes".into(),
                    json!([{"id":"works","description":"Works"}]),
                ),
            ]),
        },
        &RequestId::new(),
    )
    .unwrap();
    let owners = [
        CriterionOwner::Feature(feature.record.metadata.id),
        CriterionOwner::Milestone("Release_One".into()),
        CriterionOwner::Project("capability".into()),
    ];
    let mut definitions = std::collections::BTreeSet::new();
    for owner in owners {
        let resolved = repo.resolve_criterion(&owner, "works").unwrap();
        assert_eq!(resolved.declaration, CriterionDeclaration::Declared);
        definitions.insert(resolved.reference.definition.clone());
        input.requirements[0].criterion = resolved.reference;
        let gate = created(&repo, &input);
        let assessment = repo
            .assess_gate(&GateAssessmentRequest {
                gate: gate.definition.id,
                subject: ExactSubject {
                    repository: repo.identity().clone(),
                    kind: ExactSubjectKind::Artifact,
                    content: ContentHash::of(b"build"),
                },
                as_of: chrono::Utc::now(),
            })
            .unwrap();
        assert_eq!(assessment.state, ConditionState::Unknown);
    }
    assert_eq!(
        definitions.len(),
        3,
        "same criterion text on distinct typed owners has distinct semantic identity"
    );
    assert!(
        serde_json::from_value::<CriterionOwner>(
            json!({"kind":"criterion","id":{"owner":"nested"}})
        )
        .is_err()
    );
}
