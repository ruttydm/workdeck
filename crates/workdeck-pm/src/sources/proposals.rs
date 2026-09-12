use super::*;
use crate::{
    ContentHash, OperationId, Repository, RepositoryId, RequestId, Result, SchemaVersion,
    transactions::ChangedPath,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
/// Canonical compact UTF8 JSON bytes of a complete reviewed proposal plan.
pub const MAX_PROPOSAL_PLAN_BYTES: usize = 8 * 1024 * 1024;
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalRequest {
    pub reference: GitRefName,
    pub title: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalPlan {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub config: ContentHash,
    pub binding: ContentHash,
    pub source: PlanningSourceIdentity,
    pub accepted: PlanningSourceIdentity,
    pub expected_proposal: Option<PlanningSourceIdentity>,
    pub reference: GitRefName,
    pub title: String,
    /// Exact planning changes compared to the accepted source.
    pub changed: Vec<ChangedPath>,
    /// Exact planning changes compared to the reviewed existing proposal (or accepted source for creation).
    pub proposal_changed: Vec<ChangedPath>,
    /// The candidate preserves application bytes from this accepted commit.
    pub application_base: GitOid,
    pub fingerprint: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalOutcome {
    pub repository: RepositoryId,
    pub request_id: RequestId,
    pub operation_id: OperationId,
    pub state: PublicationState,
    pub plan: ProposalPlan,
    pub candidate: GitOid,
    pub confirmation: Option<PublicationConfirmation>,
    pub current: Option<SourceObservation>,
    pub reason_codes: Vec<String>,
    pub replayed: bool,
}
impl ProposalPlan {
    pub fn validate(&self) -> Result<()> {
        super::proposal_impl::validate(self)
    }
}
impl Repository {
    pub fn preview_proposal(&self, request: &ProposalRequest) -> Result<ProposalPlan> {
        super::proposal_impl::preview(self, request)
    }
    pub fn publish_proposal(
        &self,
        plan: &ProposalPlan,
        request: &RequestId,
    ) -> Result<ProposalOutcome> {
        self.publish_proposal_with_faults(plan, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn publish_proposal_with_faults(
        &self,
        plan: &ProposalPlan,
        request: &RequestId,
        fault: impl FnMut(PublicationFaultPoint) -> Result<()>,
    ) -> Result<ProposalOutcome> {
        super::proposal_impl::publish(self, plan, request, fault)
    }
    pub fn save_proposal_plan(&self, plan: &ProposalPlan) -> Result<PathBuf> {
        super::proposal_impl::save(self, plan)
    }
    pub fn load_proposal_plan(&self, fingerprint: &ContentHash) -> Result<ProposalPlan> {
        super::proposal_impl::load(self, fingerprint)
    }
    pub fn proposal_status(&self, request: &RequestId) -> Result<ProposalOutcome> {
        super::proposal_impl::status(self, request)
    }
    pub fn resume_proposal(&self, request: &RequestId) -> Result<ProposalOutcome> {
        super::proposal_impl::resume(self, request)
    }
}
