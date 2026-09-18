use super::*;
use crate::transactions::{Snapshot, canonical_hash};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) fn validate_metadata(issue: &IssueMetadata) -> Result<()> {
    if issue.parent.as_ref() == Some(&issue.id) || issue.prerequisites.contains(&issue.id) {
        return Err(invalid("an issue cannot be its own parent or prerequisite"));
    }
    for (name, values) in [
        (
            "prerequisites",
            issue
                .prerequisites
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ),
        (
            "features",
            issue.features.iter().map(ToString::to_string).collect(),
        ),
        (
            "gates",
            issue.gates.iter().map(ToString::to_string).collect(),
        ),
    ] {
        if values.len() > 256 || values.iter().collect::<BTreeSet<_>>().len() != values.len() {
            return Err(invalid(format!(
                "{name} requires at most 256 unique canonical references"
            )));
        }
    }
    Ok(())
}
pub(crate) fn related_path(a: &IssueId, b: &IssueId) -> Result<PathBuf> {
    if a == b {
        return Err(invalid("related endpoints must be different"));
    }
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    Ok(format!("relations/issues/{low}/{high}.yml").into())
}
pub(crate) fn waiver_path(a: &IssueId, b: &IssueId) -> PathBuf {
    format!("relations/waivers/{a}/{b}.yml").into()
}
pub(crate) fn validate_path(path: &Path) -> Result<()> {
    let p = path
        .components()
        .map(|c| c.as_os_str().to_str().unwrap_or(""))
        .collect::<Vec<_>>();
    match p.as_slice() {
        ["relations", kind, a, b]
            if (*kind == "issues" || *kind == "waivers") && b.ends_with(".yml") =>
        {
            let a = a.parse::<IssueId>()?;
            let b = b.trim_end_matches(".yml").parse::<IssueId>()?;
            if a == b || (*kind == "issues" && related_path(&a, &b)? != path) {
                return Err(invalid("noncanonical issue relation path").at(path));
            }
            Ok(())
        }
        _ => Err(invalid("unknown issue relation path").at(path)),
    }
}
pub(crate) fn requirement_hash(issue: &IssueMetadata) -> Result<ContentHash> {
    let mut prerequisites = issue.prerequisites.clone();
    prerequisites.sort();
    canonical_hash(
        &json!({"issue":issue.id,"prerequisites":prerequisites,"acceptance":issue.acceptance.iter().map(|a|(&a.id,&a.description)).collect::<Vec<_>>()}),
    )
}
pub(crate) fn policy_hash(config: &Config) -> Result<ContentHash> {
    canonical_hash(&json!({"repository":config.repository,"acceptance":config.acceptance}))
}

