use serde_json::json;
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;
use workdeck_pm::{CreateIssue, ErrorCode, IssueRecord, Repository, RequestId, UpdateIssue};

fn setup() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    (temp, repo)
}
fn create(repo: &Repository, title: &str) -> IssueRecord {
    serde_json::from_value(
        repo.create_issue(&CreateIssue::new(title, ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}

fn declare_gate_directly(repo: &Repository, issue: &IssueRecord, gate: &workdeck_pm::GateId) {
    let path = repo.root().join(&issue.path);
    let body = fs::read_to_string(&path).unwrap();
    fs::write(
        path,
        body.replacen("\n---\n", &format!("\ngates:\n- {gate}\n---\n"), 1),
    )
    .unwrap();
}

#[test]
fn required_gate_debt_blocks_completed_prerequisites_and_children_without_rewriting_them() {
    for child in [false, true] {
        let (_temp, repo) = setup();
        let a = create(&repo, "Dependent or parent");
        let b = create(&repo, "Required issue");
        if child {
            edit(&repo, &b, json!({"parent":a.metadata.id})).unwrap();
        } else {
            edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
        }
        let b = repo.show_issue(b.metadata.id.as_str()).unwrap();
        repo.complete_issue(b.metadata.id.as_str(), &b.source, None, &RequestId::new())
            .unwrap();
        let gate = workdeck_pm::GateId::new();
        declare_gate_directly(&repo, &b, &gate);
        assert!(
            !repo
                .completion_report(b.metadata.id.as_str())
                .unwrap()
                .allowed
        );
        let before_a = fs::read(repo.root().join(&a.path)).unwrap();
        let before_b = fs::read(repo.root().join(&b.path)).unwrap();
        let report = repo.completion_report(a.metadata.id.as_str()).unwrap();
        assert!(
            !report.allowed,
            "a completed requirement cannot hide its unresolved gate"
        );
        assert!(report.conditions.iter().any(|condition| {
            condition.reason_code == "gate_unresolved"
                && condition.state == ConditionState::Unknown
                && condition.path
                    == vec![
                        workdeck_pm::SubjectRef::Issue(a.metadata.id.clone()),
                        workdeck_pm::SubjectRef::Issue(b.metadata.id.clone()),
                        workdeck_pm::SubjectRef::Gate(gate.clone()),
                    ]
        }));
        assert_eq!(
            repo.issue_readiness(a.metadata.id.as_str()).unwrap().ready,
            child,
            "readiness depends on hard prerequisites; child completion is a separate contract"
        );
        let current = repo.show_issue(a.metadata.id.as_str()).unwrap();
        assert_eq!(
            repo.complete_issue(
                a.metadata.id.as_str(),
                &current.source,
                None,
                &RequestId::new()
            )
            .unwrap_err()
            .code,
            ErrorCode::PolicyBlocked
        );
        assert_eq!(fs::read(repo.root().join(&a.path)).unwrap(), before_a);
        assert_eq!(fs::read(repo.root().join(&b.path)).unwrap(), before_b);
    }
}

#[test]
fn gate_and_criterion_sources_change_graph_identity_while_captured_queries_remain_immutable() {
    use workdeck_pm::*;
    let (_temp, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    repo.complete_issue(b.metadata.id.as_str(), &b.source, None, &RequestId::new())
        .unwrap();
    let mut feature = CreateFeature::new("Capability");
    feature.fields.insert(
        "criteria".into(),
        json!([{"id":"works","description":"Behavior works"}]),
    );
    let feature: FeatureOutcome = serde_json::from_value(
        repo.create_feature(&feature, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let criterion = repo
        .resolve_criterion(
            &CriterionOwner::Feature(feature.record.metadata.id.clone()),
            "works",
        )
        .unwrap()
        .reference;
    let gate = CreateGate {
        name: "Required gate".into(),
        description: String::new(),
        requirements: vec![GateRequirement {
            id: "works".into(),
            criterion,
            producer: ProducerRef {
                id: "runner".into(),
                definition: ContentHash::of(b"runner"),
            },
            check: CheckRef {
                id: "check".into(),
                definition: ContentHash::of(b"check"),
            },
            max_age_seconds: None,
        }],
        custom: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let gate: GateMutationResult =
        serde_json::from_value(repo.create_gate(&gate, &RequestId::new()).unwrap().result).unwrap();
    declare_gate_directly(&repo, &b, &gate.gate.definition.id);
    let graph = repo.issue_graph_snapshot().unwrap();
    let readiness = graph.readiness(&a.metadata.id).unwrap();
    assert!(!readiness.ready);
    assert!(
        readiness
            .conditions
            .iter()
            .flat_map(|c| &c.source_pins)
            .any(|pin| pin.path == feature.record.path
                && pin.content == feature.record.source.content)
    );
    let gate_path = repo.root().join(&gate.gate.path);
    fs::write(
        &gate_path,
        format!("{}\n# direct gate edit\n", gate.gate.document),
    )
    .unwrap();
    let changed_gate = repo.issue_graph_snapshot().unwrap();
    assert_ne!(graph.fingerprint(), changed_gate.fingerprint());
    assert_eq!(graph.readiness(&a.metadata.id).unwrap(), readiness);
    assert_eq!(
        repo.mutate_issue_graph(
            a.metadata.id.as_str(),
            None,
            Some(graph.fingerprint()),
            &GraphMutation::RemovePrerequisite {
                prerequisite: b.metadata.id.to_string(),
                reason: "Reviewed against the earlier gate definition".into()
            },
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource,
    );
    assert_eq!(
        repo.show_issue(a.metadata.id.as_str())
            .unwrap()
            .metadata
            .prerequisites,
        vec![b.metadata.id.clone()]
    );
    let feature_path = repo.root().join(&feature.record.path);
    let bytes = fs::read_to_string(&feature_path).unwrap();
    fs::write(
        &feature_path,
        bytes.replace("Behavior works", "Behavior now differs"),
    )
    .unwrap();
    let changed_criterion = repo.issue_graph_snapshot().unwrap();
    assert_ne!(changed_gate.fingerprint(), changed_criterion.fingerprint());
    assert!(
        changed_criterion
            .readiness(&a.metadata.id)
            .unwrap()
            .conditions
            .iter()
            .any(|c| c.reason_code == "criterion_definition_changed")
    );
    assert_eq!(graph.readiness(&a.metadata.id).unwrap(), readiness);
    let other = create(&repo, "Related issue");
    let original_gate = fs::read_to_string(&gate_path).unwrap();
    assert_eq!(
        repo.mutate_issue_graph_with_faults(
            a.metadata.id.as_str(),
            None,
            None,
            &GraphMutation::SetRelated {
                other: other.metadata.id.to_string(),
                related: true
            },
            &RequestId::new(),
            |point| {
                if point == workdeck_pm::transactions::FaultPoint::BeforeJournal {
                    fs::write(
                        &gate_path,
                        format!("{original_gate}\n# changed during prepare\n"),
                    )
                    .unwrap();
                }
                Ok(())
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert!(
        repo.issue_relations(a.metadata.id.as_str())
            .unwrap()
            .related
            .is_empty()
    );
    let renamed_root = repo.root().with_extension("removed-after-capture");
    fs::rename(repo.root(), &renamed_root).unwrap();
    assert_eq!(
        graph.readiness(&a.metadata.id).unwrap(),
        readiness,
        "captured queries must not perform later filesystem reads"
    );
}

#[test]
fn explicit_prerequisite_waiver_excludes_required_gate_debt() {
    let (_temp, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    declare_gate_directly(&repo, &b, &workdeck_pm::GateId::new());
    let path = repo.root().join("config.yml");
    let mut config: workdeck_pm::Config =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    config.acceptance.allow_prerequisite_waivers = true;
    fs::write(&path, serde_yaml_ng::to_string(&config).unwrap()).unwrap();
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::WaivePrerequisite {
            prerequisite: b.metadata.id.to_string(),
            actor: "human".into(),
            reason: "Explicitly accept this deferred requirement".into(),
        },
    )
    .unwrap();
    let graph = repo.issue_graph_snapshot().unwrap();
    let readiness = graph.readiness(&a.metadata.id).unwrap();
    assert!(readiness.ready);
    assert!(
        readiness
            .conditions
            .iter()
            .all(|c| c.reason_code != "gate_unresolved")
    );
    assert!(
        repo.completion_report(a.metadata.id.as_str())
            .unwrap()
            .allowed
    );
}

#[test]
fn dependency_path_retains_reachable_missing_references_when_no_path_is_known() {
    let (_temp, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let path = repo.root().join(&a.path);
    let bytes = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        bytes.replacen("\n---\n", "\nprerequisites:\n- WD-999\n---\n", 1),
    )
    .unwrap();
    let result = repo
        .issue_dependency_path(a.metadata.id.as_str(), b.metadata.id.as_str())
        .unwrap();
    assert!(!result.found);
    let result = serde_json::to_value(result).unwrap();
    assert!(
        result["conditions"]
            .as_array()
            .is_some_and(|conditions| conditions
                .iter()
                .any(|c| c["reason_code"] == "missing_prerequisite")),
        "absence of a known path must retain reachable unresolved references"
    );
    fs::write(
        &path,
        bytes.replacen(
            "\n---\n",
            &format!("\nprerequisites:\n- {}\n- WD-999\n---\n", b.metadata.id),
            1,
        ),
    )
    .unwrap();
    let known = repo
        .issue_dependency_path(a.metadata.id.as_str(), b.metadata.id.as_str())
        .unwrap();
    assert!(known.found);
    assert!(
        known
            .conditions
            .iter()
            .any(|c| c.reason_code == "missing_prerequisite"),
        "a known path does not hide another unresolved reachable branch"
    );
}

#[test]
fn gate_capture_rejects_unbounded_distinct_references_before_evaluation() {
    let (_temp, repo) = setup();
    let issues = (0..17)
        .map(|i| create(&repo, &format!("Issue {i}")))
        .collect::<Vec<_>>();
    for issue in issues {
        let path = repo.root().join(&issue.path);
        let bytes = fs::read_to_string(&path).unwrap();
        let gates = (0..256)
            .map(|_| format!("- {}\n", workdeck_pm::GateId::new()))
            .collect::<String>();
        fs::write(
            path,
            bytes.replacen("\n---\n", &format!("\ngates:\n{gates}---\n"), 1),
        )
        .unwrap();
    }
    assert_eq!(
        repo.issue_graph_snapshot().unwrap_err().code,
        ErrorCode::Unsupported
    );
}
fn edit(
    repo: &Repository,
    issue: &IssueRecord,
    fields: serde_json::Value,
) -> workdeck_pm::Result<IssueRecord> {
    let fields: BTreeMap<_, _> = serde_json::from_value(fields).unwrap();
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue { fields, body: None },
        &RequestId::new(),
    )
    .map(|r| serde_json::from_value(r.result).unwrap())
}
#[test]
fn prerequisites_survive_cancellation_and_block_every_completion_route() {
    let (_t, repo) = setup();
    let a = create(&repo, "Dependent");
    let b = create(&repo, "Required");
    let a = edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    repo.mutate_issue(
        b.metadata.id.as_str(),
        Some(&b.source),
        &workdeck_pm::IssueMutation::Cancel,
        &RequestId::new(),
    )
    .unwrap();
    let report = repo.completion_report(a.metadata.id.as_str()).unwrap();
    assert!(!report.allowed);
    assert!(report.reasons.iter().any(|r| r.contains("canceled")));
    let before = fs::read(repo.root().join(&a.path)).unwrap();
    assert_eq!(
        edit(&repo, &a, json!({"status":"done"})).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(fs::read(repo.root().join(&a.path)).unwrap(), before);
    assert_eq!(
        serde_json::to_value(repo.show_issue(a.metadata.id.as_str()).unwrap().metadata).unwrap()["prerequisites"],
        json!([b.metadata.id])
    );
}
#[test]
fn parent_children_and_mixed_cycles_are_real_completion_constraints() {
    let (_t, repo) = setup();
    let parent = create(&repo, "Parent");
    let child = create(&repo, "Child");
    let child = edit(&repo, &child, json!({"parent":parent.metadata.id})).unwrap();
    assert!(
        !repo
            .completion_report(parent.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    let before = fs::read(repo.root().join(&child.path)).unwrap();
    assert_eq!(
        edit(&repo, &child, json!({"prerequisites":[parent.metadata.id]}))
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(fs::read(repo.root().join(&child.path)).unwrap(), before);
    assert_eq!(
        edit(&repo, &parent, json!({"parent":child.metadata.id}))
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
}
#[test]
fn completed_parent_cannot_gain_a_requirement_through_another_issue() {
    let (_t, repo) = setup();
    let parent = create(&repo, "Parent");
    let child = create(&repo, "Child");
    repo.complete_issue(
        parent.metadata.id.as_str(),
        &parent.source,
        None,
        &RequestId::new(),
    )
    .unwrap();
    let before = fs::read(repo.root().join(&child.path)).unwrap();
    assert_eq!(
        edit(&repo, &child, json!({"parent":parent.metadata.id}))
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(fs::read(repo.root().join(&child.path)).unwrap(), before);
}

use workdeck_pm::{ConditionState, IssueGraphMutation as GraphMutation};
fn graph_mutate(
    repo: &Repository,
    issue: &IssueRecord,
    mutation: &GraphMutation,
) -> workdeck_pm::Result<workdeck_pm::transactions::MutationReceipt> {
    repo.mutate_issue_graph(
        issue.metadata.id.as_str(),
        None,
        None,
        mutation,
        &RequestId::new(),
    )
}
#[test]
fn symmetric_related_links_have_one_source_and_leave_accepted_items_unchanged() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    repo.complete_issue(a.metadata.id.as_str(), &a.source, None, &RequestId::new())
        .unwrap();
    let before_a = repo.show_issue(a.metadata.id.as_str()).unwrap();
    let before_b = repo.show_issue(b.metadata.id.as_str()).unwrap();
    let first = graph_mutate(
        &repo,
        &a,
        &GraphMutation::SetRelated {
            other: b.metadata.id.to_string(),
            related: true,
        },
    )
    .unwrap();
    assert_eq!(first.changed.len(), 1);
    assert!(first.changed[0].path.starts_with("relations/issues"));
    let repeated = graph_mutate(
        &repo,
        &b,
        &GraphMutation::SetRelated {
            other: a.metadata.id.to_string(),
            related: true,
        },
    )
    .unwrap();
    assert!(repeated.changed.is_empty());
    assert_eq!(
        repo.issue_relations(a.metadata.id.as_str())
            .unwrap()
            .related,
        vec![b.metadata.id.clone()]
    );
    assert_eq!(
        repo.issue_relations(b.metadata.id.as_str())
            .unwrap()
            .related,
        vec![a.metadata.id.clone()]
    );
    assert_eq!(repo.show_issue(a.metadata.id.as_str()).unwrap(), before_a);
    assert_eq!(repo.show_issue(b.metadata.id.as_str()).unwrap(), before_b);
    assert!(repo.issue_readiness(b.metadata.id.as_str()).unwrap().ready);
    graph_mutate(
        &repo,
        &b,
        &GraphMutation::SetRelated {
            other: a.metadata.id.to_string(),
            related: false,
        },
    )
    .unwrap();
    assert!(
        repo.issue_relations(a.metadata.id.as_str())
            .unwrap()
            .related
            .is_empty()
    );
}
#[test]
fn waivers_are_explicit_policy_decisions_bound_to_requirement_and_prerequisite_source() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let a = edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    let waive = GraphMutation::WaivePrerequisite {
        prerequisite: b.metadata.id.to_string(),
        actor: "human".into(),
        reason: "Requirement consciously deferred".into(),
    };
    assert_eq!(
        graph_mutate(&repo, &a, &waive).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
    let path = repo.root().join("config.yml");
    let mut config: workdeck_pm::Config =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    config.acceptance.allow_prerequisite_waivers = true;
    fs::write(&path, serde_yaml_ng::to_string(&config).unwrap()).unwrap();
    graph_mutate(&repo, &a, &waive).unwrap();
    let report = repo.completion_report(a.metadata.id.as_str()).unwrap();
    assert!(report.allowed);
    assert!(report.conditions.iter().any(|c| c.basis == "policy_waiver"));
    assert_eq!(
        repo.show_issue(a.metadata.id.as_str()).unwrap().source,
        a.source
    );
    let b = edit(&repo, &b, json!({"title":"Changed subject"})).unwrap();
    assert!(
        !repo
            .completion_report(a.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    graph_mutate(&repo, &a, &waive).unwrap();
    repo.complete_issue(a.metadata.id.as_str(), &a.source, None, &RequestId::new())
        .unwrap();
    assert!(
        repo.completion_report(a.metadata.id.as_str())
            .unwrap()
            .allowed,
        "completing dependent must not stale its waiver"
    );
    assert_ne!(b.source.content, a.source.content);
}
#[test]
fn prerequisite_replacement_invalidates_waiver_atomically_and_recovery_replays_original_intent() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let c = create(&repo, "C");
    let a = edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    let path = repo.root().join("config.yml");
    let mut config: workdeck_pm::Config =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    config.acceptance.allow_prerequisite_waivers = true;
    fs::write(path, serde_yaml_ng::to_string(&config).unwrap()).unwrap();
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::WaivePrerequisite {
            prerequisite: b.metadata.id.to_string(),
            actor: "human".into(),
            reason: "Temporary exception".into(),
        },
    )
    .unwrap();
    let input = GraphMutation::ReplacePrerequisite {
        prerequisite: b.metadata.id.to_string(),
        replacement: c.metadata.id.to_string(),
        reason: "New requirement".into(),
    };
    let request = RequestId::new();
    let error = repo
        .mutate_issue_graph_with_faults(a.metadata.id.as_str(), None, None, &input, &request, |p| {
            if p == workdeck_pm::transactions::FaultPoint::AfterChange(0) {
                Err(workdeck_pm::PmError::new(ErrorCode::Io, "injected"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    assert_eq!(
        repo.issue_graph_snapshot().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    repo.recover_operations().unwrap();
    let receipt = repo
        .mutate_issue_graph(a.metadata.id.as_str(), None, None, &input, &request)
        .unwrap();
    assert_eq!(receipt.changed.len(), 2);
    assert_eq!(
        repo.show_issue(a.metadata.id.as_str())
            .unwrap()
            .metadata
            .prerequisites,
        vec![c.metadata.id]
    );
    assert!(repo.issue_graph_snapshot().unwrap().waivers().is_empty());
    let current = repo.show_issue(a.metadata.id.as_str()).unwrap();
    edit(&repo, &current, json!({"title":"Later edit"})).unwrap();
    assert_eq!(
        repo.mutate_issue_graph(a.metadata.id.as_str(), None, None, &input, &request)
            .unwrap(),
        receipt
    );
}
#[test]
fn graph_identity_detects_hidden_prerequisite_changes_and_retirement_never_removes_edges() {
    let (_t, repo) = setup();
    let a = create(&repo, "Visible");
    let b = create(&repo, "Hidden");
    let a = edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    let b = edit(&repo, &b, json!({"archived":true})).unwrap();
    let graph = repo.issue_graph_snapshot().unwrap();
    let readiness = graph.readiness(&a.metadata.id).unwrap();
    assert!(!readiness.ready);
    assert!(
        readiness
            .conditions
            .iter()
            .any(|c| c.related_subject
                == Some(workdeck_pm::SubjectRef::Issue(b.metadata.id.clone())))
    );
    assert!(
        !repo
            .retirement_preview_issue(b.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    edit(&repo, &b, json!({"title":"Changed hidden requirement"})).unwrap();
    let mutation = GraphMutation::RemovePrerequisite {
        prerequisite: b.metadata.id.to_string(),
        reason: "Reviewed removal".into(),
    };
    assert_eq!(
        repo.mutate_issue_graph(
            a.metadata.id.as_str(),
            None,
            Some(graph.fingerprint()),
            &mutation,
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repo.show_issue(a.metadata.id.as_str())
            .unwrap()
            .metadata
            .prerequisites,
        a.metadata.prerequisites
    );
}
#[test]
fn concurrent_opposite_edges_cannot_both_commit() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let root = repo.root().to_owned();
    let handles = [
        (a.metadata.id.clone(), b.metadata.id.clone()),
        (b.metadata.id.clone(), a.metadata.id.clone()),
    ]
    .into_iter()
    .map(|(a, b)| {
        let barrier = barrier.clone();
        let root = root.clone();
        std::thread::spawn(move || {
            let repo = Repository::open_source(&root).unwrap();
            barrier.wait();
            repo.mutate_issue_graph(
                a.as_str(),
                None,
                None,
                &GraphMutation::AddPrerequisite {
                    prerequisite: b.to_string(),
                },
                &RequestId::new(),
            )
        })
    })
    .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| r
                .as_ref()
                .is_err_and(|e| e.code == ErrorCode::PolicyBlocked))
            .count(),
        1
    );
    assert!(repo.doctor().unwrap().valid);
}
#[test]
fn dangling_direct_edits_are_inspectable_but_never_ready_or_clean() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let path = repo.root().join(&a.path);
    let text = fs::read_to_string(&path).unwrap().replacen(
        "\n---\n",
        "\nprerequisites: [WD-999]\n---\n",
        1,
    );
    fs::write(path, text).unwrap();
    let report = repo.issue_readiness(a.metadata.id.as_str()).unwrap();
    assert!(!report.ready);
    assert!(
        report
            .conditions
            .iter()
            .any(|c| c.state == ConditionState::Unknown)
    );
    assert!(!repo.doctor().unwrap().valid);
    let current = repo.show_issue(a.metadata.id.as_str()).unwrap();
    graph_mutate(
        &repo,
        &current,
        &GraphMutation::RemovePrerequisite {
            prerequisite: "WD-999".into(),
            reason: "Resolve stale historical reference".into(),
        },
    )
    .unwrap();
    assert!(repo.issue_readiness(a.metadata.id.as_str()).unwrap().ready);
}

#[test]
fn prerequisite_read_set_direct_edit_prevents_publication_and_replay_rejects_changed_intent() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let path = repo.root().join(&b.path);
    let before = fs::read(&path).unwrap();
    let edited = String::from_utf8(before)
        .unwrap()
        .replace("title: B", "title: Direct edit");
    let mutation = GraphMutation::AddPrerequisite {
        prerequisite: b.metadata.id.to_string(),
    };
    let request = RequestId::new();
    let error = repo
        .mutate_issue_graph_with_faults(
            a.metadata.id.as_str(),
            None,
            None,
            &mutation,
            &request,
            |p| {
                if p == workdeck_pm::transactions::FaultPoint::BeforeJournal {
                    fs::write(&path, &edited).unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert!(
        repo.show_issue(a.metadata.id.as_str())
            .unwrap()
            .metadata
            .prerequisites
            .is_empty()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), edited);
    assert!(repo.pending_operations().unwrap().is_empty());
    let receipt = repo
        .mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
        .unwrap();
    assert_eq!(
        repo.mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
            .unwrap(),
        receipt
    );
    assert_eq!(
        repo.mutate_issue_graph(
            a.metadata.id.as_str(),
            None,
            None,
            &GraphMutation::SetParent {
                parent: Some(b.metadata.id.to_string())
            },
            &request
        )
        .unwrap_err()
        .code,
        ErrorCode::IdempotencyConflict
    );
}
#[test]
fn dependency_paths_use_only_hard_edges_and_graph_snapshot_remains_stable() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let c = create(&repo, "C");
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::AddPrerequisite {
            prerequisite: b.metadata.id.to_string(),
        },
    )
    .unwrap();
    graph_mutate(
        &repo,
        &b,
        &GraphMutation::AddPrerequisite {
            prerequisite: c.metadata.id.to_string(),
        },
    )
    .unwrap();
    let graph = repo.issue_graph_snapshot().unwrap();
    assert_eq!(
        graph
            .dependency_path(&a.metadata.id, &c.metadata.id)
            .unwrap()
            .path,
        vec![
            a.metadata.id.clone(),
            b.metadata.id.clone(),
            c.metadata.id.clone()
        ]
    );
    assert!(
        !graph
            .dependency_path(&c.metadata.id, &a.metadata.id)
            .unwrap()
            .found
    );
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::RemovePrerequisite {
            prerequisite: b.metadata.id.to_string(),
            reason: "Remove ordered requirement".into(),
        },
    )
    .unwrap();
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::SetRelated {
            other: c.metadata.id.to_string(),
            related: true,
        },
    )
    .unwrap();
    assert!(
        !repo
            .issue_dependency_path(a.metadata.id.as_str(), c.metadata.id.as_str())
            .unwrap()
            .found
    );
    assert!(
        graph
            .dependency_path(&a.metadata.id, &c.metadata.id)
            .unwrap()
            .found
    );
}
#[test]
fn graph_records_round_trip_native_snapshot_and_reject_noncanonical_records() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::SetRelated {
            other: b.metadata.id.to_string(),
            related: true,
        },
    )
    .unwrap();
    let snapshot = repo.export_snapshot().unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|f| f.kind == workdeck_pm::SnapshotKind::IssueRelation)
    );
    snapshot.validate().unwrap();
    let other = TempDir::new().unwrap();
    let root = other.path().join(".workdeck");
    for file in &snapshot.files {
        let path = root.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.content).unwrap();
    }
    let restored = Repository::open_source(&root).unwrap();
    assert_eq!(
        repo.issue_relations(a.metadata.id.as_str()).unwrap(),
        restored.issue_relations(a.metadata.id.as_str()).unwrap()
    );
    let relation = snapshot
        .files
        .iter()
        .find(|f| f.kind == workdeck_pm::SnapshotKind::IssueRelation)
        .unwrap();
    let mut record: workdeck_pm::RelatedIssueLink =
        serde_yaml_ng::from_slice(&relation.content).unwrap();
    record.issues.reverse();
    fs::write(
        root.join(&relation.path),
        serde_yaml_ng::to_string(&record).unwrap(),
    )
    .unwrap();
    assert!(!restored.doctor().unwrap().valid);
    assert!(restored.export_snapshot().is_err());
}

#[test]
fn prospective_relation_import_rejects_retired_endpoints_but_preserves_historical_restoration() {
    use workdeck_pm::*;
    let (_temp, repo) = setup();
    let a = create(&repo, "Live issue");
    let b = create(&repo, "Retired issue");
    repo.retire_issue(
        b.metadata.id.as_str(),
        Some(&b.source),
        None,
        &RequestId::new(),
    )
    .unwrap();
    let original = repo.export_snapshot().unwrap();
    let source = TempDir::new().unwrap();
    let root = source.path().join(".workdeck");
    for file in &original.files {
        let path = root.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.content).unwrap();
    }
    let mut pair = [a.metadata.id.clone(), b.metadata.id.clone()];
    pair.sort();
    let relation_path =
        std::path::PathBuf::from(format!("relations/issues/{}/{}.yml", pair[0], pair[1]));
    let relation = RelatedIssueLink {
        schema: SchemaVersion::CURRENT,
        repository: repo.identity().clone(),
        issues: pair,
        created_at: chrono::Utc::now(),
    };
    let path = root.join(&relation_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_yaml_ng::to_string(&relation).unwrap()).unwrap();
    let source = Repository::open_source(&root).unwrap();
    let historical = source.export_snapshot().unwrap();
    let preview = repo
        .preview_snapshot_import(&historical, SnapshotImportMode::Merge)
        .unwrap();
    assert!(
        !preview.allowed,
        "new related links must obey the same retired-endpoint policy as SetRelated"
    );
    assert!(
        preview
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::PolicyBlocked
                && error.path.as_deref() == relation_path.to_str())
    );
    assert_eq!(repo.export_snapshot().unwrap(), original);
    let destination = TempDir::new().unwrap();
    let restored_root = destination.path().join(".workdeck");
    let plan = preview_snapshot_restore(&restored_root, &historical).unwrap();
    assert!(
        plan.allowed,
        "exact historical restoration does not author a new relationship"
    );
    restore_snapshot(
        &restored_root,
        &historical,
        Some(&plan.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    let restored = Repository::open_source(&restored_root).unwrap();
    assert!(
        restored
            .preview_snapshot_import(&historical, SnapshotImportMode::Merge)
            .unwrap()
            .allowed,
        "an unchanged retained historical relationship is not a new mutation"
    );
}
#[cfg(unix)]
#[test]
fn graph_relation_symlinks_are_rejected_without_following_external_source() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let mut ids = [a.metadata.id, b.metadata.id];
    ids.sort();
    let directory = repo.root().join(format!("relations/issues/{}", ids[0]));
    fs::create_dir_all(&directory).unwrap();
    let external = TempDir::new().unwrap();
    let target = external.path().join("outside.yml");
    fs::write(&target, b"not a relation").unwrap();
    std::os::unix::fs::symlink(&target, directory.join(format!("{}.yml", ids[1]))).unwrap();
    assert_eq!(
        repo.issue_graph_snapshot().unwrap_err().code,
        ErrorCode::UnsafePath
    );
    assert_eq!(fs::read(target).unwrap(), b"not a relation");
}

#[test]
fn graph_completion_contract_cannot_be_removed_by_json_or_raw_editor_while_closing() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let a = edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    let before = fs::read_to_string(repo.root().join(&a.path)).unwrap();
    assert_eq!(
        edit(&repo, &a, json!({"prerequisites":[],"status":"done"}))
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    let mut doc =
        workdeck_pm::documents::MarkdownDocument::parse(&repo.root().join(&a.path), &before)
            .unwrap();
    let patch =
        serde_yaml_ng::from_str::<serde_yaml_ng::Mapping>("prerequisites: []\nstatus: done\n")
            .unwrap();
    doc.patch(&patch).unwrap();
    assert_eq!(
        repo.mutate_issue(
            a.metadata.id.as_str(),
            Some(&a.source),
            &workdeck_pm::IssueMutation::EditDocument {
                markdown: doc.render()
            },
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(
        fs::read_to_string(repo.root().join(&a.path)).unwrap(),
        before
    );
}
#[test]
fn incomplete_children_do_not_affect_readiness_and_archival_does_not_satisfy_completion() {
    let (_t, repo) = setup();
    let a = create(&repo, "Parent");
    let b = create(&repo, "Child");
    let b = edit(&repo, &b, json!({"parent":a.metadata.id,"archived":true})).unwrap();
    assert!(repo.issue_readiness(a.metadata.id.as_str()).unwrap().ready);
    assert!(
        !repo
            .completion_report(a.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    repo.complete_issue(b.metadata.id.as_str(), &b.source, None, &RequestId::new())
        .unwrap();
    assert!(
        repo.completion_report(a.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    assert!(
        !repo
            .retirement_preview_issue(b.metadata.id.as_str())
            .unwrap()
            .allowed,
        "retirement cannot silently detach a required child"
    );
}

#[test]
fn waiver_decisions_cannot_be_forged_or_resurrected_after_explicit_revocation() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let a = edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    let path = repo.root().join("config.yml");
    let mut config: workdeck_pm::Config =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    config.acceptance.allow_prerequisite_waivers = true;
    fs::write(path, serde_yaml_ng::to_string(&config).unwrap()).unwrap();
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::WaivePrerequisite {
            prerequisite: b.metadata.id.to_string(),
            actor: "human".into(),
            reason: "Audited exception".into(),
        },
    )
    .unwrap();
    let path = repo.root().join(format!(
        "relations/waivers/{}/{}.yml",
        a.metadata.id, b.metadata.id
    ));
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        original.replace("Audited exception", "Forged reason"),
    )
    .unwrap();
    assert!(
        repo.completion_report(a.metadata.id.as_str()).is_err(),
        "a mutated declaration cannot impersonate the recorded decision"
    );
    fs::write(&path, &original).unwrap();
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::RevokeWaiver {
            prerequisite: b.metadata.id.to_string(),
            reason: "Exception withdrawn".into(),
        },
    )
    .unwrap();
    fs::write(&path, original).unwrap();
    assert!(
        repo.completion_report(a.metadata.id.as_str()).is_err(),
        "restoring a revoked decision cannot make it active again"
    );
}

#[test]
fn graph_receipt_mutation_must_match_the_original_request_intent_on_replay_and_export() {
    let (_temp, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let mutation = GraphMutation::AddPrerequisite {
        prerequisite: b.metadata.id.to_string(),
    };
    let request = RequestId::new();
    let mut receipt = repo
        .mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
        .unwrap();
    receipt.result["mutation"] = json!(GraphMutation::RemovePrerequisite {
        prerequisite: b.metadata.id.to_string(),
        reason: "Replaced original recorded intent".into()
    });
    let path = repo
        .root()
        .join(format!("operations/{}.yml", receipt.operation_id));
    fs::write(path, serde_yaml_ng::to_string(&receipt).unwrap()).unwrap();
    assert!(
        repo.mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
            .is_err(),
        "a valid request hash cannot authorize a different reported graph mutation"
    );
    assert!(repo.export_snapshot().is_err());
    assert!(repo.operation_history().is_err());
}

#[test]
fn historical_graph_receipts_without_stored_intent_remain_readable_but_replay_checks_the_caller() {
    let (_temp, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let mutation = GraphMutation::AddPrerequisite {
        prerequisite: b.metadata.id.to_string(),
    };
    let request = RequestId::new();
    let mut receipt = repo
        .mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
        .unwrap();
    // Historical results did not retain the exact reference/precondition input;
    // no validator can retroactively reconstruct those omitted values on export.
    receipt.result.as_object_mut().unwrap().remove("intent");
    let path = repo
        .root()
        .join(format!("operations/{}.yml", receipt.operation_id));
    fs::write(&path, serde_yaml_ng::to_string(&receipt).unwrap()).unwrap();
    assert!(repo.export_snapshot().is_ok());
    assert!(repo.operation_history().is_ok());
    let current = repo.show_issue(a.metadata.id.as_str()).unwrap();
    edit(&repo, &current, json!({"title":"Later native edit"})).unwrap();
    assert_eq!(
        repo.mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
            .unwrap(),
        receipt
    );
    receipt.result["mutation"] = json!(GraphMutation::RemovePrerequisite {
        prerequisite: b.metadata.id.to_string(),
        reason: "Different historic result".into()
    });
    fs::write(&path, serde_yaml_ng::to_string(&receipt).unwrap()).unwrap();
    assert!(
        repo.mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
            .is_err(),
        "replay has the original caller input even when an old result omitted it"
    );
}

#[test]
fn paired_waiver_and_result_edits_cannot_change_the_retained_request_intent() {
    use workdeck_pm::*;
    let (_temp, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    let path = repo.root().join("config.yml");
    let mut config: Config = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    config.acceptance.allow_prerequisite_waivers = true;
    fs::write(&path, serde_yaml_ng::to_string(&config).unwrap()).unwrap();
    let request = RequestId::new();
    let mutation = GraphMutation::WaivePrerequisite {
        prerequisite: b.metadata.id.to_string(),
        actor: "human".into(),
        reason: "Original scoped exception".into(),
    };
    let mut receipt = repo
        .mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
        .unwrap();
    let mut waiver: PrerequisiteWaiver =
        serde_json::from_value(receipt.result["waivers"][0].clone()).unwrap();
    waiver.reason = "Broader exception never requested".into();
    let bytes = serde_yaml_ng::to_string(&waiver).unwrap();
    fs::write(repo.root().join(&receipt.changed[0].path), &bytes).unwrap();
    receipt.changed[0].after = Some(ContentHash::of(bytes.as_bytes()));
    receipt.result["waivers"][0] = json!(waiver);
    receipt.result["mutation"]["reason"] = json!(waiver.reason);
    fs::write(
        repo.root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&receipt).unwrap(),
    )
    .unwrap();
    assert!(repo.issue_graph_snapshot().is_err());
    assert!(repo.export_snapshot().is_err());
    assert!(
        repo.mutate_issue_graph(a.metadata.id.as_str(), None, None, &mutation, &request)
            .is_err()
    );
}

#[test]
#[cfg(unix)]
fn graph_receipt_tampering_cannot_be_staged_as_a_successful_operation() {
    let (temp, repo) = setup();
    let mut git = std::process::Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            git.env_remove(key);
        }
    }
    let output = git
        .current_dir(temp.path())
        .args(["-c", "core.hooksPath=/dev/null", "init", "--quiet"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(output.status.success());
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let mut receipt = graph_mutate(
        &repo,
        &a,
        &GraphMutation::AddPrerequisite {
            prerequisite: b.metadata.id.to_string(),
        },
    )
    .unwrap();
    receipt.result["mutation"] = json!(GraphMutation::RemovePrerequisite {
        prerequisite: b.metadata.id.to_string(),
        reason: "Forged result".into()
    });
    fs::write(
        repo.root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&receipt).unwrap(),
    )
    .unwrap();
    let before = fs::read(temp.path().join(".git/index")).ok();
    assert!(repo.stage_operation(&receipt).is_err());
    assert_eq!(fs::read(temp.path().join(".git/index")).ok(), before);
}

#[test]
fn graph_recovery_validates_retained_intent_before_publishing_any_change() {
    let (_temp, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let before = fs::read(repo.root().join(&a.path)).unwrap();
    let mutation = GraphMutation::AddPrerequisite {
        prerequisite: b.metadata.id.to_string(),
    };
    assert_eq!(
        repo.mutate_issue_graph_with_faults(
            a.metadata.id.as_str(),
            None,
            None,
            &mutation,
            &RequestId::new(),
            |point| {
                if point == workdeck_pm::transactions::FaultPoint::AfterJournal {
                    return Err(workdeck_pm::PmError::new(
                        ErrorCode::Io,
                        "injected before publication",
                    ));
                }
                Ok(())
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::RecoveryRequired
    );
    let path = fs::read_dir(repo.root().join(".tmp/journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original = fs::read(&path).unwrap();
    let mut journal: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&original).unwrap();
    journal["receipt"]["result"]["mutation"] =
        serde_yaml_ng::to_value(GraphMutation::RemovePrerequisite {
            prerequisite: b.metadata.id.to_string(),
            reason: "Unrequested recovery output".into(),
        })
        .unwrap();
    fs::write(&path, serde_yaml_ng::to_string(&journal).unwrap()).unwrap();
    assert!(
        repo.recover_operations().is_err(),
        "recovery must reject a changed recorded intent before publication"
    );
    assert_eq!(fs::read(repo.root().join(&a.path)).unwrap(), before);
    fs::write(path, original).unwrap();
    assert_eq!(repo.recover_operations().unwrap().len(), 1);
}

#[test]
fn later_reopen_exposes_invalidated_parent_completion_without_rewriting_parent() {
    let (_t, repo) = setup();
    let parent = create(&repo, "Parent");
    let child = create(&repo, "Child");
    let child = edit(&repo, &child, json!({"parent":parent.metadata.id})).unwrap();
    let child: IssueRecord = serde_json::from_value(
        repo.complete_issue(
            child.metadata.id.as_str(),
            &child.source,
            None,
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let parent: IssueRecord = serde_json::from_value(
        repo.complete_issue(
            parent.metadata.id.as_str(),
            &parent.source,
            None,
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.reopen_issue(child.metadata.id.as_str(), &child.source, &RequestId::new())
        .unwrap();
    assert!(
        !repo
            .completion_report(parent.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    assert_eq!(
        repo.show_issue(parent.metadata.id.as_str()).unwrap(),
        parent
    );
    assert_eq!(
        repo.complete_issue(
            parent.metadata.id.as_str(),
            &parent.source,
            None,
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
}
#[test]
fn waived_snapshot_roundtrip_keeps_decision_proof_and_revoke_does_not_touch_item() {
    let (_t, repo) = setup();
    let a = create(&repo, "A");
    let b = create(&repo, "B");
    let a = edit(&repo, &a, json!({"prerequisites":[b.metadata.id]})).unwrap();
    let path = repo.root().join("config.yml");
    let mut config: workdeck_pm::Config =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    config.acceptance.allow_prerequisite_waivers = true;
    fs::write(path, serde_yaml_ng::to_string(&config).unwrap()).unwrap();
    graph_mutate(
        &repo,
        &a,
        &GraphMutation::WaivePrerequisite {
            prerequisite: b.metadata.id.to_string(),
            actor: "human".into(),
            reason: "Deliberate policy exception".into(),
        },
    )
    .unwrap();
    let snapshot = repo.export_snapshot().unwrap();
    snapshot.validate().unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|f| f.kind == workdeck_pm::SnapshotKind::PrerequisiteWaiver)
    );
    let dir = TempDir::new().unwrap();
    let root = dir.path().join(".workdeck");
    for file in &snapshot.files {
        let p = root.join(&file.path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, &file.content).unwrap();
    }
    let clone = Repository::open_source(&root).unwrap();
    assert!(
        clone
            .completion_report(a.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    let receipt = graph_mutate(
        &clone,
        &a,
        &GraphMutation::RevokeWaiver {
            prerequisite: b.metadata.id.to_string(),
            reason: "Restore original requirement".into(),
        },
    )
    .unwrap();
    assert_eq!(receipt.changed.len(), 1);
    assert!(receipt.changed[0].path.starts_with("relations/waivers"));
    assert_eq!(clone.show_issue(a.metadata.id.as_str()).unwrap(), a);
    assert!(
        !clone
            .completion_report(a.metadata.id.as_str())
            .unwrap()
            .allowed
    );
}

#[test]
fn deep_graphs_have_bounded_explanations_and_iterative_hard_paths() {
    let (_t, repo) = setup();
    let config: workdeck_pm::Config =
        serde_yaml_ng::from_slice(&fs::read(repo.root().join("config.yml")).unwrap()).unwrap();
    for n in 1..=500 {
        let mut metadata =
            workdeck_pm::IssueMetadata::new(&config, "Bounded graph fixture", chrono::Utc::now())
                .unwrap();
        metadata.id = format!("WD-{n}").parse().unwrap();
        if n < 500 {
            metadata
                .prerequisites
                .push(format!("WD-{}", n + 1).parse().unwrap());
        }
        let path = repo.root().join(format!("issues/{}/item.md", metadata.id));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!("---\n{}---\n", serde_yaml_ng::to_string(&metadata).unwrap()),
        )
        .unwrap();
    }
    assert_eq!(
        repo.issue_readiness("WD-1").unwrap_err().code,
        ErrorCode::Unsupported
    );
    let path = repo.issue_dependency_path("WD-1", "WD-500").unwrap();
    assert!(path.found);
    assert_eq!(path.path.len(), 500);
    assert!(repo.doctor().unwrap().valid);
}
