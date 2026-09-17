use super::*;
use crate::{
    OperationId, Repository, RepositoryId, RequestId, Result, Timestamp,
    transactions::{MutationReceipt, PreparedOperation},
};
use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationState {
    Prepared,
    Confirmed,
    Rejected,
    Uncertain,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationAttempt {
    pub number: u8,
    pub observed: Option<GitOid>,
    pub candidate: GitOid,
    pub state: PublicationState,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationConfirmation {
    pub remote: String,
    pub reference: GitRefName,
    pub commit: GitOid,
    pub observed_parent: Option<GitOid>,
    pub confirmed_at: Timestamp,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationOutcome {
    pub repository: RepositoryId,
    pub request_id: RequestId,
    pub operation_id: OperationId,
    pub state: PublicationState,
    pub receipt: Option<MutationReceipt>,
    pub confirmation: Option<PublicationConfirmation>,
    pub accepted: Option<SourceObservation>,
    pub coordination: Option<SourceObservation>,
    pub attempts: Vec<PublicationAttempt>,
    pub reason_codes: Vec<String>,
    pub replayed: bool,
}
pub(crate) struct CoordinationBasis {
    pub accepted: SourceSnapshot,
    pub coordination: SourceSnapshot,
    pub accepted_observation: SourceObservation,
    pub coordination_observation: SourceObservation,
    pub request_id: RequestId,
    pub operation_id: OperationId,
    pub attempt: u8,
    pub as_of: Timestamp,
    pub observed: RemoteRefObservation,
}
/// Explicit deterministic fault injection; never selected by an environment variable.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationFaultPoint {
    AfterPrepared,
    BeforePush,
    AfterPush,
    AfterConfirmation,
}
pub(crate) struct PublicationAdmission<'a> {
    pub deadline: std::time::Instant,
    /// A prior observed operation may require this exact Git directory and remote binding.
    pub expected_binding: Option<&'a crate::ContentHash>,
}
pub(crate) fn coordinate_with_faults_before(
    repository: &Repository,
    request: &RequestId,
    operation: &str,
    input: &serde_json::Value,
    prepare: impl FnMut(&CoordinationBasis) -> Result<PreparedOperation>,
    admission: PublicationAdmission<'_>,
    fault: impl FnMut(PublicationFaultPoint) -> Result<()>,
) -> Result<PublicationOutcome> {
    super::coordination::coordinate_before(
        repository, request, operation, input, prepare, admission, fault,
    )
}
