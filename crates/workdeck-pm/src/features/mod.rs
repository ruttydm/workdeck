//! Native capability declarations, independent of issue workflow and evidence
//! qualification. Logical hierarchy and physical file placement do not own IDs.

use crate::{
    ContentHash, CustomPatch, ErrorCode, FeatureId, GateId, IssueQuery, IssueRecord,
    PlanningCriterion, PmError, RepositoryId, Result, Revision, SchemaVersion, SourceLink,
    SourceToken, Timestamp, Tombstone,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

mod policy;
mod proof;
pub(crate) mod relations;
mod store;
pub use relations::{FeatureRelatedInput, FeatureRelatedOutcome, RelatedFeatureLink};
mod coverage;
pub(crate) use coverage::{incoming, inspect, validate_import};
pub(crate) use policy::{criterion, validate_change, validate_issue_associations};
pub(crate) use proof::validate_receipt;
pub(crate) use store::{
    load_feature, load_features, parse, prepare_record, validate_path, validate_record,
};

macro_rules! declaration {
    ($name:ident, $default:ident, $($variant:ident),+) => {
        #[derive(schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all="snake_case")]
        pub enum $name { #[default] $default, $($variant),+ }
    };
}
declaration!(FeatureDecision, Proposed, Accepted, Deferred, Rejected);
declaration!(FeatureMaturity, Draft, Specified, Implemented);
declaration!(
    FeatureAvailability,
    Unavailable,
    Experimental,
    Available,
    Deprecated
);

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureMetadata {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: FeatureId,
    pub revision: Revision,
    pub name: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<FeatureId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<FeatureId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub milestones: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<PlanningCriterion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gates: Vec<GateId>,
    #[serde(default)]
    pub decision: FeatureDecision,
    #[serde(default)]
    pub maturity: FeatureMaturity,
    #[serde(default)]
    pub availability: FeatureAvailability,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl FeatureMetadata {
    pub fn validate(&self) -> Result<()> {
        line(&self.name, "feature name", 512)?;
        if self.updated_at < self.created_at {
            return Err(invalid("feature update predates creation"));
        }
        if let Some(lead) = &self.lead {
            line(lead, "feature lead", 512)?;
        }
        if self.parent.as_ref() == Some(&self.id) || self.prerequisites.contains(&self.id) {
            return Err(invalid(
                "a feature cannot refer to itself as parent or prerequisite",
            ));
        }
        unique(&self.prerequisites, "feature prerequisites")?;
        unique(&self.gates, "feature gates")?;
        for (name, values) in [
            ("feature projects", &self.projects),
            ("feature milestones", &self.milestones),
            ("feature targets", &self.targets),
        ] {
            unique(values, name)?;
            for value in values {
                crate::planning::validate_id(value)?;
            }
        }
        if self.sources.len() > 256 || self.criteria.len() > 256 {
            return Err(invalid(
                "feature sources and criteria are limited to 256 entries",
            ));
        }
        let mut sources = BTreeSet::new();
        for source in &self.sources {
            source.validate()?;
            if !sources.insert(serde_json::to_string(source).map_err(|e| invalid(e.to_string()))?) {
                return Err(invalid("duplicate feature source"));
            }
        }
        let mut criteria = BTreeSet::new();
        for criterion in &self.criteria {
            if !crate::identity::valid_slug(&criterion.id) || !criteria.insert(&criterion.id) {
                return Err(invalid(
                    "feature criterion IDs must be unique stable lowercase slugs",
                ));
            }
            line(
                &criterion.description,
                "feature criterion description",
                4096,
            )?;
        }
        for key in self.extra.keys() {
            if !key
                .strip_prefix("x-")
                .is_some_and(crate::identity::valid_slug)
            {
                return Err(invalid(format!(
                    "unsupported feature field {key:?}; use custom or x-* for inert extension data"
                )));
            }
        }
        Ok(())
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureRecord {
    pub metadata: FeatureMetadata,
    pub body: String,
    pub path: PathBuf,
    pub source: SourceToken,
    /// Exact source needed for pure historical source/result proof validation.
    pub document: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement: Option<Tombstone>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFeature {
    pub name: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
    #[serde(default)]
    pub directory: Option<String>,
}
impl CreateFeature {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            body: String::new(),
            fields: BTreeMap::new(),
            directory: None,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum FeatureMutation {
    Update {
        fields: BTreeMap<String, Value>,
        body: Option<String>,
    },
    Reparent {
        parent: Option<FeatureId>,
    },
    Relocate {
        directory: String,
    },
    Archive {
        archived: bool,
    },
    PatchCustom {
        patch: CustomPatch,
    },
    /// Advance one declared maturity stage through the shared policy evaluator.
    Promote {
        maturity: FeatureMaturity,
        acceptance: crate::PolicyAcceptance,
    },
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum FeatureIntent {
    Create {
        input: CreateFeature,
    },
    Mutate {
        reference: String,
        expected: Option<SourceToken>,
        mutation: FeatureMutation,
    },
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureOutcome {
    pub record: FeatureRecord,
    pub before: Option<FeatureRecord>,
    pub intent: FeatureIntent,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureCoverageQuery {
    pub feature: String,
    #[serde(default)]
    pub issues: IssueQuery,
}
impl FeatureCoverageQuery {
    pub fn new(feature: impl Into<String>) -> Self {
        Self {
            feature: feature.into(),
            issues: IssueQuery::default(),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureCoverage {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub query: FeatureCoverageQuery,
    pub feature: FeatureRecord,
    pub issues: Vec<IssueRecord>,
    pub outside_prerequisites: Vec<IssueRecord>,
    pub children: Vec<FeatureRecord>,
    pub prerequisites: Vec<FeatureRecord>,
    pub dependents: Vec<FeatureRecord>,
    pub related: Vec<FeatureRecord>,
    pub warnings: Vec<PmError>,
    pub config: ContentHash,
    pub fingerprint: ContentHash,
}

pub(crate) fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
fn line(value: &str, name: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(invalid(format!(
            "{name} must be nonempty single-line text up to {max} bytes"
        )))
    } else {
        Ok(())
    }
}
fn unique<T: Ord>(values: &[T], name: &str) -> Result<()> {
    if values.len() > 256 || values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        Err(invalid(format!(
            "{name} must contain at most 256 unique references"
        )))
    } else {
        Ok(())
    }
}
