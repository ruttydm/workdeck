use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
use workdeck_pm::*;
fn fixture() -> (tempfile::TempDir, Repository) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path(), "WD").unwrap();
    (dir, repo)
}
fn field(required: bool) -> CustomFieldDefinition {
    CustomFieldDefinition {
        field_type: CustomFieldType::Enum,
        scopes: BTreeSet::from([CustomScope::Issue]),
        required,
        archived: false,
        options: vec!["low".into(), "high".into()],
        custom: BTreeMap::new(),
        extra: BTreeMap::new(),
    }
}
fn change(required: bool) -> SchemaChange {
    SchemaChange {
        fields: BTreeMap::from([("risk".into(), field(required))]),
        ..Default::default()
    }
}
#[test]
fn absent_policy_reads_are_optional_and_create_no_files() {
    let (_d, r) = fixture();
    assert!(r.users().unwrap().source.is_none());
    assert!(r.organization_schema().unwrap().source.is_none());
    assert!(r.organization_compliance().unwrap().compliant);
    assert!(!r.root().join("users.yml").exists());
    assert!(!r.root().join("schema.yml").exists());
}
#[test]
fn users_preserve_ids_and_replay_before_later_policy() {
    let (_d, r) = fixture();
    let q = RequestId::new();
    let input = UserMutation::Create {
        user: UserDefinition::new("Agent One"),
    };
    let receipt = r.mutate_user("Agent_One", None, &input, &q).unwrap();
    assert_eq!(r.user("Agent_One").unwrap().name, "Agent One");
    r.mutate_user(
        "Agent_One",
        None,
        &UserMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        r.mutate_user("agent_one", None, &input, &RequestId::new())
            .is_err()
    );
    assert_eq!(
        r.mutate_user("Agent_One", None, &input, &q)
            .unwrap()
            .operation_id,
        receipt.operation_id
    );
    assert!(r.user("Agent_One").unwrap().archived);
}
#[test]
fn schema_activation_is_reviewed_and_blocks_missing_live_values() {
    let (_d, r) = fixture();
    r.create_issue(&CreateIssue::new("Existing", "Body"), &RequestId::new())
        .unwrap();
    let p = r.preview_schema_change(&change(true)).unwrap();
    assert!(!p.allowed);
    assert!(!p.compliance.compliant);
    assert!(
        r.apply_schema_change(&change(true), Some(&p.fingerprint), &RequestId::new())
            .is_err()
    );
    assert!(!r.root().join("schema.yml").exists());
}
#[test]
fn exact_decimal_values_never_sum_units() {
    let (_d, r) = fixture();
    for (value, unit) in [("0.1", "hours"), ("0.2", "hours"), ("3", "points")] {
        let mut issue = CreateIssue::new("Estimated", "");
        issue
            .fields
            .insert("estimate".into(), json!({"value":value,"unit":unit}));
        r.create_issue(&issue, &RequestId::new()).unwrap();
    }
    let report = r.estimate_report(&IssueQuery::default()).unwrap();
    assert_eq!(report.by_unit["hours"].to_string(), "0.3");
    assert_eq!(report.by_unit["points"].to_string(), "3");
    assert_eq!(report.estimated, 3);
}
#[test]
fn organization_snapshots_restore_exact_policy_bytes() {
    let (_d, r) = fixture();
    r.apply_schema_change(&change(false), None, &RequestId::new())
        .unwrap();
    r.mutate_user(
        "worker",
        None,
        &UserMutation::Create {
            user: UserDefinition::new("Worker"),
        },
        &RequestId::new(),
    )
    .unwrap();
    let native = r.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    restore_snapshot(&root, &native, None, &RequestId::new()).unwrap();
    for path in ["schema.yml", "users.yml"] {
        assert_eq!(
            fs::read(r.root().join(path)).unwrap(),
            fs::read(root.join(path)).unwrap()
        );
    }
    assert!(
        Repository::open_source(&root)
            .unwrap()
            .doctor()
            .unwrap()
            .valid
    );
}
fn issue(r: &Repository, custom: serde_json::Value) -> IssueRecord {
    let mut input = CreateIssue::new("Policy fixture", "Authored body\n");
    input.fields.insert("custom".into(), custom);
    serde_json::from_value(r.create_issue(&input, &RequestId::new()).unwrap().result).unwrap()
}
fn update(fields: serde_json::Value) -> IssueMutation {
    IssueMutation::Update {
        input: UpdateIssue {
            body: None,
            fields: serde_json::from_value(fields).unwrap(),
        },
    }
}
fn authority(r: &Repository) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    r.export_snapshot()
        .unwrap()
        .files
        .into_iter()
        .map(|f| (f.path, f.content))
        .collect()
}
#[test]
fn typed_requirements_apply_to_create_update_raw_edit_templates_and_completion() {
    let (_d, r) = fixture();
    r.apply_schema_change(&change(true), None, &RequestId::new())
        .unwrap();
    let input = CreateIssue::new("Missing", "");
    assert_eq!(
        r.create_issue(&input, &RequestId::new()).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
    let record = issue(
        &r,
        json!({"risk":"low","legacy":{"kept":[null,18446744073709551615_u64]},"unknown":{"keep":true}}),
    );
    let before = authority(&r);
    for mutation in [
        update(json!({"custom":{"risk":"invalid"}})),
        IssueMutation::PatchCustom {
            patch: CustomPatch {
                set: BTreeMap::new(),
                unset: vec!["risk".into()],
            },
        },
    ] {
        assert_eq!(
            r.mutate_issue(
                record.metadata.id.as_str(),
                None,
                &mutation,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
            ErrorCode::PolicyBlocked
        );
    }
    let raw = invalid_risk(&fs::read_to_string(r.root().join(&record.path)).unwrap());
    assert!(
        r.mutate_issue(
            record.metadata.id.as_str(),
            None,
            &IssueMutation::EditDocument { markdown: raw },
            &RequestId::new()
        )
        .is_err()
    );
    assert_eq!(authority(&r), before);
    let templates = r.root().join("templates/issues");
    fs::create_dir_all(&templates).unwrap();
    fs::write(
        templates.join("partial.md"),
        "---\nschema: 1\nid: partial\nname: Partial\ndefaults: {}\n---\nTemplate body\n",
    )
    .unwrap();
    let partial = r.issue_template("partial").unwrap();
    assert!(
        r.create_issue(
            &partial.apply("Incomplete".into(), None, BTreeMap::new()),
            &RequestId::new()
        )
        .is_err()
    );
    let good = partial.apply(
        "Complete inputs".into(),
        None,
        BTreeMap::from([("custom".into(), json!({"risk":"high"}))]),
    );
    r.create_issue(&good, &RequestId::new()).unwrap();
    fs::write(
        templates.join("partial.md"),
        "---\nschema: 1\nid: partial\nname: Partial\ndefaults: {custom: {risk: wrong}}\n---\n",
    )
    .unwrap();
    assert!(r.issue_template("partial").is_err());
    fs::remove_file(templates.join("partial.md")).unwrap();
    // Direct edits remain readable and diagnosable, but do not bypass completion.
    let raw = invalid_risk(&fs::read_to_string(r.root().join(&record.path)).unwrap());
    fs::write(r.root().join(&record.path), raw).unwrap();
    assert!(
        !r.completion_report(record.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    assert_eq!(
        r.mutate_issue(
            record.metadata.id.as_str(),
            None,
            &IssueMutation::Complete {
                manual: Some(ManualAcceptanceInput {
                    actor: "human".into(),
                    reason: "explicit".into()
                })
            },
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    let doctor = r.doctor().unwrap();
    assert!(doctor.valid);
    assert!(!doctor.policy_compliant);
    assert!(!doctor.policy_violations.is_empty());
}
#[test]
fn completed_history_remains_readable_and_reopen_can_repair_new_policy_debt() {
    let (_d, r) = fixture();
    let record = issue(&r, json!({"old":"preserved"}));
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &IssueMutation::Complete { manual: None },
        &RequestId::new(),
    )
    .unwrap();
    let p = r.preview_schema_change(&change(true)).unwrap();
    assert!(p.allowed);
    assert!(p.compliance.violations.iter().all(|v| v.historical));
    r.apply_schema_change(&change(true), Some(&p.fingerprint), &RequestId::new())
        .unwrap();
    assert!(r.export_snapshot().is_ok());
    assert!(!r.organization_compliance().unwrap().compliant);
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &IssueMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &IssueMutation::Reopen,
        &RequestId::new(),
    )
    .unwrap();
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &IssueMutation::PatchCustom {
            patch: CustomPatch {
                set: BTreeMap::from([("risk".into(), json!("low"))]),
                unset: vec![],
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    assert!(r.organization_compliance().unwrap().compliant);
    assert_eq!(
        r.show_issue(record.metadata.id.as_str())
            .unwrap()
            .metadata
            .custom["old"],
        "preserved"
    );
}
#[test]
fn newly_required_fields_do_not_accept_spoofed_legacy_provenance() {
    let (_d, r) = fixture();
    r.apply_schema_change(&change(true), None, &RequestId::new())
        .unwrap();
    let mut input = CreateIssue::new("Spoof", "");
    input
        .fields
        .insert("custom".into(), json!({"legacy":{"source":"invented"}}));
    assert!(r.create_issue(&input, &RequestId::new()).is_err());
    let mut legacy = change(false);
    legacy.fields = BTreeMap::from([("legacy".into(), field(false))]);
    assert!(r.preview_schema_change(&legacy).is_err());
}
#[test]
fn custom_patch_preserves_unknown_null_u64_and_legacy_values() {
    let (_d, r) = fixture();
    let preserved = json!({"legacy":{"tag":"u64","value":18446744073709551615_u64},"unknown":null,"nested":{"other":[1,false]}});
    let record = issue(&r, preserved.clone());
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &IssueMutation::PatchCustom {
            patch: CustomPatch {
                set: BTreeMap::from([("risk".into(), json!("low"))]),
                unset: vec![],
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    let next = r.show_issue(record.metadata.id.as_str()).unwrap();
    for (k, v) in preserved.as_object().unwrap() {
        assert_eq!(&next.metadata.custom[k], v);
    }
    assert_eq!(next.body, record.body);
    assert!(
        CustomPatch {
            set: BTreeMap::from([("x".into(), json!(1))]),
            unset: vec!["x".into()]
        }
        .apply(&BTreeMap::new())
        .is_err()
    );
}
#[test]
fn registered_identities_cover_new_assignments_comments_time_and_acceptance() {
    let (_d, r) = fixture();
    r.mutate_user(
        "worker",
        None,
        &UserMutation::Create {
            user: UserDefinition::new("Worker"),
        },
        &RequestId::new(),
    )
    .unwrap();
    r.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    let record = issue(&r, json!({}));
    for mutation in [
        update(json!({"assignee":"missing"})),
        IssueMutation::Comment {
            author: "missing".into(),
            body: "Text".into(),
        },
        IssueMutation::Complete {
            manual: Some(ManualAcceptanceInput {
                actor: "missing".into(),
                reason: "Reason".into(),
            }),
        },
    ] {
        assert_eq!(
            r.mutate_issue(
                record.metadata.id.as_str(),
                None,
                &mutation,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
            ErrorCode::PolicyBlocked
        );
    }
    let input = TimeEntryInput {
        user: "missing".into(),
        actor: "worker".into(),
        seconds: 30,
        worked_at: "2026-09-01T00:00:00Z".parse().unwrap(),
    };
    assert!(
        r.log_time(record.metadata.id.as_str(), None, &input, &RequestId::new())
            .is_err()
    );
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &update(json!({"assignee":"worker"})),
        &RequestId::new(),
    )
    .unwrap();
    r.mutate_user(
        "worker",
        None,
        &UserMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    // Unchanged attribution survives; authoring a new reference does not.
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &update(json!({"title":"Edited"})),
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        r.mutate_issue(
            record.metadata.id.as_str(),
            None,
            &IssueMutation::Comment {
                author: "worker".into(),
                body: "Later".into()
            },
            &RequestId::new()
        )
        .is_err()
    );
    assert!(!r.organization_compliance().unwrap().compliant);
    assert!(r.doctor().unwrap().valid);
}
#[test]
fn schema_preview_binds_membership_and_current_source_then_replay_survives_later_changes() {
    let (_d, r) = fixture();
    let p = r.preview_schema_change(&change(true)).unwrap();
    let record = issue(&r, json!({}));
    let q = RequestId::new();
    assert_eq!(
        r.apply_schema_change(&change(true), Some(&p.fingerprint), &q)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert!(!r.root().join("schema.yml").exists());
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &IssueMutation::PatchCustom {
            patch: CustomPatch {
                set: BTreeMap::from([("risk".into(), json!("low"))]),
                unset: vec![],
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    let p = r.preview_schema_change(&change(true)).unwrap();
    let receipt = r
        .apply_schema_change(&change(true), Some(&p.fingerprint), &q)
        .unwrap();
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &update(json!({"title":"Later"})),
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        r.apply_schema_change(&change(true), Some(&p.fingerprint), &q)
            .unwrap()
            .operation_id,
        receipt.operation_id
    );
}
#[test]
fn schema_transaction_readset_rejects_editor_race_before_journal() {
    let (_d, r) = fixture();
    let record = issue(&r, json!({"risk":"low"}));
    let path = r.root().join(&record.path);
    let before = fs::read_to_string(&path).unwrap();
    let result =
        r.apply_schema_change_with_faults(&change(true), None, &RequestId::new(), |point| {
            if point == transactions::FaultPoint::BeforeJournal {
                fs::write(&path, invalid_risk(&before)).unwrap();
            }
            Ok(())
        });
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
    assert!(!r.root().join("schema.yml").exists());
}
#[test]
fn schema_faults_recover_with_one_receipt_and_no_partial_visibility() {
    for point in [
        transactions::FaultPoint::BeforeJournal,
        transactions::FaultPoint::AfterJournal,
        transactions::FaultPoint::AfterChange(0),
        transactions::FaultPoint::BeforeReceipt,
        transactions::FaultPoint::AfterReceipt,
    ] {
        let (_d, r) = fixture();
        let q = RequestId::new();
        let result = r.apply_schema_change_with_faults(&change(false), None, &q, |seen| {
            if seen == point {
                Err(PmError::new(ErrorCode::Io, "test fault"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        if point != transactions::FaultPoint::BeforeJournal {
            assert_eq!(
                r.organization_schema().unwrap_err().code,
                ErrorCode::RecoveryRequired
            );
            r.recover_operations().unwrap();
        }
        let receipt = r.apply_schema_change(&change(false), None, &q).unwrap();
        assert_eq!(
            r.apply_schema_change(&change(false), None, &q)
                .unwrap()
                .operation_id,
            receipt.operation_id
        );
        assert_eq!(r.organization_schema().unwrap().definition.fields.len(), 1);
    }
}
#[test]
fn definitions_cannot_repurpose_types_and_values_are_precise() {
    let (_d, r) = fixture();
    r.apply_schema_change(&change(false), None, &RequestId::new())
        .unwrap();
    let mut changed = change(false);
    changed.fields.get_mut("risk").unwrap().field_type = CustomFieldType::Text;
    changed.fields.get_mut("risk").unwrap().options.clear();
    assert!(r.preview_schema_change(&changed).is_err());
    for bad in ["-1", "NaN", "inf", "1e2", "1.0000001", ".5", "1."] {
        assert!(bad.parse::<DecimalAmount>().is_err(), "{bad}");
    }
    assert_eq!(
        "0001.250000".parse::<DecimalAmount>().unwrap().to_string(),
        "1.25"
    );
    assert!(serde_json::from_value::<Estimate>(json!({"value":0.1,"unit":"hours"})).is_err());
}
#[test]
fn registry_edits_preserve_unrelated_comments_and_extension_values() {
    let (_d, r) = fixture();
    r.mutate_user(
        "a",
        None,
        &UserMutation::Create {
            user: UserDefinition::new("A"),
        },
        &RequestId::new(),
    )
    .unwrap();
    r.mutate_user(
        "b",
        None,
        &UserMutation::Create {
            user: UserDefinition::new("B"),
        },
        &RequestId::new(),
    )
    .unwrap();
    let path = r.root().join("users.yml");
    let text = fs::read_to_string(&path).unwrap();
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).unwrap();
    let mut rendered = serde_yaml_ng::to_string(&value).unwrap();
    rendered = rendered.replace("  b:", "  # Preserve B attribution\n  b:");
    rendered.push_str("x-future: {preserve: [null, true]}\n");
    fs::write(&path, rendered).unwrap();
    r.mutate_user(
        "a",
        None,
        &UserMutation::Patch {
            name: Some("Changed".into()),
            kind: None,
            custom: CustomPatch::default(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let after = fs::read_to_string(path).unwrap();
    assert!(after.contains("# Preserve B attribution"));
    assert!(after.contains("x-future: {preserve: [null, true]}"));
    assert_eq!(r.user("b").unwrap().name, "B");
}
#[test]
fn previously_used_user_ids_cannot_be_recreated_after_direct_registry_removal() {
    let (_d, r) = fixture();
    r.mutate_user(
        "worker",
        None,
        &UserMutation::Create {
            user: UserDefinition::new("Original"),
        },
        &RequestId::new(),
    )
    .unwrap();
    let path = r.root().join("users.yml");
    let mut value: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["users"] = json!({});
    fs::write(&path, serde_yaml_ng::to_string(&value).unwrap()).unwrap();
    assert_eq!(
        r.mutate_user(
            "worker",
            None,
            &UserMutation::Create {
                user: UserDefinition::new("Reused")
            },
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
}

fn invalid_risk(source: &str) -> String {
    let mut document =
        documents::MarkdownDocument::parse(std::path::Path::new("item.md"), source).unwrap();
    let metadata: IssueMetadata = document.deserialize().unwrap();
    let mut custom = metadata.custom;
    custom.insert("risk".into(), json!("invalid"));
    let patch = serde_yaml_ng::to_value(BTreeMap::from([("custom", custom)])).unwrap();
    document.patch(patch.as_mapping().unwrap()).unwrap();
    let next = document.render();
    assert_ne!(next, source);
    next
}
fn clone_repository(source: &Repository) -> (tempfile::TempDir, Repository) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".workdeck");
    for file in source.export_snapshot().unwrap().files {
        let path = root.join(file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, file.content).unwrap();
    }
    let repository = Repository::open_source(&root).unwrap();
    (dir, repository)
}
#[test]
fn native_import_checks_projected_custom_policy_and_registry_evolution() {
    let (_d, r) = fixture();
    r.apply_schema_change(&change(true), None, &RequestId::new())
        .unwrap();
    let record = issue(&r, json!({"risk":"low"}));
    let (_copy, source) = clone_repository(&r);
    let path = source.root().join(&record.path);
    let raw = invalid_risk(&fs::read_to_string(&path).unwrap());
    let mut document = documents::MarkdownDocument::parse(&path, &raw).unwrap();
    let patch = serde_yaml_ng::to_value(json!({"revision":2})).unwrap();
    document.patch(patch.as_mapping().unwrap()).unwrap();
    fs::write(&path, document.render()).unwrap();
    let incoming = source.export_snapshot().unwrap();
    let p = r
        .preview_snapshot_import(&incoming, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(!p.allowed);
    let before = authority(&r);
    assert!(
        r.import_snapshot(
            &incoming,
            SnapshotImportMode::ReplaceMatching,
            Some(&p.fingerprint),
            &RequestId::new()
        )
        .is_err()
    );
    assert_eq!(authority(&r), before);
    let (_copy, source) = clone_repository(&r);
    let schema_path = source.root().join("schema.yml");
    let mut schema: OrganizationSchema =
        serde_yaml_ng::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    schema.fields.clear();
    schema.revision = schema.revision.next().unwrap();
    fs::write(&schema_path, serde_yaml_ng::to_string(&schema).unwrap()).unwrap();
    let p = r
        .preview_snapshot_import(
            &source.export_snapshot().unwrap(),
            SnapshotImportMode::ReplaceMatching,
        )
        .unwrap();
    assert!(!p.allowed);
}
#[test]
fn native_import_cannot_author_unregistered_comments() {
    let (_d, r) = fixture();
    r.mutate_user(
        "worker",
        None,
        &UserMutation::Create {
            user: UserDefinition::new("Worker"),
        },
        &RequestId::new(),
    )
    .unwrap();
    r.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    let record = issue(&r, json!({}));
    let (_copy, source) = clone_repository(&r);
    let id = IssueId::new("COM").unwrap();
    let path = source
        .root()
        .join(format!("issues/{}/comments/{id}.md", record.metadata.id));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path,format!("---\nschema: 1\nid: {id}\nissue: {}\nauthor: unknown\ncreated_at: {}\nrevision: 1\n---\nComment text\n",record.metadata.id,record.metadata.created_at.to_rfc3339())).unwrap();
    let p = r
        .preview_snapshot_import(
            &source.export_snapshot().unwrap(),
            SnapshotImportMode::Merge,
        )
        .unwrap();
    assert!(
        !p.allowed,
        "native import bypassed registered attribution: {p:?}"
    );
}
#[test]
fn planning_requirements_and_identity_leads_use_final_shared_candidates() {
    let (_d, r) = fixture();
    let mut definition = field(true);
    definition.scopes = BTreeSet::from([CustomScope::Project]);
    r.apply_schema_change(
        &SchemaChange {
            fields: BTreeMap::from([("risk".into(), definition)]),
            ..Default::default()
        },
        None,
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        r.create_planning(
            PlanningKind::Project,
            &CreatePlanning::new("Incomplete"),
            &RequestId::new()
        )
        .is_err()
    );
    let mut input = CreatePlanning::new("Complete");
    input.fields.insert(
        "custom".into(),
        json!({"risk":"low","legacy":{"keep":true}}),
    );
    let record: PlanningRecord = serde_json::from_value(
        r.create_planning(PlanningKind::Project, &input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    assert!(
        r.mutate_planning(
            PlanningKind::Project,
            &record.metadata.id,
            None,
            &PlanningMutation::PatchCustom {
                patch: CustomPatch {
                    set: BTreeMap::new(),
                    unset: vec!["risk".into()]
                }
            },
            &RequestId::new()
        )
        .is_err()
    );
    r.mutate_planning(
        PlanningKind::Project,
        &record.metadata.id,
        None,
        &PlanningMutation::PatchCustom {
            patch: CustomPatch {
                set: BTreeMap::from([("risk".into(), json!("high"))]),
                unset: vec![],
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        r.planning_record(PlanningKind::Project, &record.metadata.id)
            .unwrap()
            .metadata
            .custom["legacy"],
        json!({"keep":true})
    );
}
#[test]
fn required_false_and_zero_values_are_valid_and_typed_null_is_not() {
    let (_d, r) = fixture();
    let mut boolean = field(true);
    boolean.field_type = CustomFieldType::Boolean;
    boolean.options.clear();
    let mut integer = boolean.clone();
    integer.field_type = CustomFieldType::Integer;
    r.apply_schema_change(
        &SchemaChange {
            fields: BTreeMap::from([("approved".into(), boolean), ("count".into(), integer)]),
            ..Default::default()
        },
        None,
        &RequestId::new(),
    )
    .unwrap();
    issue(&r, json!({"approved":false,"count":0}));
    let mut invalid = CreateIssue::new("Null", "");
    invalid
        .fields
        .insert("custom".into(), json!({"approved":null,"count":0}));
    assert!(r.create_issue(&invalid, &RequestId::new()).is_err());
}
#[test]
fn organization_sources_fail_closed_on_wrong_repository_future_schema_and_oversize() {
    for filename in ["users.yml", "schema.yml"] {
        let (_d, r) = fixture();
        let wrong = RepositoryId::new();
        fs::write(
            r.root().join(filename),
            format!("schema: 1\nrepository: {wrong}\nrevision: 1\n"),
        )
        .unwrap();
        assert!(!r.doctor().unwrap().valid);
        fs::write(r.root().join(filename), "schema: 99\n").unwrap();
        let doctor = r.doctor().unwrap();
        assert!(
            doctor
                .errors
                .iter()
                .any(|e| e.code == ErrorCode::UnsupportedSchema)
        );
    }
    let (_d, r) = fixture();
    let mut user = UserDefinition::new("Big");
    user.custom
        .insert("big".into(), json!("x".repeat(MAX_ORGANIZATION_BYTES)));
    assert!(
        r.mutate_user(
            "big",
            None,
            &UserMutation::Create { user },
            &RequestId::new()
        )
        .is_err()
    );
    assert!(!r.root().join("users.yml").exists());
}
#[test]
fn explicit_legacy_conversion_retains_data_and_reports_debt_without_granting_new_writes() {
    let (_d, r) = fixture();
    r.apply_schema_change(&change(true), None, &RequestId::new())
        .unwrap();
    r.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    let bytes=serde_json::to_vec(&json!({"issues":[{"key":"WD-7","title":"Imported","description":"Original body\n","status":"todo","priority":"high","created_at":"2020-01-01T01:02:03Z","updated_at":"2021-02-02T03:04:05Z","assignee":"Historical Person","extra":{"null":null,"unsigned":u64::MAX}}]})).unwrap();
    let ImportSource::Legacy(source) = decode_transfer(&bytes).unwrap() else {
        panic!("legacy")
    };
    let p = r
        .preview_legacy_import(&source, None, SnapshotImportMode::Merge)
        .unwrap();
    assert!(p.plan.allowed, "{:?}", p.plan.blockers);
    r.import_legacy_export(
        &source,
        None,
        SnapshotImportMode::Merge,
        None,
        &RequestId::new(),
    )
    .unwrap();
    let imported = r.show_issue("WD-7").unwrap();
    assert_eq!(
        imported.metadata.assignee.as_deref(),
        Some("Historical Person")
    );
    assert_eq!(imported.body, "Original body\n");
    assert!(imported.metadata.custom.contains_key("legacy"));
    let doctor = r.doctor().unwrap();
    assert!(doctor.valid);
    assert!(!doctor.policy_compliant);
    assert!(!r.completion_report("WD-7").unwrap().allowed);
    assert!(
        r.mutate_issue(
            "WD-7",
            None,
            &update(json!({"title":"Unrepaired"})),
            &RequestId::new()
        )
        .is_err()
    );
    r.mutate_issue(
        "WD-7",
        None,
        &IssueMutation::PatchCustom {
            patch: CustomPatch {
                set: BTreeMap::from([("risk".into(), json!("low"))]),
                unset: vec![],
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        r.show_issue("WD-7").unwrap().metadata.custom["legacy"],
        imported.metadata.custom["legacy"]
    );
}
#[test]
fn concurrent_user_patches_preserve_other_keys_and_stale_tokens_fail() {
    let (_d, r) = fixture();
    r.mutate_user(
        "worker",
        None,
        &UserMutation::Create {
            user: UserDefinition::new("Worker"),
        },
        &RequestId::new(),
    )
    .unwrap();
    let token = r.users().unwrap().source.unwrap();
    let root = r.root().to_path_buf();
    let handles = (0..2)
        .map(|i| {
            let root = root.clone();
            std::thread::spawn(move || {
                let repo = Repository::open_source(&root).unwrap();
                let q = RequestId::new();
                let intent = UserMutation::Patch {
                    name: None,
                    kind: None,
                    custom: CustomPatch {
                        set: BTreeMap::from([(format!("key{i}"), json!(i))]),
                        unset: vec![],
                    },
                };
                loop {
                    match repo.mutate_user("worker", None, &intent, &q) {
                        Ok(_) => break,
                        Err(e) if e.code == ErrorCode::Locked => std::thread::yield_now(),
                        Err(e) => panic!("{e:?}"),
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
    let user = r.user("worker").unwrap();
    assert_eq!(user.custom.len(), 2);
    assert_eq!(
        r.mutate_user(
            "worker",
            Some(&token),
            &UserMutation::Archive { archived: true },
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
}
#[test]
fn exact_estimate_overflow_fails_instead_of_wrapping_or_rounding() {
    let (_d, r) = fixture();
    let max = "340282366920938463463374607431768.211455";
    for _ in 0..2 {
        let mut issue = CreateIssue::new("Large", "");
        issue
            .fields
            .insert("estimate".into(), json!({"value":max,"unit":"points"}));
        r.create_issue(&issue, &RequestId::new()).unwrap();
    }
    assert!(r.estimate_report(&IssueQuery::default()).is_err());
}
#[test]
fn archiving_a_populated_field_retains_values_and_only_blocks_new_or_changed_values() {
    let (_d, r) = fixture();
    r.apply_schema_change(&change(false), None, &RequestId::new())
        .unwrap();
    let record = issue(&r, json!({"risk":"low"}));
    let mut archived = change(false);
    archived.fields.get_mut("risk").unwrap().archived = true;
    let plan = r.preview_schema_change(&archived).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    r.apply_schema_change(&archived, Some(&plan.fingerprint), &RequestId::new())
        .unwrap();
    assert!(r.organization_compliance().unwrap().compliant);
    r.mutate_issue(
        record.metadata.id.as_str(),
        None,
        &update(json!({"title":"Retained"})),
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        r.mutate_issue(
            record.metadata.id.as_str(),
            None,
            &IssueMutation::PatchCustom {
                patch: CustomPatch {
                    set: BTreeMap::from([("risk".into(), json!("high"))]),
                    unset: vec![]
                }
            },
            &RequestId::new()
        )
        .is_err()
    );
    let mut new = CreateIssue::new("New", "");
    new.fields.insert("custom".into(), json!({"risk":"low"}));
    assert!(r.create_issue(&new, &RequestId::new()).is_err());
}
#[test]
fn native_import_validates_supplied_template_defaults_against_current_policy() {
    let (_d, r) = fixture();
    r.apply_schema_change(&change(false), None, &RequestId::new())
        .unwrap();
    let (_copy, source) = clone_repository(&r);
    let path = source.root().join("templates/issues/imported.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path,"---\nschema: 1\nid: imported\nname: Imported\ndefaults: {custom: {risk: invalid}}\n---\nTemplate body\n").unwrap();
    let incoming = source.export_snapshot().unwrap();
    let p = r
        .preview_snapshot_import(&incoming, SnapshotImportMode::Merge)
        .unwrap();
    assert!(
        !p.allowed,
        "native import admitted invalid authoring defaults"
    );
}
#[test]
fn native_time_amendment_preserves_unchanged_historical_user_but_checks_new_actor() {
    let (_d, r) = fixture();
    for id in ["worker", "recorder"] {
        r.mutate_user(
            id,
            None,
            &UserMutation::Create {
                user: UserDefinition::new(id),
            },
            &RequestId::new(),
        )
        .unwrap();
    }
    let issue = issue(&r, json!({}));
    let input = TimeEntryInput {
        user: "worker".into(),
        actor: "recorder".into(),
        seconds: 60,
        worked_at: "2026-09-01T00:00:00Z".parse().unwrap(),
    };
    let original: TimeEntryRecord = serde_json::from_value(
        r.log_time(issue.metadata.id.as_str(), None, &input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    r.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    r.mutate_user(
        "worker",
        None,
        &UserMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    let (_copy, source) = clone_repository(&r);
    let mut next = original.entry.clone();
    next.id = RecordId::new("TIME").unwrap();
    next.supersedes = Some(original.entry.id.clone());
    next.reason = Some("Correct seconds".into());
    next.seconds = 90;
    next.recorded_at += chrono::Duration::seconds(1);
    let path = source
        .root()
        .join(format!("issues/{}/time/{}.yml", issue.metadata.id, next.id));
    fs::write(&path, serde_yaml_ng::to_string(&next).unwrap()).unwrap();
    let p = r
        .preview_snapshot_import(
            &source.export_snapshot().unwrap(),
            SnapshotImportMode::Merge,
        )
        .unwrap();
    assert!(
        p.allowed,
        "unchanged historical user was rejected: {:?}",
        p.blockers
    );
    next.actor = "worker".into();
    fs::write(path, serde_yaml_ng::to_string(&next).unwrap()).unwrap();
    let p = r
        .preview_snapshot_import(
            &source.export_snapshot().unwrap(),
            SnapshotImportMode::Merge,
        )
        .unwrap();
    assert!(!p.allowed, "new actor must be active");
}
#[test]
fn lifecycle_repair_preserves_unchanged_invalid_custom_until_an_explicit_content_fix() {
    let mut failures = Vec::new();
    for mutation in [
        IssueMutation::Archive { archived: true },
        IssueMutation::Reopen,
    ] {
        let (_dir, repository) = fixture();
        let record = issue(&repository, json!({"risk": {"historical": true}}));
        repository
            .mutate_issue(
                record.metadata.id.as_str(),
                None,
                &IssueMutation::Complete { manual: None },
                &RequestId::new(),
            )
            .unwrap();
        repository
            .apply_schema_change(&change(false), None, &RequestId::new())
            .unwrap();
        let before = repository.show_issue(record.metadata.id.as_str()).unwrap();
        assert!(!repository.organization_compliance().unwrap().compliant);
        let result = repository.mutate_issue(
            record.metadata.id.as_str(),
            None,
            &mutation,
            &RequestId::new(),
        );
        if let Err(error) = result {
            failures.push(format!("{mutation:?}: {error:?}"));
            continue;
        }
        assert_eq!(
            repository
                .show_issue(record.metadata.id.as_str())
                .unwrap()
                .metadata
                .custom,
            before.metadata.custom
        );
        if matches!(mutation, IssueMutation::Archive { .. }) {
            repository
                .mutate_issue(
                    record.metadata.id.as_str(),
                    None,
                    &IssueMutation::Reopen,
                    &RequestId::new(),
                )
                .unwrap();
        }
        assert!(
            repository
                .mutate_issue(
                    record.metadata.id.as_str(),
                    None,
                    &update(json!({"title":"Unrepaired content"})),
                    &RequestId::new()
                )
                .is_err()
        );
        assert!(
            !repository
                .completion_report(record.metadata.id.as_str())
                .unwrap()
                .allowed
        );
        repository
            .mutate_issue(
                record.metadata.id.as_str(),
                None,
                &IssueMutation::PatchCustom {
                    patch: CustomPatch {
                        set: BTreeMap::from([("risk".into(), json!("low"))]),
                        unset: vec![],
                    },
                },
                &RequestId::new(),
            )
            .unwrap();
        assert!(repository.organization_compliance().unwrap().compliant);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
