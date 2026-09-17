//! Semantic requirement projections over already bounded immutable source files.
//! Progress declarations and physical feature placement are not requirement IDs.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CiSubjectIdentity {
    Issue {
        id: IssueId,
    },
    Feature {
        id: FeatureId,
    },
    Planning {
        record_kind: PlanningKind,
        id: String,
    },
    Gate {
        id: GateId,
    },
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CiSubjectRequirements {
    Issue {
        acceptance: Vec<PlanningCriterion>,
        features: Vec<FeatureId>,
        parent: Option<IssueId>,
        prerequisites: Vec<IssueId>,
        gates: Vec<GateId>,
    },
    Feature {
        criteria: Vec<PlanningCriterion>,
        decision: FeatureDecision,
        parent: Option<FeatureId>,
        prerequisites: Vec<FeatureId>,
        gates: Vec<GateId>,
    },
    Planning {
        exit_criteria: Vec<PlanningCriterion>,
        outcomes: Vec<PlanningCriterion>,
    },
    Gate {
        requirements: Vec<GateRequirement>,
        archived: bool,
        custom: BTreeMap<String, serde_json::Value>,
        extra: BTreeMap<String, serde_json::Value>,
    },
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiSubjectContract {
    pub subject: CiSubjectIdentity,
    pub path: PathBuf,
    /// Exact document hash, including progress declarations and prose.
    pub content: ContentHash,
    pub requirements: CiSubjectRequirements,
}

fn criteria(mut values: Vec<PlanningCriterion>) -> Vec<PlanningCriterion> {
    values.sort_by(|a, b| a.id.cmp(&b.id));
    values
}

pub(crate) fn capture(
    snapshot: &crate::transactions::Snapshot<'_>,
    config: &Config,
) -> Result<Vec<CiSubjectContract>> {
    let root = Path::new("source-snapshot");
    let mut subjects = Vec::new();
    for issue in crate::issues::load_issues(root, snapshot, config)? {
        let mut metadata = issue.metadata;
        if metadata.acceptance.is_empty()
            && metadata.features.is_empty()
            && metadata.parent.is_none()
            && metadata.prerequisites.is_empty()
            && metadata.gates.is_empty()
        {
            continue;
        }
        metadata.features.sort();
        metadata.prerequisites.sort();
        metadata.gates.sort();
        subjects.push(CiSubjectContract {
            subject: CiSubjectIdentity::Issue { id: metadata.id },
            path: issue.path,
            content: issue.source.content,
            requirements: CiSubjectRequirements::Issue {
                acceptance: criteria(
                    metadata
                        .acceptance
                        .into_iter()
                        .map(|criterion| PlanningCriterion {
                            id: criterion.id,
                            description: criterion.description,
                        })
                        .collect(),
                ),
                features: metadata.features,
                parent: metadata.parent,
                prerequisites: metadata.prerequisites,
                gates: metadata.gates,
            },
        });
    }
    for feature in crate::features::load_features(snapshot, config)? {
        let mut metadata = feature.metadata;
        metadata.prerequisites.sort();
        metadata.gates.sort();
        subjects.push(CiSubjectContract {
            subject: CiSubjectIdentity::Feature { id: metadata.id },
            path: feature.path,
            content: feature.source.content,
            requirements: CiSubjectRequirements::Feature {
                criteria: criteria(metadata.criteria),
                decision: metadata.decision,
                parent: metadata.parent,
                prerequisites: metadata.prerequisites,
                gates: metadata.gates,
            },
        });
    }
    for kind in [
        PlanningKind::Initiative,
        PlanningKind::Project,
        PlanningKind::Milestone,
        PlanningKind::Target,
        PlanningKind::Cycle,
    ] {
        for record in crate::planning::store::list_planning(root, snapshot, kind)? {
            let metadata = record.metadata;
            if metadata.exit_criteria.is_empty() && metadata.outcomes.is_empty() {
                continue;
            }
            subjects.push(CiSubjectContract {
                subject: CiSubjectIdentity::Planning {
                    record_kind: kind,
                    id: metadata.id,
                },
                path: record.path,
                content: record.source.content,
                requirements: CiSubjectRequirements::Planning {
                    exit_criteria: criteria(metadata.exit_criteria),
                    outcomes: criteria(metadata.outcomes),
                },
            });
        }
    }
    for gate in crate::gates::load_gates(snapshot, config)? {
        let mut definition = gate.definition;
        definition.requirements.sort_by(|a, b| a.id.cmp(&b.id));
        subjects.push(CiSubjectContract {
            subject: CiSubjectIdentity::Gate { id: definition.id },
            path: gate.path,
            content: gate.source.content,
            requirements: CiSubjectRequirements::Gate {
                requirements: definition.requirements,
                archived: definition.archived,
                custom: definition.custom,
                extra: definition.extra,
            },
        });
    }
    subjects.sort_by(|a, b| a.subject.cmp(&b.subject));
    Ok(subjects)
}

pub(crate) fn changes(
    base: &[CiSubjectContract],
    head: &[CiSubjectContract],
) -> Result<Vec<CiContractChange>> {
    let candidate: BTreeMap<_, _> = head
        .iter()
        .map(|record| (&record.subject, record))
        .collect();
    let mut changes = Vec::new();
    for record in base {
        let after = candidate.get(&record.subject);
        if after.is_some_and(|after| after.requirements == record.requirements) {
            continue;
        }
        changes.push(CiContractChange {
            path: record.path.clone(),
            subject: Some(record.subject.clone()),
            base: crate::transactions::canonical_hash(&serde_json::json!(record.requirements))?,
            head: after
                .map(|after| {
                    crate::transactions::canonical_hash(&serde_json::json!(after.requirements))
                })
                .transpose()?,
        });
    }
    Ok(changes)
}
