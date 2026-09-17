use serde_json::json;
use std::collections::BTreeMap;
use workdeck_pm::*;

#[test]
fn feature_declarations_preserve_identity_through_rename_reparent_and_physical_move() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let parent = repo
        .create_feature(&CreateFeature::new("Parent capability"), &RequestId::new())
        .unwrap();
    let parent: FeatureRecord = serde_json::from_value(parent.result["record"].clone()).unwrap();
    let input = CreateFeature {
        name: "Native capability".into(),
        body: "# Contract\r\nExact body\r\n".into(),
        fields: BTreeMap::from([("custom".into(), json!({"kept":[1,null,"three"]}))]),
        directory: None,
    };
    let request = RequestId::new();
    let created = repo.create_feature(&input, &request).unwrap();
    let first: FeatureRecord = serde_json::from_value(created.result["record"].clone()).unwrap();
    let id = first.metadata.id.clone();
    let rename = repo
        .mutate_feature(
            id.as_str(),
            Some(&first.source),
            &FeatureMutation::Update {
                fields: BTreeMap::from([("name".into(), json!("Renamed capability"))]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    let renamed: FeatureRecord = serde_json::from_value(rename.result["record"].clone()).unwrap();
    repo.mutate_feature(
        id.as_str(),
        Some(&renamed.source),
        &FeatureMutation::Reparent {
            parent: Some(parent.metadata.id.clone()),
        },
        &RequestId::new(),
    )
    .unwrap();
    let before = repo.feature(id.as_str()).unwrap();
    repo.mutate_feature(
        id.as_str(),
        Some(&before.source),
        &FeatureMutation::Relocate {
            directory: "delivery/native".into(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let moved = repo.feature(id.as_str()).unwrap();
    assert_eq!(moved.metadata.id, id);
    assert_eq!(moved.metadata.parent, Some(parent.metadata.id));
    assert_eq!(moved.metadata.name, "Renamed capability");
    assert_eq!(moved.body, input.body);
    assert_eq!(moved.metadata.custom["kept"], json!([1, null, "three"]));
    assert_eq!(
        moved.path,
        std::path::PathBuf::from(format!("features/delivery/native/{id}.md"))
    );
    assert!(!repo.root().join(&first.path).exists());
    assert_eq!(repo.create_feature(&input, &request).unwrap(), created);
}

#[test]
fn new_feature_associations_cannot_point_to_missing_planning_records() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    for field in ["projects", "milestones", "targets"] {
        let mut input = CreateFeature::new("Unresolved capability");
        input.fields.insert(field.into(), json!(["missing"]));
        assert!(
            repo.create_feature(&input, &RequestId::new()).is_err(),
            "new {field} must resolve"
        );
    }
    assert!(repo.list_features().unwrap().is_empty());
    assert!(!repo.root().join("features").exists());
}

#[test]
fn feature_parent_and_prerequisite_cycles_are_rejected_without_changing_sources() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let left: FeatureOutcome = serde_json::from_value(
        repo.create_feature(&CreateFeature::new("Left"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let right: FeatureOutcome = serde_json::from_value(
        repo.create_feature(&CreateFeature::new("Right"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let a = left.record.metadata.id;
    let b = right.record.metadata.id;
    repo.mutate_feature(
        a.as_str(),
        None,
        &FeatureMutation::Reparent {
            parent: Some(b.clone()),
        },
        &RequestId::new(),
    )
    .unwrap();
    let before = repo.feature(b.as_str()).unwrap();
    assert!(
        repo.mutate_feature(
            b.as_str(),
            Some(&before.source),
            &FeatureMutation::Reparent {
                parent: Some(a.clone())
            },
            &RequestId::new()
        )
        .is_err()
    );
    assert_eq!(repo.feature(b.as_str()).unwrap(), before);
    repo.mutate_feature(
        a.as_str(),
        None,
        &FeatureMutation::Update {
            fields: BTreeMap::from([("prerequisites".into(), json!([b]))]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        repo.mutate_feature(
            before.metadata.id.as_str(),
            None,
            &FeatureMutation::Update {
                fields: BTreeMap::from([("prerequisites".into(), json!([a]))]),
                body: None
            },
            &RequestId::new()
        )
        .is_err()
    );
}

fn feature(repo: &Repository, name: &str) -> FeatureRecord {
    serde_json::from_value::<FeatureOutcome>(
        repo.create_feature(&CreateFeature::new(name), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
    .record
}
fn issue(repo: &Repository, title: &str, fields: serde_json::Value) -> IssueRecord {
    serde_json::from_value(
        repo.create_issue(
            &CreateIssue {
                title: title.into(),
                body: "Preserved issue body".into(),
                fields: serde_json::from_value(fields).unwrap(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}
#[test]
fn coverage_retains_outside_prerequisites_and_done_does_not_promote_feature_declarations() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let first = feature(&repo, "First");
    let second = feature(&repo, "Second");
    let outside = issue(&repo, "Outside prerequisite", json!({}));
    let member = issue(
        &repo,
        "Capability work",
        json!({"features":[first.metadata.id,second.metadata.id],"prerequisites":[outside.metadata.id]}),
    );
    let mut query = FeatureCoverageQuery::new(first.metadata.id.as_str());
    query.issues.query = "matches nothing".into();
    let coverage = repo.feature_coverage(&query).unwrap();
    assert!(coverage.issues.is_empty());
    assert_eq!(coverage.outside_prerequisites, vec![outside.clone()]);
    assert!(
        !repo
            .completion_report(member.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    repo.complete_issue(
        outside.metadata.id.as_str(),
        &outside.source,
        None,
        &RequestId::new(),
    )
    .unwrap();
    repo.complete_issue(
        member.metadata.id.as_str(),
        &member.source,
        None,
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(repo.feature(first.metadata.id.as_str()).unwrap(), first);
    assert_eq!(
        repo.feature_coverage(&FeatureCoverageQuery::new(second.metadata.id.as_str()))
            .unwrap()
            .issues
            .len(),
        1
    );
    assert!(repo.doctor().unwrap().valid);
}
#[test]
fn soft_feature_relations_use_independent_canonical_source_and_exact_endpoint_preconditions() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let a = feature(&repo, "A");
    let b = feature(&repo, "B");
    let request = RequestId::new();
    let receipt = repo
        .set_feature_related(
            a.metadata.id.as_str(),
            b.metadata.id.as_str(),
            Some(&a.source),
            Some(&b.source),
            true,
            &request,
        )
        .unwrap();
    assert_eq!(repo.feature(a.metadata.id.as_str()).unwrap(), a);
    assert_eq!(repo.feature(b.metadata.id.as_str()).unwrap(), b);
    assert_eq!(
        repo.feature_coverage(&FeatureCoverageQuery::new(b.metadata.id.as_str()))
            .unwrap()
            .related,
        vec![a.clone()]
    );
    repo.mutate_feature(
        a.metadata.id.as_str(),
        Some(&a.source),
        &FeatureMutation::Update {
            fields: BTreeMap::from([("name".into(), json!("Renamed"))]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.set_feature_related(
            a.metadata.id.as_str(),
            b.metadata.id.as_str(),
            Some(&a.source),
            Some(&b.source),
            true,
            &request
        )
        .unwrap(),
        receipt
    );
    assert_eq!(
        repo.set_feature_related(
            a.metadata.id.as_str(),
            b.metadata.id.as_str(),
            Some(&a.source),
            Some(&b.source),
            false,
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    repo.set_feature_related(
        a.metadata.id.as_str(),
        b.metadata.id.as_str(),
        None,
        None,
        false,
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        repo.feature_coverage(&FeatureCoverageQuery::new(a.metadata.id.as_str()))
            .unwrap()
            .related
            .is_empty()
    );
}
#[test]
fn feature_references_block_planning_resolution_and_retirement_preserves_only_own_source() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let mut project = CreatePlanning::new("Project");
    project.id = Some("project".into());
    repo.create_planning(PlanningKind::Project, &project, &RequestId::new())
        .unwrap();
    let mut input = CreateFeature::new("Linked");
    input.fields.insert("projects".into(), json!(["project"]));
    let a: FeatureOutcome = serde_json::from_value(
        repo.create_feature(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let b = feature(&repo, "Sibling");
    let target = RetirementTarget::new(RetirementKind::Project, "project").unwrap();
    assert!(!repo.retirement_preview(&target).unwrap().allowed);
    let resolution = repo.reference_retirement_preview(&target).unwrap();
    assert!(
        !resolution.allowed,
        "force must not orphan feature planning associations"
    );
    let target =
        RetirementTarget::new(RetirementKind::Feature, a.record.metadata.id.as_str()).unwrap();
    repo.mutate_feature(
        a.record.metadata.id.as_str(),
        None,
        &FeatureMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    let receipt = repo
        .retire_record(&RetirementInput::new(target.clone()), &RequestId::new())
        .unwrap();
    let retired = repo.feature(a.record.metadata.id.as_str()).unwrap();
    assert!(retired.retirement.as_ref().unwrap().history.is_empty());
    assert_eq!(repo.feature(b.metadata.id.as_str()).unwrap(), b);
    assert_eq!(receipt.changed.len(), 2);
    assert_eq!(
        repo.mutate_feature(
            a.record.metadata.id.as_str(),
            None,
            &FeatureMutation::Archive { archived: false },
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    assert!(repo.doctor().unwrap().valid);
}
#[test]
fn feature_snapshot_restore_retains_physical_paths_original_receipts_and_tamper_rejection() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let input = CreateFeature::new("Historical capability");
    let request = RequestId::new();
    let receipt = repo.create_feature(&input, &request).unwrap();
    let record: FeatureOutcome = serde_json::from_value(receipt.result.clone()).unwrap();
    repo.mutate_feature(
        record.record.metadata.id.as_str(),
        None,
        &FeatureMutation::Relocate {
            directory: "nested/capabilities".into(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let snapshot = repo.export_snapshot().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let root = destination.path().join(".workdeck");
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new()).unwrap();
    let restored = Repository::discover(destination.path()).unwrap();
    assert_eq!(restored.create_feature(&input, &request).unwrap(), receipt);
    assert_eq!(
        restored
            .feature(record.record.metadata.id.as_str())
            .unwrap(),
        repo.feature(record.record.metadata.id.as_str()).unwrap()
    );
    let mut forged = receipt.clone();
    forged.result["record"]["metadata"]["name"] = json!("Forged label");
    let path = repo
        .root()
        .join(format!("operations/{}.yml", receipt.operation_id));
    std::fs::write(path, serde_yaml_ng::to_string(&forged).unwrap()).unwrap();
    assert!(repo.create_feature(&input, &request).is_err());
    assert!(repo.operation_history().is_err());
    assert!(repo.export_snapshot().is_err());
}

#[test]
fn relocation_recovers_every_partial_boundary_and_keeps_original_replay_after_later_edits() {
    use workdeck_pm::transactions::{FaultPoint, TransactionStore};
    for interrupt in [
        FaultPoint::AfterJournal,
        FaultPoint::AfterChange(0),
        FaultPoint::BeforeReceipt,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path(), "WD").unwrap();
        let before = feature(&repo, "Crash-safe feature");
        let mutation = FeatureMutation::Relocate {
            directory: "group/nested".into(),
        };
        let request = RequestId::new();
        let result = repo.mutate_feature_with_faults(
            before.metadata.id.as_str(),
            Some(&before.source),
            &mutation,
            &request,
            |point| {
                if point == interrupt {
                    Err(PmError::new(ErrorCode::Io, "interrupted"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(
            repo.list_features().unwrap_err().code,
            ErrorCode::RecoveryRequired
        );
        TransactionStore::open(repo.root())
            .unwrap()
            .recover()
            .unwrap();
        let receipt = repo
            .mutate_feature(
                before.metadata.id.as_str(),
                Some(&before.source),
                &mutation,
                &request,
            )
            .unwrap();
        let current = repo.feature(before.metadata.id.as_str()).unwrap();
        assert!(!repo.root().join(&before.path).exists());
        assert_eq!(current.metadata.id, before.metadata.id);
        assert_eq!(repo.list_features().unwrap().len(), 1);
        repo.mutate_feature(
            before.metadata.id.as_str(),
            Some(&current.source),
            &FeatureMutation::Archive { archived: true },
            &RequestId::new(),
        )
        .unwrap();
        assert_eq!(
            repo.mutate_feature(
                before.metadata.id.as_str(),
                Some(&before.source),
                &mutation,
                &request
            )
            .unwrap(),
            receipt
        );
    }
}
#[test]
fn concurrent_feature_writers_and_direct_editor_membership_races_are_source_checked() {
    use std::sync::{Arc, Barrier};
    use workdeck_pm::transactions::FaultPoint;
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let before = feature(&repo, "Concurrent");
    let barrier = Arc::new(Barrier::new(2));
    let workers = (0..2)
        .map(|n| {
            let root = repo.root().to_owned();
            let before = before.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let repo = Repository::open_source(&root).unwrap();
                barrier.wait();
                repo.mutate_feature(
                    before.metadata.id.as_str(),
                    Some(&before.source),
                    &FeatureMutation::Update {
                        fields: BTreeMap::from([("name".into(), json!(format!("Writer {n}")))]),
                        body: None,
                    },
                    &RequestId::new(),
                )
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|w| w.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| e.code == ErrorCode::StaleSource)
    );
    let current = repo.feature(before.metadata.id.as_str()).unwrap();
    let path = repo.root().join(&current.path);
    let edited = format!("{}\nExternal authored body\n", current.document);
    let result = repo.mutate_feature_with_faults(
        current.metadata.id.as_str(),
        Some(&current.source),
        &FeatureMutation::Relocate {
            directory: "elsewhere".into(),
        },
        &RequestId::new(),
        |point| {
            if point == FaultPoint::BeforeJournal {
                std::fs::write(&path, &edited).unwrap();
            }
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
    assert_eq!(std::fs::read_to_string(path).unwrap(), edited);
    assert_eq!(repo.list_features().unwrap().len(), 1);
}
#[test]
fn native_import_cannot_author_new_dangling_features_or_bypass_managed_revisions() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let record = feature(&repo, "Original feature");
    let original = repo.export_snapshot().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = source.path().join(".workdeck");
    for file in &original.files {
        let path = root.join(&file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.content).unwrap();
    }
    let source = Repository::open_source(&root).unwrap();
    std::fs::write(
        root.join(&record.path),
        record
            .document
            .replace("Original feature", "Revision bypass"),
    )
    .unwrap();
    let snapshot = source.export_snapshot().unwrap();
    let plan = repo
        .preview_snapshot_import(&snapshot, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(
        !plan.allowed,
        "replacement cannot bypass revision transition"
    );
}

#[test]
fn feature_edits_preserve_unknown_comments_flow_metadata_and_reject_qualification_claims() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let original = feature(&repo, "Authoring");
    let text = original
        .document
        .replacen(
            "\n---\n",
            "\nx-tool: {nested: [one, null, three]} # keep this\n---\n",
            1,
        )
        .replace('\n', "\r\n");
    std::fs::write(repo.root().join(&original.path), &text).unwrap();
    let before = repo.feature(original.metadata.id.as_str()).unwrap();
    repo.mutate_feature(
        before.metadata.id.as_str(),
        Some(&before.source),
        &FeatureMutation::Update {
            fields: BTreeMap::from([
                ("decision".into(), json!("accepted")),
                ("maturity".into(), json!("implemented")),
                ("availability".into(), json!("experimental")),
            ]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let current = repo.feature(before.metadata.id.as_str()).unwrap();
    assert!(
        current
            .document
            .contains("x-tool: {nested: [one, null, three]} # keep this\r\n")
    );
    assert!(!current.document.replace("\r\n", "").contains('\n'));
    assert_eq!(current.metadata.extra, before.metadata.extra);
    for fields in [
        json!({"maturity":"verified"}),
        json!({"qualification":"verified"}),
        json!({"id":FeatureId::new()}),
        json!({"gates":[GateId::new()]}),
    ] {
        assert!(
            repo.mutate_feature(
                current.metadata.id.as_str(),
                Some(&current.source),
                &FeatureMutation::Update {
                    fields: serde_json::from_value(fields).unwrap(),
                    body: None
                },
                &RequestId::new()
            )
            .is_err()
        );
    }
    assert_eq!(repo.feature(current.metadata.id.as_str()).unwrap(), current);
}
#[test]
fn duplicate_feature_identity_unsafe_source_and_invalid_relocation_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let record = feature(&repo, "One identity");
    for directory in ["../outside", "/absolute", "bad//path", "bad/./path"] {
        assert!(
            repo.mutate_feature(
                record.metadata.id.as_str(),
                None,
                &FeatureMutation::Relocate {
                    directory: directory.into()
                },
                &RequestId::new()
            )
            .is_err(),
            "{directory}"
        );
    }
    let duplicate = repo
        .root()
        .join("features/duplicate")
        .join(format!("{}.md", record.metadata.id));
    std::fs::create_dir_all(duplicate.parent().unwrap()).unwrap();
    std::fs::write(&duplicate, &record.document).unwrap();
    assert!(repo.list_features().is_err());
    assert!(!repo.doctor().unwrap().valid);
    std::fs::remove_file(&duplicate).unwrap();
    #[cfg(unix)]
    {
        let outside = temp.path().join("external.md");
        std::fs::write(&outside, &record.document).unwrap();
        std::os::unix::fs::symlink(outside, &duplicate).unwrap();
        assert!(repo.list_features().is_err());
    }
}
#[test]
fn native_import_cannot_introduce_dangling_soft_feature_relations() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let record = feature(&repo, "Known feature");
    let original = repo.export_snapshot().unwrap();
    let source = tempfile::tempdir().unwrap();
    let root = source.path().join(".workdeck");
    for file in &original.files {
        let path = root.join(&file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.content).unwrap();
    }
    let mut ids = [record.metadata.id, FeatureId::new()];
    ids.sort();
    let link = RelatedFeatureLink {
        schema: SchemaVersion::CURRENT,
        repository: repo.identity().clone(),
        features: ids,
    };
    let path = root.join(format!(
        "relations/features/{}/{}.yml",
        link.features[0], link.features[1]
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_yaml_ng::to_string(&link).unwrap()).unwrap();
    let source = Repository::open_source(&root).unwrap();
    let snapshot = source.export_snapshot().unwrap();
    let plan = repo
        .preview_snapshot_import(&snapshot, SnapshotImportMode::Merge)
        .unwrap();
    assert!(
        !plan.allowed,
        "ordinary import cannot author dangling soft relations"
    );
}

#[test]
fn feature_criterion_identity_survives_rename_and_move_while_definition_edits_stale_gate_pins() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let mut input = CreateFeature::new("Stable criterion");
    input.fields.insert(
        "criteria".into(),
        json!([{"id":"native","description":"Native behavior works"}]),
    );
    let record: FeatureOutcome = serde_json::from_value(
        repo.create_feature(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let id = record.record.metadata.id;
    let owner = CriterionOwner::Feature(id.clone());
    let original = repo.resolve_criterion(&owner, "native").unwrap();
    assert_eq!(original.declaration, CriterionDeclaration::Declared);
    let gate: GateMutationResult = serde_json::from_value(
        repo.create_gate(
            &CreateGate {
                name: "Native gate".into(),
                description: "".into(),
                requirements: vec![GateRequirement {
                    id: "native".into(),
                    criterion: original.reference.clone(),
                    producer: ProducerRef {
                        id: "runner".into(),
                        definition: ContentHash::of(b"runner"),
                    },
                    check: CheckRef {
                        id: "test".into(),
                        definition: ContentHash::of(b"test"),
                    },
                    max_age_seconds: None,
                }],
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.mutate_feature(
        id.as_str(),
        None,
        &FeatureMutation::Relocate {
            directory: "product".into(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let moved = repo.resolve_criterion(&owner, "native").unwrap();
    assert_eq!(moved.reference, original.reference);
    assert_ne!(moved.source.path, original.source.path);
    let request = GateAssessmentRequest {
        gate: gate.gate.definition.id,
        subject: ExactSubject {
            repository: repo.identity().clone(),
            kind: ExactSubjectKind::Source,
            content: ContentHash::of(b"source"),
        },
        as_of: chrono::Utc::now(),
    };
    assert_eq!(
        repo.assess_gate(&request).unwrap().state,
        ConditionState::Unknown
    );
    repo.mutate_feature(
        id.as_str(),
        None,
        &FeatureMutation::Update {
            fields: BTreeMap::from([(
                "criteria".into(),
                json!([{"id":"native","description":"Changed required behavior"}]),
            )]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let changed = repo.resolve_criterion(&owner, "native").unwrap();
    assert_ne!(changed.reference.definition, original.reference.definition);
    assert_eq!(changed.reference.id, original.reference.id);
    let assessment = repo.assess_gate(&request).unwrap();
    assert_ne!(assessment.state, ConditionState::Satisfied);
    assert!(
        assessment.requirements[0]
            .conditions
            .iter()
            .any(|condition| condition.reason_code.contains("criterion"))
    );
    assert!(
        !repo
            .retirement_preview(
                &RetirementTarget::new(RetirementKind::Feature, id.as_str()).unwrap()
            )
            .unwrap()
            .allowed
    );
}
#[test]
fn feature_custom_schema_preview_and_semantic_patch_use_real_feature_documents() {
    use std::collections::BTreeSet;
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let record = feature(&repo, "Policy-aware");
    let change = SchemaChange {
        fields: BTreeMap::from([(
            "risk".into(),
            CustomFieldDefinition {
                field_type: CustomFieldType::Enum,
                scopes: BTreeSet::from([CustomScope::Feature]),
                required: true,
                archived: false,
                options: vec!["low".into(), "high".into()],
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
        )]),
        ..Default::default()
    };
    let blocked = repo.preview_schema_change(&change).unwrap();
    assert!(!blocked.allowed);
    repo.mutate_feature(
        record.metadata.id.as_str(),
        None,
        &FeatureMutation::PatchCustom {
            patch: CustomPatch {
                set: BTreeMap::from([("risk".into(), json!("low"))]),
                unset: Default::default(),
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    let allowed = repo.preview_schema_change(&change).unwrap();
    assert!(allowed.allowed, "{:?}", allowed);
    repo.apply_schema_change(&change, Some(&allowed.fingerprint), &RequestId::new())
        .unwrap();
    let current = repo.feature(record.metadata.id.as_str()).unwrap();
    assert!(
        repo.mutate_feature(
            current.metadata.id.as_str(),
            Some(&current.source),
            &FeatureMutation::PatchCustom {
                patch: CustomPatch {
                    set: BTreeMap::from([("risk".into(), json!("invalid"))]),
                    unset: Default::default()
                }
            },
            &RequestId::new()
        )
        .is_err()
    );
    assert_eq!(repo.feature(current.metadata.id.as_str()).unwrap(), current);
}

#[test]
fn directly_authored_feature_cycles_are_invalid_without_hiding_their_readable_sources() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let a = feature(&repo, "Cycle A");
    let b = feature(&repo, "Cycle B");
    for (record, parent) in [(&a, &b), (&b, &a)] {
        let mut document =
            workdeck_pm::documents::MarkdownDocument::parse(&record.path, &record.document)
                .unwrap();
        let patch = serde_yaml_ng::to_value(json!({"parent":parent.metadata.id})).unwrap();
        document.patch(patch.as_mapping().unwrap()).unwrap();
        std::fs::write(repo.root().join(&record.path), document.render()).unwrap();
    }
    assert_eq!(repo.list_features().unwrap().len(), 2);
    let report = repo.doctor().unwrap();
    assert!(
        !report.valid,
        "cycles are invalid graph authority, not a valid completion declaration"
    );
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("cycle"))
    );
    assert!(repo.export_snapshot().is_err());
}

#[test]
fn feature_coverage_context_keeps_same_feature_prerequisites_hidden_by_archive_or_status() {
    for filter in ["archive", "status"] {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path(), "WD").unwrap();
        let feature = feature(&repo, "Shared capability");
        let prerequisite = issue(
            &repo,
            "Required work",
            json!({"features":[feature.metadata.id], "status": if filter == "status" {"backlog"} else {"ready"}}),
        );
        let dependent = issue(
            &repo,
            "Visible work",
            json!({"features":[feature.metadata.id], "prerequisites":[prerequisite.metadata.id]}),
        );
        if filter == "archive" {
            repo.mutate_issue(
                prerequisite.metadata.id.as_str(),
                Some(&prerequisite.source),
                &IssueMutation::Archive { archived: true },
                &RequestId::new(),
            )
            .unwrap();
        }
        let prerequisite = repo.show_issue(prerequisite.metadata.id.as_str()).unwrap();
        let mut query = FeatureCoverageQuery::new(feature.metadata.id.as_str());
        query.issues.archive = ArchiveFilter::All;
        let complete = repo.feature_coverage(&query).unwrap();
        assert_eq!(complete.issues.len(), 2);
        assert!(complete.outside_prerequisites.is_empty());
        if filter == "archive" {
            query.issues.archive = ArchiveFilter::Active;
        } else {
            query.issues.status = Some("ready".into());
        }
        let filtered = repo.feature_coverage(&query).unwrap();
        assert_eq!(filtered.issues, vec![dependent]);
        assert_eq!(
            filtered.outside_prerequisites,
            vec![prerequisite],
            "{filter} filtering must retain prerequisites outside the displayed member list"
        );
        assert_eq!(filtered.feature, feature);
    }
}

#[test]
fn feature_coverage_context_reports_retained_missing_prerequisites_before_filtering() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let feature = feature(&repo, "Capability with retained graph history");
    let dependent = issue(
        &repo,
        "Visible work",
        json!({"features":[feature.metadata.id]}),
    );
    let missing = IssueId::new("WD").unwrap();
    let path = repo.root().join(&dependent.path);
    let raw = std::fs::read_to_string(&path).unwrap();
    let mut document = workdeck_pm::documents::MarkdownDocument::parse(&path, &raw).unwrap();
    let patch = serde_yaml_ng::to_value(json!({"prerequisites":[missing]})).unwrap();
    document.patch(patch.as_mapping().unwrap()).unwrap();
    let authored = document.render();
    std::fs::write(&path, &authored).unwrap();
    let mut query = FeatureCoverageQuery::new(feature.metadata.id.as_str());
    for hidden in [false, true] {
        if hidden {
            query.issues.query = "matches no work".into();
        }
        let result = repo.feature_coverage(&query).unwrap();
        assert_eq!(result.issues.len(), usize::from(!hidden));
        assert!(
            result.outside_prerequisites.is_empty(),
            "a missing prerequisite is not an invented issue record"
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.code == ErrorCode::NotFound
                    && warning.message.contains(missing.as_str())
                    && warning.path.as_deref() == dependent.path.to_str()),
            "coverage must preserve the unresolved prerequisite ID and owning source: {:?}",
            result.warnings
        );
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), authored);
}
