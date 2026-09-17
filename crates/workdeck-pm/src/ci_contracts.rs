//! Baseline and candidate contracts are captured independently. Comparing them
//! does not certify who accepted the baseline or who reviewed a contract change.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiEvaluationContract {
    pub repository: RepositoryId,
    pub configuration: ContentHash,
    pub policy: AcceptancePolicy,
    pub workflow: Workflow,
    pub organization: CiOrganizationContract,
    pub profiles: Vec<CheckProfileRecord>,
    pub checks: Vec<CheckRecord>,
    pub commands: Vec<CommandRecord>,
    pub subjects: Vec<CiSubjectContract>,
    pub evaluators: Vec<CiEvaluatorManifest>,
    pub fingerprint: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiContractSide {
    Base,
    Head,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiContractError {
    pub side: CiContractSide,
    pub error: PmError,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiContractChange {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<CiSubjectIdentity>,
    /// Semantic hashes of policy, check definitions or subject requirements.
    /// Subject progress declarations and YAML comments are excluded.
    pub base: ContentHash,
    pub head: Option<ContentHash>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiContractComparison {
    pub base: Option<CiEvaluationContract>,
    pub head: Option<CiEvaluationContract>,
    pub changes: Vec<CiContractChange>,
    pub evaluator_changes: Vec<CiEvaluatorChange>,
    pub errors: Vec<CiContractError>,
    pub review_required: bool,
}

fn blocked(path: &Path, message: &str) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message).at(path)
}

pub(crate) fn capture(
    snapshot: &crate::transactions::Snapshot<'_>,
    config: &Config,
    capture_evaluators: impl FnOnce(&[CheckRecord]) -> Result<Vec<CiEvaluatorManifest>>,
) -> Result<CiEvaluationContract> {
    let catalog = crate::commands::catalog::capture(snapshot, config)?;
    let mut check_ids: BTreeSet<_> = config.acceptance.required_checks.iter().cloned().collect();
    let profile_ids: BTreeSet<_> = config
        .acceptance
        .required_profiles
        .iter()
        .cloned()
        .collect();
    let mut profiles = Vec::new();
    for id in profile_ids {
        let path = Path::new("check-profiles").join(format!("{id}.yml"));
        let profile = catalog
            .profiles
            .iter()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| blocked(&path, "required check profile is missing"))?;
        if profile.definition.archived {
            return Err(blocked(&path, "required check profile is archived"));
        }
        check_ids.extend(profile.definition.checks.iter().cloned());
        profiles.push(profile.clone());
    }
    let mut checks = Vec::new();
    let mut command_ids = BTreeSet::new();
    for id in check_ids {
        let path = Path::new("checks").join(format!("{id}.yml"));
        let check = catalog
            .checks
            .iter()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| blocked(&path, "required check is missing"))?;
        if check.definition.archived {
            return Err(blocked(&path, "required check is archived"));
        }
        command_ids.insert(check.definition.command.clone());
        checks.push(check.clone());
    }
    let mut commands = Vec::new();
    for id in command_ids {
        let path = Path::new("commands").join(format!("{id}.yml"));
        let command = catalog
            .commands
            .iter()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| blocked(&path, "required command recipe is missing"))?;
        if command.definition.archived {
            return Err(blocked(&path, "required command recipe is archived"));
        }
        commands.push(command.clone());
    }
    let configuration = ContentHash::of(
        &snapshot
            .read_bounded(Path::new("config.yml"), 64 * 1024 * 1024)?
            .ok_or_else(|| blocked(Path::new("config.yml"), "contract configuration is missing"))?,
    );
    let organization = CiOrganizationContract {
        users: crate::organization::capture_users(snapshot, &config.repository)?,
        schema: crate::organization::capture_schema(snapshot, &config.repository)?,
    };
    let subjects = crate::ci_subjects::capture(snapshot, config)?;
    let evaluators = capture_evaluators(&checks)?;
    let fingerprint = crate::transactions::canonical_hash(&serde_json::json!({
        "schema":1, "repository":config.repository, "configuration":configuration,
        "organization":organization, "policy":config.acceptance, "workflow":config.workflow, "profiles":profiles, "checks":checks, "commands":commands, "subjects":subjects, "evaluators":evaluators,
    }))?;
    Ok(CiEvaluationContract {
        repository: config.repository.clone(),
        configuration,
        policy: config.acceptance.clone(),
        workflow: config.workflow.clone(),
        organization,
        profiles,
        checks,
        commands,
        subjects,
        evaluators,
        fingerprint,
    })
}

fn semantic_records(contract: &CiEvaluationContract) -> Result<BTreeMap<PathBuf, ContentHash>> {
    let mut records = BTreeMap::new();
    records.insert(
        PathBuf::from("config.yml"),
        crate::transactions::canonical_hash(
            &serde_json::json!({"acceptance":contract.policy,"workflow":contract.workflow}),
        )?,
    );
    records.extend(crate::ci_organization::semantic_records(
        &contract.organization,
    )?);
    for record in &contract.profiles {
        records.insert(
            record.path.clone(),
            crate::transactions::canonical_hash(&serde_json::json!(record.definition))?,
        );
    }
    for record in &contract.checks {
        records.insert(
            record.path.clone(),
            crate::transactions::canonical_hash(&serde_json::json!(record.definition))?,
        );
    }
    for record in &contract.commands {
        records.insert(
            record.path.clone(),
            crate::transactions::canonical_hash(&serde_json::json!(record.definition))?,
        );
    }
    Ok(records)
}

pub(crate) fn compare(
    base: Result<CiEvaluationContract>,
    head: Result<CiEvaluationContract>,
) -> Result<CiContractComparison> {
    let mut errors = Vec::new();
    let mut admit = |result: Result<CiEvaluationContract>, side| match result {
        Ok(contract) => Some(contract),
        Err(error) => {
            errors.push(CiContractError { side, error });
            None
        }
    };
    let base = admit(base, CiContractSide::Base);
    let head = admit(head, CiContractSide::Head);
    let mut changes = Vec::new();
    let mut evaluator_changes = Vec::new();
    if let (Some(base), Some(head)) = (&base, &head) {
        evaluator_changes = crate::ci_evaluators::changes(&base.evaluators, &head.evaluators);
        changes.extend(crate::ci_subjects::changes(&base.subjects, &head.subjects)?);
        let candidate = semantic_records(head)?;
        for (path, before) in semantic_records(base)? {
            let after = candidate.get(&path);
            if after != Some(&before) {
                changes.push(CiContractChange {
                    path,
                    subject: None,
                    base: before,
                    head: after.cloned(),
                });
            }
        }
    }
    Ok(CiContractComparison {
        base,
        head,
        review_required: !changes.is_empty() || !evaluator_changes.is_empty(),
        evaluator_changes,
        changes,
        errors,
    })
}
