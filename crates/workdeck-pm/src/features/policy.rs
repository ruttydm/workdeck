use super::*;
use crate::{
    Config, IssueMetadata, PlanningKind, RetirementKind, RetirementTarget, transactions::Snapshot,
};
use std::path::Path;

fn require_feature(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    index: &BTreeMap<&FeatureId, &FeatureMetadata>,
    id: &FeatureId,
) -> Result<()> {
    if !index.contains_key(id) {
        return Err(PmError::new(
            ErrorCode::NotFound,
            format!("feature association {id} does not resolve; create it first"),
        ));
    }
    crate::retirement::ensure_writable(
        root,
        snapshot,
        config,
        &RetirementTarget::new(RetirementKind::Feature, id.as_str())?,
    )
}
pub(crate) fn validate_issue_associations(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    old: Option<&IssueMetadata>,
    next: &IssueMetadata,
) -> Result<()> {
    let added = next
        .features
        .iter()
        .filter(|id| old.is_none_or(|old| !old.features.contains(id)))
        .collect::<Vec<_>>();
    if added.is_empty() {
        return Ok(());
    }
    let records = load_features(snapshot, config)?;
    let index = validation_index(&records);
    for id in added {
        require_feature(root, snapshot, config, &index, id)?;
    }
    Ok(())
}
pub(crate) fn validate_change(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    records: &[FeatureRecord],
    old: Option<&FeatureMetadata>,
    next: &FeatureMetadata,
    required: bool,
) -> Result<()> {
    validate_change_indexed(
        root,
        snapshot,
        config,
        &validation_index(records),
        old,
        next,
        required,
    )
}

pub(super) fn validate_change_indexed(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    index: &BTreeMap<&FeatureId, &FeatureMetadata>,
    old: Option<&FeatureMetadata>,
    next: &FeatureMetadata,
    required: bool,
) -> Result<()> {
    for (kind, previous, ids) in [
        (
            PlanningKind::Project,
            old.map(|m| &m.projects),
            &next.projects,
        ),
        (
            PlanningKind::Milestone,
            old.map(|m| &m.milestones),
            &next.milestones,
        ),
        (PlanningKind::Target, old.map(|m| &m.targets), &next.targets),
    ] {
        let added = ids
            .iter()
            .filter(|id| previous.is_none_or(|values| !values.contains(id)))
            .collect::<Vec<_>>();
        if added.is_empty() {
            continue;
        }
        let available = crate::planning::store::list_planning(root, snapshot, kind)?;
        for id in added {
            let record = available
                .iter()
                .find(|record| record.metadata.id.eq_ignore_ascii_case(id))
                .ok_or_else(|| {
                    PmError::new(
                        ErrorCode::NotFound,
                        format!(
                            "feature {kind:?} association {id:?} does not resolve; create it first"
                        ),
                    )
                })?;
            if &record.metadata.id != id {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    format!("use canonical planning identity {:?}", record.metadata.id),
                ));
            }
            crate::retirement::ensure_writable(
                root,
                snapshot,
                config,
                &RetirementTarget::new(kind.into(), id)?,
            )?;
        }
    }
    if old.map(|old| &old.parent) != Some(&next.parent)
        && let Some(id) = &next.parent
    {
        require_feature(root, snapshot, config, index, id)?;
    }
    for id in &next.prerequisites {
        if old.is_none_or(|old| !old.prerequisites.contains(id)) {
            require_feature(root, snapshot, config, index, id)?;
        }
    }
    for parent in [true, false] {
        let changed = if parent {
            old.map(|old| &old.parent) != Some(&next.parent)
        } else {
            old.map(|old| &old.prerequisites) != Some(&next.prerequisites)
        };
        if !changed {
            continue;
        }
        let edges = |metadata: &FeatureMetadata| -> Vec<FeatureId> {
            if parent {
                metadata.parent.iter().cloned().collect()
            } else {
                metadata.prerequisites.clone()
            }
        };
        let mut pending = edges(next);
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if id == next.id {
                return Err(PmError::new(
                    ErrorCode::PolicyBlocked,
                    if parent {
                        "feature parent change would create a cycle"
                    } else {
                        "feature prerequisite change would create a cycle"
                    },
                ));
            }
            if !visited.insert(id.clone()) {
                continue;
            }
            if let Some(metadata) = index.get(&id) {
                pending.extend(edges(metadata));
            }
        }
    }
    crate::gates::validate_associations(
        snapshot,
        config,
        old.map_or(&[], |old| old.gates.as_slice()),
        &next.gates,
    )?;
    crate::organization::validate_feature_change(snapshot, config, old, next, required)
}

