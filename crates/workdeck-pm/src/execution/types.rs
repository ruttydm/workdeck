use crate::{ContentHash, RepositoryId, RequestId, Result, SchemaVersion, Timestamp};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

pub const MAX_RUN_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_LOCAL_LOG_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_RUN_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_RUN_INVOCATIONS: usize = 256;
pub const MAX_RUN_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct LocalRunId(String);
impl LocalRunId {
    pub fn new() -> Self {
        Self(format!("RUN-{}", ulid::Ulid::new()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Default for LocalRunId {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for LocalRunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::str::FromStr for LocalRunId {
    type Err = crate::PmError;
    fn from_str(value: &str) -> Result<Self> {
        let valid = value
            .strip_prefix("RUN-")
            .filter(|s| s.len() == 26)
            .and_then(|s| s.parse::<ulid::Ulid>().ok())
            .is_some_and(|id| format!("RUN-{id}") == value);
        if !valid {
            return Err(crate::PmError::new(
                crate::ErrorCode::InvalidInput,
                "run identity must be RUN followed by a canonical ULID",
            ));
        }
        Ok(Self(value.into()))
    }
}
impl TryFrom<String> for LocalRunId {
    type Error = crate::PmError;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl From<LocalRunId> for String {
    fn from(value: LocalRunId) -> Self {
        value.0
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    Passed,
    Failed,
    Skipped,
    Blocked,
    NotRun,
    Canceled,
    Stale,
    Unknown,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessTermination {
    Exited,
    SpawnFailed,
    TimedOut,
    Canceled,
    NotRun,
    CleanupFailed,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogDescriptor {
    pub path: PathBuf,
    pub content: ContentHash,
    pub retained_bytes: u64,
    pub observed_bytes: u64,
    pub truncated: bool,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessObservation {
    pub started_at: Option<Timestamp>,
    pub finished_at: Timestamp,
    pub elapsed_millis: u64,
    pub termination: ProcessTermination,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub cleanup_complete: bool,
    pub stdout: LogDescriptor,
    pub stderr: LogDescriptor,
    pub reason_codes: Vec<String>,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactAvailability {
    Present,
    Missing,
    Changed,
    Unsafe,
    TooLarge,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactObservation {
    pub id: String,
    pub path: PathBuf,
    pub availability: ArtifactAvailability,
    pub content: Option<ContentHash>,
    pub bytes: u64,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedInvocation {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub fingerprint: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRunRequest {
    pub plan: crate::CheckPlan,
    pub expected_plan: ContentHash,
    pub actor: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunIntent {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: LocalRunId,
    pub request_id: RequestId,
    pub recorded_at: Timestamp,
    pub worktree: PathBuf,
    pub input: CheckRunRequest,
    /// Exact committed inputs at admission; remains local feedback without producer trust.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<crate::CiCheckInputBinding>,
    pub invocations: Vec<ResolvedInvocation>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub intent: RunIntent,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationOutcome {
    pub index: usize,
    pub invocation: ContentHash,
    pub process: ProcessObservation,
    pub artifacts: Vec<ArtifactObservation>,
    pub inputs_unchanged: bool,
    pub state: RunState,
    pub reason_codes: Vec<String>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckOutcome {
    pub check: crate::CheckRef,
    pub invocation: usize,
    pub report: crate::ReportAssessment,
    pub state: RunState,
    pub reason_codes: Vec<String>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunResult {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: LocalRunId,
    pub request_id: RequestId,
    pub intent_content: ContentHash,
    pub started_at: Timestamp,
    pub finished_at: Timestamp,
    pub invocations: Vec<InvocationOutcome>,
    pub checks: Vec<CheckOutcome>,
    pub state: RunState,
    pub basis: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunResultRecord {
    pub result: RunResult,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalResultAssessment {
    pub run: LocalRunId,
    pub state: RunState,
    pub historical_state: RunState,
    pub basis: String,
    pub reason_codes: Vec<String>,
    pub artifacts: Vec<ArtifactObservation>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunOutcome {
    pub run: RunRecord,
    pub state: RunState,
    pub results: Option<RunResultRecord>,
    pub assessment: LocalResultAssessment,
    pub receipts: Vec<crate::transactions::MutationReceipt>,
    pub replayed: bool,
}
#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunQuery {
    pub issue: Option<crate::IssueId>,
    pub state: Option<RunState>,
    pub limit: Option<usize>,
}

/// The caller owns signal registration and foreground lifecycle. No global signal
/// handler, agent process, daemon, or automatic retry is created by this control.
#[derive(Clone, Debug, Default)]
pub struct RunControl {
    pub(super) cancellation: Arc<AtomicU8>,
    pub(super) cleanup: Arc<AtomicBool>,
    pub(super) cleanup_failed: Arc<AtomicBool>,
}
impl RunControl {
    pub fn cancel(&self) {
        self.cancellation.fetch_max(1, Ordering::SeqCst);
    }
    pub fn force_cancel(&self) {
        self.cancellation.store(2, Ordering::SeqCst);
    }
    pub fn cleanup_complete(&self) -> bool {
        self.cleanup.load(Ordering::SeqCst)
    }
    pub fn cancellation_requested(&self) -> bool {
        self.cancellation.load(Ordering::SeqCst) > 0
    }
}
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunFaultPoint {
    AfterIntent,
    BeforeSpawn,
    AfterSpawn,
    AfterProcess,
    BeforeResultJournal,
    AfterResultJournal,
    AfterResultFile,
    AfterResultPublication,
}
