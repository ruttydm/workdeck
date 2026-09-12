use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SarifLevel {
    Error,
    Warning,
    Note,
    None,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportExpectation {
    Process {
        allowed_exit_codes: Vec<i32>,
    },
    #[serde(rename = "junit")]
    JUnit {
        artifact: String,
        suites: Vec<String>,
        minimum_tests: u64,
        maximum_skipped: Option<u64>,
        allowed_exit_codes: Vec<i32>,
    },
    Sarif {
        artifact: String,
        tool: String,
        minimum_invocations: u64,
        failure_levels: Vec<SarifLevel>,
        allowed_exit_codes: Vec<i32>,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckDefinition {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub archived: bool,
    pub command: String,
    #[serde(default)]
    pub arguments: ArgumentValues,
    pub expectation: ReportExpectation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_green: Option<RedGreenRequirement>,
    /// Repository evaluator/test inputs pinned independently in CI. None preserves
    /// local-only definitions; it is not a declaration of an empty CI evaluator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_inputs: Option<EvaluatorInputSelection>,
    /// Advisory impact selectors. Incomplete impact never removes required checks.
    #[serde(default)]
    pub affected: Vec<PathBuf>,
    #[serde(default)]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckProfileDefinition {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub archived: bool,
    pub checks: Vec<String>,
    #[serde(default)]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRecord {
    pub definition: CheckDefinition,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckProfileRecord {
    pub definition: CheckProfileDefinition,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandCatalogSnapshot {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub commands: Vec<CommandRecord>,
    pub checks: Vec<CheckRecord>,
    pub profiles: Vec<CheckProfileRecord>,
    pub fingerprint: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogValidation {
    pub valid: bool,
    pub checked_records: usize,
    pub errors: Vec<PmError>,
}
#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CheckPlanRequest {
    pub issue: Option<String>,
    pub checks: Vec<String>,
    pub profiles: Vec<String>,
    /// Overrides keyed by check ID, not command ID; duplicate recipe uses remain independent.
    pub arguments: BTreeMap<String, ArgumentValues>,
    /// Advisory paths never claim that other inputs are unchanged.
    pub changed_paths: Vec<PathBuf>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandPlanRequest {
    pub command: String,
    #[serde(default)]
    pub arguments: ArgumentValues,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionPlanRequest {
    Command { request: CommandPlanRequest },
    Checks { request: CheckPlanRequest },
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationBasis {
    LocalFeedback,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlannedArgument {
    Literal { value: String },
    ArtifactPath { artifact: String },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedInvocation {
    /// Check ID, or command ID for a direct command plan.
    pub id: String,
    pub command: String,
    pub definition: ContentHash,
    pub tool: String,
    /// Arguments after argv[0]. Explicit shell expansion includes -c, unchanged script, $0.
    pub args: Vec<PlannedArgument>,
    pub cwd: PathBuf,
    pub environment: BTreeMap<String, EnvironmentValue>,
    pub tools: Vec<ToolRequirement>,
    pub input_selection: InputSelection,
    pub inputs: InputManifest,
    pub bounds: RunBounds,
    pub artifacts: Vec<ArtifactSpec>,
    pub effects: Vec<DeclaredEffect>,
    pub fingerprint: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedCheck {
    pub id: String,
    pub definition: ContentHash,
    pub invocation: String,
    pub expectation: ReportExpectation,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionReason {
    pub check: String,
    pub reason_code: String,
    pub source: Option<String>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDiagnostic {
    pub invocation: Option<String>,
    pub reason_code: String,
    pub message: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanIssue {
    pub id: IssueId,
    pub source: SourceToken,
    pub requirements: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckPlan {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub basis: VerificationBasis,
    pub request: ExecutionPlanRequest,
    pub issue: Option<PlanIssue>,
    pub config: ContentHash,
    pub configuration: Config,
    pub config_document: String,
    pub definitions: CommandCatalogSnapshot,
    pub invocations: Vec<PlannedInvocation>,
    pub checks: Vec<PlannedCheck>,
    pub subject: ExactSubject,
    pub selection: Vec<SelectionReason>,
    pub blockers: Vec<PlanDiagnostic>,
    pub fingerprint: ContentHash,
}
