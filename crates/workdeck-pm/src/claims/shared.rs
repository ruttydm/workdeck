use super::*;
use crate::sources;
use std::{
    path::Path,
    time::{Duration, Instant},
};

impl Repository {
    pub fn claim_contract(&self, issue: &IssueId) -> Result<ClaimWorkContract> {
        if self.config()?.sources.is_none() {
            return self.local_claim_contract(issue);
        }
        let view = sources::capture(
            worktree(self)?,
            &SourceSelector::Accepted,
            &SourceCaptureLimits::default(),
        )?;
        let contract = view.snapshot.with_snapshot(|snapshot| {
            let root = Path::new("source-snapshot");
            let config = crate::repository::config_from_snapshot(root, snapshot)?;
            store::contract_from_snapshot(
                root,
                snapshot,
                &config,
                issue,
                view.observation.identity.clone(),
            )
        })?;
        view.revalidate()?;
        Ok(contract)
    }

    /// Cached shared views never authorize work. Explicit publication supplies a
    /// separate confirmed observation; ordinary reads do not contact the remote.
    pub fn claims(&self) -> Result<Vec<ClaimStatus>> {
        if self.config()?.sources.is_none() {
            return self.local_claims();
        }
        let root = worktree(self)?;
        let accepted = sources::capture(
            root,
            &SourceSelector::Accepted,
            &SourceCaptureLimits::default(),
        )?;
        let coordination = sources::capture(
            root,
            &SourceSelector::Coordination,
            &SourceCaptureLimits::default(),
        )?;
        coordination.claim_statuses(&accepted)
    }

    pub fn mutate_claim(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
    ) -> Result<ClaimOperationOutcome> {
        self.mutate_claim_with_publication_faults(request, request_id, |_| Ok(()))
    }

