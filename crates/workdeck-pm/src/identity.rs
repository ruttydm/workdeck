use crate::{ErrorCode, PmError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, str::FromStr};
use ulid::Ulid;

fn invalid(kind: &str, value: &str) -> PmError {
    PmError::new(
        ErrorCode::InvalidInput,
        format!("invalid {kind}: {value:?}"),
    )
}

pub(crate) fn valid_prefix(value: &str) -> bool {
    !value.is_empty() && value.len() <= 12 && value.bytes().all(|c| c.is_ascii_uppercase())
}

pub(crate) fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
        && value.as_bytes()[0].is_ascii_alphanumeric()
}

fn valid_ulid(value: &str) -> bool {
    value.len() == 26 && Ulid::from_string(value).is_ok_and(|id| id.to_string() == value)
}

macro_rules! string_value {
    ($name:ident) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = PmError;
            fn try_from(value: String) -> Result<Self> {
                Self::from_str(&value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IssueId(String);

/// A prefix is not a substitute for the explicit document kind.
pub type RecordId = IssueId;

impl IssueId {
    pub fn new(prefix: &str) -> Result<Self> {
        if !valid_prefix(prefix) {
            return Err(invalid("ID prefix", prefix));
        }
        Ok(Self(format!("{prefix}-{}", Ulid::new())))
    }
}

impl FromStr for IssueId {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        let Some((prefix, suffix)) = value.split_once('-') else {
            return Err(invalid("record ID", value));
        };
        let legacy = prefix == "WD"
            && !suffix.starts_with('0')
            && suffix.parse::<u64>().is_ok_and(|n| n > 0)
            && suffix.bytes().all(|b| b.is_ascii_digit());
        if !valid_prefix(prefix) || !(valid_ulid(suffix) || legacy) {
            return Err(invalid("record ID", value));
        }
        Ok(Self(value.into()))
    }
}
string_value!(IssueId);

// Native domain IDs are distinct types even when their serialized spelling is
// passed through a generic command argument. Paths never allocate identity.
macro_rules! native_domain_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn new() -> Self {
                Self(format!("{}-{}", $prefix, Ulid::new()))
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl FromStr for $name {
            type Err = PmError;
            fn from_str(value: &str) -> Result<Self> {
                if !value
                    .strip_prefix(concat!($prefix, "-"))
                    .is_some_and(valid_ulid)
                {
                    return Err(invalid(concat!($prefix, " record ID"), value));
                }
                Ok(Self(value.into()))
            }
        }
        string_value!($name);
    };
}
native_domain_id!(FeatureId, "FEAT");
native_domain_id!(GateId, "GATE");
native_domain_id!(EvidenceId, "EVD");
native_domain_id!(AttestationId, "ATST");
native_domain_id!(ContractReviewId, "CRVW");
native_domain_id!(QuestionId, "Q");
native_domain_id!(HandoffId, "H");
native_domain_id!(ClaimToken, "CLM");

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepositoryId(String);

impl RepositoryId {
    pub fn new() -> Self {
        Self(format!("repo-{}", Ulid::new()))
    }
}
impl Default for RepositoryId {
    fn default() -> Self {
        Self::new()
    }
}
impl FromStr for RepositoryId {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        if !value.strip_prefix("repo-").is_some_and(valid_ulid) {
            return Err(invalid("repository ID", value));
        }
        Ok(Self(value.into()))
    }
}
string_value!(RepositoryId);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct QualifiedRef {
    pub repository: RepositoryId,
    pub record: RecordId,
}

impl QualifiedRef {
    pub fn new(repository: RepositoryId, record: RecordId) -> Self {
        Self { repository, record }
    }
}
impl fmt::Display for QualifiedRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.repository, self.record)
    }
}
impl FromStr for QualifiedRef {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        let Some((repository, record)) = value.split_once("::") else {
            return Err(invalid("qualified reference", value));
        };
        Ok(Self::new(repository.parse()?, record.parse()?))
    }
}
impl TryFrom<String> for QualifiedRef {
    type Error = PmError;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl From<QualifiedRef> for String {
    fn from(value: QualifiedRef) -> String {
        value.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Revision(u64);
impl Revision {
    pub const INITIAL: Self = Self(1);
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            Err(invalid("positive revision", "0"))
        } else {
            Ok(Self(value))
        }
    }
    pub const fn get(self) -> u64 {
        self.0
    }
    pub fn next(self) -> Result<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| PmError::new(ErrorCode::Conflict, "revision exhausted"))
    }
}
impl TryFrom<u64> for Revision {
    type Error = PmError;
    fn try_from(value: u64) -> Result<Self> {
        Self::new(value)
    }
}
impl From<Revision> for u64 {
    fn from(value: Revision) -> u64 {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct SchemaVersion(u64);
impl SchemaVersion {
    pub const CURRENT: Self = Self(1);
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl TryFrom<u64> for SchemaVersion {
    type Error = PmError;
    fn try_from(value: u64) -> Result<Self> {
        if value == 1 {
            Ok(Self::CURRENT)
        } else {
            Err(PmError::new(
                ErrorCode::UnsupportedSchema,
                format!("unsupported PM schema {value}; this build supports schema 1"),
            ))
        }
    }
}
impl From<SchemaVersion> for u64 {
    fn from(value: SchemaVersion) -> u64 {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContentHash(String);
impl ContentHash {
    pub fn of(bytes: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(bytes)))
    }
}
impl FromStr for ContentHash {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(invalid("SHA-256 content identity", value));
        }
        Ok(Self(value.into()))
    }
}
string_value!(ContentHash);

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[schemars(
    description = "Expected revision and exact SHA-256 bytes; both are required for a stale-source precondition."
)]
#[serde(deny_unknown_fields)]
pub struct SourceToken {
    pub revision: Revision,
    pub content: ContentHash,
}
impl SourceToken {
    pub fn new(revision: Revision, bytes: &[u8]) -> Self {
        Self {
            revision,
            content: ContentHash::of(bytes),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OperationId(String);
impl OperationId {
    pub fn new() -> Self {
        Self(format!("OP-{}", Ulid::new()))
    }
}
impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}
impl FromStr for OperationId {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        if !value.strip_prefix("OP-").is_some_and(valid_ulid) {
            return Err(invalid("operation ID", value));
        }
        Ok(Self(value.into()))
    }
}
string_value!(OperationId);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RequestId(String);
impl RequestId {
    pub fn new() -> Self {
        Self(Ulid::new().to_string())
    }
}
impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}
impl FromStr for RequestId {
    type Err = PmError;
    fn from_str(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > 96
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(invalid("request ID", value));
        }
        Ok(Self(value.into()))
    }
}
string_value!(RequestId);

/// UTC instants use RFC 3339. Naive local timestamps are rejected.
pub type Timestamp = DateTime<Utc>;
