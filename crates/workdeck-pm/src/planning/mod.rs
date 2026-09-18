//! Native planning records and source-bound hierarchy policy.
//! Planned dates and outcome declarations never represent execution evidence.

use crate::{
    ContentHash, ErrorCode, PmError, Result, Revision, SchemaVersion, SourceLink, Timestamp,
};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

mod carryover;
pub(crate) mod hierarchy;
pub use carryover::{
    CycleCarryoverExcluded, CycleCarryoverIssue, CycleCarryoverPlan, CycleCarryoverRequest,
    MAX_CARRYOVER_ISSUES,
};
mod membership;
pub(crate) mod store;
pub use membership::{PlanningMembership, PlanningMembershipQuery};

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PlanningKind {
    Initiative,
    Project,
    Milestone,
    Cycle,
    Target,
    Label,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningCriterion {
    pub id: String,
    pub description: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningImportFormat {
    LegacyToml,
    /// Exact retained JSON/JSONL export, relative to the planning root.
    LegacyJson,
}

/// Known migration origin, independent of unknown historical record times.
/// Ordinary creation/update inputs cannot set or replace this provenance.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedPlanningProvenance {
    pub source: String,
    pub format: PlanningImportFormat,
    pub content: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningMetadata {
    pub schema: SchemaVersion,
    pub id: String,
    pub revision: Revision,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiative: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exit_criteria: Vec<PlanningCriterion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<PlanningCriterion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported: Option<ImportedPlanningProvenance>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl PlanningMetadata {
    pub fn validate(&self, kind: PlanningKind) -> Result<()> {
        validate_id(&self.id)?;
        text_field("name", &self.name, 1000)?;
        if self.name.trim().is_empty() {
            return Err(invalid("planning record name must be nonempty"));
        }
        for (key, value) in [
            ("status", &self.status),
            ("starts_at", &self.starts_at),
            ("ends_at", &self.ends_at),
            ("color", &self.color),
            ("initiative", &self.initiative),
            ("project", &self.project),
            ("lead", &self.lead),
            ("scope", &self.scope),
            ("goal", &self.goal),
        ] {
            if let Some(value) = value {
                text_field(key, value, 1000)?;
            }
        }
        let hierarchy = self.initiative.is_some()
            || self.project.is_some()
            || self.lead.is_some()
            || self.scope.is_some()
            || self.goal.is_some()
            || !self.targets.is_empty()
            || !self.exit_criteria.is_empty()
            || !self.outcomes.is_empty();
        if (kind == PlanningKind::Label
            && (hierarchy
                || self.status.is_some()
                || self.starts_at.is_some()
                || self.ends_at.is_some()))
            || (kind != PlanningKind::Label && self.color.is_some())
            || (kind != PlanningKind::Project && self.initiative.is_some())
            || (kind != PlanningKind::Milestone && self.project.is_some())
            || (!matches!(kind, PlanningKind::Project | PlanningKind::Milestone)
                && !self.targets.is_empty())
            || (kind != PlanningKind::Project && !self.exit_criteria.is_empty())
            || (!matches!(
                kind,
                PlanningKind::Initiative | PlanningKind::Milestone | PlanningKind::Target
            ) && !self.outcomes.is_empty())
        {
            return Err(invalid("planning fields do not belong to this record kind"));
        }
        if kind == PlanningKind::Milestone && self.project.is_none() {
            return Err(invalid("a milestone requires its owning project"));
        }
        for id in self
            .initiative
            .iter()
            .chain(&self.project)
            .chain(&self.targets)
        {
            validate_id(id)?;
        }
        let mut targets = std::collections::BTreeSet::new();
        for target in &self.targets {
            if !targets.insert(target.to_ascii_lowercase()) {
                return Err(invalid("duplicate or case-colliding target association"));
            }
        }
        for criteria in [&self.exit_criteria, &self.outcomes] {
            let mut ids = std::collections::BTreeSet::new();
            for criterion in criteria {
                if !crate::identity::valid_slug(&criterion.id) || !ids.insert(&criterion.id) {
                    return Err(invalid(
                        "planning criteria require unique stable lowercase slug IDs",
                    ));
                }
                text_field("criterion description", &criterion.description, 4000)?;
                if criterion.description.trim().is_empty() {
                    return Err(invalid("planning criterion description must be nonempty"));
                }
            }
        }
        // Existing cycle boundaries may be imported free-form text. New hierarchy
        // dates use an explicit date/instant representation, without inventing a schedule.
        if kind != PlanningKind::Cycle {
            for date in self.starts_at.iter().chain(&self.ends_at) {
                let valid_date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .is_ok_and(|parsed| parsed.format("%Y-%m-%d").to_string() == *date);
                if !valid_date && DateTime::parse_from_rfc3339(date).is_err() {
                    return Err(invalid(
                        "planned dates require YYYY-MM-DD or an RFC 3339 instant",
                    ));
                }
            }
        }
        if self.imported.is_some()
            && !matches!(
                kind,
                PlanningKind::Project | PlanningKind::Cycle | PlanningKind::Label
            )
        {
            return Err(invalid(
                "legacy planning import provenance is supported only for projects, cycles, and labels; new hierarchy kinds require native history",
            ));
        }
        if self.imported.is_none() && (self.created_at.is_none() || self.updated_at.is_none()) {
            return Err(invalid(
                "unknown historical timestamps require explicit legacy import provenance",
            ));
        }
        if let Some(imported) = &self.imported {
            SourceLink {
                path: imported.source.clone(),
                line: None,
                end_line: None,
            }
            .validate()?;
            if !match imported.format {
                PlanningImportFormat::LegacyToml => imported.source.ends_with(".toml"),
                PlanningImportFormat::LegacyJson => {
                    let path = std::path::Path::new(&imported.source);
                    path.parent() == Some(std::path::Path::new("imported-history/exports"))
                        && path
                            .extension()
                            .is_some_and(|ext| ext == "json" || ext == "jsonl")
                        && path
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .is_some_and(|stem| {
                                stem.parse::<ContentHash>()
                                    .is_ok_and(|hash| hash == imported.content)
                            })
                }
            } {
                return Err(invalid(
                    "legacy import provenance path must match its declared source format",
                ));
            }
        }
        if self
            .created_at
            .zip(self.updated_at)
            .is_some_and(|(created, updated)| updated < created)
        {
            return Err(invalid("updated_at cannot precede created_at"));
        }
        if let (Some(start), Some(end)) = (&self.starts_at, &self.ends_at) {
            let reversed_dates = NaiveDate::parse_from_str(start, "%Y-%m-%d")
                .ok()
                .zip(NaiveDate::parse_from_str(end, "%Y-%m-%d").ok())
                .is_some_and(|(a, b)| b < a);
            let reversed_times = DateTime::parse_from_rfc3339(start)
                .ok()
                .zip(DateTime::parse_from_rfc3339(end).ok())
                .is_some_and(|(a, b)| b < a);
            if reversed_dates || reversed_times {
                return Err(invalid("planned end precedes its start"));
            }
        }
        validate_extra(&self.extra)?;
        Ok(())
    }
}

impl LabelsMetadata {
    pub fn validate(&self) -> Result<()> {
        validate_extra(&self.extra)?;
        let mut identities = std::collections::BTreeSet::new();
        for label in &self.labels {
            label.validate(PlanningKind::Label)?;
            if !identities.insert(label.id.to_ascii_lowercase()) {
                return Err(invalid("duplicate or case-colliding label identity"));
            }
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}

pub(crate) fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(invalid(
            "planning IDs must contain 1–128 ASCII letters, digits, dashes, or underscores",
        ));
    }
    let upper = id.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
    {
        return Err(invalid("planning ID is a reserved filesystem device name"));
    }
    Ok(())
}

fn text_field(name: &str, text: &str, limit: usize) -> Result<()> {
    if text.len() > limit || text.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{name} must be at most {limit} bytes without control characters"
        )));
    }
    Ok(())
}

