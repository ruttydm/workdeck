//! Prospective association checks and derived membership from one source snapshot.
use super::{PlanningKind, PlanningMetadata, PlanningRecord, store};
use crate::{
    Config, ErrorCode, IssueId, IssueMetadata, IssueRecord, PmError, Result, RetirementTarget,
    transactions::Snapshot,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) const KINDS: [PlanningKind; 6] = [
    PlanningKind::Initiative,
    PlanningKind::Project,
    PlanningKind::Milestone,
    PlanningKind::Cycle,
    PlanningKind::Target,
    PlanningKind::Label,
];

struct Index {
    records: BTreeMap<(PlanningKind, String), PlanningRecord>,
}
impl Index {
    fn capture(root: &Path, snapshot: &Snapshot<'_>) -> Result<Self> {
        Self::capture_kinds(root, snapshot, KINDS)
    }
    fn capture_kinds(
        root: &Path,
        snapshot: &Snapshot<'_>,
        kinds: impl IntoIterator<Item = PlanningKind>,
    ) -> Result<Self> {
        let mut records = BTreeMap::new();
        for kind in kinds {
            for record in store::list_planning(root, snapshot, kind)? {
                records.insert((kind, record.metadata.id.to_ascii_lowercase()), record);
            }
        }
        Ok(Self { records })
    }
    fn get(&self, kind: PlanningKind, id: &str) -> Option<&PlanningRecord> {
        self.records.get(&(kind, id.to_ascii_lowercase()))
    }
    fn require(
        &self,
        root: &Path,
        snapshot: &Snapshot<'_>,
        config: &Config,
        kind: PlanningKind,
        id: &str,
    ) -> Result<&PlanningRecord> {
        let record = self.get(kind, id).ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                format!(
                    "{kind:?} association {id:?} does not resolve; create the planning record first"
                ),
            )
        })?;
        if id != record.metadata.id {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                format!(
                    "association {id:?} must use canonical identity {:?}",
                    record.metadata.id
                ),
            ));
        }
        crate::retirement::ensure_writable(
            root,
            snapshot,
            config,
            &RetirementTarget::new(kind.into(), &record.metadata.id)?,
        )?;
        Ok(record)
    }
}

