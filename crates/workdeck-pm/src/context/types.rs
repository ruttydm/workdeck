use crate::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub const MAX_CONTEXT_BUDGET_BYTES: usize = 1024 * 1024;
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextFaultPoint {
    BeforeSourceValidation,
}

/// Requirements and action inputs, independent of packet rendering and handoff membership.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextAnchor {
    pub schema_version: SchemaVersion,
    pub repository: RepositoryId,
    pub issue: IssueId,
    pub issue_source: SourceToken,
    pub requirements: ContentHash,
    /// Only essential issue/config pins; expanded citations are budgeted content.
    pub source_pins: Vec<SourcePin>,
    pub fingerprint: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRequest {
    pub issue: String,
    /// Bounds compact ContextPacket JSON bytes, excluding an adapter envelope.
    pub budget_bytes: usize,
    #[serde(default)]
    pub as_of: Option<Timestamp>,
    #[serde(default)]
    pub expected_context: Option<ContentHash>,
}
impl ContextRequest {
    pub fn new(issue: impl Into<String>, budget_bytes: usize) -> Self {
        Self {
            issue: issue.into(),
            budget_bytes,
            as_of: None,
            expected_context: None,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextFreshness {
    Current,
    Stale,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextSectionKind {
    Requirements,
    Actions,
    Blockers,
    Questions,
    Handoffs,
    Summary,
    Documents,
    Features,
    Verification,
    Sources,
    Instructions,
    Evidence,
    Checks,
    Reviews,
    Overlaps,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextTarget {
    Issue { id: IssueId },
    Feature { id: FeatureId },
    Question { id: String },
    Handoff { issue: IssueId, id: String },
    Evidence { id: EvidenceId },
    PlanningSource { path: PathBuf },
    WorktreeSource { link: SourceLink },
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCitation {
    pub target: ContextTarget,
    pub source: Option<ContentHash>,
}

/// A compact inspected local result, independent of completion or CI authority.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCheckRun {
    pub id: LocalRunId,
    pub request_id: RequestId,
    pub recorded_at: Timestamp,
    pub state: RunState,
    pub historical_state: RunState,
    pub basis: VerificationBasis,
    pub plan: ContentHash,
    pub intent: SourcePin,
    pub result: Option<SourcePin>,
    pub reason_codes: Vec<String>,
    pub artifacts: Vec<ArtifactObservation>,
    pub omitted_artifacts: usize,
    pub failures: Vec<ContextCheckFailure>,
    pub omitted_failures: usize,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCheckFailure {
    pub check: String,
    pub diagnostic: ReportFailure,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextContent {
    ContractReview {
        summary: Box<ReviewCoverageRow>,
        selected_revision: Option<GitOid>,
    },
    CheckRun {
        summary: ContextCheckRun,
    },
    Requirement {
        id: String,
        description: String,
        checked_declaration: bool,
    },
    Summary {
        title: String,
        status: String,
        body: String,
        manual_acceptance: Option<ManualAcceptance>,
        imported_completion: Option<ImportedCompletion>,
    },
    /// An inert authored reference. Citations pin its declaration, not document contents.
    Document {
        reference: String,
        /// `document_not_fetched`: neither local files nor remote resources were opened.
        reason_code: String,
    },
    Condition {
        condition: CompletionCondition,
    },
    Question {
        record: QuestionRecord,
        applicability: QuestionApplicability,
    },
    Handoff {
        record: HandoffRecord,
        freshness: ContextFreshness,
        reason_codes: Vec<String>,
    },
    Action {
        action: SuggestedAction,
    },
    Feature {
        record: FeatureRecord,
    },
    Verification {
        policy: AcceptancePolicy,
        execution_available: bool,
        reason_code: String,
    },
    Source {
        link: SourceLink,
        excerpt: Option<String>,
        reason_code: Option<String>,
    },
    Instruction {
        path: PathBuf,
        text: String,
    },
    Evidence {
        record: EvidenceRecord,
        freshness: ContextFreshness,
        reason_codes: Vec<String>,
    },
    Overlap {
        overlap: IssueOverlap,
    },
    Notice {
        reason_code: String,
        message: String,
    },
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEntry {
    pub content: ContextContent,
    pub citations: Vec<ContextCitation>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSection {
    pub kind: ContextSectionKind,
    pub total: usize,
    pub omitted: usize,
    pub entries: Vec<ContextEntry>,
    /// False means `total` is a lower bound because bounded inspection stopped.
    pub coverage_complete: bool,
    pub omission_reasons: Vec<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBudget {
    pub scope: String,
    pub limit_bytes: usize,
    pub used_bytes: usize,
    pub minimum_bytes: usize,
    pub omitted_entries: usize,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPacket {
    pub schema_version: SchemaVersion,
    pub anchor: ContextAnchor,
    pub graph: ContentHash,
    pub as_of: Option<Timestamp>,
    pub authority: String,
    pub budget: ContextBudget,
    pub sections: Vec<ContextSection>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionPreconditions {
    pub repository: RepositoryId,
    pub issue: IssueId,
    pub source: SourceToken,
    pub graph: ContentHash,
    pub requirements: ContentHash,
    pub context: ContentHash,
    pub target_source: SourceToken,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextActionKind {
    ClarifyRequirements,
    ResolveQuestion,
    ResolvePrerequisite,
    Implement,
    ContinueImplementation,
    RequestReview,
    RecordHandoff,
    RunChecks,
    InspectCompletion,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestedAction {
    pub kind: NextActionKind,
    pub available: bool,
    pub reason_code: String,
    pub explanation: String,
    pub target: ContextTarget,
    pub preconditions: ActionPreconditions,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextActionRequest {
    pub issue: String,
    #[serde(default)]
    pub expected_context: Option<ContentHash>,
}
impl NextActionRequest {
    pub fn new(issue: impl Into<String>) -> Self {
        Self {
            issue: issue.into(),
            expected_context: None,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextActions {
    pub anchor: ContextAnchor,
    pub graph: ContentHash,
    pub actions: Vec<SuggestedAction>,
    pub conditions: Vec<CompletionCondition>,
    pub omitted_conditions: usize,
    pub omitted_actions: usize,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextIssueRequest {
    /// Filters use shared issue predicates. Ranking is fixed; sort must be empty.
    pub query: IssueQuery,
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<NextIssueCursor>,
}
impl Default for NextIssueRequest {
    fn default() -> Self {
        Self {
            query: IssueQuery {
                sort: Vec::new(),
                ..IssueQuery::default()
            },
            limit: 20,
            cursor: None,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextIssueCursor {
    pub fingerprint: ContentHash,
    pub offset: usize,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextIssueCandidate {
    pub issue: IssueId,
    pub title: String,
    pub title_truncated: bool,
    pub source: SourceToken,
    pub eligible: bool,
    pub reason_codes: Vec<String>,
    pub conditions: Vec<CompletionCondition>,
    pub omitted_conditions: usize,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextIssueSelection {
    pub repository: RepositoryId,
    pub fingerprint: ContentHash,
    pub selected: Option<NextIssueCandidate>,
    pub candidates: Vec<NextIssueCandidate>,
    pub total: usize,
    pub eligible: usize,
    pub excluded: usize,
    pub next_cursor: Option<NextIssueCursor>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OverlapBasis {
    Path {
        own: SourceLink,
        other: SourceLink,
    },
    SharedPrerequisite {
        issue: IssueId,
    },
    Dependency {
        dependent: IssueId,
        prerequisite: IssueId,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueOverlap {
    pub issue: IssueId,
    pub source: SourceToken,
    pub basis: OverlapBasis,
    pub advisory: bool,
}
