use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A report interpretation, independent of process termination and source freshness.
#[derive(JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportState {
    Passed,
    Failed,
    Skipped,
    Unknown,
}

#[derive(JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportCounts {
    pub discovered: u64,
    pub passed: u64,
    pub failed: u64,
    pub errors: u64,
    pub skipped: u64,
    pub invocations: u64,
    pub findings: u64,
}

#[derive(JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportFailure {
    pub id: String,
    pub message: String,
    pub path: Option<String>,
    pub line: Option<u64>,
}

#[derive(JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportAssessment {
    pub state: ReportState,
    pub reason_codes: Vec<String>,
    pub counts: ReportCounts,
    pub failures: Vec<ReportFailure>,
    pub omitted_failures: usize,
}