    /// Publish against the exact Git/remote binding inspected by the caller.
    /// Work-contract identity remains portable across clones; this precondition
    /// belongs to the local publication request, not the stored requirements.
    pub fn mutate_claim_reviewed(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
        expected_binding: &ContentHash,
    ) -> Result<ClaimOperationOutcome> {
        self.mutate_claim_bound(request, request_id, Some(expected_binding), |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn mutate_claim_reviewed_with_publication_faults(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
        expected_binding: &ContentHash,
        publication_fault: impl FnMut(sources::PublicationFaultPoint) -> Result<()>,
    ) -> Result<ClaimOperationOutcome> {
        self.mutate_claim_bound(
            request,
            request_id,
            Some(expected_binding),
            publication_fault,
        )
    }

    /// Explicit publication fault injection for recovery qualification.
    #[doc(hidden)]
    pub fn mutate_claim_with_publication_faults(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
        publication_fault: impl FnMut(sources::PublicationFaultPoint) -> Result<()>,
    ) -> Result<ClaimOperationOutcome> {
        self.mutate_claim_bound(request, request_id, None, publication_fault)
    }

    /// Keep an inspected local action local even if configuration later changes.
    pub fn mutate_local_claim_outcome(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
    ) -> Result<ClaimOperationOutcome> {
        let receipt = self.mutate_local_claim(request, request_id)?;
        let mut reasons = Vec::new();
        let current = match self.local_claims() {
            Ok(claims) => claims
                .into_iter()
                .find(|s| &s.claim.metadata.issue == request.issue()),
            Err(error) => {
                reasons.push(format!("current_claim_unavailable:{:?}", error.code));
                None
            }
        };
        finish(
            self.identity().clone(),
            request_id.clone(),
            Some(receipt),
            None,
            current,
            reasons,
        )
    }

    pub(crate) fn mutate_claim_bound(
        &self,
        request: &ClaimRequest,
        request_id: &RequestId,
        expected_binding: Option<&ContentHash>,
        publication_fault: impl FnMut(sources::PublicationFaultPoint) -> Result<()>,
    ) -> Result<ClaimOperationOutcome> {
        let deadline =
            Instant::now() + Duration::from_secs(SourceCaptureLimits::default().timeout_seconds);
        if self.config()?.sources.is_none() {
            if expected_binding.is_some() {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "reviewed shared publication cannot become a local mutation",
                ));
            }
            return self.mutate_local_claim_outcome(request, request_id);
        }
        let publication = sources::coordinate_with_faults_before(
            self,
            request_id,
            request.operation(),
            &serde_json::json!({"request":request}),
            |basis| {
                let config = basis.accepted.config()?;
                let previous = basis
                    .coordination
                    .with_snapshot(|snapshot| store::load_claims(snapshot, &config))?
                    .into_iter()
                    .find(|r| &r.metadata.issue == request.issue());
                let terminal = matches!(
                    request,
                    ClaimRequest::Mutate {
                        mutation: ClaimMutation::Release { .. }
                            | ClaimMutation::Cancel { .. }
                            | ClaimMutation::Supersede { .. },
                        ..
                    }
                );
                let current = basis.accepted.with_snapshot(|snapshot| {
                    let root = Path::new("source-snapshot");
                    let current = match store::contract_from_snapshot(
                        root,
                        snapshot,
                        &config,
                        request.issue(),
                        basis.accepted_observation.identity.clone(),
                    ) {
                        Ok(contract) => contract,
                        Err(error) if terminal && error.code == ErrorCode::NotFound => previous
                            .as_ref()
                            .ok_or_else(|| {
                                PmError::new(ErrorCode::ClaimLost, "claim no longer exists")
                            })?
                            .metadata
                            .contract
                            .clone(),
                        Err(error) => return Err(error),
                    };
                    if !terminal {
                        let actor = match request {
                            ClaimRequest::Acquire { input } => &input.actor,
                            ClaimRequest::Mutate { mutation, .. } => mutation.actor(),
                        };
                        store::eligible(root, snapshot, &config, request.issue(), actor)?;
                    }
                    Ok(current)
                })?;
                let token: ClaimToken = format!(
                    "CLM-{}",
                    basis
                        .operation_id
                        .as_str()
                        .strip_prefix("OP-")
                        .expect("validated operation identity")
                )
                .parse()?;
                basis.coordination.with_snapshot(|snapshot| {
                    store::prepare(
                        snapshot,
                        &config,
                        request,
                        &current,
                        &basis.request_id,
                        basis.as_of,
                        token,
                    )
                })
            },
            sources::PublicationAdmission {
                deadline,
                expected_binding,
            },
            publication_fault,
        )?;
        observe_publication(self, request, request_id, publication, deadline)
    }
}

fn observe_publication(
    repository: &Repository,
    request: &ClaimRequest,
    request_id: &RequestId,
    publication: PublicationOutcome,
    deadline: Instant,
) -> Result<ClaimOperationOutcome> {
    if let Some(receipt) = &publication.receipt {
        validation::validate_receipt(receipt)?;
    }
    let mut reasons = publication.reason_codes.clone();
    let current = (|| -> Result<Option<ClaimStatus>> {
        let root = worktree(repository)?;
        let accepted = sources::capture_with_deadline(
            root,
            &SourceSelector::Accepted,
            &SourceCaptureLimits::default(),
            deadline,
        )?;
        let coordination = sources::capture_with_deadline(
            root,
            &SourceSelector::Coordination,
            &SourceCaptureLimits::default(),
            deadline,
        )?;
        let confirmed = publication.state == PublicationState::Confirmed
            && publication
                .accepted
                .as_ref()
                .is_some_and(|o| o.identity == accepted.observation.identity)
            && publication
                .coordination
                .as_ref()
                .is_some_and(|o| o.identity == coordination.observation.identity);
        let guarantee = if confirmed {
            ClaimGuarantee::SharedConfirmed
        } else {
            ClaimGuarantee::Unconfirmed
        };
        Ok(
            statuses_before(&coordination, &accepted, guarantee, Some(deadline))?
                .into_iter()
                .find(|s| &s.claim.metadata.issue == request.issue()),
        )
    })();
    let current = match current {
        Ok(current) => current,
        Err(error) => {
            reasons.push(format!("current_claim_unavailable:{:?}", error.code));
            None
        }
    };
    finish(
        repository.identity().clone(),
        request_id.clone(),
        publication.receipt.clone(),
        Some(publication),
        current,
        reasons,
    )
}

