//! Snapshot-bound planning workspace data. Membership never implies completion.
use super::{PlanningKind, PlanningRecord, hierarchy, store};
use crate::{
    ContentHash, IssueQuery, IssueRecord, PmError, Repository, RepositoryId, Result,
    RetirementTarget, SchemaVersion, transactions::Snapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningMembershipQuery {
    pub kind: PlanningKind,
    pub id: String,
    #[serde(default)]
    pub issues: IssueQuery,
}
impl PlanningMembershipQuery {
    pub fn new(kind: PlanningKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            issues: IssueQuery::default(),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningMembership {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub root: PathBuf,
    pub query: PlanningMembershipQuery,
    pub record: PlanningRecord,
    /// Direct planning members, including archived records with their state intact.
    /// Initiatives contain projects; projects contain milestones; targets contain
    /// projects/milestones carrying that outgoing target association.
    pub related_records: Vec<PlanningRecord>,
    /// Parent membership intersected with every shared query predicate/sort.
    pub issues: Vec<IssueRecord>,
    /// Hash of the exact captured config bytes, including unknown metadata/comments.
    pub config_hash: ContentHash,
    /// Bound to parent/member sources, directory membership, query and config.
    /// Use record.source for a record mutation; this view fingerprint is not an issue token.
    pub fingerprint: ContentHash,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PmError>,
}

fn qualify(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &crate::Config,
    mut record: PlanningRecord,
) -> Result<PlanningRecord> {
    record.retirement = crate::retirement::read_tombstone(
        root,
        snapshot,
        config,
        &RetirementTarget::new(record.kind.into(), &record.metadata.id)?,
    )?;
    Ok(record)
}

impl Repository {
    pub fn planning_membership(
        &self,
        query: &PlanningMembershipQuery,
    ) -> Result<PlanningMembership> {
        super::validate_id(&query.id)?;
        self.store()?.with_snapshot(|snapshot| {
            let config=crate::repository::config_from_snapshot(self.root(),snapshot)?;
            let record=qualify(self.root(),snapshot,&config,store::load_planning(self.root(),snapshot,query.kind,&query.id)?)?;
            let captured=crate::queries::capture(self.root(),snapshot)?;
            let selected=captured.select_indices(&query.issues)?;
            let mut related_records=Vec::new();
            for kind in match query.kind {
                PlanningKind::Initiative => vec![PlanningKind::Project],
                PlanningKind::Project => vec![PlanningKind::Milestone],
                PlanningKind::Target => vec![PlanningKind::Project,PlanningKind::Milestone],
                _ => Vec::new(),
            } {
                for member in store::list_planning(self.root(),snapshot,kind)? {
                    let belongs=match query.kind {
                        PlanningKind::Initiative => member.metadata.initiative.as_deref()==Some(&record.metadata.id),
                        PlanningKind::Project => member.metadata.project.as_deref()==Some(&record.metadata.id),
                        PlanningKind::Target => member.metadata.targets.contains(&record.metadata.id),
                        _ => false,
                    };
                    if belongs {related_records.push(qualify(self.root(),snapshot,&config,member)?);}
                }
            }
            let projects:BTreeSet<_>=related_records.iter().filter(|member|member.kind==PlanningKind::Project).map(|member|member.metadata.id.as_str()).collect();
            let targets=if query.kind==PlanningKind::Target {
                hierarchy::target_memberships(self.root(),snapshot,&config,captured.issues())?
            }else{Default::default()};
            let issues=selected.into_iter().filter_map(|index| {
                let issue=&captured.issues()[index];
                let belongs=match query.kind {
                    PlanningKind::Initiative => issue.metadata.project.as_deref().is_some_and(|project|projects.contains(project)),
                    PlanningKind::Project => issue.metadata.project.as_deref()==Some(&record.metadata.id),
                    PlanningKind::Milestone => issue.metadata.milestone.as_deref()==Some(&record.metadata.id),
                    PlanningKind::Cycle => issue.metadata.cycle.as_deref()==Some(&record.metadata.id),
                    PlanningKind::Target => targets.get(&record.metadata.id).is_some_and(|members|members.contains(&issue.metadata.id)),
                    PlanningKind::Label => issue.metadata.labels.contains(&record.metadata.id),
                };
                belongs.then(||issue.clone())
            }).collect::<Vec<_>>();
            let config_hash=ContentHash::of(&snapshot.read(Path::new("config.yml"))?.ok_or_else(||PmError::new(crate::ErrorCode::CorruptStore,"planning config disappeared"))?);
            let warnings=hierarchy::diagnostics(self.root(),snapshot,&config)?;
            let fingerprint=crate::transactions::canonical_hash(&serde_json::json!({
                "repository":config.repository,"query":query,"record":record,"related_records":related_records,
                "issues":issues,"config":config_hash,"warnings":warnings,
                "issue_sources":captured.issues().iter().map(|issue|(&issue.metadata.id,&issue.source)).collect::<Vec<_>>()
            }))?;
            Ok(PlanningMembership {schema:SchemaVersion::CURRENT,repository:config.repository,root:self.root().to_owned(),query:query.clone(),record,related_records,issues,config_hash,fingerprint,warnings})
        })
    }
}
