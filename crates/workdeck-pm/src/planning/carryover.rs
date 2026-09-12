//! Reviewed cycle membership changes share the ordinary issue mutation engine.
use crate::{
    ContentHash, ErrorCode, IssueId, IssueMutation, IssueRecord, PlanningKind, PmError, Repository,
    RepositoryId, RequestId, Result, RetirementTarget, SchemaVersion, SourceToken, UpdateIssue,
    WorkflowCategory,
    transactions::{self, MutationReceipt, PreparedOperation, Snapshot},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub const MAX_CARRYOVER_ISSUES: usize = 100;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CycleCarryoverRequest {
    pub from: String,
    pub to: String,
    /// Empty selects all eligible members. Explicit IDs must be eligible source members.
    #[serde(default)]
    pub issues: Vec<IssueId>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CycleCarryoverIssue {
    pub id: IssueId,
    pub title: String,
    pub source: SourceToken,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CycleCarryoverExcluded {
    pub issue: CycleCarryoverIssue,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CycleCarryoverPlan {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub root: PathBuf,
    pub request: CycleCarryoverRequest,
    pub from_source: SourceToken,
    pub to_source: SourceToken,
    pub issues: Vec<CycleCarryoverIssue>,
    pub excluded: Vec<CycleCarryoverExcluded>,
    pub fingerprint: ContentHash,
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn summary(issue: &IssueRecord) -> CycleCarryoverIssue {
    CycleCarryoverIssue {
        id: issue.metadata.id.clone(),
        title: issue.metadata.title.clone(),
        source: issue.source.clone(),
    }
}
fn mutation(request: &CycleCarryoverRequest) -> IssueMutation {
    IssueMutation::Update {
        input: UpdateIssue {
            fields: BTreeMap::from([("cycle".into(), json!(request.to))]),
            body: None,
        },
    }
}
fn prepare(
    repo: &Repository,
    snapshot: &Snapshot<'_>,
    request: &CycleCarryoverRequest,
) -> Result<(CycleCarryoverPlan, PreparedOperation)> {
    super::validate_id(&request.from)?;
    super::validate_id(&request.to)?;
    if request.from == request.to {
        return Err(invalid("carryover requires two different cycles"));
    }
    if request.issues.len() > MAX_CARRYOVER_ISSUES {
        return Err(invalid(
            "carryover selects at most 100 issues per reviewed batch",
        ));
    }
    let selected: BTreeSet<_> = request.issues.iter().collect();
    if selected.len() != request.issues.len() {
        return Err(invalid("carryover contains duplicate issue IDs"));
    }
    let config = crate::repository::config_from_snapshot(repo.root(), snapshot)?;
    let from =
        super::store::load_planning(repo.root(), snapshot, PlanningKind::Cycle, &request.from)?;
    let to = super::store::load_planning(repo.root(), snapshot, PlanningKind::Cycle, &request.to)?;
    if to.metadata.archived {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "carryover destination cycle is archived",
        ));
    }
    crate::retirement::ensure_writable(
        repo.root(),
        snapshot,
        &config,
        &RetirementTarget::new(crate::RetirementKind::Cycle, &request.to)?,
    )?;
    let captured = crate::queries::capture(repo.root(), snapshot)?;
    let mut members = captured
        .issues()
        .iter()
        .filter(|issue| issue.metadata.cycle.as_deref() == Some(&request.from))
        .collect::<Vec<_>>();
    members.sort_by(|a, b| a.metadata.id.cmp(&b.metadata.id));
    for id in &request.issues {
        if !members.iter().any(|issue| &issue.metadata.id == id) {
            return Err(invalid(
                "selected carryover issue is not a member of the source cycle",
            ));
        }
    }
    let mut issues = Vec::new();
    let mut excluded = Vec::new();
    let mut changes = Vec::new();
    let mut results = Vec::new();
    for issue in &members {
        if !selected.is_empty() && !selected.contains(&issue.metadata.id) {
            continue;
        }
        let reason = if issue.retirement.is_some() {
            Some("retired")
        } else if issue.metadata.archived {
            Some("archived")
        } else {
            match config.workflow.state(&issue.metadata.status)?.category {
                WorkflowCategory::Completed => Some("completed"),
                WorkflowCategory::Canceled => Some("canceled"),
                _ => None,
            }
        };
        if let Some(reason) = reason {
            if !selected.is_empty() {
                return Err(PmError::new(
                    ErrorCode::PolicyBlocked,
                    format!("selected carryover issue {} is {reason}", issue.metadata.id),
                ));
            }
            excluded.push(CycleCarryoverExcluded {
                issue: summary(issue),
                reason: reason.into(),
            });
            continue;
        }
        if issues.len() >= MAX_CARRYOVER_ISSUES {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "cycle has more than 100 eligible issues; select an explicit reviewed batch",
            ));
        }
        let prepared = crate::issues::prepare_issue_mutation(
            repo.root(),
            snapshot,
            &config,
            (*issue).clone(),
            Some(&issue.source),
            &mutation(request),
        )?;
        issues.push(summary(issue));
        changes.extend(prepared.changes);
        results.push(prepared.result);
    }
    let config_bytes = snapshot
        .read(std::path::Path::new("config.yml"))?
        .ok_or_else(|| invalid("planning configuration disappeared"))?;
    let fingerprint = transactions::canonical_hash(&json!({
        "schema":1,"reads":snapshot.read_fingerprint()?,"operation":"cycle.carryover","root":repo.root(),"repository":repo.identity(),
        "request":request,"from":from.source,"to":to.source,"config":ContentHash::of(&config_bytes),
        "members":members.iter().map(|issue| (&issue.metadata.id, &issue.source, &issue.retirement)).collect::<Vec<_>>()
    }))?;
    let plan = CycleCarryoverPlan {
        schema: SchemaVersion::CURRENT,
        repository: repo.identity().clone(),
        root: repo.root().to_owned(),
        request: request.clone(),
        from_source: from.source,
        to_source: to.source,
        issues,
        excluded,
        fingerprint,
    };
    Ok((
        plan,
        PreparedOperation {
            changes,
            result: json!({"from":request.from,"to":request.to,"issues":results}),
        },
    ))
}
impl Repository {
    pub fn preview_cycle_carryover(
        &self,
        request: &CycleCarryoverRequest,
    ) -> Result<CycleCarryoverPlan> {
        self.store()?
            .with_snapshot(|snapshot| prepare(self, snapshot, request).map(|(plan, _)| plan))
    }
    pub fn apply_cycle_carryover(
        &self,
        input: &CycleCarryoverRequest,
        expected: &ContentHash,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.apply_cycle_carryover_with_faults(input, expected, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn apply_cycle_carryover_with_faults(
        &self,
        input: &CycleCarryoverRequest,
        expected: &ContentHash,
        request: &RequestId,
        fault: impl FnMut(transactions::FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.store()?.transact_with_faults(
            request,
            "cycle.carryover",
            &json!({"input":input,"expected":expected}),
            |snapshot| {
                let (plan, prepared) = prepare(self, snapshot, input)?;
                if &plan.fingerprint != expected {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "cycle carryover preview changed; inspect a new plan before applying",
                    ));
                }
                Ok(prepared)
            },
            fault,
        )
    }
}
