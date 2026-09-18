use super::*;
use crate::transactions::{FileChange, MutationReceipt, PreparedOperation, Snapshot};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) fn load_claims(snapshot: &Snapshot<'_>, config: &Config) -> Result<Vec<ClaimRecord>> {
    load_claims_with_limits(snapshot, config, &ClaimCatalogLimits::default())
}

fn load_claims_with_limits(
    snapshot: &Snapshot<'_>,
    config: &Config,
    limits: &ClaimCatalogLimits,
) -> Result<Vec<ClaimRecord>> {
    Ok(load_catalog(snapshot, config, limits)?.records)
}

struct ClaimCatalog {
    records: Vec<ClaimRecord>,
    claim_bytes: usize,
    operation_count: usize,
    operation_bytes: usize,
}

fn load_catalog(
    snapshot: &Snapshot<'_>,
    config: &Config,
    limits: &ClaimCatalogLimits,
) -> Result<ClaimCatalog> {
    limits.validate()?;
    let mut records = BTreeMap::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("claims"), limits.max_claims * 2)? {
        let bytes = snapshot
            .read_bounded(&path, MAX_CLAIM_BYTES)?
            .ok_or_else(|| invalid("claim disappeared").at(&path))?;
        total = total.saturating_add(bytes.len());
        if records.len() >= limits.max_claims || total > limits.max_claim_bytes {
            return Err(invalid("claim catalog exceeds its record or byte bound"));
        }
        let record = validation::parse(&path, &bytes, &config.repository)?;
        if records
            .insert(record.metadata.issue.clone(), record)
            .is_some()
        {
            return Err(invalid("duplicate claim identity"));
        }
    }
    let mut history = BTreeMap::<RequestId, ClaimChange>::new();
    let mut latest = BTreeMap::<IssueId, ClaimRecord>::new();
    let mut generations = BTreeSet::new();
    let claim_bytes = total;
    let mut operation_count = 0;
    total = 0;
    for path in snapshot.list_bounded(Path::new("operations"), limits.max_operations)? {
        let bytes = snapshot
            .read_bounded(&path, limits.max_operation_bytes.saturating_sub(total))?
            .ok_or_else(|| invalid("operation receipt disappeared").at(&path))?;
        total = total.saturating_add(bytes.len());
        operation_count += 1;
        let receipt: MutationReceipt =
            serde_yaml_ng::from_slice(&bytes).map_err(|e| invalid(e.to_string()).at(&path))?;
        if crate::completion::is_operation(&receipt.operation) {
            crate::completion::validate_receipt(&receipt).map_err(|e| e.at(&path))?;
            if receipt.repository.as_ref() != Some(&config.repository)
                || path != Path::new(&format!("operations/{}.yml", receipt.operation_id))
            {
                return Err(invalid(
                    "completion receipt belongs to a different repository or path",
                )
                .at(&path));
            }
        }
        let claim_operation = receipt.operation.starts_with("claim.");
        if !claim_operation && receipt.operation != "issue.complete_claimed" {
            continue;
        }
        crate::transactions::validate_receipt(&receipt)?;
        crate::claims::validate_receipt(&receipt).map_err(|e| e.at(&path))?;
        if receipt.repository.as_ref() != Some(&config.repository)
            || path != Path::new(&format!("operations/{}.yml", receipt.operation_id))
        {
            return Err(invalid("claim receipt belongs to a different source or path").at(&path));
        }
        if !claim_operation {
            continue;
        }
        let change: ClaimChange =
            serde_json::from_value(receipt.result).map_err(|e| invalid(e.to_string()))?;
        let record = &change.after;
        if !generations.insert((record.metadata.issue.clone(), record.metadata.generation)) {
            return Err(invalid("claim history has competing generations"));
        }
        if latest
            .get(&record.metadata.issue)
            .is_none_or(|old| old.metadata.generation < record.metadata.generation)
        {
            latest.insert(record.metadata.issue.clone(), record.clone());
        }
        if history.insert(receipt.request_id, change).is_some() {
            return Err(invalid("claim history repeats a request identity"));
        }
    }
    for change in history.values() {
        if let Some(before) = &change.before
            && history
                .get(&before.metadata.last_request)
                .is_none_or(|origin| origin.after != *before)
        {
            return Err(invalid(
                "claim history is missing its previous generation's authority",
            ));
        }
    }
    if records != latest {
        return Err(invalid(
            "claim records disagree with retained operation authority; missing or edited records require reconciliation",
        ));
    }
    Ok(ClaimCatalog {
        records: records.into_values().collect(),
        claim_bytes,
        operation_count,
        operation_bytes: total,
    })
}

