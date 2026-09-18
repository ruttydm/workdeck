use super::*;
use serde_json::json;

#[test]
fn independent_native_families_are_searchable_with_exact_inert_documents() {
    let (temp, repo) = fixture();
    let record = issue(
        &repo,
        "Continuity",
        json!({"acceptance":[{"id":"works","description":"Criterionneedle","checked":true}]}),
    );
    repo.add_comment(
        record.metadata.id.as_str(),
        &record.source,
        "Ada",
        "Commentneedle",
        &RequestId::new(),
    )
    .unwrap();
    repo.log_time(
        record.metadata.id.as_str(),
        None,
        &TimeEntryInput {
            user: "Ada".into(),
            actor: "Ada".into(),
            seconds: 60,
            worked_at: chrono::Utc::now() - chrono::Duration::seconds(5),
        },
        &RequestId::new(),
    )
    .unwrap();
    let mut feature = CreateFeature::new("Featureneedle");
    feature.body = "Native capability".into();
    repo.create_feature(&feature, &RequestId::new()).unwrap();
    repo.write_wiki(
        &WriteWiki {
            path: "guide.md".into(),
            body: "Wikineedle".into(),
            expected: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    repo.write_saved_view(
        &WriteSavedView {
            id: "mine".into(),
            name: "Viewneedle".into(),
            query: IssueQuery::default(),
            archived: false,
            expected: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let record = repo.show_issue(record.metadata.id.as_str()).unwrap();
    let criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(record.metadata.id.clone()), "works")
        .unwrap()
        .reference;
    let question = CreateQuestion {
        actor: "Ada".into(),
        body: "Questionneedle".into(),
        subjects: vec![QuestionSubject {
            subject: SubjectRef::Issue(record.metadata.id.clone()),
            source: record.source.clone(),
        }],
        requirements: vec![criterion.clone()],
        blocks_work: false,
        custom: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    repo.create_question(&question, &RequestId::new()).unwrap();
    let anchor = repo
        .context(&ContextRequest::new(record.metadata.id.as_str(), 64 * 1024))
        .unwrap()
        .anchor;
    repo.create_handoff(
        &CreateHandoff {
            actor: "Ada".into(),
            anchor,
            body: "Handoffneedle".into(),
            attempted: vec![],
            uncertainties: vec![],
            evidence_refs: vec![],
            questions: vec![],
            pending_operations: vec![],
            next_steps: vec![],
            custom: BTreeMap::new(),
            extra: BTreeMap::new(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let projected = view(temp.path()).unwrap();
    for (family, text) in [
        (SnapshotKind::Comment, "Commentneedle"),
        (SnapshotKind::Feature, "Featureneedle"),
        (SnapshotKind::Wiki, "Wikineedle"),
        (SnapshotKind::SavedView, "Viewneedle"),
        (SnapshotKind::Question, "Questionneedle"),
        (SnapshotKind::Handoff, "Handoffneedle"),
        (SnapshotKind::Issue, "Criterionneedle"),
    ] {
        let handle = projected
            .query(&ProjectionQuery::Records {
                family: Some(family),
                query: text.into(),
            })
            .unwrap();
        assert_eq!(handle.total, 1, "{family:?}");
        let page = projected.page(&handle, 0, 1).unwrap();
        let detail = projected.detail(&page.rows[0].token).unwrap();
        assert!(detail.document.unwrap().contains(text), "{family:?}");
        assert_eq!(detail.omitted_document_bytes, 0);
    }
    assert_eq!(
        projected
            .query(&ProjectionQuery::Records {
                family: Some(SnapshotKind::TimeEntry),
                query: String::new()
            })
            .unwrap()
            .total,
        1
    );
    // Rich-record FTS does not silently broaden the established issue search.
    assert!(
        titles(
            &projected,
            &IssueQuery {
                query: "Criterionneedle".into(),
                ..IssueQuery::default()
            }
        )
        .is_empty()
    );
}

#[test]
fn feature_rows_keep_independent_authored_states_and_full_parent_edges() {
    let (temp, repo) = fixture();
    let parent: FeatureRecord = serde_json::from_value::<FeatureOutcome>(
        repo.create_feature(&CreateFeature::new("Parent"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
    .record;
    let mut input = CreateFeature::new("Child");
    input.fields = BTreeMap::from([
        ("parent".into(), json!(parent.metadata.id)),
        ("decision".into(), json!("accepted")),
        ("maturity".into(), json!("specified")),
        ("availability".into(), json!("experimental")),
        ("lead".into(), json!("Ada")),
    ]);
    repo.create_feature(&input, &RequestId::new()).unwrap();
    let projected = view(temp.path()).unwrap();
    let handle = projected
        .query(&ProjectionQuery::Features {
            query: ProjectionFeatureQuery {
                parent: Some(parent.metadata.id.clone()),
                ..Default::default()
            },
        })
        .unwrap();
    let row = projected.page(&handle, 0, 1).unwrap().rows.remove(0);
    assert_eq!(row.decision, Some(FeatureDecision::Accepted));
    assert_eq!(row.maturity, Some(FeatureMaturity::Specified));
    assert_eq!(row.availability, Some(FeatureAvailability::Experimental));
    assert_eq!(row.lead.as_deref(), Some("Ada"));
    let detail = projected.detail(&row.token).unwrap();
    assert!(
        detail
            .relations
            .iter()
            .any(|relation| relation.relation == "parent"
                && relation.to.id == parent.metadata.id.as_str())
    );
}

#[test]
fn nested_question_and_evidence_citations_remain_visible_in_projection_details() {
    let (temp, repo) = fixture();
    let project: PlanningRecord = serde_json::from_value(
        repo.create_planning(
            PlanningKind::Project,
            &CreatePlanning {
                id: Some("project-citations".into()),
                name: "Project citations".into(),
                body: String::new(),
                fields: BTreeMap::from([(
                    "exit_criteria".into(),
                    json!([{"id":"exit","description":"Required exit"}]),
                )]),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let criterion = repo
        .resolve_criterion(
            &CriterionOwner::Project(project.metadata.id.clone()),
            "exit",
        )
        .unwrap()
        .reference;
    repo.create_question(
        &CreateQuestion {
            actor: "fixture".into(),
            body: "Project question".into(),
            subjects: vec![QuestionSubject {
                subject: SubjectRef::Project(project.metadata.id.clone()),
                source: project.source,
            }],
            requirements: vec![criterion.clone()],
            blocks_work: true,
            custom: BTreeMap::new(),
            extra: BTreeMap::new(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let attestation = AttestationId::new();
    let declaration: DeclareEvidence = serde_json::from_value(json!({
        "criterion":criterion,"subject":{"repository":repo.identity(),"kind":"source","content":ContentHash::of(b"declared source")},
        "producer":{"id":"runner","definition":ContentHash::of(b"producer")},
        "check":{"id":"unit","definition":ContentHash::of(b"check")},
        "result":{"id":"result","content":ContentHash::of(b"result")},
        "observed_at":chrono::Utc::now(),"provenance":{"actor":"fixture","reason":"Historical citation, not qualification"},
        "links":[{"kind":"attestation","id":attestation,"content":ContentHash::of(b"historical attestation")}]
    })).unwrap();
    repo.declare_evidence(&declaration, &RequestId::new())
        .unwrap();
    let projected = view(temp.path()).unwrap();
    for family in [SnapshotKind::Question, SnapshotKind::Evidence] {
        let handle = projected
            .query(&ProjectionQuery::Records {
                family: Some(family),
                query: String::new(),
            })
            .unwrap();
        let page = projected.page(&handle, 0, 10).unwrap();
        let detail = projected.detail(&page.rows[0].token).unwrap();
        assert!(
            detail
                .relations
                .iter()
                .any(|relation| relation.to.kind == SnapshotKind::Project
                    && relation.to.id == project.metadata.id),
            "{family:?}: {:?}",
            detail.relations
        );
        if family == SnapshotKind::Evidence {
            assert!(
                detail
                    .relations
                    .iter()
                    .any(|relation| relation.relation == "attestation"
                        && relation.to.kind == SnapshotKind::Attestation
                        && relation.to.id == attestation.as_str())
            );
        } else {
            assert!(
                detail
                    .relations
                    .iter()
                    .any(|relation| relation.relation == "subject")
            );
            assert!(
                detail
                    .relations
                    .iter()
                    .any(|relation| relation.relation == "criterion")
            );
        }
    }
}
