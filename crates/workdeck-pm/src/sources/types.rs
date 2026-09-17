use crate::{ContentHash, ErrorCode, PmError, RepositoryId, Result, Timestamp};
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf, str::FromStr};

fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}

#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct GitRefName(String);
impl GitRefName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for GitRefName {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        if value.len() > 240
            || !value.starts_with("refs/")
            || value.ends_with('/')
            || value.ends_with('.')
            || value.contains("..")
            || value.contains("@{")
            || value.contains("//")
            || value
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
            || value
                .split('/')
                .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
        {
            return Err(invalid(
                "Git references must be bounded full refs without revision/refspec expressions",
            ));
        }
        Ok(Self(value.into()))
    }
}
impl TryFrom<String> for GitRefName {
    type Error = PmError;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl From<GitRefName> for String {
    fn from(value: GitRefName) -> Self {
        value.0
    }
}
impl fmt::Display for GitRefName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct GitOid(String);
impl GitOid {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for GitOid {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        if !matches!(value.len(), 40 | 64)
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "Git object identity requires exact lowercase SHA-1 or SHA-256 hex",
            ));
        }
        Ok(Self(value.into()))
    }
}
impl TryFrom<String> for GitOid {
    type Error = PmError;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl From<GitOid> for String {
    fn from(value: GitOid) -> Self {
        value.0
    }
}
impl fmt::Display for GitOid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedSources {
    pub remote: String,
    pub accepted_ref: GitRefName,
    pub coordination_ref: GitRefName,
    pub proposal_namespace: GitRefName,
}
impl SharedSources {
    pub fn validate(&self) -> Result<()> {
        if self.remote.is_empty()
            || self.remote.len() > 96
            || self.remote.starts_with('-')
            || !self
                .remote
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(invalid(
                "shared sources require a portable configured remote name",
            ));
        }
        let refs = [
            &self.accepted_ref,
            &self.coordination_ref,
            &self.proposal_namespace,
        ];
        if refs.iter().any(|r| !r.as_str().starts_with("refs/heads/")) {
            return Err(invalid(
                "shared planning and coordination use full refs/heads references",
            ));
        }
        for (index, left) in refs.iter().enumerate() {
            for right in &refs[index + 1..] {
                if left == right
                    || left.as_str().starts_with(&format!("{right}/"))
                    || right.as_str().starts_with(&format!("{left}/"))
                {
                    return Err(invalid(
                        "accepted, coordination and proposal references must be distinct nonoverlapping namespaces",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClaimPolicy {
    pub default_ttl_seconds: u64,
    pub max_ttl_seconds: u64,
    pub max_clock_skew_seconds: u64,
    pub max_publish_attempts: u8,
}
impl Default for ClaimPolicy {
    fn default() -> Self {
        Self {
            default_ttl_seconds: 1800,
            max_ttl_seconds: 86400,
            max_clock_skew_seconds: 30,
            max_publish_attempts: 3,
        }
    }
}
impl ClaimPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.default_ttl_seconds == 0
            || self.default_ttl_seconds > self.max_ttl_seconds
            || self.max_ttl_seconds > 7 * 86400
            || self.max_clock_skew_seconds > 3600
            || self.default_ttl_seconds <= self.max_clock_skew_seconds.saturating_mul(2)
            || !(1..=8).contains(&self.max_publish_attempts)
        {
            return Err(invalid(
                "claim policy requires bounded leases, a usable clock-skew interval and 1–8 publication attempts",
            ));
        }
        Ok(())
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IndexSelection {
    Default,
    EffectiveHook,
    Explicit { path: PathBuf },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceSelector {
    WorkingTree,
    Accepted,
    Proposal { reference: GitRefName },
    Coordination,
    Staged { index: IndexSelection },
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRole {
    Local,
    Accepted,
    Proposal,
    Coordination,
    Staged,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFreshness {
    CurrentAtObservation,
    Cached,
    Unknown,
    Diverged,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningSourceIdentity {
    pub repository: RepositoryId,
    pub role: SourceRole,
    pub ref_name: Option<GitRefName>,
    pub commit: Option<GitOid>,
    pub tree: Option<GitOid>,
    pub index_content: Option<ContentHash>,
    pub content: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteRefObservation {
    pub remote: String,
    pub reference: GitRefName,
    pub commit: Option<GitOid>,
    pub observed_at: Timestamp,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceObservation {
    pub identity: PlanningSourceIdentity,
    pub observed_at: Timestamp,
    pub remote_observation: Option<RemoteRefObservation>,
    pub freshness: SourceFreshness,
    pub reason_codes: Vec<String>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceCaptureLimits {
    pub max_entries: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
    pub max_index_bytes: usize,
    pub timeout_seconds: u64,
}
impl Default for SourceCaptureLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_file_bytes: 64 * 1024 * 1024,
            max_total_bytes: 256 * 1024 * 1024,
            max_index_bytes: 64 * 1024 * 1024,
            timeout_seconds: 30,
        }
    }
}
impl SourceCaptureLimits {
    pub fn validate(&self) -> Result<()> {
        if self.max_entries == 0
            || self.max_entries > 1_000_000
            || self.max_file_bytes == 0
            || self.max_file_bytes > 64 * 1024 * 1024
            || self.max_total_bytes == 0
            || self.max_total_bytes > 1024 * 1024 * 1024
            || self.max_index_bytes == 0
            || self.max_index_bytes > 128 * 1024 * 1024
            || !(1..=300).contains(&self.timeout_seconds)
        {
            return Err(invalid("source capture limits exceed supported bounds"));
        }
        Ok(())
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceEntry {
    pub path: PathBuf,
    pub mode: String,
    pub oid: Option<GitOid>,
    pub stage: u8,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceStatus {
    pub repository: RepositoryId,
    pub config: ContentHash,
    pub binding: Option<ContentHash>,
    pub shared: Option<SharedSources>,
    pub working: SourceObservation,
    pub accepted: Option<SourceObservation>,
    pub coordination: Option<SourceObservation>,
    pub errors: Vec<PmError>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFetchRequest {
    pub expected_config: ContentHash,
    pub expected_binding: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFetchOutcome {
    pub repository: RepositoryId,
    pub request_id: crate::RequestId,
    pub operation_id: crate::OperationId,
    pub config: ContentHash,
    pub observations: Vec<RemoteRefObservation>,
    pub materialized: Vec<PathBuf>,
    pub replayed: bool,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinationMarker {
    pub schema: crate::SchemaVersion,
    pub repository: RepositoryId,
    pub coordination_ref: GitRefName,
}