pub(crate) fn contract_from_snapshot(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueId,
    identity: PlanningSourceIdentity,
) -> Result<ClaimWorkContract> {
    let record = crate::issues::resolve_issue(root, snapshot, config, issue.as_str())?;
    if identity.repository != config.repository
        || !matches!(identity.role, SourceRole::Local | SourceRole::Accepted)
    {
        return Err(invalid(
            "claim contract requires its local or accepted planning authority",
        ));
    }
    Ok(ClaimWorkContract {
        issue: issue.clone(),
        issue_source: record.source,
        requirements: crate::context::requirement_fingerprint(root, snapshot, config, issue)?,
        accepted_source: identity,
    })
}

pub(crate) fn eligible(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueId,
    actor: &str,
) -> Result<()> {
    crate::organization::validate_actor(snapshot, &config.repository, actor)?;
    let record = crate::issues::resolve_issue(root, snapshot, config, issue.as_str())?;
    let retired = crate::retirement::read_tombstone(
        root,
        snapshot,
        config,
        &RetirementTarget::new(RetirementKind::Issue, issue.as_str())?,
    )?
    .is_some();
    let readiness = crate::graph::capture(root, snapshot, config)?.readiness(issue)?;
    let category = config.workflow.state(&record.metadata.status)?.category;
    if retired
        || record.metadata.archived
        || !readiness.ready
        || matches!(
            category,
            WorkflowCategory::Completed | WorkflowCategory::Canceled
        )
        || record
            .metadata
            .assignee
            .as_deref()
            .is_some_and(|owner| owner != actor)
    {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "issue is retired, archived, blocked, terminal or assigned to another actor",
        )
        .details(serde_json::json!({"issue":issue,"readiness":readiness})));
    }
    let subjects = crate::context::issue_subjects(&record);
    let questions = crate::questions::load_questions(root, snapshot, config)?;
    if crate::questions::applicability(root, snapshot, config, &questions, &subjects)?
        .iter()
        .any(|q| q.blocks_implementation)
    {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "a question or stale decision blocks implementation",
        ));
    }
    Ok(())
}

pub(crate) fn prepare(
    snapshot: &Snapshot<'_>,
    config: &Config,
    request: &ClaimRequest,
    current: &ClaimWorkContract,
    request_id: &RequestId,
    now: Timestamp,
    token: ClaimToken,
) -> Result<PreparedOperation> {
    prepare_with_limits(
        snapshot,
        config,
        request,
        current,
        request_id,
        now,
        token,
        &ClaimCatalogLimits::default(),
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_with_limits(
    snapshot: &Snapshot<'_>,
    config: &Config,
    request: &ClaimRequest,
    current: &ClaimWorkContract,
    request_id: &RequestId,
    now: Timestamp,
    token: ClaimToken,
    limits: &ClaimCatalogLimits,
) -> Result<PreparedOperation> {
    let catalog = load_catalog(snapshot, config, limits)?;
    let previous = catalog
        .records
        .iter()
        .find(|record| &record.metadata.issue == request.issue())
        .cloned();
    let policy = config.claims.clone().unwrap_or_default();
    let after = transition::apply(
        request,
        previous.as_ref(),
        current,
        &policy,
        now,
        request_id,
        token,
    )?;
    let projected_claims = catalog.records.len() + usize::from(previous.is_none());
    let projected_bytes = catalog.claim_bytes
        - previous.as_ref().map_or(0, |record| record.document.len())
        + after.document.len();
    if projected_claims > limits.max_claims || projected_bytes > limits.max_claim_bytes {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "claim mutation would exceed the readable claim catalog record or byte bound",
        ));
    }
    let changes = vec![FileChange {
        path: after.path.clone(),
        expected: previous.as_ref().map(|r| r.source.content.clone()),
        content: Some(after.document.as_bytes().to_vec()),
    }];
    let result = ClaimChange {
        request: request.clone(),
        policy,
        accepted_contract: current.clone(),
        before: previous,
        after,
    };
    let prepared = PreparedOperation {
        changes,
        result: serde_json::to_value(result).map_err(|e| invalid(e.to_string()))?,
    };
    // Every publisher uses MutationReceipt's canonical YAML shape. The final
    // operation ID is allocated by that publisher; all validated OP-ULIDs have
    // the same 29-byte, unquoted YAML representation, so this is its exact size.
    let receipt = MutationReceipt {
        schema_version: SchemaVersion::CURRENT,
        repository: Some(config.repository.clone()),
        operation_id: "OP-00000000000000000000000000".parse()?,
        request_id: request_id.clone(),
        operation: request.operation().into(),
        input_hash: crate::transactions::canonical_hash(&serde_json::json!({"request": request}))?,
        result: prepared.result.clone(),
        changed: prepared
            .changes
            .iter()
            .map(|change| crate::transactions::ChangedPath {
                path: change.path.clone(),
                before: change.expected.clone(),
                after: change.content.as_deref().map(ContentHash::of),
            })
            .collect(),
    };
    check_operation_capacity(
        catalog.operation_count,
        catalog.operation_bytes,
        &receipt,
        limits,
    )?;
    Ok(prepared)
}

