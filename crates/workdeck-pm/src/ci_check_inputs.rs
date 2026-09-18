//! Admission of selected local check inputs against immutable committed bytes.
//! A source binding is not an accepted baseline or a trusted producer result.
use crate::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Candidate invocation inputs; distinct from the accepted evaluator selection.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiInvocationInputManifest {
    pub invocation: String,
    pub selection: InputSelection,
    pub entries: Vec<CiEvaluatorEntry>,
    pub absent: Vec<PathBuf>,
    pub fingerprint: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiCheckInputBinding {
    pub source: CiSourceIdentity,
    pub plan: ContentHash,
    pub inputs: Vec<CiInvocationInputManifest>,
    pub fingerprint: ContentHash,
}

pub fn bind_ci_check_plan(
    worktree: &Path,
    source: &CiSourceIdentity,
    plan: &CheckPlan,
) -> Result<CiCheckInputBinding> {
    plan.validate()?;
    let repository = Repository::open_source(&worktree.join(".workdeck"))?;
    repository.revalidate_check_plan(plan)?;
    let inputs = crate::sources::bind_check_inputs(worktree, source, plan)?;
    let fingerprint = crate::transactions::canonical_hash(&serde_json::json!({
        "domain":"workdeck.ci-check-input-binding.v1", "source":source,
        "plan":plan.fingerprint, "inputs":inputs,
    }))?;
    repository.revalidate_check_plan(plan)?;
    Ok(CiCheckInputBinding {
        source: source.clone(),
        plan: plan.fingerprint.clone(),
        inputs,
        fingerprint,
    })
}

impl CiCheckInputBinding {
    /// Validate retained data without reading the checkout or granting CI trust.
    pub fn validate(&self, plan: &CheckPlan) -> Result<()> {
        plan.validate()?;
        crate::sources::validate_check_binding(self, plan)
    }
}
