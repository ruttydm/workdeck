use serde_json::{Value, json};
use std::collections::BTreeMap;
use workdeck_pm::{
    CreateIssue, CreatePlanning, ErrorCode, IssueRecord, PlanningKind, PlanningRecord, Repository,
    RequestId,
};

fn fixture() -> (tempfile::TempDir, Repository) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    (temp, repo)
}

fn create(
    repo: &Repository,
    kind: PlanningKind,
    id: &str,
    fields: BTreeMap<String, Value>,
) -> PlanningRecord {
    serde_json::from_value(
        repo.create_planning(
            kind,
            &CreatePlanning {
                id: Some(id.into()),
                name: id.into(),
                body: String::new(),
                fields,
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}

#[test]
fn native_hierarchy_records_and_outgoing_target_associations_roundtrip() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Initiative, "outcome", BTreeMap::new());
    create(&repo, PlanningKind::Target, "release", BTreeMap::new());
    create(&repo, PlanningKind::Cycle, "cycle", BTreeMap::new());
    let project = create(
        &repo,
        PlanningKind::Project,
        "project",
        BTreeMap::from([
            ("initiative".into(), json!("outcome")),
            ("lead".into(), json!("owner")),
            ("scope".into(), json!("Bounded deliverable")),
            ("goal".into(), json!("A verifiable outcome")),
            ("starts_at".into(), json!("2026-09-01")),
            ("ends_at".into(), json!("2026-09-30")),
            ("targets".into(), json!(["release"])),
            (
                "exit_criteria".into(),
                json!([{"id":"ready","description":"Review accepted"}]),
            ),
        ]),
    );
    let milestone = create(
        &repo,
        PlanningKind::Milestone,
        "checkpoint",
        BTreeMap::from([
            ("project".into(), json!("project")),
            (
                "outcomes".into(),
                json!([{"id":"tested","description":"Behavior demonstrated"}]),
            ),
        ]),
    );
    let record: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue {
                title: "Deliver work".into(),
                body: "Authored description".into(),
                fields: BTreeMap::from([
                    ("project".into(), json!("project")),
                    ("milestone".into(), json!("checkpoint")),
                    ("cycle".into(), json!("cycle")),
                    ("targets".into(), json!(["release"])),
                ]),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    assert_eq!(project.metadata.initiative.as_deref(), Some("outcome"));
    assert_eq!(milestone.metadata.project.as_deref(), Some("project"));
    assert_eq!(record.metadata.targets, ["release"]);
    assert!(repo.doctor().unwrap().valid);
    assert!(repo.export_snapshot().unwrap().validate().is_ok());
}

#[test]
fn new_native_issue_associations_must_resolve_to_real_records() {
    for (field, value) in [
        ("project", json!("missing")),
        ("cycle", json!("missing")),
        ("labels", json!(["missing"])),
        ("milestone", json!("missing")),
    ] {
        let (_temp, repo) = fixture();
        let result = repo.create_issue(
            &CreateIssue {
                title: "No dangling new refs".into(),
                body: String::new(),
                fields: BTreeMap::from([(field.into(), value)]),
            },
            &RequestId::new(),
        );
        assert!(result.is_err(), "native create accepted unresolved {field}");
        assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
        assert!(repo.list_issues().unwrap().is_empty());
    }
}

fn issue(repo: &Repository, fields: BTreeMap<String, Value>) -> IssueRecord {
    serde_json::from_value(
        repo.create_issue(
            &CreateIssue {
                title: "Work".into(),
                body: "# Authored\nPreserve narrative.\n".into(),
                fields,
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}
fn update(fields: BTreeMap<String, Value>) -> workdeck_pm::IssueMutation {
    workdeck_pm::IssueMutation::Update {
        input: workdeck_pm::UpdateIssue { fields, body: None },
    }
}
fn planning_update(fields: BTreeMap<String, Value>) -> workdeck_pm::PlanningMutation {
    workdeck_pm::PlanningMutation::Update { fields, body: None }
}

#[test]
fn milestone_ownership_changes_require_explicit_consistent_reassociation() {
    let (_temp, repo) = fixture();
    for id in ["a", "b"] {
        create(&repo, PlanningKind::Project, id, BTreeMap::new());
    }
    let milestone = create(
        &repo,
        PlanningKind::Milestone,
        "m",
        BTreeMap::from([("project".into(), json!("a"))]),
    );
    let member = issue(
        &repo,
        BTreeMap::from([
            ("project".into(), json!("a")),
            ("milestone".into(), json!("m")),
        ]),
    );
    let bad = update(BTreeMap::from([("project".into(), json!("b"))]));
    assert_eq!(
        repo.mutate_issue(
            &member.metadata.id.to_string(),
            None,
            &bad,
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    let move_milestone = planning_update(BTreeMap::from([("project".into(), json!("b"))]));
    assert_eq!(
        repo.mutate_planning(
            PlanningKind::Milestone,
            "m",
            Some(&milestone.source),
            &move_milestone,
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    let cleared: IssueRecord = serde_json::from_value(
        repo.mutate_issue(
            &member.metadata.id.to_string(),
            None,
            &update(BTreeMap::from([("milestone".into(), Value::Null)])),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.mutate_planning(
        PlanningKind::Milestone,
        "m",
        Some(&milestone.source),
        &move_milestone,
        &RequestId::new(),
    )
    .unwrap();
    let moved: IssueRecord = serde_json::from_value(
        repo.mutate_issue(
            &member.metadata.id.to_string(),
            Some(&cleared.source),
            &update(BTreeMap::from([
                ("project".into(), json!("b")),
                ("milestone".into(), json!("m")),
            ])),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    assert_eq!(moved.metadata.project.as_deref(), Some("b"));
    assert_eq!(moved.body, member.body);
}

#[test]
fn historical_dangling_associations_are_readable_diagnosed_and_preserved_on_unrelated_edit() {
    let (_temp, repo) = fixture();
    let original = issue(&repo, BTreeMap::new());
    let path = repo.root().join(&original.path);
    let raw = std::fs::read_to_string(&path).unwrap();
    let edited = raw.replacen("---\n", "---\nproject: disappeared\nlabels: [old_label]\nx-retained:\n  nested: [a, b] # retain metadata\n",1);
    std::fs::write(&path, &edited).unwrap();
    let historical = repo.show_issue(&original.metadata.id.to_string()).unwrap();
    let report = repo.doctor().unwrap();
    assert!(report.valid, "{:?}", report.errors);
    assert_eq!(report.warnings.len(), 2);
    repo.export_snapshot().unwrap().validate().unwrap();
    let receipt = repo
        .mutate_issue(
            &original.metadata.id.to_string(),
            Some(&historical.source),
            &update(BTreeMap::from([("title".into(), json!("Renamed"))])),
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(receipt.result["metadata"]["project"], "disappeared");
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("nested: [a, b] # retain metadata"));
    assert!(after.ends_with(&original.body));
    let blocked = repo
        .mutate_issue(
            &original.metadata.id.to_string(),
            None,
            &update(BTreeMap::from([(
                "project".into(),
                json!("another_missing"),
            )])),
            &RequestId::new(),
        )
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::NotFound);
    assert_eq!(std::fs::read_to_string(path).unwrap(), after);
}

#[test]
fn new_reference_case_aliases_are_actionable_and_archived_refs_remain_resolvable() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Project, "Project_A", BTreeMap::new());
    let failed = repo
        .create_issue(
            &CreateIssue {
                title: "case alias".into(),
                body: String::new(),
                fields: BTreeMap::from([("project".into(), json!("project_a"))]),
            },
            &RequestId::new(),
        )
        .unwrap_err();
    assert_eq!(failed.code, ErrorCode::InvalidInput);
    assert!(failed.message.contains("Project_A"));
    repo.mutate_planning(
        PlanningKind::Project,
        "Project_A",
        None,
        &workdeck_pm::PlanningMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    let member = issue(
        &repo,
        BTreeMap::from([("project".into(), json!("Project_A"))]),
    );
    assert_eq!(member.metadata.project.as_deref(), Some("Project_A"));
}

#[test]
fn initiative_project_and_target_retirement_cannot_orphan_planning_members() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Initiative, "i", BTreeMap::new());
    create(&repo, PlanningKind::Target, "t", BTreeMap::new());
    create(
        &repo,
        PlanningKind::Project,
        "p",
        BTreeMap::from([
            ("initiative".into(), json!("i")),
            ("targets".into(), json!(["t"])),
        ]),
    );
    create(
        &repo,
        PlanningKind::Milestone,
        "m",
        BTreeMap::from([
            ("project".into(), json!("p")),
            ("targets".into(), json!(["t"])),
        ]),
    );
    for (kind, id, count) in [
        (workdeck_pm::RetirementKind::Initiative, "i", 1),
        (workdeck_pm::RetirementKind::Project, "p", 1),
        (workdeck_pm::RetirementKind::Target, "t", 2),
    ] {
        let target = workdeck_pm::RetirementTarget::new(kind, id).unwrap();
        let ordinary = repo.retirement_preview(&target).unwrap();
        assert!(!ordinary.allowed);
        assert_eq!(ordinary.planning_blockers.len(), count);
        let force = repo.reference_retirement_preview(&target).unwrap();
        assert!(!force.allowed);
        assert_eq!(force.blockers.len(), count);
        let mut input = workdeck_pm::RetirementInput::new(target);
        input.expected_preview = Some(force.fingerprint);
        assert_eq!(
            repo.retire_reference(&input, &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::PolicyBlocked
        );
    }
    assert!(repo.doctor().unwrap().valid);
    assert!(!repo.root().join("tombstones").exists());
}

#[test]
fn target_and_milestone_issue_resolution_is_replayable_and_preserves_unrelated_membership() {
    for kind in [PlanningKind::Target, PlanningKind::Milestone] {
        let (_temp, repo) = fixture();
        create(&repo, PlanningKind::Project, "p", BTreeMap::new());
        let fields = if kind == PlanningKind::Milestone {
            BTreeMap::from([("project".into(), json!("p"))])
        } else {
            BTreeMap::new()
        };
        create(&repo, kind, "selected", fields.clone());
        create(&repo, kind, "keep", fields);
        let field = if kind == PlanningKind::Milestone {
            "milestone"
        } else {
            "targets"
        };
        let value = if kind == PlanningKind::Milestone {
            json!("selected")
        } else {
            json!(["selected", "keep"])
        };
        let member = issue(
            &repo,
            BTreeMap::from([("project".into(), json!("p")), (field.into(), value)]),
        );
        let target = workdeck_pm::RetirementTarget::new(kind.into(), "selected").unwrap();
        let plan = repo.reference_retirement_preview(&target).unwrap();
        assert!(plan.allowed, "{:?}", plan.blockers);
        assert_eq!(plan.affected.len(), 1);
        let input = workdeck_pm::RetirementInput {
            target,
            expected: None,
            expected_preview: Some(plan.fingerprint),
        };
        let request = RequestId::new();
        let first = repo.retire_reference(&input, &request).unwrap();
        let current = repo.show_issue(&member.metadata.id.to_string()).unwrap();
        assert_eq!(current.body, member.body);
        assert_eq!(current.metadata.project, member.metadata.project);
        if kind == PlanningKind::Milestone {
            assert!(current.metadata.milestone.is_none());
        } else {
            assert_eq!(current.metadata.targets, ["keep"]);
        }
        repo.mutate_issue(
            &member.metadata.id.to_string(),
            None,
            &update(BTreeMap::from([("title".into(), json!("Later edit"))])),
            &RequestId::new(),
        )
        .unwrap();
        assert_eq!(repo.retire_reference(&input, &request).unwrap(), first);
        assert!(repo.doctor().unwrap().valid);
        repo.export_snapshot().unwrap().validate().unwrap();
    }
}

#[test]
fn completed_issue_target_changes_use_shared_reopen_policy() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Target, "t", BTreeMap::new());
    let member = issue(&repo, BTreeMap::new());
    repo.mutate_issue(
        &member.metadata.id.to_string(),
        None,
        &workdeck_pm::IssueMutation::Complete { manual: None },
        &RequestId::new(),
    )
    .unwrap();
    let mutation = update(BTreeMap::from([("targets".into(), json!(["t"]))]));
    assert_eq!(
        repo.mutate_issue(
            &member.metadata.id.to_string(),
            None,
            &mutation,
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    repo.mutate_issue(
        &member.metadata.id.to_string(),
        None,
        &workdeck_pm::IssueMutation::Reopen,
        &RequestId::new(),
    )
    .unwrap();
    repo.mutate_issue(
        &member.metadata.id.to_string(),
        None,
        &mutation,
        &RequestId::new(),
    )
    .unwrap();
}

#[test]
fn membership_views_share_query_predicates_and_bind_parent_members_and_config() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Initiative, "i", BTreeMap::new());
    for target in ["project-target", "milestone-target", "direct-target"] {
        create(&repo, PlanningKind::Target, target, BTreeMap::new());
    }
    create(&repo, PlanningKind::Cycle, "c", BTreeMap::new());
    let project = create(
        &repo,
        PlanningKind::Project,
        "p",
        BTreeMap::from([
            ("initiative".into(), json!("i")),
            ("targets".into(), json!(["project-target"])),
        ]),
    );
    create(
        &repo,
        PlanningKind::Milestone,
        "m",
        BTreeMap::from([
            ("project".into(), json!("p")),
            ("targets".into(), json!(["milestone-target"])),
        ]),
    );
    let member = issue(
        &repo,
        BTreeMap::from([
            ("project".into(), json!("p")),
            ("milestone".into(), json!("m")),
            ("cycle".into(), json!("c")),
            ("targets".into(), json!(["direct-target"])),
            ("assignee".into(), json!("owner")),
        ]),
    );
    let mut query = workdeck_pm::PlanningMembershipQuery::new(PlanningKind::Project, "p");
    query.issues.assignee = Some("owner".into());
    let original = repo.planning_membership(&query).unwrap();
    assert_eq!(original.record, project);
    assert_eq!(original.related_records.len(), 1);
    assert_eq!(original.related_records[0].metadata.id, "m");
    assert_eq!(original.issues.as_slice(), std::slice::from_ref(&member));
    assert!(original.warnings.is_empty());
    for (kind, id) in [
        (PlanningKind::Initiative, "i"),
        (PlanningKind::Milestone, "m"),
        (PlanningKind::Cycle, "c"),
        (PlanningKind::Target, "project-target"),
        (PlanningKind::Target, "milestone-target"),
        (PlanningKind::Target, "direct-target"),
    ] {
        assert_eq!(
            repo.planning_membership(&workdeck_pm::PlanningMembershipQuery::new(kind, id))
                .unwrap()
                .issues
                .as_slice(),
            std::slice::from_ref(&member)
        );
    }
    query.issues.assignee = Some("someone-else".into());
    assert!(repo.planning_membership(&query).unwrap().issues.is_empty());
    query.issues.assignee = None;
    repo.mutate_issue(
        member.metadata.id.as_str(),
        None,
        &workdeck_pm::IssueMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    let active = repo.planning_membership(&query).unwrap();
    assert!(active.issues.is_empty());
    assert_ne!(original.fingerprint, active.fingerprint);
    query.issues.archive = workdeck_pm::ArchiveFilter::All;
    assert_eq!(repo.planning_membership(&query).unwrap().issues.len(), 1);
    let config = repo.root().join("config.yml");
    let raw = std::fs::read_to_string(&config).unwrap();
    std::fs::write(config, format!("{raw}\n# human annotation\n")).unwrap();
    let annotated = repo.planning_membership(&query).unwrap();
    assert_ne!(annotated.config_hash, original.config_hash);
    assert_eq!(original.issues, [member]);
    let path = repo.root().join(&project.path);
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, format!("{raw}\nDirect project edit\n")).unwrap();
    assert_eq!(
        repo.mutate_planning(
            PlanningKind::Project,
            "p",
            Some(&original.record.source),
            &planning_update(BTreeMap::from([("name".into(), json!("Renamed"))])),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn hierarchy_restoration_and_native_import_validate_prospective_refs_without_rewriting_history() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Initiative, "i", BTreeMap::new());
    create(&repo, PlanningKind::Target, "t", BTreeMap::new());
    create(
        &repo,
        PlanningKind::Project,
        "p",
        BTreeMap::from([("initiative".into(), json!("i"))]),
    );
    create(
        &repo,
        PlanningKind::Milestone,
        "m",
        BTreeMap::from([
            ("project".into(), json!("p")),
            ("targets".into(), json!(["t"])),
        ]),
    );
    let snapshot = repo.export_snapshot().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let root = destination.path().join(".workdeck");
    workdeck_pm::restore_snapshot(&root, &snapshot, None, &RequestId::new()).unwrap();
    let restored = Repository::open_source(&root).unwrap();
    for file in &snapshot.files {
        assert_eq!(std::fs::read(root.join(&file.path)).unwrap(), file.content);
    }
    // Directly authored source has no absent durable operation to restore.
    let mut metadata = workdeck_pm::IssueMetadata::new(
        &repo.config().unwrap(),
        "Authored import",
        chrono::Utc::now(),
    )
    .unwrap();
    metadata.project = Some("p".into());
    metadata.milestone = Some("m".into());
    let path = repo.root().join(format!("issues/{}/item.md", metadata.id));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            "---\n{}---\nBody\n",
            serde_yaml_ng::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let member = repo.show_issue(metadata.id.as_str()).unwrap();
    let changed = repo.export_snapshot().unwrap();
    let plan = restored
        .preview_snapshot_import(&changed, workdeck_pm::SnapshotImportMode::Merge)
        .unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restored
        .import_snapshot(
            &changed,
            workdeck_pm::SnapshotImportMode::Merge,
            Some(&plan.fingerprint),
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(
        restored.show_issue(member.metadata.id.as_str()).unwrap(),
        member
    );
    // Old declarations remain legal snapshot data, but adding one as NEW work
    // through ordinary import must obey the same native authoring policy.
    let mut invalid = workdeck_pm::IssueMetadata::new(
        &repo.config().unwrap(),
        "Historical dangling",
        chrono::Utc::now(),
    )
    .unwrap();
    invalid.project = Some("disappeared".into());
    let path = repo.root().join(format!("issues/{}/item.md", invalid.id));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!(
            "---\n{}---\nBody\n",
            serde_yaml_ng::to_string(&invalid).unwrap()
        ),
    )
    .unwrap();
    let historical = repo.export_snapshot().unwrap();
    historical.validate().unwrap();
    let blocked = restored
        .preview_snapshot_import(&historical, workdeck_pm::SnapshotImportMode::Merge)
        .unwrap();
    assert!(!blocked.allowed);
    assert!(
        blocked
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::NotFound)
    );
    assert_eq!(restored.list_issues().unwrap().len(), 1);
}

#[test]
fn wrong_kind_fields_outcome_proof_claims_and_duplicate_target_links_never_publish() {
    let (_temp, repo) = fixture();
    for (kind, fields) in [
        (
            PlanningKind::Label,
            BTreeMap::from([("lead".into(), json!("owner"))]),
        ),
        (
            PlanningKind::Target,
            BTreeMap::from([("targets".into(), json!(["self"]))]),
        ),
        (
            PlanningKind::Project,
            BTreeMap::from([(
                "exit_criteria".into(),
                json!([{"id":"ready","description":"Proven","verified":true}]),
            )]),
        ),
        (
            PlanningKind::Project,
            BTreeMap::from([("starts_at".into(), json!("next week"))]),
        ),
        (
            PlanningKind::Project,
            BTreeMap::from([("targets".into(), json!(["t", "T"]))]),
        ),
    ] {
        let input = CreatePlanning {
            id: Some("blocked".into()),
            name: "Blocked".into(),
            body: String::new(),
            fields,
        };
        assert!(
            repo.create_planning(kind, &input, &RequestId::new())
                .is_err()
        );
        assert!(repo.list_planning(kind).unwrap().is_empty());
    }
}

#[test]
fn planning_membership_race_before_journal_blocks_retirement_without_authority_changes() {
    let (_temp, repo) = fixture();
    let target = create(&repo, PlanningKind::Target, "t", BTreeMap::new());
    let project = create(&repo, PlanningKind::Project, "p", BTreeMap::new());
    let retirement = workdeck_pm::RetirementTarget::new(PlanningKind::Target.into(), "t").unwrap();
    let plan = repo.reference_retirement_preview(&retirement).unwrap();
    let input = workdeck_pm::RetirementInput {
        target: retirement,
        expected: None,
        expected_preview: Some(plan.fingerprint),
    };
    let path = repo.root().join(&project.path);
    let raw = std::fs::read_to_string(&path).unwrap();
    let changed = raw.replacen("---\n", "---\ntargets: [t]\n", 1);
    let error = repo
        .retire_reference_with_faults(&input, &RequestId::new(), |point| {
            if point == workdeck_pm::transactions::FaultPoint::BeforeJournal {
                std::fs::write(&path, &changed).unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(std::fs::read_to_string(path).unwrap(), changed);
    assert_eq!(
        repo.planning_record(PlanningKind::Target, "t").unwrap(),
        target
    );
    assert!(!repo.root().join("tombstones/targets/t.yml").exists());
}

#[test]
fn target_resolution_fault_recovers_before_any_reader_observes_partial_membership() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Target, "t", BTreeMap::new());
    let member = issue(&repo, BTreeMap::from([("targets".into(), json!(["t"]))]));
    let target = workdeck_pm::RetirementTarget::new(PlanningKind::Target.into(), "t").unwrap();
    let plan = repo.reference_retirement_preview(&target).unwrap();
    let input = workdeck_pm::RetirementInput {
        target,
        expected: None,
        expected_preview: Some(plan.fingerprint),
    };
    let request = RequestId::new();
    let error = repo
        .retire_reference_with_faults(&input, &request, |point| {
            if point == workdeck_pm::transactions::FaultPoint::AfterChange(0) {
                Err(workdeck_pm::PmError::new(
                    ErrorCode::Io,
                    "deliberate interruption",
                ))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    assert_eq!(
        repo.planning_membership(&workdeck_pm::PlanningMembershipQuery::new(
            PlanningKind::Target,
            "t"
        ))
        .unwrap_err()
        .code,
        ErrorCode::RecoveryRequired
    );
    let recovered = workdeck_pm::transactions::TransactionStore::open(repo.root())
        .unwrap()
        .recover()
        .unwrap();
    assert_eq!(
        repo.retire_reference(&input, &request).unwrap(),
        recovered[0]
    );
    assert!(
        repo.show_issue(member.metadata.id.as_str())
            .unwrap()
            .metadata
            .targets
            .is_empty()
    );
    assert!(
        repo.planning_record(PlanningKind::Target, "t")
            .unwrap()
            .retirement
            .is_some()
    );
    assert!(repo.doctor().unwrap().valid);
}

#[test]
fn orphan_hierarchy_authority_blocks_legacy_cutover_from_assigning_a_new_identity() {
    for directory in [
        "initiatives",
        "milestones",
        "targets",
        "wiki",
        "issues/retained/time",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join(".agents/workdeck");
        let destination = temp.path().join(".workdeck");
        std::fs::create_dir_all(source.join("issues")).unwrap();
        let path = destination.join(directory).join("retained/item.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "Retain this orphan source").unwrap();
        let options = workdeck_pm::migration::PreviewOptions {
            config: workdeck_pm::Config::new("WD").unwrap(),
            imported_at: "2026-09-09T00:00:00Z".parse().unwrap(),
        };
        let preview = workdeck_pm::migration::preview(&source, &destination, &options).unwrap();
        assert!(
            !preview.complete,
            "{directory} orphan authority was ignored"
        );
        assert!(
            preview
                .blockers
                .iter()
                .any(|error| error.code == ErrorCode::RecoveryRequired)
        );
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "Retain this orphan source"
        );
        assert!(!destination.join("config.yml").exists());
    }
}

#[test]
fn target_creation_defaults_share_native_reference_validation_and_authority_metadata() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Target, "delivery", BTreeMap::new());
    let path = repo.root().join("templates/issues/delivery.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path,"---\nschema: 1\nid: delivery\nname: Delivery\ndefaults:\n  targets: [delivery]\n---\nScope\n").unwrap();
    let input = workdeck_pm::TemplateIssueInput {
        template: "delivery".into(),
        title: "Templated work".into(),
        body: None,
        fields: BTreeMap::new(),
    };
    let receipt = repo
        .create_issue_from_template(&input, &RequestId::new())
        .unwrap();
    assert_eq!(receipt.result["metadata"]["targets"], json!(["delivery"]));
    assert_eq!(
        workdeck_pm::MetadataAuthority::for_issue_field("targets"),
        Some(workdeck_pm::MetadataAuthority::Authoritative)
    );
}

#[test]
fn historical_case_aliases_are_explicit_diagnostics_without_rewriting_or_changing_queries() {
    let (_temp, repo) = fixture();
    create(&repo, PlanningKind::Project, "Project_A", BTreeMap::new());
    let original = issue(&repo, BTreeMap::new());
    let path = repo.root().join(&original.path);
    let raw = std::fs::read_to_string(&path).unwrap();
    let historical = raw.replacen("---\n", "---\nproject: project_a\n", 1);
    std::fs::write(&path, &historical).unwrap();
    let report = repo.doctor().unwrap();
    assert!(report.valid, "{:?}", report.errors);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.message.contains("case alias")
                && warning.message.contains("Project_A")),
        "{:?}",
        report.warnings
    );
    let view = repo
        .planning_membership(&workdeck_pm::PlanningMembershipQuery::new(
            PlanningKind::Project,
            "Project_A",
        ))
        .unwrap();
    assert!(
        view.issues.is_empty(),
        "exact stored-reference query contract must not change"
    );
    assert!(!view.warnings.is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), historical);
}

#[test]
fn new_hierarchy_kinds_cannot_claim_unsupported_legacy_import_provenance() {
    for (kind, directory) in [
        (PlanningKind::Initiative, "initiatives"),
        (PlanningKind::Milestone, "milestones"),
        (PlanningKind::Target, "targets"),
    ] {
        let (_temp, repo) = fixture();
        let mut metadata = serde_json::json!({"schema":1,"id":"claimed","revision":1,"name":"Unsupported origin","imported":{"source":".agents/workdeck/projects.toml","format":"legacy_toml","content":workdeck_pm::ContentHash::of(b"legacy")}});
        if kind == PlanningKind::Milestone {
            metadata["project"] = json!("retained_project");
        }
        let metadata: workdeck_pm::PlanningMetadata = serde_json::from_value(metadata).unwrap();
        assert!(
            metadata.validate(kind).is_err(),
            "{kind:?} accepted an origin its converter cannot produce"
        );
        let path = repo.root().join(directory).join("claimed/item.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let raw = format!(
            "---\n{}---\nOriginal bytes\n",
            serde_yaml_ng::to_string(&metadata).unwrap()
        );
        std::fs::write(&path, &raw).unwrap();
        assert_eq!(
            repo.planning_record(kind, "claimed").unwrap_err().code,
            ErrorCode::InvalidSchema
        );
        assert!(!repo.doctor().unwrap().valid);
        assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
    }
}
