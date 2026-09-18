use crate::{ContentHash, RepositoryId, Revision, SourceSelector, SourceToken};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const MAX_REGISTERED_CHECKOUTS: usize = 256;
pub const MAX_REGISTRY_BYTES: usize = 1024 * 1024;
pub const MAX_REGISTRY_REQUESTS: usize = 1024;

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryFaultPoint {
    BeforePublish,
    AfterPublish,
}

/// Local navigation mapping, never a portable planning authority.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredCheckout {
    pub alias: String,
    pub repository: RepositoryId,
    pub checkout: PathBuf,
    pub checkout_binding: ContentHash,
    pub source: SourceSelector,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySnapshot {
    pub owner: RepositoryId,
    pub source: SourceToken,
    pub entries: Vec<RegisteredCheckout>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegistryMutation {
    Register { checkout: RegisteredCheckout },
    Remove { alias: String },
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryRequest {
    pub expected: SourceToken,
    pub mutation: RegistryMutation,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryOutcome {
    pub request: crate::RequestId,
    pub revision: Revision,
    /// The historical mapping changed by this request, not current availability.
    pub checkout: RegisteredCheckout,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub input: ContentHash,
    pub outcome: RegistryOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Document {
    pub schema: u32,
    pub owner: RepositoryId,
    pub revision: Revision,
    pub entries: BTreeMap<String, RegisteredCheckout>,
    pub requests: BTreeMap<String, Receipt>,
}
