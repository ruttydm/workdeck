use super::capture::{condition, policy_hash, requirement_hash};
use super::*;
use crate::transactions::Snapshot;
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};

impl Repository {
    pub fn issue_graph_snapshot(&self) -> Result<IssueGraphSnapshot> {
        self.store()?.with_snapshot(|s| {
            let c = crate::repository::config_from_snapshot(self.root(), s)?;
            capture(self.root(), s, &c)
        })
    }
    pub fn issue_relations(&self, reference: &str) -> Result<IssueRelations> {
        self.store()?.with_snapshot(|s| {
            let c = crate::repository::config_from_snapshot(self.root(), s)?;
            let graph = capture(self.root(), s, &c)?;
            let issue = crate::issues::resolve_issue(self.root(), s, &c, reference)?;
            graph.relations(&issue.metadata.id)
        })
    }
    pub fn issue_readiness(&self, reference: &str) -> Result<IssueReadiness> {
        self.store()?.with_snapshot(|s| {
            let c = crate::repository::config_from_snapshot(self.root(), s)?;
            let graph = capture(self.root(), s, &c)?;
            let issue = crate::issues::resolve_issue(self.root(), s, &c, reference)?;
            graph.readiness(&issue.metadata.id)
        })
    }
    pub fn issue_dependency_path(&self, from: &str, to: &str) -> Result<IssueDependencyPath> {
        self.store()?.with_snapshot(|s| {
            let c = crate::repository::config_from_snapshot(self.root(), s)?;
            let graph = capture(self.root(), s, &c)?;
            let a = crate::issues::resolve_issue(self.root(), s, &c, from)?;
            let b = crate::issues::resolve_issue(self.root(), s, &c, to)?;
            graph.dependency_path(&a.metadata.id, &b.metadata.id)
        })
    }
}
impl IssueGraphSnapshot {
    fn issue(&self, id: &IssueId) -> Result<&IssueRecord> {
        self.issues
            .iter()
            .find(|i| &i.metadata.id == id)
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::NotFound,
                    format!("issue {id} is absent from this graph snapshot"),
                )
            })
    }
    pub fn relations(&self, id: &IssueId) -> Result<IssueRelations> {
        let issue = self.issue(id)?;
        let mut children = Vec::new();
        let mut dependents = Vec::new();
        let mut related = Vec::new();
        for i in &self.issues {
            if i.metadata.parent.as_ref() == Some(id) {
                children.push(i.metadata.id.clone())
            }
            if i.metadata.prerequisites.contains(id) {
                dependents.push(i.metadata.id.clone())
            }
        }
        for link in &self.related {
            if &link.issues[0] == id {
                related.push(link.issues[1].clone())
            } else if &link.issues[1] == id {
                related.push(link.issues[0].clone())
            }
        }
        children.sort();
        dependents.sort();
        related.sort();
        let mut prerequisites = issue.metadata.prerequisites.clone();
        prerequisites.sort();
        Ok(IssueRelations {
            repository: self.repository.clone(),
            issue: id.clone(),
            source: issue.source.clone(),
            fingerprint: self.fingerprint.clone(),
            parent: issue.metadata.parent.clone(),
            children,
            prerequisites,
            dependents,
            related,
            conditions: self.conditions(issue, true)?,
        })
    }
    pub fn readiness(&self, id: &IssueId) -> Result<IssueReadiness> {
        let issue = self.issue(id)?;
        let mut conditions = self.conditions(issue, false)?;
        if matches!(
            self.config.workflow.state(&issue.metadata.status)?.category,
            WorkflowCategory::Completed | WorkflowCategory::Canceled
        ) || issue.retirement.is_some()
        {
            conditions.push(condition(
                ConditionKind::Graph,
                issue,
                id,
                ConditionState::Unsatisfied,
                "issue_terminal",
                "terminal or retired issues are not ready for new work".into(),
                None,
            ));
        }
        Ok(IssueReadiness {
            repository: self.repository.clone(),
            issue: id.clone(),
            source: issue.source.clone(),
            fingerprint: self.fingerprint.clone(),
            ready: conditions
                .iter()
                .all(|c| c.state == ConditionState::Satisfied),
            conditions,
        })
    }
    pub fn fingerprint(&self) -> &ContentHash {
        &self.fingerprint
    }
    pub fn repository(&self) -> &RepositoryId {
        &self.repository
    }
    pub fn issues(&self) -> &[IssueRecord] {
        &self.issues
    }
    pub fn related_links(&self) -> &[RelatedIssueLink] {
        &self.related
    }
    pub fn waivers(&self) -> &[PrerequisiteWaiver] {
        &self.waivers
    }
    pub fn diagnostics(&self) -> &[CompletionCondition] {
        &self.diagnostics
    }
    pub fn dependency_path(&self, from: &IssueId, to: &IssueId) -> Result<IssueDependencyPath> {
        let subject = self.issue(from)?;
        self.issue(to)?;
        let edges = self.hard_edges();
        let index = self.index();
        let mut queue = VecDeque::from([from.clone()]);
        let mut parents = std::collections::BTreeMap::from([(from.clone(), None)]);
        let mut result = Vec::new();
        let mut conditions = Vec::new();
        let mut output_references = 0usize;
        while let Some(last) = queue.pop_front() {
            if &last == to {
                result = dependency_lineage(&parents, &last);
            }
            let target = index.get(&last).copied();
            let unresolved = match target {
                None => Some((
                    "missing_prerequisite",
                    format!("{last} is missing and its onward dependencies are unknown"),
                )),
                Some(issue) if issue.retirement.is_some() => Some((
                    "retired_requirement",
                    format!("{last} is retired and remains unresolved"),
                )),
                _ => None,
            };
            if let Some((code, message)) = unresolved {
                let mut c = condition(
                    ConditionKind::Prerequisite,
                    subject,
                    &last,
                    ConditionState::Unknown,
                    code,
                    message,
                    target,
                );
                c.path = dependency_lineage(&parents, &last)
                    .into_iter()
                    .map(SubjectRef::Issue)
                    .collect();
                charge_explanation(&mut output_references, c.path.len())?;
                c.source_pins.push(SourcePin {
                    path: "config.yml".into(),
                    content: self.sources[Path::new("config.yml")].clone(),
                });
                conditions.push(c);
            }
            if let Some(next) = edges.get(&last) {
                let mut next = next.clone();
                next.sort();
                for n in next {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        parents.entry(n.clone())
                    {
                        entry.insert(Some(last.clone()));
                        queue.push_back(n);
                    }
                }
            }
        }
        let mut reachable = edges;
        reachable.retain(|id, _| parents.contains_key(id));
        if let Some(path) = super::capture::cycle(&reachable) {
            charge_explanation(&mut output_references, path.len())?;
            let mut c = condition(
                ConditionKind::Graph,
                subject,
                &path[0],
                ConditionState::Unknown,
                "prerequisite_cycle",
                "reachable hard prerequisites contain a cycle".into(),
                None,
            );
            c.path = path.into_iter().map(SubjectRef::Issue).collect();
            conditions.push(c);
        }
        Ok(IssueDependencyPath {
            repository: self.repository.clone(),
            fingerprint: self.fingerprint.clone(),
            from: from.clone(),
            to: to.clone(),
            found: !result.is_empty(),
            path: result,
            basis: "hard_prerequisite_path_not_calendar_forecast".into(),
            conditions,
        })
    }
    fn waiver(
        &self,
        dependent: &IssueRecord,
        required: &IssueRecord,
    ) -> Result<Option<&PrerequisiteWaiver>> {
        if !self.config.acceptance.allow_prerequisite_waivers {
            return Ok(None);
        }
        let contract = requirement_hash(&dependent.metadata)?;
        let policy = policy_hash(&self.config)?;
        let index = self
            .waivers
            .binary_search_by(|w| {
                (&w.issue, &w.prerequisite).cmp(&(&dependent.metadata.id, &required.metadata.id))
            })
            .ok();
        Ok(index.and_then(|index| self.waivers.get(index)).filter(|w| {
            w.requirement_hash == contract
                && w.policy_hash == policy
                && w.prerequisite_source == required.source
        }))
    }
    pub(crate) fn conditions(
        &self,
        subject: &IssueRecord,
        children: bool,
    ) -> Result<Vec<CompletionCondition>> {
        let index = self.index();
        let mut queue = VecDeque::from([vec![subject.metadata.id.clone()]]);
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        let mut output_references = 0usize;
        while let Some(path) = queue.pop_front() {
            let id = path.last().unwrap();
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(owner) = index.get(id) else { continue };
            let mut edges = owner
                .metadata
                .prerequisites
                .iter()
                .cloned()
                .map(|i| (ConditionKind::Prerequisite, i))
                .collect::<Vec<_>>();
            if children && self.config.acceptance.require_completed_children {
                edges.extend(
                    self.issues
                        .iter()
                        .filter(|i| i.metadata.parent.as_ref() == Some(id))
                        .map(|i| (ConditionKind::Child, i.metadata.id.clone())),
                );
            }
            edges.sort_by(|a, b| a.1.cmp(&b.1));
            for (kind, target_id) in edges {
                let target = index.get(&target_id).copied();
                let mut waived = false;
                let (state, code, message, basis) = match target {
                    None => (
                        ConditionState::Unknown,
                        "missing_prerequisite",
                        format!("{target_id} is missing and remains unresolved"),
                        "declared",
                    ),
                    Some(target) if target.retirement.is_some() => (
                        ConditionState::Unknown,
                        "retired_requirement",
                        format!("{target_id} is retired and remains unresolved"),
                        "declared",
                    ),
                    Some(target)
                        if kind == ConditionKind::Prerequisite
                            && self.waiver(owner, target)?.is_some() =>
                    {
                        waived = true;
                        (
                            ConditionState::Satisfied,
                            "prerequisite_waived",
                            format!("{target_id} is explicitly waived under the captured policy"),
                            "policy_waiver",
                        )
                    }
                    Some(target) => {
                        let state = self
                            .config
                            .workflow
                            .state(&target.metadata.status)?
                            .category;
                        match state {
                            WorkflowCategory::Completed
                                if (self.config.acceptance.require_all_criteria
                                    && target.metadata.acceptance.iter().any(|c| !c.checked))
                                    || (self.config.acceptance.require_description
                                        && target.body.trim().is_empty()) =>
                            {
                                (
                                    ConditionState::Unsatisfied,
                                    "invalid_completed_declaration",
                                    format!(
                                        "{target_id} is marked completed but its current acceptance requirements are unresolved"
                                    ),
                                    "declared",
                                )
                            }
                            WorkflowCategory::Completed => (
                                ConditionState::Satisfied,
                                "completed_declaration",
                                format!("{target_id} is declared completed"),
                                if target.metadata.manual_acceptance.is_some() {
                                    "manual"
                                } else if target
                                    .metadata
                                    .imported_completion
                                    .as_ref()
                                    .is_some_and(ImportedCompletion::is_active)
                                {
                                    "imported"
                                } else {
                                    "declared"
                                },
                            ),
                            WorkflowCategory::Canceled => (
                                ConditionState::Unsatisfied,
                                "canceled_requirement",
                                format!(
                                    "{target_id} is canceled; explicitly resolve, replace, or waive the requirement"
                                ),
                                "declared",
                            ),
                            _ => (
                                ConditionState::Unsatisfied,
                                if kind == ConditionKind::Child {
                                    "unresolved_child"
                                } else {
                                    "incomplete_prerequisite"
                                },
                                format!("{target_id} remains {}", target.metadata.status),
                                "declared",
                            ),
                        }
                    }
                };
                let mut c = condition(kind, subject, &target_id, state, code, message, target);
                charge_explanation(&mut output_references, path.len() + 1)?;
                let mut next = path.clone();
                next.push(target_id.clone());
                c.path = next.iter().cloned().map(SubjectRef::Issue).collect();
                c.basis = basis.into();
                if !waived
                    && c.state != ConditionState::Satisfied
                    && self
                        .waivers
                        .iter()
                        .any(|w| w.issue == owner.metadata.id && w.prerequisite == target_id)
                {
                    c.message
                        .push_str("; the recorded waiver is stale or disabled by current policy");
                }
                if waived {
                    let w = self.waiver(owner, target.unwrap())?.unwrap();
                    if let Some(pin) = self.waiver_sources.get(&w.request_id) {
                        c.source_pins.push(pin.clone());
                    }
                    c.source_pins.push(SourcePin {
                        path: super::capture::waiver_path(&w.issue, &w.prerequisite),
                        content: self.sources
                            [&super::capture::waiver_path(&w.issue, &w.prerequisite)]
                            .clone(),
                    });
                }
                c.source_pins.push(SourcePin {
                    path: "config.yml".into(),
                    content: self.sources[Path::new("config.yml")].clone(),
                });
                out.push(c);
                if !waived && let Some(target) = target {
                    for gate in &target.metadata.gates {
                        if let Some(conditions) = self.gate_conditions.get(gate) {
                            for condition in conditions {
                                let mut condition = condition.clone();
                                let mut gate_path = next
                                    .iter()
                                    .cloned()
                                    .map(SubjectRef::Issue)
                                    .collect::<Vec<_>>();
                                gate_path.extend(condition.path);
                                charge_explanation(&mut output_references, gate_path.len())?;
                                condition.path = gate_path;
                                condition.message =
                                    format!("{}: {}", target.metadata.id, condition.message);
                                condition.source_pins.extend([
                                    SourcePin {
                                        path: subject.path.clone(),
                                        content: subject.source.content.clone(),
                                    },
                                    SourcePin {
                                        path: target.path.clone(),
                                        content: target.source.content.clone(),
                                    },
                                    SourcePin {
                                        path: "config.yml".into(),
                                        content: self.sources[Path::new("config.yml")].clone(),
                                    },
                                ]);
                                out.push(condition);
                            }
                        }
                    }
                    queue.push_back(next);
                }
            }
        }
        let mut edges = if children {
            self.completion_edges()
        } else {
            self.hard_edges()
        };
        edges.retain(|id, _| seen.contains(id));
        if let Some(path) = super::capture::cycle(&edges) {
            let mut c = condition(
                ConditionKind::Graph,
                subject,
                &path[0],
                ConditionState::Unknown,
                if children {
                    "completion_cycle"
                } else {
                    "prerequisite_cycle"
                },
                format!(
                    "unresolved cycle: {}",
                    path.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" -> ")
                ),
                None,
            );
            c.path = path.into_iter().map(SubjectRef::Issue).collect();
            out.push(c);
        }
        Ok(out)
    }
}

