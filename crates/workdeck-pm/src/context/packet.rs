use super::*;
use crate::transactions::Snapshot;
use std::path::Path;

fn cite_issue(issue: &IssueRecord) -> Vec<ContextCitation> {
    vec![ContextCitation {
        target: ContextTarget::Issue {
            id: issue.metadata.id.clone(),
        },
        source: Some(issue.source.content.clone()),
    }]
}
fn section(kind: ContextSectionKind, mut entries: Vec<ContextEntry>) -> ContextSection {
    let total = entries.len();
    entries.truncate(1024);
    let omission_reasons = if entries.len() < total {
        vec!["capture_limit".into()]
    } else {
        Vec::new()
    };
    ContextSection {
        kind,
        total,
        omitted: 0,
        entries,
        coverage_complete: true,
        omission_reasons,
    }
}
pub(super) fn packet(
    snapshot: &Snapshot<'_>,
    captured: &capture::Capture,
    issue: &IssueRecord,
    anchor: ContextAnchor,
    questions: &[QuestionApplicability],
    request: &ContextRequest,
    external: (
        Vec<ContextEntry>,
        Vec<ContextEntry>,
        &check_runs::Runs,
        &reviews::Reviews,
    ),
) -> Result<ContextPacket> {
    let root = &captured.root;
    let (sources, instructions, runs, reviews) = external;
    let mut sections = Vec::new();
    sections.push(section(
        ContextSectionKind::Requirements,
        issue
            .metadata
            .acceptance
            .iter()
            .map(|criterion| ContextEntry {
                content: ContextContent::Requirement {
                    id: criterion.id.clone(),
                    description: criterion.description.clone(),
                    checked_declaration: criterion.checked,
                },
                citations: cite_issue(issue),
            })
            .collect(),
    ));
    let actions = next::actions(captured, issue, anchor.clone(), questions)?;
    let mut action_entries = actions
        .actions
        .into_iter()
        .map(|action| ContextEntry {
            content: ContextContent::Action { action },
            citations: cite_issue(issue),
        })
        .collect::<Vec<_>>();
    if actions.omitted_actions > 0 {
        action_entries.push(notice(
            "action_limit",
            &format!(
                "{} additional actions exceed the bounded suggestion limit",
                actions.omitted_actions
            ),
        ));
    }
    sections.push(section(ContextSectionKind::Actions, action_entries));
    sections.push(runs.section());
    let mut conditions = captured.graph.relations(&issue.metadata.id)?.conditions;
    conditions.extend(crate::gates::issue_conditions(
        root,
        snapshot,
        &captured.config,
        issue,
    )?);
    sections.push(section(
        ContextSectionKind::Blockers,
        conditions
            .into_iter()
            .map(|condition| {
                let citations = condition
                    .source_pins
                    .iter()
                    .map(|pin| ContextCitation {
                        target: ContextTarget::PlanningSource {
                            path: pin.path.clone(),
                        },
                        source: Some(pin.content.clone()),
                    })
                    .collect();
                ContextEntry {
                    content: ContextContent::Condition { condition },
                    citations,
                }
            })
            .collect(),
    ));
    sections.push(section(
        ContextSectionKind::Questions,
        questions
            .iter()
            .filter_map(|applicability| {
                captured
                    .questions
                    .iter()
                    .find(|record| record.metadata.id == applicability.question_id)
                    .map(|record| ContextEntry {
                        content: ContextContent::Question {
                            record: record.clone(),
                            applicability: applicability.clone(),
                        },
                        citations: vec![ContextCitation {
                            target: ContextTarget::Question {
                                id: record.metadata.id.to_string(),
                            },
                            source: Some(record.source.content.clone()),
                        }],
                    })
            })
            .collect(),
    ));
    let evidence = relevant_evidence(snapshot, &captured.config, issue)?;
    let all_evidence = crate::evidence::store::load_evidence(snapshot, &captured.config)?;
    let mut handoffs =
        crate::handoffs::load_handoffs(root, snapshot, &captured.config, &issue.metadata.id)?;
    handoffs.sort_by(|a, b| {
        (&b.metadata.created_at, &b.metadata.id).cmp(&(&a.metadata.created_at, &a.metadata.id))
    });
    let handoff_count = handoffs.len();
    let mut handoff_entries = Vec::new();
    for record in handoffs.into_iter().take(64) {
        let mut freshness = if record.metadata.anchor == anchor {
            ContextFreshness::Current
        } else {
            ContextFreshness::Stale
        };
        let mut reasons = vec!["declared_continuity_not_evidence".into()];
        if freshness == ContextFreshness::Stale {
            reasons.push("context_changed".into());
        }
        for reference in &record.metadata.evidence_refs {
            match all_evidence.iter().find(|r| r.reference.id == reference.id) {
                Some(e) if e.content == reference.content => {
                    let (state, codes) = evidence_freshness(
                        root,
                        snapshot,
                        &captured.config,
                        e,
                        &all_evidence,
                        request.as_of,
                    )?;
                    if state == ContextFreshness::Stale {
                        freshness = ContextFreshness::Stale;
                    } else if state == ContextFreshness::Unknown
                        && freshness == ContextFreshness::Current
                    {
                        freshness = ContextFreshness::Unknown;
                    }
                    reasons.extend(codes);
                }
                _ => {
                    freshness = ContextFreshness::Stale;
                    reasons.push("evidence_source_changed_or_missing".into());
                }
            }
        }
        reasons.sort();
        reasons.dedup();
        let citations = vec![ContextCitation {
            target: ContextTarget::Handoff {
                issue: issue.metadata.id.clone(),
                id: record.metadata.id.to_string(),
            },
            source: Some(record.content.clone()),
        }];
        handoff_entries.push(ContextEntry {
            content: ContextContent::Handoff {
                record,
                freshness,
                reason_codes: reasons,
            },
            citations,
        });
    }
    let mut handoff_section = section(ContextSectionKind::Handoffs, handoff_entries);
    if handoff_section.total < handoff_count {
        handoff_section.total = handoff_count;
        handoff_section
            .omission_reasons
            .push("capture_limit".into());
    }
    sections.push(handoff_section);
    sections.push(section(
        ContextSectionKind::Summary,
        vec![ContextEntry {
            content: ContextContent::Summary {
                title: issue.metadata.title.clone(),
                status: issue.metadata.status.clone(),
                body: issue.body.clone(),
                manual_acceptance: issue.metadata.manual_acceptance.clone(),
                imported_completion: issue.metadata.imported_completion.clone(),
            },
            citations: cite_issue(issue),
        }],
    ));
    // Historical reviews follow active requirements, blockers and continuity.
    if let Some(section) = reviews.section() {
        sections.push(section);
    }
    // Documents are arbitrary authored strings, not necessarily URLs or worktree paths.
    // Keep them inert and cite only the declaring issue. Explicit SourceLinks use the
    // separate descriptor-safe source capture when document contents are required.
    let mut documents = section(
        ContextSectionKind::Documents,
        issue
            .metadata
            .documents
            .iter()
            .take(1024)
            .map(|reference| ContextEntry {
                content: ContextContent::Document {
                    reference: reference.clone(),
                    reason_code: "document_not_fetched".into(),
                },
                citations: cite_issue(issue),
            })
            .collect(),
    );
    documents.total = issue.metadata.documents.len();
    if documents.total > documents.entries.len() {
        documents.omission_reasons.push("capture_limit".into());
    }
    sections.push(documents);
    let mut feature_entries = Vec::new();
    if !issue.metadata.features.is_empty() {
        let features = crate::features::load_features(snapshot, &captured.config)?;
        for id in &issue.metadata.features {
            if let Some(record) = features.iter().find(|r| &r.metadata.id == id) {
                feature_entries.push(ContextEntry {
                    content: ContextContent::Feature {
                        record: record.clone(),
                    },
                    citations: vec![ContextCitation {
                        target: ContextTarget::Feature { id: id.clone() },
                        source: Some(record.source.content.clone()),
                    }],
                });
            } else {
                feature_entries.push(notice(
                    "feature_unresolved",
                    &format!("Associated feature {id} is missing; its requirements are unknown."),
                ));
            }
        }
    }
    sections.push(section(ContextSectionKind::Features, feature_entries));
    sections.push(section(
        ContextSectionKind::Verification,
        vec![ContextEntry {
            content: ContextContent::Verification {
                policy: captured.config.acceptance.clone(),
                execution_available: cfg!(unix),
                reason_code: if cfg!(unix) {
                    "explicit_foreground_local_feedback"
                } else {
                    "check_execution_unavailable"
                }
                .into(),
            },
            citations: vec![ContextCitation {
                target: ContextTarget::PlanningSource {
                    path: "config.yml".into(),
                },
                source: anchor.source_pins.first().map(|p| p.content.clone()),
            }],
        }],
    ));
    sections.push(section(ContextSectionKind::Sources, sources));
    sections.push(section(ContextSectionKind::Instructions, instructions));
    let mut evidence_entries = Vec::new();
    let evidence_count = evidence.len();
    for record in evidence.into_iter().take(256) {
        let (freshness, reason_codes) = evidence_freshness(
            root,
            snapshot,
            &captured.config,
            &record,
            &all_evidence,
            request.as_of,
        )?;
        let citations = vec![ContextCitation {
            target: ContextTarget::Evidence {
                id: record.reference.id.clone(),
            },
            source: Some(record.content.clone()),
        }];
        evidence_entries.push(ContextEntry {
            content: ContextContent::Evidence {
                record,
                freshness,
                reason_codes,
            },
            citations,
        });
    }
    let mut evidence_section = section(ContextSectionKind::Evidence, evidence_entries);
    if evidence_section.total < evidence_count {
        evidence_section.total = evidence_count;
        evidence_section
            .omission_reasons
            .push("capture_limit".into());
    }
    sections.push(evidence_section);
    let (overlaps, coverage_complete) = overlaps(captured, issue)?;
    let mut overlap_section = section(
        ContextSectionKind::Overlaps,
        overlaps
            .into_iter()
            .map(|overlap| ContextEntry {
                citations: vec![ContextCitation {
                    target: ContextTarget::Issue {
                        id: overlap.issue.clone(),
                    },
                    source: Some(overlap.source.content.clone()),
                }],
                content: ContextContent::Overlap { overlap },
            })
            .collect(),
    );
    overlap_section.coverage_complete = coverage_complete;
    if !coverage_complete {
        overlap_section
            .omission_reasons
            .push("inspection_limit; total_is_a_lower_bound".into());
    }
    sections.push(overlap_section);
    budget::pack(ContextPacket {schema_version:SchemaVersion::CURRENT,anchor,graph:captured.graph.fingerprint().clone(),as_of:request.as_of,authority:"context_only; authored declarations and historical summaries do not authorize actions or certify verification".into(),budget:ContextBudget {scope:"compact_context_packet_json".into(),limit_bytes:request.budget_bytes,used_bytes:0,minimum_bytes:0,omitted_entries:0},sections})
}

