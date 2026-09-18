use super::*;
use crate::transactions::{Snapshot, canonical_hash};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
};

/// Canonical declarations for the reachable completion contract. Workflow state,
/// checked boxes, comments and summaries are deliberately not requirement text.
pub(super) fn fingerprint(
    root: &Path,
    snapshot: &Snapshot<'_>,
    captured: &capture::Capture,
    issue: &IssueRecord,
) -> Result<ContentHash> {
    let index = captured
        .graph
        .issues()
        .iter()
        .map(|i| (i.metadata.id.clone(), i))
        .collect::<BTreeMap<_, _>>();
    let mut children = BTreeMap::<IssueId, Vec<IssueId>>::new();
    if captured.config.acceptance.require_completed_children {
        for other in captured.graph.issues() {
            if let Some(parent) = &other.metadata.parent {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(other.metadata.id.clone());
            }
        }
    }
    let mut queue = VecDeque::from([issue.metadata.id.clone()]);
    let mut seen = BTreeSet::new();
    let mut contracts = BTreeMap::new();
    let mut gates = BTreeSet::new();
    let mut features = BTreeSet::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if seen.len() > 10_000 {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "context requirement traversal exceeds 10000 issue references",
            ));
        }
        let Some(record) = index.get(&id) else {
            contracts.insert(id, json!({"unresolved":true}));
            continue;
        };
        let mut criteria = record
            .metadata
            .acceptance
            .iter()
            .map(|c| (&c.id, &c.description))
            .collect::<Vec<_>>();
        criteria.sort();
        let mut prerequisites = record.metadata.prerequisites.clone();
        prerequisites.sort();
        let mut required_children = children.get(&id).cloned().unwrap_or_default();
        required_children.sort();
        queue.extend(prerequisites.iter().cloned());
        queue.extend(required_children.iter().cloned());
        gates.extend(record.metadata.gates.iter().cloned());
        features.extend(record.metadata.features.iter().cloned());
        contracts.insert(id,json!({"criteria":criteria,"prerequisites":prerequisites,"children":required_children,"gates":record.metadata.gates,"features":record.metadata.features}));
    }
    let mut feature_contracts = BTreeMap::new();
    if !features.is_empty() {
        for feature in crate::features::load_features(snapshot, &captured.config)? {
            if features.contains(&feature.metadata.id) {
                gates.extend(feature.metadata.gates.iter().cloned());
                feature_contracts.insert(feature.metadata.id.clone(),json!({"criteria":feature.metadata.criteria,"decision":feature.metadata.decision,"gates":feature.metadata.gates,"prerequisites":feature.metadata.prerequisites}));
            }
        }
    }
    let mut gate_contracts = BTreeMap::new();
    let mut criterion_contracts = BTreeMap::new();
    for id in gates {
        match crate::gates::load_gate(snapshot, &captured.config, &id) {
            Ok(record) => {
                for requirement in &record.definition.requirements {
                    let key =
                        serde_json::to_string(&requirement.criterion).map_err(serialization)?;
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        criterion_contracts.entry(key)
                    {
                        let resolved = match crate::gates::resolve_criterion(
                            root,
                            snapshot,
                            &captured.config,
                            &requirement.criterion.owner,
                            &requirement.criterion.id,
                        ) {
                            Ok(current) => {
                                json!({"definition":current.reference,"retired":current.retired})
                            }
                            Err(error)
                                if matches!(
                                    error.code,
                                    ErrorCode::NotFound
                                        | ErrorCode::InvalidSchema
                                        | ErrorCode::UnsupportedSchema
                                ) =>
                            {
                                json!({"unresolved":error.code})
                            }
                            Err(error) => return Err(error),
                        };
                        entry.insert(resolved);
                    }
                }
                gate_contracts.insert(id,json!({"requirements":record.definition.requirements,"archived":record.definition.archived}));
            }
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::NotFound | ErrorCode::InvalidSchema | ErrorCode::UnsupportedSchema
                ) =>
            {
                gate_contracts.insert(id, json!({"unresolved":error.code}));
            }
            Err(error) => return Err(error),
        }
    }
    canonical_hash(
        &json!({"repository":captured.config.repository,"issue":issue.metadata.id,"policy":captured.config.acceptance,"issues":contracts,"features":feature_contracts,"gates":gate_contracts,"criteria":criterion_contracts}),
    )
}