pub(crate) fn preflight(snapshot: &Snapshot<'_>) -> Result<BTreeMap<PathBuf, ContentHash>> {
    let issue_paths = snapshot.list_bounded(Path::new("issues"), MAX_ENTRIES)?;
    let mut item_total = 0usize;
    let mut sources = BTreeMap::new();
    for path in issue_paths {
        if path.file_name().is_some_and(|n| n == "item.md") && path.components().count() == 3 {
            if sources.len() >= MAX_NODES {
                return Err(unsupported("issue graph exceeds 10000 nodes"));
            }
            let bytes = snapshot
                .read_bounded(
                    &path,
                    MAX_BYTES
                        .saturating_sub(item_total)
                        .min(crate::documents::MAX_DOCUMENT_BYTES),
                )?
                .ok_or_else(|| invalid("issue disappeared"))?;
            item_total += bytes.len();
            sources.insert(path, ContentHash::of(&bytes));
        }
    }
    Ok(sources)
}
pub(crate) fn capture(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> Result<IssueGraphSnapshot> {
    let mut sources = preflight(snapshot)?;
    let config_bytes = snapshot
        .read_bounded(
            Path::new("config.yml"),
            crate::documents::MAX_DOCUMENT_BYTES,
        )?
        .ok_or_else(|| invalid("configuration disappeared"))?;
    sources.insert("config.yml".into(), ContentHash::of(&config_bytes));
    let mut issues = crate::issues::load_issues(root, snapshot, config)?;
    if issues.len() > MAX_NODES {
        return Err(unsupported("issue graph exceeds 10000 nodes"));
    }
    if !issues.is_empty() {
        let retirements = crate::retirement::RetirementIndex::capture(root, snapshot, config)?;
        for issue in &mut issues {
            issue.retirement = retirements.get(&crate::RetirementTarget::new(
                crate::RetirementKind::Issue,
                issue.metadata.id.as_str(),
            )?)?;
        }
    }
    let mut related = Vec::new();
    let mut waivers = Vec::new();
    let mut pins = Vec::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("relations"), MAX_ENTRIES)? {
        // Other typed relation namespaces belong to their own validators.
        if path.starts_with("relations/features") {
            continue;
        }
        if !path.starts_with("relations/issues") && !path.starts_with("relations/waivers") {
            return Err(invalid("unknown relation namespace").at(path));
        }
        validate_path(&path)?;
        let bytes = snapshot
            .read_bounded(
                &path,
                MAX_BYTES
                    .saturating_sub(total)
                    .min(crate::documents::MAX_DOCUMENT_BYTES),
            )?
            .ok_or_else(|| invalid("relation disappeared").at(&path))?;
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| unsupported("graph size overflow"))?;
        pins.push(json!({"path":path,"content":ContentHash::of(&bytes)}));
        sources.insert(path.clone(), ContentHash::of(&bytes));
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_slice(&bytes).map_err(|e| invalid(e.to_string()).at(&path))?;
        if value.get("schema").and_then(serde_yaml_ng::Value::as_u64) != Some(1) {
            return Err(PmError::new(
                ErrorCode::UnsupportedSchema,
                "unsupported graph record schema",
            )
            .at(path));
        }
        if path.starts_with("relations/issues") {
            let link: RelatedIssueLink =
                serde_yaml_ng::from_value(value).map_err(|e| invalid(e.to_string()).at(&path))?;
            if link.repository != config.repository
                || link.issues[0] >= link.issues[1]
                || related_path(&link.issues[0], &link.issues[1])? != path
            {
                return Err(invalid("related link identity differs from source").at(path));
            }
            related.push(link);
        } else {
            let waiver: PrerequisiteWaiver =
                serde_yaml_ng::from_value(value).map_err(|e| invalid(e.to_string()).at(&path))?;
            if waiver.repository != config.repository
                || waiver.issue == waiver.prerequisite
                || waiver_path(&waiver.issue, &waiver.prerequisite) != path
            {
                return Err(invalid("waiver identity differs from source").at(path));
            }
            text_value(&waiver.actor, "waiver actor")?;
            text_value(&waiver.reason, "waiver reason")?;
            waivers.push(waiver);
        }
    }
    let waiver_sources = super::proof::validate_waivers(snapshot, config, &waivers, &mut sources)?;
    waivers.sort_by(|a, b| (&a.issue, &a.prerequisite).cmp(&(&b.issue, &b.prerequisite)));
    let edge_count = issues
        .iter()
        .map(|i| i.metadata.prerequisites.len() + usize::from(i.metadata.parent.is_some()))
        .sum::<usize>()
        + related.len()
        + waivers.len();
    if edge_count > MAX_EDGES {
        return Err(unsupported("issue graph exceeds 100000 edges"));
    }
    let (gate_conditions, gate_sources) = capture_gate_conditions(root, snapshot, config, &issues)?;
    let mut fingerprint_input = json!({"config":ContentHash::of(&config_bytes),"issues":issues.iter().map(|i|json!({"id":i.metadata.id,"source":i.source,"retirement":i.retirement})).collect::<Vec<_>>(),"relations":pins,"decision_sources":sources.iter().filter(|(p,_)|p.starts_with("operations")).collect::<BTreeMap<_,_>>()});
    if !gate_conditions.is_empty() {
        fingerprint_input["gate_conditions"] = json!(gate_conditions);
        fingerprint_input["gate_sources"] = json!(gate_sources);
    }
    let fingerprint = canonical_hash(&fingerprint_input)?;
    sources.extend(gate_sources);
    let mut graph = IssueGraphSnapshot {
        schema: SchemaVersion::CURRENT,
        repository: config.repository.clone(),
        fingerprint,
        issues,
        related,
        waivers,
        diagnostics: Vec::new(),
        config: config.clone(),
        sources,
        waiver_sources,
        gate_conditions,
    };
    graph.diagnostics = diagnostics(&graph);
    Ok(graph)
}