fn dependency_lineage(
    parents: &std::collections::BTreeMap<IssueId, Option<IssueId>>,
    id: &IssueId,
) -> Vec<IssueId> {
    let mut result = Vec::new();
    let mut current = Some(id.clone());
    while let Some(id) = current {
        current = parents[&id].clone();
        result.push(id);
    }
    result.reverse();
    result
}

fn charge_explanation(total: &mut usize, references: usize) -> Result<()> {
    *total = total
        .checked_add(references)
        .ok_or_else(|| unsupported("graph explanation size overflow"))?;
    if *total > MAX_EDGES {
        return Err(unsupported(
            "graph explanation exceeds 100000 path references",
        ));
    }
    Ok(())
}
pub(crate) fn completion_conditions(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueRecord,
) -> Result<Vec<CompletionCondition>> {
    let mut graph = capture(root, snapshot, config)?;
    if let Some(stored) = graph
        .issues
        .iter_mut()
        .find(|i| i.metadata.id == issue.metadata.id)
    {
        *stored = issue.clone();
    }
    graph.conditions(issue, true)
}
pub(crate) fn inspect(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (usize, Vec<PmError>) {
    match capture(root, snapshot, config) {
        Ok(graph) => {
            let errors = graph
                .diagnostics
                .iter()
                .map(|c| invalid(&c.message).details(json!(c)))
                .collect();
            (graph.related.len() + graph.waivers.len(), errors)
        }
        Err(e) => (0, vec![e]),
    }
}

pub(crate) fn retirement_blockers(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<(Vec<RetirementBlocker>, Option<ContentHash>)> {
    if target.kind != RetirementKind::Issue {
        return Ok((Vec::new(), None));
    }
    let graph = capture(root, snapshot, config)?;
    let id = target.id.parse::<IssueId>()?;
    let mut blockers = Vec::new();
    for issue in &graph.issues {
        let mut fields = Vec::new();
        if issue.metadata.parent.as_ref() == Some(&id) {
            fields.push("parent")
        }
        if issue.metadata.prerequisites.contains(&id) {
            fields.push("prerequisites")
        }
        if issue.metadata.id == id && issue.metadata.parent.is_some() {
            fields.push("parent_membership")
        }
        if graph
            .related
            .iter()
            .any(|r| r.issues.contains(&id) && r.issues.contains(&issue.metadata.id))
        {
            fields.push("related")
        }
        for field in fields {
            blockers.push(RetirementBlocker {
                issue: issue.metadata.id.clone(),
                path: issue.path.clone(),
                field: field.into(),
                source: issue.source.clone(),
            });
        }
    }
    let active = !blockers.is_empty()
        || graph
            .issues
            .iter()
            .any(|i| i.metadata.parent.is_some() || !i.metadata.prerequisites.is_empty())
        || !graph.related.is_empty()
        || !graph.waivers.is_empty();
    Ok((blockers, active.then_some(graph.fingerprint)))
}