pub(crate) fn criterion(
    snapshot: &Snapshot<'_>,
    config: &Config,
    id: &FeatureId,
    criterion: &str,
) -> Result<(PlanningCriterion, SourceToken, PathBuf)> {
    let record = load_feature(snapshot, config, id)?;
    let definition = record
        .metadata
        .criteria
        .iter()
        .find(|item| item.id == criterion)
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "feature criterion does not resolve"))?
        .clone();
    Ok((definition, record.source, record.path))
}

/// Source audit is independent of prospective association and organization policy.
/// Iterative traversal retains a bounded cycle witness even for long feature chains.
pub(super) fn graph_errors(records: &[FeatureRecord]) -> Vec<PmError> {
    let index = records
        .iter()
        .map(|record| (&record.metadata.id, record))
        .collect::<BTreeMap<_, _>>();
    let mut errors = Vec::new();
    for parent in [true, false] {
        let edges = records
            .iter()
            .map(|record| {
                (
                    &record.metadata.id,
                    if parent {
                        record.metadata.parent.iter().collect::<Vec<_>>()
                    } else {
                        record.metadata.prerequisites.iter().collect()
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut colors: BTreeMap<&FeatureId, u8> = BTreeMap::new();
        let mut witness = None;
        'walk: for start in edges.keys() {
            if colors.contains_key(start) {
                continue;
            }
            let mut stack = vec![(*start, 0usize)];
            colors.insert(start, 1);
            while let Some((node, next)) = stack.last_mut() {
                let targets = edges.get(node).map(Vec::as_slice).unwrap_or(&[]);
                if *next == targets.len() {
                    colors.insert(node, 2);
                    stack.pop();
                    continue;
                }
                let target = targets[*next];
                *next += 1;
                match colors.get(target) {
                    Some(1) => {
                        let offset = stack
                            .iter()
                            .position(|(id, _)| *id == target)
                            .expect("gray node is on stack");
                        let length = stack.len() - offset + 1;
                        let mut path = stack[offset..]
                            .iter()
                            .take(63)
                            .map(|(id, _)| (*id).clone())
                            .collect::<Vec<_>>();
                        path.push(target.clone());
                        witness = Some((target, path, length));
                        break 'walk;
                    }
                    Some(2) => {}
                    _ => {
                        colors.insert(target, 1);
                        stack.push((target, 0));
                    }
                }
            }
        }
        if let Some((id, witness, length)) = witness {
            let kind = if parent { "parent" } else { "prerequisite" };
            let error=PmError::new(ErrorCode::InvalidSchema,format!("feature {kind} graph contains a cycle"))
                .details(serde_json::json!({"kind":kind,"cycle":witness,"length":length,"truncated":length>64}))
                .hint("Clear or reassign a cycle edge in the authored feature source; completed issues cannot qualify an invalid feature graph.");
            errors.push(if let Some(record) = index.get(id) {
                error.at(&record.path)
            } else {
                error
            });
        }
    }
    errors
}

#[cfg(test)]
thread_local! { static VALIDATION_INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
pub(super) fn validation_index(
    records: &[FeatureRecord],
) -> BTreeMap<&FeatureId, &FeatureMetadata> {
    #[cfg(test)]
    VALIDATION_INDEX_BUILDS.with(|count| count.set(count.get() + 1));
    records
        .iter()
        .map(|record| (&record.metadata.id, &record.metadata))
        .collect()
}

#[cfg(test)]
mod scale_tests {
    use super::*;
    #[test]
    fn feature_diagnostics_share_one_graph_index() {
        let temporary = tempfile::tempdir().unwrap();
        let repository = crate::Repository::init(temporary.path(), "WD").unwrap();
        std::fs::create_dir_all(repository.root().join("features")).unwrap();
        for index in 0..80 {
            let id: FeatureId = format!("FEAT-{index:026}").parse().unwrap();
            let metadata: FeatureMetadata = serde_json::from_value(serde_json::json!({
                "schema":1,"repository":repository.identity(),"id":id,"revision":1,
                "name":format!("Feature {index}"),"created_at":"2026-09-09T00:00:00Z",
                "updated_at":"2026-09-09T00:00:00Z",
                "parent":(index>0).then(||format!("FEAT-{:026}",(index-1)/8))
            }))
            .unwrap();
            std::fs::write(
                repository.root().join(format!("features/{id}.md")),
                format!(
                    "---\n{}---\nBody\n",
                    serde_yaml_ng::to_string(&metadata).unwrap()
                ),
            )
            .unwrap();
        }
        VALIDATION_INDEX_BUILDS.with(|count| count.set(0));
        let report = repository.doctor().unwrap();
        assert!(report.valid, "{report:?}");
        let builds = VALIDATION_INDEX_BUILDS.with(|count| count.get());
        assert!(
            builds <= 1,
            "one captured feature graph was rebuilt {builds} times"
        );
    }
}