fn validate_extra(extra: &BTreeMap<String, Value>) -> Result<()> {
    for key in extra.keys() {
        if !key
            .strip_prefix("x-")
            .is_some_and(crate::identity::valid_slug)
        {
            return Err(invalid(format!(
                "unknown planning field {key:?}; use custom or an x- extension"
            )));
        }
    }
    Ok(())
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningRecord {
    pub kind: PlanningKind,
    pub metadata: PlanningMetadata,
    pub body: String,
    pub path: PathBuf,
    pub source: crate::SourceToken,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement: Option<crate::Tombstone>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelsMetadata {
    pub schema: SchemaVersion,
    pub labels: Vec<PlanningMetadata>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePlanning {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

impl CreatePlanning {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            body: String::new(),
            fields: BTreeMap::new(),
        }
    }
}

/// Atomic create-or-update compatibility intent. Omitted bodies preserve an
/// existing description; an explicitly empty body clears it.
#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavePlanning {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanningMutation {
    PatchCustom {
        patch: crate::CustomPatch,
    },
    Update {
        #[serde(default)]
        fields: BTreeMap<String, Value>,
        #[serde(default)]
        body: Option<String>,
    },
    Archive {
        archived: bool,
    },
    /// Complete a project or milestone only after its shared policy evaluator
    /// accepts child issues, criteria, and the attributed manual acceptance.
    Complete {
        acceptance: crate::PolicyAcceptance,
    },
}