pub(super) fn relevant_evidence(
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueRecord,
) -> Result<Vec<EvidenceRecord>> {
    let subjects = capture::subjects(issue);
    let mut criteria = Vec::new();
    for id in &issue.metadata.gates {
        match crate::gates::load_gate(snapshot, config, id) {
            Ok(gate) => criteria.extend(
                gate.definition
                    .requirements
                    .into_iter()
                    .map(|r| r.criterion),
            ),
            Err(e)
                if matches!(
                    e.code,
                    ErrorCode::NotFound | ErrorCode::InvalidSchema | ErrorCode::UnsupportedSchema
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(crate::evidence::store::load_evidence(snapshot, config)?
        .into_iter()
        .filter(|e| {
            subjects.contains(&e.reference.declaration.criterion.owner.subject())
                || criteria.contains(&e.reference.declaration.criterion)
        })
        .collect())
}

fn evidence_freshness(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    record: &EvidenceRecord,
    all: &[EvidenceRecord],
    as_of: Option<Timestamp>,
) -> Result<(ContextFreshness, Vec<String>)> {
    let declaration = &record.reference.declaration;
    let mut state = ContextFreshness::Unknown;
    let mut reasons = vec![
        "declared_evidence_only".into(),
        "producer_unadmitted".into(),
    ];
    if let Some(as_of) = as_of {
        let active = crate::evidence::store::active_at(all, as_of)
            .iter()
            .any(|r| r.reference.id == record.reference.id);
        if !active
            || declaration.expires_at.is_some_and(|at| at <= as_of)
            || declaration.observed_at > as_of
        {
            state = ContextFreshness::Stale;
            reasons.push("evidence_expired_superseded_or_not_yet_observed".into());
        } else {
            state = ContextFreshness::Current;
        }
    } else {
        reasons.push("as_of_required_for_freshness".into());
    }
    match crate::gates::resolve_criterion(
        root,
        snapshot,
        config,
        &declaration.criterion.owner,
        &declaration.criterion.id,
    ) {
        Ok(current) if current.reference == declaration.criterion && !current.retired => (),
        Ok(_) => {
            state = ContextFreshness::Stale;
            reasons.push("criterion_definition_changed".into());
        }
        Err(error) if matches!(error.code, ErrorCode::NotFound) => {
            state = ContextFreshness::Stale;
            reasons.push("criterion_missing".into());
        }
        Err(error) => return Err(error),
    }
    Ok((state, reasons))
}

fn overlaps(captured: &capture::Capture, issue: &IssueRecord) -> Result<(Vec<IssueOverlap>, bool)> {
    let mut output = Vec::new();
    let mut comparisons = 0usize;
    for other in captured.graph.issues() {
        if other.metadata.id == issue.metadata.id
            || other.metadata.archived
            || other.retirement.is_some()
            || matches!(
                captured
                    .config
                    .workflow
                    .state(&other.metadata.status)?
                    .category,
                WorkflowCategory::Completed | WorkflowCategory::Canceled
            )
        {
            continue;
        }
        for own in &issue.metadata.files {
            for theirs in &other.metadata.files {
                comparisons += 1;
                if comparisons > 100_000 {
                    return Ok((output, false));
                }
                let same_path = own.path == theirs.path;
                let nested = Path::new(&own.path).starts_with(&theirs.path)
                    || Path::new(&theirs.path).starts_with(&own.path);
                let lines_overlap = match (own.line, theirs.line) {
                    (Some(a), Some(b)) => {
                        a <= theirs.end_line.unwrap_or(b) && b <= own.end_line.unwrap_or(a)
                    }
                    _ => true,
                };
                if (same_path && lines_overlap) || (!same_path && nested) {
                    output.push(IssueOverlap {
                        issue: other.metadata.id.clone(),
                        source: other.source.clone(),
                        basis: OverlapBasis::Path {
                            own: own.clone(),
                            other: theirs.clone(),
                        },
                        advisory: true,
                    });
                }
            }
        }
        for prerequisite in &issue.metadata.prerequisites {
            if other.metadata.prerequisites.contains(prerequisite) {
                output.push(IssueOverlap {
                    issue: other.metadata.id.clone(),
                    source: other.source.clone(),
                    basis: OverlapBasis::SharedPrerequisite {
                        issue: prerequisite.clone(),
                    },
                    advisory: true,
                });
            }
        }
        for (dependent, prerequisite) in [(issue, other), (other, issue)] {
            if dependent
                .metadata
                .prerequisites
                .contains(&prerequisite.metadata.id)
            {
                output.push(IssueOverlap {
                    issue: other.metadata.id.clone(),
                    source: other.source.clone(),
                    basis: OverlapBasis::Dependency {
                        dependent: dependent.metadata.id.clone(),
                        prerequisite: prerequisite.metadata.id.clone(),
                    },
                    advisory: true,
                });
            }
        }
        if output.len() > 4096 {
            output.truncate(4096);
            return Ok((output, false));
        }
    }
    Ok((output, true))
}
