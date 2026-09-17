use crate::{ContentHash, RepositoryId, RequestId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookMode {
    Install,
    Update,
    Remove,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookPlan {
    pub schema_version: u32,
    pub repository: RepositoryId,
    pub worktree: PathBuf,
    pub target: PathBuf,
    /// Machine-local existing parent identities observed when reviewing this plan.
    pub parent_identity: ContentHash,
    pub hook_configuration: ContentHash,
    pub planning_configuration: ContentHash,
    pub mode: HookMode,
    pub before: Option<ContentHash>,
    pub before_mode: Option<u32>,
    pub after: Option<ContentHash>,
    pub after_mode: Option<u32>,
    pub generated_hook: Option<String>,
    pub integration_snippet: String,
    pub allowed: bool,
    pub blockers: Vec<String>,
    pub changed: bool,
    pub fingerprint: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookApply {
    pub mode: HookMode,
    pub expected_plan: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookReceipt {
    pub schema_version: u32,
    pub repository: RepositoryId,
    pub request_id: RequestId,
    pub plan: HookPlan,
    pub scope: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookStatus {
    pub repository: RepositoryId,
    pub worktree: PathBuf,
    pub target: PathBuf,
    pub owned: bool,
    pub content: Option<ContentHash>,
    pub executable: bool,
    pub pending_request: Option<RequestId>,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookFaultPoint {
    BeforeJournal,
    AfterJournal,
    BeforePublish,
    AfterPublish,
    AfterReceipt,
}
