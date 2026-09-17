use crate::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_CLAIM_BYTES: usize = 128 * 1024;
pub(crate) const MAX_CLAIMS: usize = 4096;

/// Explicit bounded test instrumentation; ordinary APIs retain these defaults.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct ClaimCatalogLimits {
    pub max_claims: usize,
    /// Aggregate bytes of current claim documents, not a per-record limit.
    pub max_claim_bytes: usize,
    pub max_operations: usize,
    pub max_operation_bytes: usize,
}
impl Default for ClaimCatalogLimits {
    fn default() -> Self {
        Self {
            max_claims: MAX_CLAIMS,
            max_claim_bytes: 32 * 1024 * 1024,
            max_operations: 10_000,
            max_operation_bytes: 64 * 1024 * 1024,
        }
    }
}
impl ClaimCatalogLimits {
    pub(crate) fn validate(&self) -> Result<()> {
        let defaults = Self::default();
        if self.max_claims == 0
            || self.max_claims > defaults.max_claims
            || self.max_claim_bytes == 0
            || self.max_claim_bytes > defaults.max_claim_bytes
            || self.max_operations == 0
            || self.max_operations > defaults.max_operations
            || self.max_operation_bytes == 0
            || self.max_operation_bytes > defaults.max_operation_bytes
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "claim catalog limits must be positive and cannot exceed production bounds",
            ));
        }
        Ok(())
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimWorkContract {
    pub issue: IssueId,
    pub issue_source: SourceToken,
    pub requirements: ContentHash,
    pub accepted_source: PlanningSourceIdentity,
}
impl ClaimWorkContract {
    pub fn fingerprint(&self) -> Result<ContentHash> {
        crate::transactions::canonical_hash(
            &serde_json::to_value(self)
                .map_err(|e| PmError::new(ErrorCode::InvalidInput, e.to_string()))?,
        )
    }
    pub fn validate(&self) -> Result<()> {
        match self.accepted_source.role {
            SourceRole::Local
                if self.accepted_source.ref_name.is_none()
                    && self.accepted_source.commit.is_none()
                    && self.accepted_source.tree.is_none() =>
            {
                Ok(())
            }
            SourceRole::Accepted
                if self.accepted_source.ref_name.is_some()
                    && self.accepted_source.commit.is_some()
                    && self.accepted_source.tree.is_some() =>
            {
                Ok(())
            }
            _ => Err(PmError::new(
                ErrorCode::InvalidSchema,
                "claim contract requires an exact local or accepted source identity",
            )),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimPrecondition {
    pub token: ClaimToken,
    pub generation: u64,
    pub content: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimState {
    Active,
    Released,
    Canceled,
    Superseded,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimMetadata {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub issue: IssueId,
    pub token: ClaimToken,
    pub generation: u64,
    pub actor: String,
    pub contract: ClaimWorkContract,
    pub state: ClaimState,
    pub acquired_at: Timestamp,
    pub updated_at: Timestamp,
    pub expires_at: Timestamp,
    /// The immutable receipt supplies its operation ID; this request joins it.
    pub last_request: RequestId,
    pub last_operation: String,
    pub reason: Option<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRecord {
    pub metadata: ClaimMetadata,
    pub source: SourceToken,
    pub path: PathBuf,
    pub document: String,
}
impl ClaimRecord {
    pub fn precondition(&self) -> ClaimPrecondition {
        ClaimPrecondition {
            token: self.metadata.token.clone(),
            generation: self.metadata.generation,
            content: self.source.content.clone(),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimGuarantee {
    LocalSourceOnly,
    SharedConfirmed,
    Unconfirmed,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimDisposition {
    Usable,
    Blocked,
    NeedsRevalidation,
    ClockUncertain,
    Expired,
    Released,
    Canceled,
    Superseded,
    Unknown,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimAssessment {
    pub disposition: ClaimDisposition,
    pub guarantee: ClaimGuarantee,
    pub assessed_at: Timestamp,
    pub may_continue: bool,
    pub recovery_required: bool,
    pub reason_codes: Vec<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRecovery {
    pub expected: ClaimPrecondition,
    /// Explicit acknowledgement of abandoned work; expiry does not stop its process.
    pub reason: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquireClaim {
    pub actor: String,
    pub contract: ClaimWorkContract,
    pub ttl_seconds: Option<u64>,
    pub recovery: Option<ClaimRecovery>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClaimMutation {
    Renew {
        actor: String,
        ttl_seconds: Option<u64>,
    },
    Revalidate {
        actor: String,
        contract: Box<ClaimWorkContract>,
        ttl_seconds: Option<u64>,
    },
    Release {
        actor: String,
        reason: String,
    },
    Cancel {
        actor: String,
        reason: String,
    },
    Supersede {
        actor: String,
        reason: String,
    },
}
impl ClaimMutation {
    pub(crate) fn actor(&self) -> &str {
        match self {
            Self::Renew { actor, .. }
            | Self::Revalidate { actor, .. }
            | Self::Release { actor, .. }
            | Self::Cancel { actor, .. }
            | Self::Supersede { actor, .. } => actor,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClaimRequest {
    Acquire {
        input: Box<AcquireClaim>,
    },
    Mutate {
        issue: IssueId,
        expected: ClaimPrecondition,
        mutation: ClaimMutation,
    },
}
impl ClaimRequest {
    pub fn issue(&self) -> &IssueId {
        match self {
            Self::Acquire { input } => &input.contract.issue,
            Self::Mutate { issue, .. } => issue,
        }
    }
    pub fn operation(&self) -> &'static str {
        match self {
            Self::Acquire { input } if input.recovery.is_some() => "claim.recover",
            Self::Acquire { .. } => "claim.acquire",
            Self::Mutate { mutation, .. } => match mutation {
                ClaimMutation::Renew { .. } => "claim.renew",
                ClaimMutation::Revalidate { .. } => "claim.revalidate",
                ClaimMutation::Release { .. } => "claim.release",
                ClaimMutation::Cancel { .. } => "claim.cancel",
                ClaimMutation::Supersede { .. } => "claim.supersede",
            },
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimChange {
    pub request: ClaimRequest,
    pub policy: ClaimPolicy,
    pub accepted_contract: ClaimWorkContract,
    pub before: Option<ClaimRecord>,
    pub after: ClaimRecord,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimStatus {
    pub claim: ClaimRecord,
    pub assessment: ClaimAssessment,
    pub source: SourceObservation,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimOperationOutcome {
    pub repository: RepositoryId,
    pub request_id: RequestId,
    pub receipt: Option<crate::transactions::MutationReceipt>,
    pub publication: Option<PublicationOutcome>,
    /// Current observation is separate from the original operation's recorded token.
    pub current: Option<ClaimStatus>,
    pub requested_token_current: bool,
    pub may_continue: bool,
    pub reason_codes: Vec<String>,
}