impl PlanningSourceView {
    pub(crate) fn claim_statuses_at(
        &self,
        accepted: &PlanningSourceView,
        as_of: Timestamp,
        deadline: Instant,
    ) -> Result<Vec<ClaimStatus>> {
        let guarantee = if self.observation.identity.role == SourceRole::Local {
            ClaimGuarantee::LocalSourceOnly
        } else {
            ClaimGuarantee::Unconfirmed
        };
        statuses_at_before(self, accepted, guarantee, as_of, Some(deadline))
    }

    /// Interpret immutable coordination records against the chosen accepted view.
    /// This is cached inspection, not remote confirmation or an execution lease.
    pub fn claim_statuses(&self, accepted: &PlanningSourceView) -> Result<Vec<ClaimStatus>> {
        let guarantee = if self.observation.identity.role == SourceRole::Local {
            ClaimGuarantee::LocalSourceOnly
        } else {
            ClaimGuarantee::Unconfirmed
        };
        statuses(self, accepted, guarantee)
    }
}

fn statuses(
    coordination: &PlanningSourceView,
    accepted: &PlanningSourceView,
    guarantee: ClaimGuarantee,
) -> Result<Vec<ClaimStatus>> {
    statuses_before(coordination, accepted, guarantee, None)
}

fn statuses_before(
    coordination: &PlanningSourceView,
    accepted: &PlanningSourceView,
    guarantee: ClaimGuarantee,
    deadline: Option<Instant>,
) -> Result<Vec<ClaimStatus>> {
    statuses_at_before(
        coordination,
        accepted,
        guarantee,
        chrono::Utc::now(),
        deadline,
    )
}