pub(crate) fn validate_operation_capacity(
    snapshot: &Snapshot<'_>,
    receipt: &MutationReceipt,
    limits: &ClaimCatalogLimits,
) -> Result<()> {
    limits.validate()?;
    let paths = snapshot.list_bounded(Path::new("operations"), limits.max_operations)?;
    let mut bytes = 0usize;
    for path in &paths {
        bytes += snapshot
            .read_bounded(path, limits.max_operation_bytes.saturating_sub(bytes))?
            .ok_or_else(|| invalid("operation receipt disappeared").at(path))?
            .len();
    }
    check_operation_capacity(paths.len(), bytes, receipt, limits)
}

fn check_operation_capacity(
    count: usize,
    bytes: usize,
    receipt: &MutationReceipt,
    limits: &ClaimCatalogLimits,
) -> Result<()> {
    let receipt_bytes = serde_yaml_ng::to_string(receipt)
        .map_err(|error| invalid(error.to_string()))?
        .len();
    if count >= limits.max_operations
        || receipt_bytes > limits.max_operation_bytes.saturating_sub(bytes)
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "mutation and its durable receipt would exceed the readable operation history count or byte bound",
        ));
    }
    Ok(())
}

impl Repository {
    pub fn local_claim_contract(&self, issue: &IssueId) -> Result<ClaimWorkContract> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            require_local(&config)?;
            let identity = crate::sources::identity_from_snapshot(
                self.root(),
                snapshot,
                &config,
                SourceRole::Local,
            )?;
            contract_from_snapshot(self.root(), snapshot, &config, issue, identity)
        })
    }

    pub fn mutate_local_claim(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_local_claim_with_faults(request, request_id, chrono::Utc::now, |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn mutate_local_claim_with_faults(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
        clock: impl FnOnce() -> Timestamp,
        fault: impl FnMut(crate::transactions::FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.mutate_local_claim_with_limits_and_faults(
            request,
            request_id,
            &ClaimCatalogLimits::default(),
            clock,
            fault,
        )
    }

    #[doc(hidden)]
    pub fn mutate_local_claim_with_limits(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
        limits: &ClaimCatalogLimits,
    ) -> Result<MutationReceipt> {
        self.mutate_local_claim_with_limits_and_faults(
            request,
            request_id,
            limits,
            chrono::Utc::now,
            |_| Ok(()),
        )
    }

    #[doc(hidden)]
    pub fn mutate_local_claim_with_limits_and_faults(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
        limits: &ClaimCatalogLimits,
        clock: impl FnOnce() -> Timestamp,
        fault: impl FnMut(crate::transactions::FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let input = serde_json::json!({"request":request});
        let receipt = self.store()?.transact_with_faults(
            request_id,
            request.operation(),
            &input,
            |snapshot| {
                let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
                require_local(&config)?;
                let identity = crate::sources::identity_from_snapshot(
                    self.root(),
                    snapshot,
                    &config,
                    SourceRole::Local,
                )?;
                let terminal = matches!(
                    request,
                    ClaimRequest::Mutate {
                        mutation: ClaimMutation::Release { .. }
                            | ClaimMutation::Cancel { .. }
                            | ClaimMutation::Supersede { .. },
                        ..
                    }
                );
                let current = match contract_from_snapshot(
                    self.root(),
                    snapshot,
                    &config,
                    request.issue(),
                    identity,
                ) {
                    Ok(contract) => contract,
                    Err(error) if terminal && error.code == ErrorCode::NotFound => {
                        load_claims(snapshot, &config)?
                            .into_iter()
                            .find(|r| &r.metadata.issue == request.issue())
                            .ok_or_else(|| {
                                PmError::new(ErrorCode::ClaimLost, "claim no longer exists")
                            })?
                            .metadata
                            .contract
                    }
                    Err(error) => return Err(error),
                };
                if !terminal {
                    let actor = match request {
                        ClaimRequest::Acquire { input } => &input.actor,
                        ClaimRequest::Mutate { mutation, .. } => mutation.actor(),
                    };
                    eligible(self.root(), snapshot, &config, request.issue(), actor)?;
                }
                prepare_with_limits(
                    snapshot,
                    &config,
                    request,
                    &current,
                    request_id,
                    clock(),
                    ClaimToken::new(),
                    limits,
                )
            },
            fault,
        )?;
        validation::validate_receipt(&receipt)?;
        Ok(receipt)
    }

    pub fn local_claims(&self) -> Result<Vec<ClaimStatus>> {
        self.local_claims_with_limits(&ClaimCatalogLimits::default())
    }

    #[doc(hidden)]
    pub fn local_claims_with_limits(
        &self,
        limits: &ClaimCatalogLimits,
    ) -> Result<Vec<ClaimStatus>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            require_local(&config)?;
            let identity = crate::sources::identity_from_snapshot(
                self.root(),
                snapshot,
                &config,
                SourceRole::Local,
            )?;
            let now = chrono::Utc::now();
            let policy = config.claims.clone().unwrap_or_default();
            let observation = SourceObservation {
                identity: identity.clone(),
                observed_at: now,
                remote_observation: None,
                freshness: SourceFreshness::CurrentAtObservation,
                reason_codes: vec!["local_source_only".into()],
            };
            load_claims_with_limits(snapshot, &config, limits)?
                .into_iter()
                .map(|claim| {
                    let contract = match contract_from_snapshot(
                        self.root(),
                        snapshot,
                        &config,
                        &claim.metadata.issue,
                        identity.clone(),
                    ) {
                        Ok(contract) => Some(contract),
                        Err(error) if error.code == ErrorCode::NotFound => None,
                        Err(error) => return Err(error),
                    };
                    let assessment = assess_current(
                        self.root(),
                        snapshot,
                        &config,
                        &claim,
                        contract.as_ref(),
                        &policy,
                        now,
                        ClaimGuarantee::LocalSourceOnly,
                    )?;
                    Ok(ClaimStatus {
                        claim,
                        assessment,
                        source: observation.clone(),
                    })
                })
                .collect()
        })
    }
}

fn require_local(config: &Config) -> Result<()> {
    if config.sources.is_some() {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "shared planning requires the shared coordination protocol; a local claim cannot substitute for publication",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn assess_current(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    claim: &ClaimRecord,
    current: Option<&ClaimWorkContract>,
    policy: &ClaimPolicy,
    now: Timestamp,
    guarantee: ClaimGuarantee,
) -> Result<ClaimAssessment> {
    let mut assessment = transition::assess(claim, current, policy, now, guarantee);
    if assessment.disposition == ClaimDisposition::Usable {
        match eligible(
            root,
            snapshot,
            config,
            &claim.metadata.issue,
            &claim.metadata.actor,
        ) {
            Ok(()) => (),
            Err(error) if error.code == ErrorCode::PolicyBlocked => {
                assessment.disposition = ClaimDisposition::Blocked;
                assessment.may_continue = false;
                assessment
                    .reason_codes
                    .push("current_work_eligibility_blocked".into());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(assessment)
}