fn optional_changed(before: Option<&Option<String>>, after: &Option<String>) -> bool {
    before != Some(after)
}
fn collection_changed(before: Option<&Vec<String>>, after: &[String]) -> bool {
    before.is_none_or(|values| values != after)
}
fn unique_associations(values: &[String]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for id in values {
        super::validate_id(id)?;
        if !ids.insert(id.to_ascii_lowercase()) {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "association IDs must be unique, including case aliases",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_issue_change(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    before: Option<&IssueMetadata>,
    after: &IssueMetadata,
) -> Result<()> {
    let changed = optional_changed(before.map(|item| &item.project), &after.project)
        || optional_changed(before.map(|item| &item.cycle), &after.cycle)
        || optional_changed(before.map(|item| &item.milestone), &after.milestone)
        || collection_changed(before.map(|item| &item.labels), &after.labels)
        || collection_changed(before.map(|item| &item.targets), &after.targets);
    if !changed {
        return Ok(());
    }
    let mut needed = BTreeSet::new();
    for (kind, old, new) in [
        (
            PlanningKind::Project,
            before.map(|item| &item.project),
            &after.project,
        ),
        (
            PlanningKind::Cycle,
            before.map(|item| &item.cycle),
            &after.cycle,
        ),
        (
            PlanningKind::Milestone,
            before.map(|item| &item.milestone),
            &after.milestone,
        ),
    ] {
        if optional_changed(old, new) && new.is_some() {
            needed.insert(kind);
        }
    }
    for (kind, old, new) in [
        (
            PlanningKind::Label,
            before.map(|item| &item.labels),
            &after.labels,
        ),
        (
            PlanningKind::Target,
            before.map(|item| &item.targets),
            &after.targets,
        ),
    ] {
        if collection_changed(old, new) {
            unique_associations(new)?;
            if new
                .iter()
                .any(|id| old.is_none_or(|values| !values.contains(id)))
            {
                needed.insert(kind);
            }
        }
    }
    if after.milestone.is_some()
        && (optional_changed(before.map(|item| &item.project), &after.project)
            || optional_changed(before.map(|item| &item.milestone), &after.milestone))
    {
        needed.insert(PlanningKind::Milestone);
    }
    if needed.is_empty() {
        return Ok(());
    }
    let index = Index::capture_kinds(root, snapshot, needed)?;
    for (kind, old, new) in [
        (
            PlanningKind::Project,
            before.map(|item| &item.project),
            &after.project,
        ),
        (
            PlanningKind::Cycle,
            before.map(|item| &item.cycle),
            &after.cycle,
        ),
        (
            PlanningKind::Milestone,
            before.map(|item| &item.milestone),
            &after.milestone,
        ),
    ] {
        if optional_changed(old, new)
            && let Some(id) = new
        {
            index.require(root, snapshot, config, kind, id)?;
        }
    }
    for (kind, old, new) in [
        (
            PlanningKind::Label,
            before.map(|item| &item.labels),
            &after.labels,
        ),
        (
            PlanningKind::Target,
            before.map(|item| &item.targets),
            &after.targets,
        ),
    ] {
        if collection_changed(old, new) {
            unique_associations(new)?;
            for id in new {
                // Retained historical declarations are not newly authored when
                // another collection member is removed or added.
                if old.is_none_or(|values| !values.contains(id)) {
                    index.require(root, snapshot, config, kind, id)?;
                }
            }
        }
    }
    if (optional_changed(before.map(|item| &item.project), &after.project)
        || optional_changed(before.map(|item| &item.milestone), &after.milestone))
        && let Some(id) = &after.milestone
    {
        let milestone = index.require(root, snapshot, config, PlanningKind::Milestone, id)?;
        if !after
            .project
            .as_ref()
            .zip(milestone.metadata.project.as_ref())
            .is_some_and(|(issue, owner)| issue.eq_ignore_ascii_case(owner))
        {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "issue project must match its milestone's owning project; set both associations together or clear the milestone",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_planning_change(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    kind: PlanningKind,
    before: Option<&PlanningMetadata>,
    after: &PlanningMetadata,
) -> Result<()> {
    let index = Index::capture(root, snapshot)?;
    for (target_kind, old, new) in [
        (
            PlanningKind::Initiative,
            before.map(|item| &item.initiative),
            &after.initiative,
        ),
        (
            PlanningKind::Project,
            before.map(|item| &item.project),
            &after.project,
        ),
    ] {
        if optional_changed(old, new)
            && let Some(id) = new
        {
            index.require(root, snapshot, config, target_kind, id)?;
        }
    }
    if collection_changed(before.map(|item| &item.targets), &after.targets) {
        for id in &after.targets {
            if before.is_none_or(|old| !old.targets.contains(id)) {
                index.require(root, snapshot, config, PlanningKind::Target, id)?;
            }
        }
    }
    if kind == PlanningKind::Milestone
        && optional_changed(before.map(|item| &item.project), &after.project)
    {
        for issue in crate::issues::load_issues(root, snapshot, config)? {
            if issue
                .metadata
                .milestone
                .as_ref()
                .is_some_and(|id| id.eq_ignore_ascii_case(&after.id))
                && !issue
                    .metadata
                    .project
                    .as_ref()
                    .zip(after.project.as_ref())
                    .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
            {
                return Err(PmError::new(ErrorCode::PolicyBlocked,
                    format!("milestone reassignment would conflict with issue {}; clear or reassign that issue's milestone first", issue.metadata.id)).at(issue.path));
            }
        }
    }
    Ok(())
}

/// Targets are derived from direct issue, project, and milestone associations.
/// Archived destinations remain resolvable; retired destinations have no current members.
pub(crate) fn target_memberships(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issues: &[IssueRecord],
) -> Result<BTreeMap<String, BTreeSet<IssueId>>> {
    let index = Index::capture(root, snapshot)?;
    let mut memberships = BTreeMap::new();
    for record in index
        .records
        .values()
        .filter(|record| record.kind == PlanningKind::Target)
    {
        let target = RetirementTarget::new(PlanningKind::Target.into(), &record.metadata.id)?;
        if crate::retirement::read_tombstone(root, snapshot, config, &target)?.is_none() {
            memberships.insert(record.metadata.id.clone(), BTreeSet::new());
        }
    }
    for issue in issues {
        let project = issue
            .metadata
            .project
            .as_ref()
            .and_then(|id| index.get(PlanningKind::Project, id));
        let milestone = issue
            .metadata
            .milestone
            .as_ref()
            .and_then(|id| index.get(PlanningKind::Milestone, id));
        let ids = issue
            .metadata
            .targets
            .iter()
            .chain(
                project
                    .into_iter()
                    .flat_map(|record| &record.metadata.targets),
            )
            .chain(
                milestone
                    .into_iter()
                    .flat_map(|record| &record.metadata.targets),
            );
        for id in ids {
            if let Some(target) = index.get(PlanningKind::Target, id)
                && let Some(members) = memberships.get_mut(&target.metadata.id)
            {
                members.insert(issue.metadata.id.clone());
            }
        }
    }
    Ok(memberships)
}

/// Explicit read diagnostics. These are warnings for retained declarations,
/// rather than new-write validation that would invalidate historical snapshots.
pub(crate) fn diagnostics(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> Result<Vec<PmError>> {
    let index = Index::capture(root, snapshot)?;
    let mut warnings = Vec::new();
    let mut inspect = |path: &Path, kind: PlanningKind, id: &str| {
        match index.get(kind, id) {
            None => warnings.push(PmError::new(ErrorCode::NotFound,
                format!("unresolved retained {kind:?} association {id:?}; create or explicitly replace the reference")).at(root.join(path))),
            Some(record) if record.metadata.id != id => warnings.push(PmError::new(ErrorCode::InvalidInput,
                format!("retained {kind:?} case alias {id:?} differs from canonical identity {:?}; explicitly update the association to appear in canonical membership queries", record.metadata.id)).at(root.join(path))),
            Some(_) => {}
        }
    };
    for record in index.records.values() {
        for (kind, values) in [
            (
                PlanningKind::Initiative,
                record.metadata.initiative.iter().collect::<Vec<_>>(),
            ),
            (
                PlanningKind::Project,
                record.metadata.project.iter().collect(),
            ),
            (
                PlanningKind::Target,
                record.metadata.targets.iter().collect(),
            ),
        ] {
            for id in values {
                inspect(&record.path, kind, id);
            }
        }
    }
    let issues = crate::issues::load_issues(root, snapshot, config)?;
    for issue in &issues {
        for (kind, values) in [
            (
                PlanningKind::Project,
                issue.metadata.project.iter().collect::<Vec<_>>(),
            ),
            (PlanningKind::Cycle, issue.metadata.cycle.iter().collect()),
            (
                PlanningKind::Milestone,
                issue.metadata.milestone.iter().collect(),
            ),
            (PlanningKind::Label, issue.metadata.labels.iter().collect()),
            (
                PlanningKind::Target,
                issue.metadata.targets.iter().collect(),
            ),
        ] {
            for id in values {
                inspect(&issue.path, kind, id);
            }
        }
    }
    for issue in &issues {
        if let Some(milestone) = issue
            .metadata
            .milestone
            .as_ref()
            .and_then(|id| index.get(PlanningKind::Milestone, id))
            && !issue
                .metadata
                .project
                .as_ref()
                .zip(milestone.metadata.project.as_ref())
                .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            warnings.push(
                PmError::new(
                    ErrorCode::PolicyBlocked,
                    "retained issue project differs from its milestone owner",
                )
                .at(root.join(&issue.path)),
            );
        }
    }
    Ok(warnings)
}

pub(crate) fn incoming(
    root: &Path,
    snapshot: &Snapshot<'_>,
    target: &RetirementTarget,
) -> Result<Vec<crate::PlanningReferenceBlocker>> {
    let mut blockers = Vec::new();
    let kinds = match target.kind {
        crate::RetirementKind::Initiative => vec![PlanningKind::Project],
        crate::RetirementKind::Project => vec![PlanningKind::Milestone],
        crate::RetirementKind::Target => vec![PlanningKind::Project, PlanningKind::Milestone],
        _ => return Ok(blockers),
    };
    let index = Index::capture_kinds(root, snapshot, kinds)?;
    for record in index.records.values() {
        let fields: Vec<_> = match target.kind {
            crate::RetirementKind::Initiative => record
                .metadata
                .initiative
                .iter()
                .map(|id| ("initiative", id))
                .collect(),
            crate::RetirementKind::Project => record
                .metadata
                .project
                .iter()
                .map(|id| ("project", id))
                .collect(),
            crate::RetirementKind::Target => record
                .metadata
                .targets
                .iter()
                .map(|id| ("targets", id))
                .collect(),
            _ => Vec::new(),
        };
        for (field, id) in fields {
            if id.eq_ignore_ascii_case(&target.id) {
                blockers.push(crate::PlanningReferenceBlocker {
                    kind: record.kind,
                    id: record.metadata.id.clone(),
                    path: record.path.clone(),
                    field: field.into(),
                    source: record.source.clone(),
                });
            }
        }
    }
    Ok(blockers)
}
