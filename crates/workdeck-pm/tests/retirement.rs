use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use workdeck_pm::transactions::FaultPoint;
use workdeck_pm::{
    AttachmentInput, CreateIssue, CreatePlanning, ErrorCode, IssueCollection, IssueMutation,
    IssueRecord, PlanningKind, PlanningMutation, PlanningRecord, Repository, RequestId,
    RetirementInput, RetirementKind, RetirementOutcome, RetirementTarget, SavePlanning,
    UpdateIssue,
};

fn fixture() -> (TempDir, Repository) {
    let root = TempDir::new().unwrap();
    let repository = Repository::init(root.path(), "WD").unwrap();
    (root, repository)
}

fn issue(repository: &Repository, fields: BTreeMap<String, Value>) -> IssueRecord {
    serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue {
                    title: "History stays readable".into(),
                    body: "Authored Markdown.\n".into(),
                    fields,
                },
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap()
}

fn planning(repository: &Repository, kind: PlanningKind, id: &str) -> PlanningRecord {
    serde_json::from_value(
        repository
            .create_planning(
                kind,
                &CreatePlanning {
                    id: Some(id.into()),
                    name: "Keep authored metadata".into(),
                    body: if kind == PlanningKind::Label {
                        String::new()
                    } else {
                        "Planning body.\n".into()
                    },
                    fields: BTreeMap::from([
                        ("custom".into(), json!({"origin":"test"})),
                        ("x-preserved".into(), json!([1, "two"])),
                    ]),
                },
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap()
}

fn target(kind: RetirementKind, id: &str) -> RetirementTarget {
    RetirementTarget::new(kind, id).unwrap()
}

fn source_snapshot(repository: &Repository) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap();
            if [".tmp", ".index", ".local"]
                .iter()
                .any(|local| relative == Path::new(local))
            {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(relative.into(), fs::read(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(repository.root(), repository.root(), &mut files);
    files
}

#[test]
fn retirement_rejects_oversized_history_metadata_before_archiving() {
    let (_root, repository) = fixture();
    let issue = issue(&repository, BTreeMap::new());
    fs::write(
        repository
            .root()
            .join(issue.path.parent().unwrap())
            .join("notes.md"),
        vec![b'x'; 2 * 1024 * 1024 + 1],
    )
    .unwrap();
    let before = source_snapshot(&repository);
    assert!(
        repository
            .retire_record(
                &RetirementInput::new(target_for_issue(&issue)),
                &RequestId::new(),
            )
            .is_err()
    );
    assert_eq!(source_snapshot(&repository), before);
}

#[test]
fn short_issue_retirement_replays_after_config_and_reference_membership_change() {
    let (_root, repository) = fixture();
    let issue = issue(&repository, BTreeMap::new());
    let reference = &issue.metadata.id.as_str()[..8];
    let preview = repository.retirement_preview_issue(reference).unwrap();
    assert_eq!(preview.target.id, issue.metadata.id.as_str());
    let request = RequestId::new();
    let receipt = repository
        .retire_issue(
            reference,
            Some(&issue.source),
            Some(&preview.fingerprint),
            &request,
        )
        .unwrap();
    let outcome: RetirementOutcome = serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(outcome.tombstone.target, target_for_issue(&issue));

    let mut config = repository.config().unwrap();
    config.prefix = "NEW".into();
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    let mut other = issue.metadata.clone();
    let mut second_id = other.id.to_string();
    let last = second_id.pop().unwrap();
    second_id.push(if last == '0' { '1' } else { '0' });
    other.id = second_id.parse().unwrap();
    let directory = repository.root().join(format!("issues/{second_id}"));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("item.md"),
        format!(
            "---\n{}---\nOther issue\n",
            serde_yaml_ng::to_string(&other).unwrap()
        ),
    )
    .unwrap();
    assert_eq!(
        repository
            .retirement_preview_issue(reference)
            .unwrap_err()
            .code,
        ErrorCode::AmbiguousReference
    );
    let before = source_snapshot(&repository);
    let replay = repository
        .retire_issue(
            reference,
            Some(&issue.source),
            Some(&preview.fingerprint),
            &request,
        )
        .unwrap();
    assert_eq!(replay.operation_id, receipt.operation_id);
    assert_eq!(replay.result, receipt.result);
    assert_eq!(source_snapshot(&repository), before);
    assert_eq!(
        repository
            .retire_issue(reference, None, None, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::AmbiguousReference
    );
    assert_eq!(source_snapshot(&repository), before);
}

#[test]
fn issue_retirement_reference_input_keeps_source_preconditions_inside_mutation() {
    let (_root, repository) = fixture();
    let issue = issue(&repository, BTreeMap::new());
    let reference = &issue.metadata.id.as_str()[..8];
    let preview = repository.retirement_preview_issue(reference).unwrap();
    repository
        .mutate_issue(
            &issue.metadata.id.to_string(),
            None,
            &IssueMutation::Update {
                input: UpdateIssue {
                    fields: BTreeMap::from([("title".into(), json!("Changed"))]),
                    ..Default::default()
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let before = source_snapshot(&repository);
    assert_eq!(
        repository
            .retire_issue(
                reference,
                Some(&issue.source),
                Some(&preview.fingerprint),
                &RequestId::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(source_snapshot(&repository), before);
}

#[test]
fn retirement_archives_all_four_kinds_and_preserves_authored_content() {
    let (_root, repository) = fixture();
    let issue = issue(
        &repository,
        BTreeMap::from([("custom".into(), json!({"keep":true}))]),
    );
    let mut targets = vec![target(RetirementKind::Issue, issue.metadata.id.as_str())];
    for kind in [
        PlanningKind::Project,
        PlanningKind::Cycle,
        PlanningKind::Label,
    ] {
        let record = planning(&repository, kind, "Legacy_ID");
        targets.push(target(kind.into(), &record.metadata.id));
    }
    for target in targets {
        let before = source_snapshot(&repository);
        let preview = repository.retirement_preview(&target).unwrap();
        assert!(preview.allowed);
        assert!(preview.blockers.is_empty());
        assert_eq!(source_snapshot(&repository), before);
        let request = RequestId::new();
        let input = RetirementInput {
            target: target.clone(),
            expected: Some(preview.source.clone()),
            expected_preview: Some(preview.fingerprint),
        };
        let receipt = repository.retire_record(&input, &request).unwrap();
        let outcome: RetirementOutcome = serde_json::from_value(receipt.result.clone()).unwrap();
        assert_eq!(outcome.tombstone.target, target);
        assert_eq!(outcome.tombstone.repository, *repository.identity());
        assert_eq!(outcome.tombstone.previous, preview.source);
        assert_eq!(
            outcome.tombstone.retained.revision,
            preview.source.revision.next().unwrap()
        );
        assert_eq!(receipt.changed.len(), 2);
        assert!(receipt.changed.iter().all(|change| change.after.is_some()));
        assert_eq!(
            repository.tombstone(&target).unwrap(),
            Some(outcome.tombstone.clone())
        );
        assert_eq!(repository.retire_record(&input, &request).unwrap(), receipt);
        match outcome.record {
            workdeck_pm::RetiredRecord::Feature(_) | workdeck_pm::RetiredRecord::Gate(_) => {
                panic!("fixture only creates issue/planning records")
            }
            workdeck_pm::RetiredRecord::Issue(record) => {
                assert!(record.metadata.archived);
                assert_eq!(record.body, issue.body);
                assert_eq!(record.metadata.custom, issue.metadata.custom);
                assert_eq!(record.metadata.created_at, issue.metadata.created_at);
                assert_eq!(
                    repository.show_issue(&target.id).unwrap().source,
                    record.source
                );
            }
            workdeck_pm::RetiredRecord::Planning(record) => {
                assert!(record.metadata.archived);
                assert_eq!(record.metadata.custom["origin"], "test");
                assert_eq!(record.metadata.extra["x-preserved"], json!([1, "two"]));
                assert_eq!(
                    repository
                        .planning_record(record.kind, &target.id)
                        .unwrap()
                        .metadata,
                    record.metadata
                );
            }
        }
        assert!(repository.doctor().unwrap().valid);
    }
}

#[test]
fn reference_blockers_include_archived_issues_and_have_exact_source_identity() {
    for (kind, field) in [
        (PlanningKind::Project, "project"),
        (PlanningKind::Cycle, "cycle"),
        (PlanningKind::Label, "labels"),
    ] {
        let (_root, repository) = fixture();
        let planning = planning(&repository, kind, "ref");
        let value = if kind == PlanningKind::Label {
            json!(["ref"])
        } else {
            json!("ref")
        };
        let issue = issue(&repository, BTreeMap::from([(field.into(), value)]));
        let archived: IssueRecord = serde_json::from_value(
            repository
                .mutate_issue(
                    issue.metadata.id.as_str(),
                    None,
                    &IssueMutation::Archive { archived: true },
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        let target = target(kind.into(), &planning.metadata.id);
        let before = source_snapshot(&repository);
        let preview = repository.retirement_preview(&target).unwrap();
        assert!(!preview.allowed);
        assert_eq!(preview.blockers.len(), 1);
        assert_eq!(preview.blockers[0].issue, issue.metadata.id);
        assert_eq!(preview.blockers[0].path, issue.path);
        assert_eq!(preview.blockers[0].source, archived.source);
        assert_eq!(preview.blockers[0].field, field);
        let error = repository
            .retire_record(&RetirementInput::new(target), &RequestId::new())
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PolicyBlocked);
        assert!(error.details.is_some());
        assert_eq!(source_snapshot(&repository), before);
    }
}

#[test]
fn stale_source_and_new_membership_invalidate_reviewed_preview_without_writes() {
    let (_root, repository) = fixture();
    let project = planning(&repository, PlanningKind::Project, "project");
    let target = target(RetirementKind::Project, &project.metadata.id);
    let preview = repository.retirement_preview(&target).unwrap();
    issue(
        &repository,
        BTreeMap::from([("project".into(), json!("project"))]),
    );
    let before = source_snapshot(&repository);
    let input = RetirementInput {
        target: target.clone(),
        expected: Some(preview.source),
        expected_preview: Some(preview.fingerprint),
    };
    assert_eq!(
        repository
            .retire_record(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(source_snapshot(&repository), before);
    let ordinary = issue(&repository, BTreeMap::new());
    let issue_target = target_for_issue(&ordinary);
    let preview = repository.retirement_preview(&issue_target).unwrap();
    repository
        .update_issue(
            ordinary.metadata.id.as_str(),
            &ordinary.source,
            &UpdateIssue {
                fields: BTreeMap::from([("title".into(), json!("Later edit"))]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    let before = source_snapshot(&repository);
    let input = RetirementInput {
        target: issue_target,
        expected: Some(preview.source),
        expected_preview: None,
    };
    assert_eq!(
        repository
            .retire_record(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(source_snapshot(&repository), before);
}

fn target_for_issue(issue: &IssueRecord) -> RetirementTarget {
    target(RetirementKind::Issue, issue.metadata.id.as_str())
}

#[test]
fn retirement_closes_issue_comment_attachment_and_restore_writers_but_keeps_reads() {
    let (_root, repository) = fixture();
    let issue = issue(&repository, BTreeMap::new());
    repository
        .add_comment(
            issue.metadata.id.as_str(),
            &issue.source,
            "reviewer",
            "Original comment",
            &RequestId::new(),
        )
        .unwrap();
    let attachment = AttachmentInput {
        name: "proof.bin".into(),
        content: vec![0, 255, 10],
        media_type: None,
        actor: "reviewer".into(),
    };
    let attached = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            None,
            &attachment,
            &RequestId::new(),
        )
        .unwrap();
    let target = target_for_issue(&issue);
    let input = RetirementInput::new(target.clone());
    let request = RequestId::new();
    let retired = repository.retire_record(&input, &request).unwrap();
    let before = source_snapshot(&repository);
    for mutation in [
        IssueMutation::Archive { archived: false },
        IssueMutation::Reopen,
        IssueMutation::Complete { manual: None },
        IssueMutation::Cancel,
        IssueMutation::Comment {
            author: "reviewer".into(),
            body: "Late comment".into(),
        },
        IssueMutation::Add {
            field: IssueCollection::Documents,
            value: json!("late.md"),
        },
        IssueMutation::Update {
            input: UpdateIssue {
                fields: BTreeMap::new(),
                body: Some("Changed".into()),
            },
        },
    ] {
        assert_eq!(
            repository
                .mutate_issue(&target.id, None, &mutation, &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::PolicyBlocked
        );
        assert_eq!(source_snapshot(&repository), before);
    }
    assert_eq!(
        repository
            .attach_issue(&target.id, None, &attachment, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(repository.retire_record(&input, &request).unwrap(), retired);
    assert_eq!(repository.comments(&target.id).unwrap().len(), 1);
    assert_eq!(
        repository
            .read_attachment(&target.id, attached.result["id"].as_str().unwrap())
            .unwrap(),
        attachment.content
    );
    assert_eq!(source_snapshot(&repository), before);
    assert!(repository.doctor().unwrap().valid);
}

#[test]
fn retired_planning_blocks_save_update_restore_and_new_issue_associations() {
    for (kind, field) in [
        (PlanningKind::Project, "project"),
        (PlanningKind::Cycle, "cycle"),
        (PlanningKind::Label, "labels"),
    ] {
        let (_root, repository) = fixture();
        let record = planning(&repository, kind, "Legacy_ID");
        let target = target(kind.into(), &record.metadata.id);
        repository
            .retire_record(&RetirementInput::new(target), &RequestId::new())
            .unwrap();
        let before = source_snapshot(&repository);
        for mutation in [
            PlanningMutation::Archive { archived: false },
            PlanningMutation::Update {
                fields: BTreeMap::from([("name".into(), json!("Changed"))]),
                body: None,
            },
        ] {
            assert_eq!(
                repository
                    .mutate_planning(kind, "Legacy_ID", None, &mutation, &RequestId::new())
                    .unwrap_err()
                    .code,
                ErrorCode::PolicyBlocked
            );
        }
        let save = SavePlanning {
            id: "Legacy_ID".into(),
            name: "Reused".into(),
            body: None,
            fields: BTreeMap::new(),
        };
        assert_eq!(
            repository
                .save_planning(kind, &save, None, &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::PolicyBlocked
        );
        let value = if kind == PlanningKind::Label {
            json!(["Legacy_ID"])
        } else {
            json!("Legacy_ID")
        };
        let input = CreateIssue {
            title: "New association".into(),
            body: String::new(),
            fields: BTreeMap::from([(field.into(), value)]),
        };
        assert_eq!(
            repository
                .create_issue(&input, &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::PolicyBlocked
        );
        assert_eq!(source_snapshot(&repository), before);
    }
}

#[test]
fn retired_label_remains_valid_when_other_aggregate_entries_change() {
    let (_root, repository) = fixture();
    let first = planning(&repository, PlanningKind::Label, "retired");
    planning(&repository, PlanningKind::Label, "active");
    let target = target(RetirementKind::Label, &first.metadata.id);
    repository
        .retire_record(&RetirementInput::new(target.clone()), &RequestId::new())
        .unwrap();
    let tombstone = repository.tombstone(&target).unwrap().unwrap();
    repository
        .mutate_planning(
            PlanningKind::Label,
            "active",
            None,
            &PlanningMutation::Update {
                fields: BTreeMap::from([("color".into(), json!("arbitrary-legacy-color"))]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(
        repository.tombstone(&target).unwrap(),
        Some(tombstone.clone())
    );
    let shown = repository
        .planning_record(PlanningKind::Label, "retired")
        .unwrap();
    assert_ne!(shown.source.content, tombstone.retained.content);
    assert_eq!(shown.source.revision, tombstone.retained.revision);
    assert!(repository.doctor().unwrap().valid);
}

#[test]
fn imported_identity_stays_reserved_after_direct_removal_of_marker_and_record() {
    let (_root, repository) = fixture();
    let path = repository.root().join("projects/Legacy_ID/item.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path,format!("---\nschema: 1\nid: Legacy_ID\nrevision: 1\nname: Imported legacy project\nstatus: active\narchived: false\nimported:\n  source: .agents/workdeck/projects.toml\n  format: legacy_toml\n  content: '{}'\n---\nHistorical body.\n","0".repeat(64))).unwrap();
    let target = target(RetirementKind::Project, "Legacy_ID");
    let original = repository
        .planning_record(PlanningKind::Project, "Legacy_ID")
        .unwrap();
    assert!(original.metadata.created_at.is_none());
    repository
        .retire_record(&RetirementInput::new(target.clone()), &RequestId::new())
        .unwrap();
    assert!(
        repository
            .planning_record(PlanningKind::Project, "Legacy_ID")
            .unwrap()
            .metadata
            .created_at
            .is_none()
    );
    fs::remove_file(path).unwrap();
    fs::remove_file(repository.root().join("tombstones/projects/Legacy_ID.yml")).unwrap();
    let before = source_snapshot(&repository);
    let input = CreatePlanning {
        id: Some("legacy_id".into()),
        name: "Must not reuse retired identity".into(),
        body: String::new(),
        fields: BTreeMap::new(),
    };
    assert!(
        repository
            .create_planning(PlanningKind::Project, &input, &RequestId::new())
            .is_err()
    );
    assert_eq!(source_snapshot(&repository), before);
    assert!(!repository.doctor().unwrap().valid);
}

#[test]
fn reversible_archive_does_not_create_a_tombstone_or_freeze_authored_work() {
    let (_root, repository) = fixture();
    let issue = issue(&repository, BTreeMap::new());
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Archive { archived: true },
            &RequestId::new(),
        )
        .unwrap();
    assert!(
        repository
            .tombstone(&target_for_issue(&issue))
            .unwrap()
            .is_none()
    );
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Archive { archived: false },
            &RequestId::new(),
        )
        .unwrap();
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Update {
                input: UpdateIssue {
                    fields: BTreeMap::new(),
                    body: Some("Still editable".into()),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
}

#[test]
fn existing_issue_updates_and_templates_cannot_add_retired_associations() {
    let (_root, repository) = fixture();
    let label = planning(&repository, PlanningKind::Label, "retired");
    repository
        .retire_record(
            &RetirementInput::new(target(RetirementKind::Label, &label.metadata.id)),
            &RequestId::new(),
        )
        .unwrap();
    let issue = issue(&repository, BTreeMap::new());
    fs::create_dir_all(repository.root().join("templates/issues")).unwrap();
    fs::write(repository.root().join("templates/issues/retired.md"), "---\nschema: 1\nid: retired\nname: Retired association\ndefaults:\n  labels: [retired]\n---\nTemplate body.\n").unwrap();
    let before = source_snapshot(&repository);
    for mutation in [
        IssueMutation::Add {
            field: IssueCollection::Labels,
            value: json!("retired"),
        },
        IssueMutation::Update {
            input: UpdateIssue {
                fields: BTreeMap::from([("labels".into(), json!(["RETIRED"]))]),
                body: None,
            },
        },
    ] {
        assert!(
            repository
                .mutate_issue(
                    issue.metadata.id.as_str(),
                    None,
                    &mutation,
                    &RequestId::new()
                )
                .is_err()
        );
        assert_eq!(source_snapshot(&repository), before);
    }
    let input = workdeck_pm::TemplateIssueInput {
        template: "retired".into(),
        title: "No stale template association".into(),
        body: None,
        fields: BTreeMap::new(),
    };
    assert_eq!(
        repository
            .create_issue_from_template(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(source_snapshot(&repository), before);
}

#[test]
fn direct_target_history_and_membership_races_before_journal_publish_preserve_external_edits() {
    for race in ["target", "history", "membership"] {
        let (_root, repository) = fixture();
        let original = issue(&repository, BTreeMap::new());
        let original_bytes = fs::read(repository.root().join(&original.path)).unwrap();
        let project = planning(&repository, PlanningKind::Project, "project");
        let retired_target = if race == "membership" {
            target(RetirementKind::Project, &project.metadata.id)
        } else {
            target_for_issue(&original)
        };
        let changed_path = if race == "target" {
            repository.root().join(&original.path)
        } else if race == "history" {
            repository
                .root()
                .join(format!("issues/{}/comments/late.md", original.metadata.id))
        } else {
            repository.root().join(format!(
                "issues/{}/item.md",
                workdeck_pm::IssueId::new("WD").unwrap()
            ))
        };
        let changed_bytes = if race == "membership" {
            let mut metadata = original.metadata.clone();
            metadata.id = changed_path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .parse()
                .unwrap();
            metadata.project = Some("project".into());
            format!(
                "---\n{}---\nNew external reference.\n",
                serde_yaml_ng::to_string(&metadata).unwrap()
            )
            .into_bytes()
        } else {
            b"external edit remains visible\n".to_vec()
        };
        let error = repository
            .retire_record_with_faults(
                &RetirementInput::new(retired_target),
                &RequestId::new(),
                |point| {
                    if point == FaultPoint::BeforeJournal {
                        fs::create_dir_all(changed_path.parent().unwrap()).unwrap();
                        fs::write(&changed_path, &changed_bytes).unwrap();
                    }
                    Ok(())
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleSource, "{race}: {error}");
        assert_eq!(fs::read(&changed_path).unwrap(), changed_bytes);
        if race != "target" {
            assert_eq!(
                fs::read(repository.root().join(&original.path)).unwrap(),
                original_bytes
            );
        }
        assert!(repository.pending_operations().unwrap().is_empty());
        assert!(!repository.root().join("tombstones").exists());
    }
}

#[test]
fn interrupted_archive_and_tombstone_publish_recovers_as_one_original_operation() {
    for interrupt in [
        FaultPoint::BeforeJournal,
        FaultPoint::AfterChange(0),
        FaultPoint::BeforeReceipt,
    ] {
        let (_root, repository) = fixture();
        let issue = issue(&repository, BTreeMap::new());
        let input = RetirementInput::new(target_for_issue(&issue));
        let request = RequestId::new();
        let before = source_snapshot(&repository);
        let error = repository
            .retire_record_with_faults(&input, &request, |point| {
                if point == interrupt {
                    Err(workdeck_pm::PmError::new(
                        ErrorCode::Canceled,
                        "injected retirement interruption",
                    ))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        if interrupt == FaultPoint::BeforeJournal {
            assert_eq!(error.code, ErrorCode::Canceled);
            assert_eq!(source_snapshot(&repository), before);
            assert!(repository.pending_operations().unwrap().is_empty());
        } else {
            assert_eq!(error.code, ErrorCode::RecoveryRequired);
            assert_eq!(
                repository
                    .show_issue(issue.metadata.id.as_str())
                    .unwrap_err()
                    .code,
                ErrorCode::RecoveryRequired
            );
            let recovered = repository.recover_operations().unwrap();
            assert_eq!(recovered.len(), 1);
            assert_eq!(
                repository.retire_record(&input, &request).unwrap(),
                recovered[0]
            );
            assert!(
                repository
                    .show_issue(issue.metadata.id.as_str())
                    .unwrap()
                    .retirement
                    .is_some()
            );
            assert!(repository.doctor().unwrap().valid);
        }
    }
}

#[test]
fn doctor_reports_forged_marker_source_history_and_orphan_retirement_records() {
    for damage in ["source", "history", "marker", "orphan"] {
        let (_root, repository) = fixture();
        let issue = issue(&repository, BTreeMap::new());
        let comment = repository
            .add_comment(
                issue.metadata.id.as_str(),
                &issue.source,
                "reviewer",
                "Original retained comment",
                &RequestId::new(),
            )
            .unwrap();
        let target = target_for_issue(&issue);
        repository
            .retire_record(&RetirementInput::new(target.clone()), &RequestId::new())
            .unwrap();
        let path = match damage {
            "source" => repository.root().join(&issue.path),
            "history" => repository
                .root()
                .join(comment.result["comment"]["path"].as_str().unwrap()),
            "marker" => repository
                .root()
                .join(format!("tombstones/issues/{}.yml", issue.metadata.id)),
            _ => repository.root().join("tombstones/future/orphan.yml"),
        };
        if damage == "source" || damage == "history" {
            let original = fs::read_to_string(&path).unwrap();
            fs::write(&path, format!("{original}\nUnauthorized history edit.\n")).unwrap();
        } else if damage == "marker" {
            let original = fs::read_to_string(&path).unwrap();
            fs::write(&path, original.replace("schema: 1", "schema: 999")).unwrap();
        } else {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "schema: 1\n").unwrap();
        }
        let report = repository.doctor().unwrap();
        assert!(!report.valid, "{damage}");
        assert!(!report.errors.is_empty());
        assert!(
            report
                .errors
                .iter()
                .any(
                    |error| error.path.as_ref().is_some_and(|value| value.contains(
                        if damage == "source" || damage == "history" {
                            "issues/"
                        } else {
                            "tombstones/"
                        }
                    ))
                )
        );
        if damage != "orphan" {
            assert!(repository.tombstone(&target).is_err());
        }
    }
}

#[test]
fn retired_metadata_inspection_does_not_read_or_qualify_attachment_payloads() {
    let (_root, repository) = fixture();
    let issue = issue(&repository, BTreeMap::new());
    let attachment = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            None,
            &AttachmentInput {
                name: "payload.bin".into(),
                content: vec![1, 2, 3],
                media_type: None,
                actor: "reviewer".into(),
            },
            &RequestId::new(),
        )
        .unwrap();
    let target = target_for_issue(&issue);
    repository
        .retire_record(&RetirementInput::new(target.clone()), &RequestId::new())
        .unwrap();
    let payload = repository
        .root()
        .join(attachment.result["content_path"].as_str().unwrap());
    fs::OpenOptions::new()
        .write(true)
        .open(payload)
        .unwrap()
        .set_len(21 * 1024 * 1024)
        .unwrap();
    assert!(repository.tombstone(&target).unwrap().is_some());
    assert!(
        repository
            .show_issue(&target.id)
            .unwrap()
            .retirement
            .is_some()
    );
    assert_eq!(repository.list_attachments(&target.id).unwrap().len(), 1);
    assert!(repository.doctor().unwrap().valid);
    assert!(
        repository
            .read_attachment(&target.id, attachment.result["id"].as_str().unwrap())
            .is_err()
    );
}

#[test]
fn retirement_receipt_stages_only_archive_marker_and_receipt_with_unrelated_index_preserved() {
    let root = TempDir::new().unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    git(&["init", "-q"]);
    let repository = Repository::init(root.path(), "WD").unwrap();
    let issue = issue(&repository, BTreeMap::new());
    fs::write(root.path().join("unrelated.txt"), "already staged\n").unwrap();
    git(&["add", "--", "unrelated.txt"]);
    fs::write(root.path().join("unrelated.txt"), "later working copy\n").unwrap();
    let receipt = repository
        .retire_record(
            &RetirementInput::new(target_for_issue(&issue)),
            &RequestId::new(),
        )
        .unwrap();
    let staged = repository.stage_operation(&receipt).unwrap();
    assert_eq!(staged.paths.len(), 3);
    assert_eq!(git(&["show", ":unrelated.txt"]), b"already staged\n");
    assert_eq!(
        fs::read(root.path().join("unrelated.txt")).unwrap(),
        b"later working copy\n"
    );
    assert!(!repository.stage_operation(&receipt).unwrap().index_changed);
}

#[test]
fn retirement_receipt_result_cannot_claim_a_different_retained_record() {
    let (_root, repository) = fixture();
    let issue = issue(&repository, BTreeMap::new());
    let target = target_for_issue(&issue);
    let input = RetirementInput::new(target.clone());
    let request = RequestId::new();
    let mut receipt = repository.retire_record(&input, &request).unwrap();
    receipt.result["record"]["record"]["metadata"]["title"] = json!("Forged retained subject");
    fs::write(
        repository
            .root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&receipt).unwrap(),
    )
    .unwrap();
    assert!(repository.tombstone(&target).is_err());
    assert!(repository.retire_record(&input, &request).is_err());
    assert!(!repository.doctor().unwrap().valid);
}

#[test]
fn earlier_comment_and_attachment_requests_replay_without_reopening_retired_history() {
    let (_root, repository) = fixture();
    let issue = issue(&repository, BTreeMap::new());
    let comment_request = RequestId::new();
    let comment = repository
        .add_comment(
            issue.metadata.id.as_str(),
            &issue.source,
            "reviewer",
            "Retained comment",
            &comment_request,
        )
        .unwrap();
    let attachment_request = RequestId::new();
    let attachment = AttachmentInput {
        name: "retained.bin".into(),
        content: vec![0, 1, 2],
        media_type: None,
        actor: "reviewer".into(),
    };
    let attached = repository
        .attach_issue(
            issue.metadata.id.as_str(),
            None,
            &attachment,
            &attachment_request,
        )
        .unwrap();
    let target = target_for_issue(&issue);
    repository
        .retire_record(&RetirementInput::new(target.clone()), &RequestId::new())
        .unwrap();
    let before = source_snapshot(&repository);
    assert_eq!(
        repository
            .add_comment(
                &target.id,
                &issue.source,
                "reviewer",
                "Retained comment",
                &comment_request
            )
            .unwrap(),
        comment
    );
    assert_eq!(
        repository
            .attach_issue(&target.id, None, &attachment, &attachment_request)
            .unwrap(),
        attached
    );
    assert_eq!(source_snapshot(&repository), before);
    assert!(!repository.completion_report(&target.id).unwrap().allowed);
}
