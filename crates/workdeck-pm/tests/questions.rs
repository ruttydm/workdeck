use workdeck_pm::*;

#[test]
fn malformed_question_is_independently_diagnosed() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    std::fs::create_dir(repo.root().join("questions")).unwrap();
    std::fs::write(repo.root().join("questions/bad.md"), "not a question").unwrap();
    assert!(
        !repo.doctor().unwrap().valid,
        "question authority cannot be invisible to doctor"
    );
}

use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier},
};
use workdeck_pm::transactions::{FaultPoint, TransactionStore};
fn setup() -> (tempfile::TempDir, Repository, IssueRecord, CreateQuestion) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let mut create = CreateIssue::new("Choose behavior", "Accepted scope\n");
    create.fields.insert(
        "acceptance".into(),
        json!([{"id":"behavior","description":"Preserve behavior","checked":true}]),
    );
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&create, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let criterion = repo
        .resolve_criterion(
            &CriterionOwner::Issue(issue.metadata.id.clone()),
            "behavior",
        )
        .unwrap()
        .reference;
    let input = CreateQuestion {
        actor: "agent-one".into(),
        body: "Which behavior applies?\n".into(),
        subjects: vec![QuestionSubject {
            subject: SubjectRef::Issue(issue.metadata.id.clone()),
            source: issue.source.clone(),
        }],
        requirements: vec![criterion],
        blocks_work: true,
        custom: BTreeMap::from([(
            "legacy".into(),
            json!({"unrecognized":[null,18446744073709551615u64]}),
        )]),
        extra: BTreeMap::from([("x-provider".into(), json!({"opaque":true}))]),
    };
    (temp, repo, issue, input)
}
fn create(repo: &Repository, input: &CreateQuestion) -> QuestionRecord {
    serde_json::from_value::<QuestionMutationResult>(
        repo.create_question(input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
    .question
}
fn answer() -> QuestionMutation {
    QuestionMutation::Answer {
        actor: "reviewer".into(),
        body: "Use the accepted behavior.\n".into(),
        decision_refs: vec![],
    }
}
fn update_title(repo: &Repository, issue: &IssueRecord) -> IssueRecord {
    serde_json::from_value(
        repo.mutate_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &IssueMutation::Update {
                input: UpdateIssue {
                    fields: BTreeMap::from([("title".into(), json!("Changed scope"))]),
                    body: None,
                },
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}
#[test]
fn question_history_preserves_unknown_metadata_and_issue_bytes_with_replay() {
    let (_temp, repo, issue, input) = setup();
    let question = create(&repo, &input);
    let item = std::fs::read(repo.root().join(&issue.path)).unwrap();
    let source = question
        .document
        .replacen("---\n", "---\nx-note: retained # keep comment\n", 1);
    std::fs::write(repo.root().join(&question.path), source).unwrap();
    let question = repo.question(&question.metadata.id).unwrap();
    let request = RequestId::new();
    let receipt = repo
        .mutate_question(&question.metadata.id, &question.source, &answer(), &request)
        .unwrap();
    let next = repo.question(&question.metadata.id).unwrap();
    assert!(next.document.contains("# keep comment"));
    assert_eq!(next.metadata.custom, input.custom);
    assert_eq!(next.metadata.extra["x-provider"], input.extra["x-provider"]);
    assert_eq!(next.body, input.body);
    assert_eq!(std::fs::read(repo.root().join(&issue.path)).unwrap(), item);
    assert_eq!(
        next.metadata.revision,
        question.metadata.revision.next().unwrap()
    );
    update_title(&repo, &issue);
    assert_eq!(
        repo.mutate_question(&question.metadata.id, &question.source, &answer(), &request)
            .unwrap(),
        receipt
    );
    assert_eq!(
        repo.mutate_question(
            &question.metadata.id,
            &question.source,
            &answer(),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
}
#[test]
fn blocking_questions_are_advisory_and_stale_answers_require_explicit_supersession() {
    let (_temp, repo, issue, input) = setup();
    let old = create(&repo, &input);
    let status = repo.question_applicability(&old.metadata.id).unwrap();
    assert!(status.blocks_implementation && status.answer.allowed);
    assert!(
        repo.completion_report(issue.metadata.id.as_str())
            .unwrap()
            .allowed,
        "question does not add an implicit completion gate"
    );
    repo.mutate_question(&old.metadata.id, &old.source, &answer(), &RequestId::new())
        .unwrap();
    let old = repo.question(&old.metadata.id).unwrap();
    assert!(
        !repo
            .question_applicability(&old.metadata.id)
            .unwrap()
            .blocks_implementation
    );
    let issue = update_title(&repo, &issue);
    let status = repo.question_applicability(&old.metadata.id).unwrap();
    assert_eq!(status.state, QuestionState::Answered);
    assert!(status.blocks_implementation && !status.answer.allowed && status.supersede.allowed);
    assert!(
        status
            .stale_reasons
            .iter()
            .any(|r| r.code == "subject_source_changed")
    );
    let mut replacement = input.clone();
    replacement.subjects[0].source = issue.source;
    let replacement = create(&repo, &replacement);
    repo.mutate_question(
        &old.metadata.id,
        &old.source,
        &QuestionMutation::Supersede {
            actor: "reviewer".into(),
            reason: "Reconsider current source".into(),
            replacement: replacement.metadata.id.clone(),
            replacement_source: replacement.source.clone(),
        },
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        !repo
            .question_applicability(&old.metadata.id)
            .unwrap()
            .blocks_implementation
    );
    repo.mutate_question(
        &replacement.metadata.id,
        &replacement.source,
        &answer(),
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        repo.doctor().unwrap().valid,
        "replacement source pin remains historical after its answer changes bytes"
    );
}
#[test]
fn stale_open_subject_and_criterion_have_distinct_diagnostics_and_no_mutation() {
    let (_temp, repo, issue, input) = setup();
    let question = create(&repo, &input);
    let mut changed = issue.metadata.clone();
    changed.acceptance[0].description = "Changed definition".into();
    let text = format!(
        "---\n{}---\n{}",
        serde_yaml_ng::to_string(&changed).unwrap(),
        issue.body
    );
    std::fs::write(repo.root().join(&issue.path), text).unwrap();
    let before = std::fs::read(repo.root().join(&question.path)).unwrap();
    let status = repo.question_applicability(&question.metadata.id).unwrap();
    assert!(
        status
            .stale_reasons
            .iter()
            .any(|r| r.code == "criterion_definition_changed")
    );
    assert!(
        status
            .stale_reasons
            .iter()
            .any(|r| r.code == "subject_source_changed")
    );
    assert!(!status.answer.allowed);
    assert_eq!(
        repo.mutate_question(
            &question.metadata.id,
            &question.source,
            &answer(),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(
        std::fs::read(repo.root().join(&question.path)).unwrap(),
        before
    );
}
#[test]
fn independent_comments_do_not_change_question_basis() {
    let (_temp, repo, issue, input) = setup();
    let question = create(&repo, &input);
    repo.add_comment(
        issue.metadata.id.as_str(),
        &issue.source,
        "reader",
        "An annotation",
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.question_applicability(&question.metadata.id)
            .unwrap()
            .freshness,
        QuestionFreshness::Current
    );
}
#[test]
fn open_question_references_block_retirement_even_without_blocks_work() {
    let (_temp, repo, issue, mut input) = setup();
    input.blocks_work = false;
    let question = create(&repo, &input);
    let target = RetirementTarget::new(RetirementKind::Issue, issue.metadata.id.as_str()).unwrap();
    let plan = repo.retirement_preview(&target).unwrap();
    assert!(!plan.allowed);
    assert_eq!(plan.question_blockers.len(), 1);
    repo.mutate_question(
        &question.metadata.id,
        &question.source,
        &answer(),
        &RequestId::new(),
    )
    .unwrap();
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
    assert!(repo.question(&question.metadata.id).is_ok());
    assert!(repo.create_question(&input, &RequestId::new()).is_err());
    assert!(repo.doctor().unwrap().valid);
}
#[test]
fn answer_cas_competition_and_fault_recovery_preserve_single_history() {
    let (_temp, repo, _issue, input) = setup();
    let question = create(&repo, &input);
    let barrier = Arc::new(Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let repo = repo.clone();
            let q = question.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                b.wait();
                repo.mutate_question(&q.metadata.id, &q.source, &answer(), &RequestId::new())
            })
        })
        .collect::<Vec<_>>();
    let result = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(result.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        result
            .iter()
            .any(|r| r.as_ref().is_err_and(|e| e.code == ErrorCode::StaleSource))
    );
    let q = create(&repo, &input);
    let request = RequestId::new();
    assert!(
        repo.mutate_question_with_faults(&q.metadata.id, &q.source, &answer(), &request, |p| {
            if p == FaultPoint::AfterChange(0) {
                Err(PmError::new(ErrorCode::Io, "interrupted"))
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert_eq!(
        repo.question(&q.metadata.id).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    TransactionStore::open(repo.root())
        .unwrap()
        .recover()
        .unwrap();
    let receipt = repo
        .mutate_question(&q.metadata.id, &q.source, &answer(), &request)
        .unwrap();
    assert_eq!(
        repo.mutate_question(&q.metadata.id, &q.source, &answer(), &request)
            .unwrap(),
        receipt
    );
    assert_eq!(
        repo.question(&q.metadata.id).unwrap().metadata.revision,
        Revision::new(2).unwrap()
    );
}
#[test]
fn invalid_question_inputs_and_wrong_repository_do_not_publish() {
    let (_temp, repo, _issue, input) = setup();
    let before = repo.export_snapshot().unwrap();
    let mut bad = input.clone();
    bad.extra.insert("verified".into(), json!(true));
    assert!(repo.create_question(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.requirements[0].repository = RepositoryId::new();
    assert!(repo.create_question(&bad, &RequestId::new()).is_err());
    let mut bad = input.clone();
    bad.subjects[0].source.content = ContentHash::of(b"stale");
    assert_eq!(
        repo.create_question(&bad, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let mut bad = input.clone();
    bad.subjects[0].subject = bad.requirements[0].subject();
    assert!(repo.create_question(&bad, &RequestId::new()).is_err());
    assert_eq!(repo.export_snapshot().unwrap().files, before.files);
    assert!("Q-../../unsafe".parse::<QuestionId>().is_err());
    assert!("H-../../unsafe".parse::<HandoffId>().is_err());
}
#[test]
fn question_snapshot_restore_preserves_answer_and_original_retry() {
    let (_temp, repo, issue, input) = setup();
    let request = RequestId::new();
    let receipt = repo.create_question(&input, &request).unwrap();
    let q = serde_json::from_value::<QuestionMutationResult>(receipt.result.clone())
        .unwrap()
        .question;
    repo.mutate_question(&q.metadata.id, &q.source, &answer(), &RequestId::new())
        .unwrap();
    update_title(&repo, &issue);
    let snapshot = repo.export_snapshot().unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|f| f.kind == SnapshotKind::Question)
    );
    let dest = tempfile::tempdir().unwrap();
    let root = dest.path().join(".workdeck");
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new()).unwrap();
    let restored = Repository::open_source(&root).unwrap();
    assert_eq!(restored.create_question(&input, &request).unwrap(), receipt);
    assert!(restored.doctor().unwrap().valid);
    assert!(
        restored
            .question_applicability(&q.metadata.id)
            .unwrap()
            .blocks_implementation
    );
}
#[test]
fn question_schema_and_self_consistent_forged_receipt_are_rejected() {
    let (_temp, repo, _issue, input) = setup();
    let receipt = repo.create_question(&input, &RequestId::new()).unwrap();
    let q = serde_json::from_value::<QuestionMutationResult>(receipt.result.clone())
        .unwrap()
        .question;
    std::fs::write(
        repo.root().join(&q.path),
        q.document.replacen("schema: 1", "schema: 99", 1),
    )
    .unwrap();
    assert_eq!(
        repo.question(&q.metadata.id).unwrap_err().code,
        ErrorCode::UnsupportedSchema
    );
    std::fs::write(repo.root().join(&q.path), &q.document).unwrap();
    let mut forged = receipt.clone();
    let mut result: QuestionMutationResult = serde_json::from_value(forged.result.clone()).unwrap();
    result.question.body = "Forged body".into();
    result.question.document = result.question.document.replace(&q.body, "Forged body");
    result.question.source.content = ContentHash::of(result.question.document.as_bytes());
    forged.changed[0].after = Some(result.question.source.content.clone());
    forged.result = json!(result);
    std::fs::write(
        repo.root()
            .join("operations")
            .join(format!("{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&forged).unwrap(),
    )
    .unwrap();
    assert!(repo.create_question(&input, &receipt.request_id).is_err());
    assert!(repo.export_snapshot().is_err());
}
#[test]
fn supersession_requires_current_replacement_and_cannot_fork_or_cycle() {
    let (_temp, repo, _issue, input) = setup();
    let a = create(&repo, &input);
    let b = create(&repo, &input);
    let c = create(&repo, &input);
    let action = |q: &QuestionRecord| QuestionMutation::Supersede {
        actor: "reviewer".into(),
        reason: "Use current question".into(),
        replacement: q.metadata.id.clone(),
        replacement_source: q.source.clone(),
    };
    let mut wrong = b.clone();
    wrong.source.content = ContentHash::of(b"wrong");
    assert_eq!(
        repo.mutate_question(
            &a.metadata.id,
            &a.source,
            &action(&wrong),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    repo.mutate_question(&a.metadata.id, &a.source, &action(&b), &RequestId::new())
        .unwrap();
    assert!(
        repo.mutate_question(&c.metadata.id, &c.source, &action(&b), &RequestId::new())
            .is_err()
    );
    let a = repo.question(&a.metadata.id).unwrap();
    assert!(
        repo.mutate_question(&b.metadata.id, &b.source, &action(&a), &RequestId::new())
            .is_err()
    );
    assert_eq!(repo.question(&c.metadata.id).unwrap(), c);
}
#[test]
fn actor_registry_is_prospective_and_does_not_reinterpret_original_request() {
    let (_temp, repo, _issue, input) = setup();
    let request = RequestId::new();
    let receipt = repo.create_question(&input, &request).unwrap();
    let q = serde_json::from_value::<QuestionMutationResult>(receipt.result.clone())
        .unwrap()
        .question;
    repo.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    assert!(repo.create_question(&input, &RequestId::new()).is_err());
    assert!(
        repo.mutate_question(&q.metadata.id, &q.source, &answer(), &RequestId::new())
            .is_err()
    );
    assert_eq!(repo.create_question(&input, &request).unwrap(), receipt);
    repo.mutate_user(
        "reviewer",
        None,
        &UserMutation::Create {
            user: UserDefinition::new("Reviewer"),
        },
        &RequestId::new(),
    )
    .unwrap();
    repo.mutate_question(&q.metadata.id, &q.source, &answer(), &RequestId::new())
        .unwrap();
    assert!(repo.question(&q.metadata.id).is_ok());
}
#[test]
fn question_import_only_applies_semantic_transitions_preserving_authored_content() {
    let (_temp, repo, _issue, input) = setup();
    let q = create(&repo, &input);
    let snapshot = repo.export_snapshot().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let root = dest.path().join(".workdeck");
    for f in &snapshot.files {
        let path = root.join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &f.content).unwrap();
    }
    let source = Repository::open_source(&root).unwrap();
    let mut next = q.metadata.clone();
    next.state = QuestionState::Answered;
    next.revision = next.revision.next().unwrap();
    next.updated_at = chrono::Utc::now();
    next.answer = Some(RecordedAnswer {
        actor: "reviewer".into(),
        body: "Decision".into(),
        answered_at: next.updated_at,
        decision_refs: vec![],
    });
    let rendered = format!(
        "---\n{}---\n{}",
        serde_yaml_ng::to_string(&next).unwrap(),
        q.body
    );
    std::fs::write(root.join(&q.path), &rendered).unwrap();
    let changed = source.export_snapshot().unwrap();
    let plan = repo
        .preview_snapshot_import(&changed, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    let valid_snapshot = changed;
    let valid_plan = plan;
    std::fs::write(
        root.join(&q.path),
        rendered.replace(&q.body, "Silently rewritten question"),
    )
    .unwrap();
    let changed = source.export_snapshot().unwrap();
    let plan = repo
        .preview_snapshot_import(&changed, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(!plan.allowed);
    let request = RequestId::new();
    let receipt = repo
        .import_snapshot(
            &valid_snapshot,
            SnapshotImportMode::ReplaceMatching,
            Some(&valid_plan.fingerprint),
            &request,
        )
        .unwrap();
    assert_eq!(repo.question(&q.metadata.id).unwrap().metadata, next);
    assert_eq!(
        repo.import_snapshot(
            &valid_snapshot,
            SnapshotImportMode::ReplaceMatching,
            Some(&valid_plan.fingerprint),
            &request
        )
        .unwrap(),
        receipt
    );
    assert!(repo.doctor().unwrap().valid);
}
#[test]
fn decision_source_race_and_orphan_question_authority_are_rejected() {
    let (_temp, repo, _issue, input) = setup();
    let q = create(&repo, &input);
    let wiki = WriteWiki {
        path: "decision.md".into(),
        body: "Decision".into(),
        expected: None,
    };
    repo.write_wiki(&wiki, &RequestId::new()).unwrap();
    let document = repo.wiki_document("decision.md").unwrap();
    let mutation = QuestionMutation::Answer {
        actor: "reviewer".into(),
        body: "Recorded decision".into(),
        decision_refs: vec![SourcePin {
            path: document.path.clone(),
            content: document.content_hash,
        }],
    };
    let file = repo.root().join(&document.path);
    let error = repo
        .mutate_question_with_faults(
            &q.metadata.id,
            &q.source,
            &mutation,
            &RequestId::new(),
            |p| {
                if p == FaultPoint::BeforeJournal {
                    std::fs::write(&file, "Changed decision").unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(repo.question(&q.metadata.id).unwrap(), q);
    std::fs::remove_file(repo.root().join("config.yml")).unwrap();
    assert!(Repository::init(repo.root().parent().unwrap(), "WD").is_err());
}
#[test]
fn question_only_orphan_source_cannot_receive_a_new_repository_identity() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join(".workdeck/questions");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{}.md", QuestionId::new()));
    std::fs::write(&path, "retained authority without config").unwrap();
    assert!(Repository::init(temp.path(), "WD").is_err());
    assert!(!temp.path().join(".workdeck/config.yml").exists());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "retained authority without config"
    );
}

#[test]
fn project_questions_follow_issue_context_and_block_project_retirement() {
    let (_temp, repo, issue, mut input) = setup();
    let project: PlanningRecord = serde_json::from_value(
        repo.create_planning(
            PlanningKind::Project,
            &CreatePlanning {
                id: Some("project-one".into()),
                name: "Project".into(),
                body: String::new(),
                fields: BTreeMap::from([(
                    "exit_criteria".into(),
                    json!([{"id":"exit","description":"Accepted exit"}]),
                )]),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: BTreeMap::from([("project".into(), json!("project-one"))]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    input.subjects = vec![QuestionSubject {
        subject: SubjectRef::Project("project-one".into()),
        source: project.source,
    }];
    input.requirements = vec![
        repo.resolve_criterion(&CriterionOwner::Project("project-one".into()), "exit")
            .unwrap()
            .reference,
    ];
    let question = create(&repo, &input);
    let packet = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    assert!(packet.sections.iter().flat_map(|section| &section.entries).any(|entry| matches!(&entry.content, ContextContent::Question { record, .. } if record.metadata.id == question.metadata.id)));
    let target = RetirementTarget::new(RetirementKind::Project, "project-one").unwrap();
    let preview = repo.retirement_preview(&target).unwrap();
    assert!(
        preview
            .question_blockers
            .iter()
            .any(|blocker| blocker.question == question.metadata.id)
    );
}