type GateCapture = (
    BTreeMap<GateId, Vec<CompletionCondition>>,
    BTreeMap<PathBuf, ContentHash>,
);

fn capture_gate_conditions(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issues: &[IssueRecord],
) -> Result<GateCapture> {
    // A shared gate is evaluated once under the source snapshot. Pure graph
    // queries consume the retained conditions without opening files or locks.
    let mut gates = BTreeMap::new();
    let mut associations = 0usize;
    for issue in issues {
        associations += issue.metadata.gates.len();
        if associations > MAX_EDGES {
            return Err(unsupported("issue graph exceeds 100000 gate associations"));
        }
        for id in &issue.metadata.gates {
            gates.entry(id.clone()).or_insert(issue);
        }
    }
    if gates.len() > crate::gates::MAX_GATE_ENTRIES {
        return Err(unsupported("issue graph exceeds 4096 referenced gates"));
    }
    let mut sources = BTreeMap::new();
    let mut total = 0usize;
    let mut criterion_namespaces = BTreeSet::new();
    for id in gates.keys() {
        let path = crate::gates::store::path(id);
        if let Some(bytes) = snapshot.read_bounded(&path, crate::MAX_GATE_BYTES)? {
            total += bytes.len();
            if total > MAX_BYTES {
                return Err(unsupported(
                    "graph gate and criterion sources exceed 64 MiB",
                ));
            }
            sources.insert(path.clone(), ContentHash::of(&bytes));
            if let Ok(record) = crate::gates::parse(&path, &bytes, &config.repository) {
                for requirement in record.definition.requirements {
                    match requirement.criterion.owner {
                        CriterionOwner::Issue(_) => (), // All issue item bytes were preflighted.
                        CriterionOwner::Feature(_) => {
                            criterion_namespaces.insert("features");
                        }
                        CriterionOwner::Milestone(_) => {
                            criterion_namespaces.insert("milestones");
                        }
                        CriterionOwner::Project(_) => {
                            criterion_namespaces.insert("projects");
                        }
                    }
                }
            }
        }
    }
    // Criterion resolution scans these catalogs. Bind their bytes even when a
    // malformed or absent owner produces Unknown rather than a resolved pin.
    for namespace in criterion_namespaces {
        for path in snapshot.list_bounded(Path::new(namespace), MAX_ENTRIES)? {
            let bytes = snapshot
                .read_bounded(
                    &path,
                    MAX_BYTES
                        .saturating_sub(total)
                        .min(crate::documents::MAX_DOCUMENT_BYTES),
                )?
                .ok_or_else(|| invalid("criterion source disappeared").at(&path))?;
            total += bytes.len();
            sources.insert(path, ContentHash::of(&bytes));
        }
    }
    let mut captured = BTreeMap::new();
    let mut condition_count = 0usize;
    let mut condition_bytes = 0usize;
    for (id, issue) in gates {
        let mut single_gate = issue.clone();
        single_gate.metadata.gates = vec![id.clone()];
        let mut conditions = crate::gates::issue_conditions(root, snapshot, config, &single_gate)?;
        let path = crate::gates::store::path(&id);
        for condition in &mut conditions {
            if let Some(content) = sources.get(&path)
                && !condition.source_pins.iter().any(|pin| pin.path == path)
            {
                condition.source_pins.push(SourcePin {
                    path: path.clone(),
                    content: content.clone(),
                });
            }
            for pin in &condition.source_pins {
                sources.insert(pin.path.clone(), pin.content.clone());
            }
        }
        condition_count += conditions.len();
        condition_bytes += serde_json::to_vec(&conditions)
            .map_err(|error| invalid(error.to_string()))?
            .len();
        if condition_count > MAX_EDGES || condition_bytes > MAX_BYTES {
            return Err(unsupported(
                "captured gate explanations exceed 100000 conditions or 64 MiB",
            ));
        }
        captured.insert(id, conditions);
    }
    Ok((captured, sources))
}
pub(crate) fn condition(
    kind: ConditionKind,
    subject: &IssueRecord,
    other: &IssueId,
    state: ConditionState,
    code: &str,
    message: String,
    target: Option<&IssueRecord>,
) -> CompletionCondition {
    let mut pins = vec![SourcePin {
        path: subject.path.clone(),
        content: subject.source.content.clone(),
    }];
    if let Some(target) = target {
        pins.push(SourcePin {
            path: target.path.clone(),
            content: target.source.content.clone(),
        });
    }
    CompletionCondition {
        kind,
        subject: SubjectRef::Issue(subject.metadata.id.clone()),
        related_subject: Some(SubjectRef::Issue(other.clone())),
        state,
        reason_code: code.into(),
        message,
        path: vec![
            SubjectRef::Issue(subject.metadata.id.clone()),
            SubjectRef::Issue(other.clone()),
        ],
        source_pins: pins,
        basis: "declared".into(),
    }
}
fn diagnostics(graph: &IssueGraphSnapshot) -> Vec<CompletionCondition> {
    let mut out = Vec::new();
    let index = graph.index();
    for issue in &graph.issues {
        for (kind, target) in issue
            .metadata
            .prerequisites
            .iter()
            .map(|i| (ConditionKind::Prerequisite, i))
            .chain(
                issue
                    .metadata
                    .parent
                    .iter()
                    .map(|i| (ConditionKind::Child, i)),
            )
        {
            if !index.contains_key(target) {
                out.push(condition(
                    kind,
                    issue,
                    target,
                    ConditionState::Unknown,
                    "dangling_reference",
                    format!("{} has missing reference {target}", issue.metadata.id),
                    None,
                ));
            }
        }
    }
    for link in &graph.related {
        for (a, b) in [
            (&link.issues[0], &link.issues[1]),
            (&link.issues[1], &link.issues[0]),
        ] {
            if !index.contains_key(b) {
                if let Some(issue) = index.get(a) {
                    out.push(condition(
                        ConditionKind::Graph,
                        issue,
                        b,
                        ConditionState::Unknown,
                        "dangling_related",
                        format!("related issue {b} is missing"),
                        None,
                    ));
                } else {
                    out.push(CompletionCondition {
                        kind: ConditionKind::Graph,
                        subject: SubjectRef::Issue(a.clone()),
                        related_subject: Some(SubjectRef::Issue(b.clone())),
                        state: ConditionState::Unknown,
                        reason_code: "dangling_related".into(),
                        message: format!("related endpoints {a} and {b} are missing"),
                        path: vec![SubjectRef::Issue(a.clone()), SubjectRef::Issue(b.clone())],
                        source_pins: vec![SourcePin {
                            path: related_path(a, b).expect("validated pair"),
                            content: graph.sources[&related_path(a, b).expect("validated pair")]
                                .clone(),
                        }],
                        basis: "declared".into(),
                    });
                }
            }
        }
    }
    for waiver in &graph.waivers {
        if !index.contains_key(&waiver.issue)
            || !index.contains_key(&waiver.prerequisite)
            || index
                .get(&waiver.issue)
                .is_some_and(|owner| !owner.metadata.prerequisites.contains(&waiver.prerequisite))
        {
            let path = waiver_path(&waiver.issue, &waiver.prerequisite);
            out.push(CompletionCondition {
                kind: ConditionKind::Prerequisite,
                subject: SubjectRef::Issue(waiver.issue.clone()),
                related_subject: Some(SubjectRef::Issue(waiver.prerequisite.clone())),
                state: ConditionState::Unknown,
                reason_code: "orphan_waiver".into(),
                message: format!(
                    "waiver for {} -> {} has no current issue, prerequisite, or requirement edge",
                    waiver.issue, waiver.prerequisite
                ),
                path: vec![
                    SubjectRef::Issue(waiver.issue.clone()),
                    SubjectRef::Issue(waiver.prerequisite.clone()),
                ],
                source_pins: vec![SourcePin {
                    content: graph.sources[&path].clone(),
                    path,
                }],
                basis: "policy_waiver".into(),
            });
        }
    }
    for (name, edges) in [
        ("parent_cycle", graph.parent_edges()),
        ("prerequisite_cycle", graph.hard_edges()),
        ("completion_cycle", graph.completion_edges()),
    ] {
        if let Some(path) = cycle(&edges)
            && let Some(issue) = index.get(&path[0])
        {
            let mut c = condition(
                ConditionKind::Graph,
                issue,
                &path[0],
                ConditionState::Unknown,
                name,
                format!(
                    "{name}: {}",
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
    }
    out
}
/// Iterative DFS includes an exact cycle witness; no recursive stack growth.
pub(crate) fn cycle(edges: &BTreeMap<IssueId, Vec<IssueId>>) -> Option<Vec<IssueId>> {
    let mut colors = BTreeMap::new();
    for start in edges.keys() {
        if colors.contains_key(start) {
            continue;
        }
        let mut stack = vec![(start.clone(), 0usize)];
        colors.insert(start.clone(), 1u8);
        while let Some((node, next)) = stack.last_mut() {
            let children = edges.get(node).map(Vec::as_slice).unwrap_or(&[]);
            if *next == children.len() {
                colors.insert(node.clone(), 2);
                stack.pop();
                continue;
            }
            let target = children[*next].clone();
            *next += 1;
            match colors.get(&target) {
                Some(1) => {
                    let from = stack.iter().position(|(i, _)| i == &target).unwrap();
                    let mut path = stack[from..]
                        .iter()
                        .map(|(i, _)| i.clone())
                        .collect::<Vec<_>>();
                    path.push(target);
                    return Some(path);
                }
                Some(2) => {}
                _ => {
                    colors.insert(target.clone(), 1);
                    stack.push((target, 0));
                }
            }
        }
    }
    None
}
impl IssueGraphSnapshot {
    pub(crate) fn index(&self) -> BTreeMap<IssueId, &IssueRecord> {
        self.issues
            .iter()
            .map(|i| (i.metadata.id.clone(), i))
            .collect()
    }
    pub(crate) fn hard_edges(&self) -> BTreeMap<IssueId, Vec<IssueId>> {
        self.issues
            .iter()
            .map(|i| (i.metadata.id.clone(), i.metadata.prerequisites.clone()))
            .collect()
    }
    fn parent_edges(&self) -> BTreeMap<IssueId, Vec<IssueId>> {
        self.issues
            .iter()
            .map(|i| {
                (
                    i.metadata.id.clone(),
                    i.metadata.parent.iter().cloned().collect(),
                )
            })
            .collect()
    }
    pub(crate) fn completion_edges(&self) -> BTreeMap<IssueId, Vec<IssueId>> {
        let mut edges = self.hard_edges();
        if self.config.acceptance.require_completed_children {
            for issue in &self.issues {
                if let Some(parent) = &issue.metadata.parent {
                    edges
                        .entry(parent.clone())
                        .or_default()
                        .push(issue.metadata.id.clone());
                }
            }
        }
        edges
    }
}
