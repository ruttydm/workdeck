use super::*;
use crate::{
    transactions::{Snapshot, canonical_hash},
    *,
};
use serde_json::json;
use std::path::Path;
pub fn criterion_definition_hash(
    repository: &RepositoryId,
    owner: &CriterionOwner,
    id: &str,
    description: &str,
) -> Result<ContentHash> {
    owner.validate()?;
    if !crate::identity::valid_slug(id) {
        return Err(invalid("criterion ID must be a stable lowercase slug"));
    }
    if description.trim().is_empty() || description.len() > 4096 || description.contains('\0') {
        return Err(invalid(
            "criterion description must be nonempty and at most 4096 bytes without NUL",
        ));
    }
    canonical_hash(
        &json!({"domain":"workdeck.criterion.v1","repository":repository,"owner":owner,"id":id,"description":description}),
    )
}
pub(crate) fn resolve(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    owner: &CriterionOwner,
    id: &str,
) -> Result<ResolvedCriterion> {
    owner.validate()?;
    let (description, declaration, source, retired) = match owner {
        CriterionOwner::Issue(issue) => {
            let record = crate::issues::resolve_issue(root, snapshot, config, issue.as_str())?;
            let criterion = record
                .metadata
                .acceptance
                .iter()
                .find(|c| c.id == id)
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "issue criterion not found"))?;
            let retired = crate::retirement::read_tombstone(
                root,
                snapshot,
                config,
                &RetirementTarget::new(RetirementKind::Issue, issue.as_str())?,
            )?
            .is_some();
            (
                criterion.description.clone(),
                if criterion.checked {
                    CriterionDeclaration::Checked
                } else {
                    CriterionDeclaration::Unchecked
                },
                SourcePin {
                    path: record.path,
                    content: record.source.content,
                },
                retired || record.metadata.archived,
            )
        }
        CriterionOwner::Feature(feature) => {
            let record = crate::features::load_feature(snapshot, config, feature)?;
            let (criterion, source, path) =
                crate::features::criterion(snapshot, config, feature, id)?;
            let retired = crate::retirement::read_tombstone(
                root,
                snapshot,
                config,
                &RetirementTarget::new(RetirementKind::Feature, feature.as_str())?,
            )?
            .is_some();
            (
                criterion.description,
                CriterionDeclaration::Declared,
                SourcePin {
                    path,
                    content: source.content,
                },
                retired || record.metadata.archived,
            )
        }
        CriterionOwner::Milestone(milestone) | CriterionOwner::Project(milestone) => {
            let project = matches!(owner, CriterionOwner::Project(_));
            let kind = if project {
                PlanningKind::Project
            } else {
                PlanningKind::Milestone
            };
            let record = crate::planning::store::load_planning(root, snapshot, kind, milestone)?;
            let criteria = if project {
                &record.metadata.exit_criteria
            } else {
                &record.metadata.outcomes
            };
            let criterion = criteria
                .iter()
                .find(|c| c.id == id)
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "planning criterion not found"))?;
            let retired = crate::retirement::read_tombstone(
                root,
                snapshot,
                config,
                &RetirementTarget::new(kind.into(), milestone)?,
            )?
            .is_some();
            (
                criterion.description.clone(),
                CriterionDeclaration::Declared,
                SourcePin {
                    path: record.path,
                    content: record.source.content,
                },
                retired || record.metadata.archived,
            )
        }
    };
    let reference = CriterionRef {
        repository: config.repository.clone(),
        owner: owner.clone(),
        id: id.into(),
        definition: criterion_definition_hash(&config.repository, owner, id, &description)?,
    };
    Ok(ResolvedCriterion {
        reference,
        description,
        declaration,
        source,
        retired,
    })
}
impl Repository {
    pub fn resolve_criterion(&self, owner: &CriterionOwner, id: &str) -> Result<ResolvedCriterion> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            resolve(self.root(), snapshot, &config, owner, id)
        })
    }
}