fn statuses_at_before(
    coordination: &PlanningSourceView,
    accepted: &PlanningSourceView,
    guarantee: ClaimGuarantee,
    now: Timestamp,
    deadline: Option<Instant>,
) -> Result<Vec<ClaimStatus>> {
    let role = coordination.observation.identity.role;
    if coordination.observation.identity.repository != accepted.observation.identity.repository
        || !matches!(
            (role, accepted.observation.identity.role),
            (SourceRole::Local, SourceRole::Local)
                | (SourceRole::Coordination, SourceRole::Accepted)
        )
        || coordination.snapshot.identity() != &coordination.observation.identity
        || accepted.snapshot.identity() != &accepted.observation.identity
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "claim observations require matching local or accepted/coordination authorities",
        ));
    }
    let config = accepted.snapshot.config()?;
    let policy = config.claims.clone().unwrap_or_default();
    let records = coordination
        .snapshot
        .with_snapshot(|snapshot| store::load_claims(snapshot, &config))?;
    let results = accepted.snapshot.with_snapshot(|snapshot| {
        let root = Path::new("source-snapshot");
        records
            .into_iter()
            .map(|claim| {
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    return Err(PmError::new(
                        ErrorCode::Io,
                        "Claim assessment budget expired",
                    ));
                }
                let contract = match store::contract_from_snapshot(
                    root,
                    snapshot,
                    &config,
                    &claim.metadata.issue,
                    accepted.observation.identity.clone(),
                ) {
                    Ok(contract) => Some(contract),
                    Err(error) if error.code == ErrorCode::NotFound => None,
                    Err(error) => return Err(error),
                };
                let assessment = store::assess_current(
                    root,
                    snapshot,
                    &config,
                    &claim,
                    contract.as_ref(),
                    &policy,
                    now,
                    guarantee,
                )?;
                Ok(ClaimStatus {
                    claim,
                    assessment,
                    source: coordination.observation.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()
    })?;
    if let Some(deadline) = deadline {
        coordination.revalidate_before(deadline)?;
        accepted.revalidate_before(deadline)?;
    } else {
        coordination.revalidate()?;
        accepted.revalidate()?;
    }
    Ok(results)
}

fn finish(
    repository: RepositoryId,
    request_id: RequestId,
    receipt: Option<crate::transactions::MutationReceipt>,
    publication: Option<PublicationOutcome>,
    current: Option<ClaimStatus>,
    reason_codes: Vec<String>,
) -> Result<ClaimOperationOutcome> {
    let after = receipt
        .as_ref()
        .map(|r| serde_json::from_value::<ClaimChange>(r.result.clone()).map(|c| c.after))
        .transpose()
        .map_err(|e| invalid(e.to_string()))?;
    let requested_token_current = after
        .as_ref()
        .zip(current.as_ref())
        .is_some_and(|(after, current)| after.precondition() == current.claim.precondition());
    let may_continue =
        requested_token_current && current.as_ref().is_some_and(|s| s.assessment.may_continue);
    Ok(ClaimOperationOutcome {
        repository,
        request_id,
        receipt,
        publication,
        current,
        requested_token_current,
        may_continue,
        reason_codes,
    })
}

fn worktree(repository: &Repository) -> Result<&Path> {
    repository
        .root()
        .parent()
        .ok_or_else(|| invalid("planning source has no project root"))
}

impl SourceSnapshot {
    /// Derive the work contract entirely from this captured authority.
    pub fn claim_contract(&self, issue: &IssueId) -> Result<ClaimWorkContract> {
        self.with_snapshot(|snapshot| {
            let root = Path::new("source-snapshot");
            let config = crate::repository::config_from_snapshot(root, snapshot)?;
            store::contract_from_snapshot(root, snapshot, &config, issue, self.identity().clone())
        })
    }
}

#[cfg(all(test, unix))]
mod deadline_tests {
    use super::*;
    use crate::{CreateIssue, SharedSources};
    use std::{fs, process::Command};
    fn git(root: &Path, args: &[&str]) -> Vec<u8> {
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
                command.env_remove(key);
            }
        }
        let output = command
            .current_dir(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Repository, IssueId) {
        let temp = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-b", "main"]);
        git(remote.path(), &["init", "--bare"]);
        git(temp.path(), &["config", "user.name", "Fixture"]);
        git(
            temp.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(
            temp.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let create = CreateIssue::new("Implement", "accepted requirement");
        let receipt = repository.create_issue(&create, &RequestId::new()).unwrap();
        let issue: IssueRecord = serde_json::from_value(receipt.result).unwrap();
        let mut config = repository.config().unwrap();
        config.sources = Some(SharedSources {
            remote: "origin".into(),
            accepted_ref: "refs/heads/main".parse().unwrap(),
            coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
            proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
        });
        fs::write(
            repository.root().join("config.yml"),
            serde_yaml_ng::to_string(&config).unwrap(),
        )
        .unwrap();
        fs::write(temp.path().join("code.txt"), "accepted code").unwrap();
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-m", "accepted"]);
        git(temp.path(), &["push", "origin", "main"]);
        (temp, remote, repository, issue.metadata.id)
    }
    fn acquire(repo: &Repository, issue: &IssueId, actor: &str) -> ClaimRequest {
        ClaimRequest::Acquire {
            input: Box::new(AcquireClaim {
                actor: actor.into(),
                contract: repo.claim_contract(issue).unwrap(),
                ttl_seconds: None,
                recovery: None,
            }),
        }
    }

    #[test]
    fn expired_postpublication_budget_retains_confirmation_without_authorizing_work() {
        let (_worktree, _remote, repository, issue) = fixture();
        let request = acquire(&repository, &issue, "agent");
        let id = RequestId::new();
        let original = repository.mutate_claim(&request, &id).unwrap();
        assert!(original.may_continue, "{original:?}");
        let publication = original.publication.clone().unwrap();
        assert_eq!(publication.state, PublicationState::Confirmed);
        let expired =
            observe_publication(&repository, &request, &id, publication, Instant::now()).unwrap();
        assert!(
            expired.current.is_none(),
            "expired observation restarted a new budget: {expired:?}"
        );
        assert!(!expired.may_continue);
        assert!(!expired.requested_token_current);
        assert_eq!(expired.receipt, original.receipt);
        assert_eq!(expired.publication, original.publication);
        assert!(
            expired
                .reason_codes
                .iter()
                .any(|reason| reason.starts_with("current_claim_unavailable:"))
        );
    }
}
