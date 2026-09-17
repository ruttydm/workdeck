//! Shared project-management contracts and application operations.
//!
//! Planning files are authoritative. CLI and terminal adapters consume this
//! crate; this crate does not depend on a renderer, runtime, or another product.

mod attachments;
pub mod projection;
pub mod registry;
pub use projection::{
    PROJECTION_SCHEMA_VERSION, ProjectionFaultPoint, ProjectionLimits, ProjectionReadView,
    ProjectionRefresh, ProjectionRefreshRequest, ProjectionState, ProjectionStatus,
    ProjectionStore, ProjectionViewId,
};
pub(crate) mod claims;
pub use claims::{
    AcquireClaim, ClaimAssessment, ClaimCatalogLimits, ClaimChange, ClaimDisposition,
    ClaimGuarantee, ClaimMetadata, ClaimMutation, ClaimOperationOutcome, ClaimPrecondition,
    ClaimRecord, ClaimRecovery, ClaimRequest, ClaimState, ClaimStatus, ClaimWorkContract,
    ClaimedCompletionConfirmation, ClaimedCompletionOutcome, ClaimedCompletionProof,
    ClaimedCompletionVerification, CompleteClaimedIssue, CompleteClaimedVerifiedIssue,
    MAX_CLAIM_BYTES,
};
mod hooks;
pub use hooks::{
    HookApply, HookFaultPoint, HookMode, HookPlan, HookReceipt, HookStatus, apply_hook,
    apply_hook_with_faults, hook_preview, hook_status, recover_hook,
};
mod red_green;
pub use red_green::{
    RedGreenAssessment, RedGreenBaselineReview, RedGreenBasis, RedGreenRequest,
    RedGreenRequirement, ReviewedRedGreenAssessment, verify_red_green, verify_reviewed_red_green,
};
mod ci_execution;
pub use ci_execution::{CiCheckRunRequest, CiPreparedCheck, prepare_ci_check};
mod ci_check_inputs;
pub use ci_check_inputs::{CiCheckInputBinding, CiInvocationInputManifest, bind_ci_check_plan};
mod ci_subjects;
pub use ci_subjects::{CiSubjectContract, CiSubjectIdentity, CiSubjectRequirements};
mod ci_evaluators;
pub use ci_evaluators::{
    CiEvaluatorChange, CiEvaluatorEntry, CiEvaluatorManifest, EvaluatorInputSelection,
};
pub(crate) mod attestations;
pub use attestations::{
    ImportCheckReportRequest, ImportedCheckReport, ImportedCheckReportRecord,
    ImportedCheckReportSummary, ImportedReportAdmission, MAX_IMPORTED_REPORT_BYTES,
    RetainedRedGreenAssessment, RetainedRedGreenAuthority, RetainedRedGreenProof,
    VerifyImportedCheck,
};
mod producers;
pub use producers::{
    AuthenticatedCheckReport, CHECK_REPORT_PAYLOAD_TYPE, MAX_PRODUCER_POLICY_BYTES,
    MAX_SIGNED_REPORT_BYTES, ProducerAuthenticationBasis, ProducerTrustPolicy, ReportSignature,
    SignedCheckReport, TrustedProducer, authenticate_check_report,
};
mod ci_organization;
pub use ci_organization::CiOrganizationContract;
mod ci_contracts;
pub use ci_contracts::{
    CiContractChange, CiContractComparison, CiContractError, CiContractSide, CiEvaluationContract,
};
mod contract_reviews;
mod review_coverage;
pub use review_coverage::{
    ReviewCoverage, ReviewCoverageAuthority, ReviewCoverageRequest, ReviewCoverageRow,
    ReviewCoverageState, ReviewWorkingTree,
};
pub(crate) mod retained_reviews;
pub use contract_reviews::{
    AuthenticatedContractReview, CONTRACT_REVIEW_PAYLOAD_TYPE, CiContractApproval,
    CiReviewedValidation, ContractReviewBasis, ContractReviewDecision, ContractReviewPolicy,
    ContractReviewer, MAX_CONTRACT_REVIEW_BYTES, MAX_CONTRACT_REVIEW_POLICY_BYTES,
    SignedContractReview, ci_validate_reviewed,
};
pub use retained_reviews::{
    ImportContractReviewRequest, ImportedContractReview, ImportedContractReviewRecord,
    ImportedContractReviewSummary, MAX_IMPORTED_REVIEW_BYTES,
};
mod ci_validation;
pub use ci_validation::{
    CiBaselinePin, CiRevision, CiSourceIdentity, CiValidateRequest, CiValidationBasis,
    CiValidationFaultPoint, CiValidationReport, ci_validate, ci_validate_pinned,
    ci_validate_with_faults,
};
mod staged_doctor;
pub use staged_doctor::{
    StagedDoctorFaultPoint, StagedDoctorReport, StagedDoctorRequest, doctor_staged,
    doctor_staged_with_faults,
};
pub mod sources;
pub use sources::{
    ClaimPolicy, CoordinationMarker, GitOid, GitRefName, IndexSelection, PlanningSourceIdentity,
    PlanningSourceView, PublicationAttempt, PublicationConfirmation, PublicationFaultPoint,
    PublicationOutcome, PublicationState, RemoteRefObservation, SharedSources, SourceCaptureLimits,
    SourceEntry, SourceFetchOutcome, SourceFetchRequest, SourceFreshness, SourceObservation,
    SourceRole, SourceSelector, SourceSnapshot, SourceStatus,
};
pub(crate) mod reports;
mod saved_check_plans;
pub use reports::{
    JUnitCaseIdentity, JUnitCaseOutcome, JUnitCaseReport, JUnitTestCase, MAX_REPORT_BYTES,
    MAX_REPORT_FAILURES, ReportAssessment, ReportCounts, ReportFailure, ReportState,
    assess_check_report, junit_test_cases,
};
pub(crate) mod checks;
pub(crate) mod commands;
pub use checks::{
    CatalogValidation, CheckDefinition, CheckPlan, CheckPlanFaultPoint, CheckPlanRequest,
    CheckProfileDefinition, CheckProfileRecord, CheckRecord, CommandCatalogSnapshot,
    CommandPlanRequest, ExecutionPlanRequest, MAX_CHECK_PLAN_BYTES, PlanDiagnostic, PlanIssue,
    PlannedArgument, PlannedCheck, PlannedInvocation, ReportExpectation, SarifLevel,
    SelectionReason, VerificationBasis,
};
pub use commands::{
    ArgumentToken, ArgumentValues, ArtifactSpec, CommandDefinition, CommandRecipe, CommandRecord,
    DeclaredEffect, EnvironmentValue, ParameterDefinition, ParameterType, RunBounds,
    ToolRequirement,
};
pub use execution::input_types::{
    EnvironmentPin, InputEntry, InputLimits, InputManifest, InputSelection, ToolLocation, ToolPin,
};
pub(crate) mod execution;
pub use execution::{
    ArtifactAvailability, ArtifactObservation, CheckOutcome, CheckReport, CheckRunRequest,
    InvocationOutcome, LocalResultAssessment, LocalRunId, LogDescriptor, MAX_CHECK_REPORT_BYTES,
    MAX_LOCAL_LOG_BYTES, MAX_RUN_ARTIFACT_BYTES, MAX_RUN_INVOCATIONS, MAX_RUN_OUTPUT_BYTES,
    MAX_RUN_RECORD_BYTES, ProcessObservation, ProcessTermination, ResolvedInvocation, RunControl,
    RunDocument, RunFaultPoint, RunIntent, RunOutcome, RunPublication, RunQuery, RunRecord,
    RunResult, RunResultRecord, RunState, validate_run_bounds,
};
pub(crate) mod context;
pub use context::{
    ActionPreconditions, ContextAnchor, ContextBudget, ContextCheckFailure, ContextCheckRun,
    ContextCitation, ContextContent, ContextEntry, ContextFaultPoint, ContextFreshness,
    ContextPacket, ContextRequest, ContextSection, ContextSectionKind, ContextTarget, IssueOverlap,
    MAX_CONTEXT_BUDGET_BYTES, NextActionKind, NextActionRequest, NextActions, NextIssueCandidate,
    NextIssueCursor, NextIssueRequest, NextIssueSelection, OverlapBasis, SuggestedAction,
};
pub mod catalog;
pub mod documents;
mod error;
pub(crate) mod features;
mod graph;
mod history;
pub use graph::{
    CompletionCondition, ConditionKind, ConditionState, IssueDependencyPath, IssueGraphMutation,
    IssueGraphSnapshot, IssueReadiness, IssueRelations, PrerequisiteWaiver, RelatedIssueLink,
    SourcePin, SubjectRef,
};
#[path = "policy.rs"]
mod completion_policy;
mod identity;
mod issues;
pub mod migration;
mod model;
mod operations;
mod planning;
mod queries;
mod repository;
pub mod restore;
mod retirement;
mod snapshots;
mod staging;
mod templates;
mod time_entries;
pub mod transactions;
mod wiki;
mod workflow;

pub use attachments::{AttachmentInput, AttachmentRecord, MAX_ATTACHMENT_BYTES};
pub use completion_policy::{PolicyAcceptance, PolicyAssessment, PolicyBasis};
pub use error::{ErrorCode, PmError, Result};
pub use features::{
    CreateFeature, FeatureAvailability, FeatureCoverage, FeatureCoverageQuery, FeatureDecision,
    FeatureIntent, FeatureMaturity, FeatureMetadata, FeatureMutation, FeatureOutcome,
    FeatureRecord,
};
pub use history::{
    NewRecordedSession, RecordedFile, RecordedSession, SessionCollection, SessionMutation,
    SessionRecord, SessionRetirement, validate_recorded_sessions_snapshot,
};
pub use identity::{
    AttestationId, ClaimToken, ContentHash, ContractReviewId, EvidenceId, FeatureId, GateId,
    HandoffId, IssueId, OperationId, QualifiedRef, QuestionId, RecordId, RepositoryId, RequestId,
    Revision, SchemaVersion, SourceToken, Timestamp,
};
pub use issues::{
    CommentRecord, CompletionReport, CreateIssue, IssueCollection, IssueMutation, IssueRecord,
    ManualAcceptanceInput, TemplateIssueInput, UpdateIssue,
};
pub use model::{
    AcceptanceCriterion, AcceptancePolicy, Config, ImportedCompletion, IssueMetadata,
    ManualAcceptance, MetadataAuthority, Priority, SourceLink,
};
pub use planning::{
    CreatePlanning, CycleCarryoverExcluded, CycleCarryoverIssue, CycleCarryoverPlan,
    CycleCarryoverRequest, ImportedPlanningProvenance, LabelsMetadata, MAX_CARRYOVER_ISSUES,
    PlanningCriterion, PlanningImportFormat, PlanningKind, PlanningMembership,
    PlanningMembershipQuery, PlanningMetadata, PlanningMutation, PlanningRecord, SavePlanning,
};
pub use queries::{
    ArchiveFilter, IssueQuery, IssueQuerySnapshot, IssueSort, IssueSortField, SortDirection,
    TargetMatch,
};
pub use repository::{DoctorReport, Repository};
pub use retirement::{
    PlanningReferenceBlocker, RecordReferenceBlocker, ReferenceRetirementChange,
    ReferenceRetirementOutcome, ReferenceRetirementPlan, RetainedFile, RetiredRecord,
    RetirementBlocker, RetirementInput, RetirementKind, RetirementOutcome, RetirementPreview,
    RetirementTarget, Tombstone,
};
pub use snapshots::{
    ImportSource, LegacyExport, LegacyExportFormat, LegacyImportContext, LegacyImportPlan,
    MAX_SNAPSHOT_CONTENT_BYTES, MAX_SNAPSHOT_FILES, MAX_SNAPSHOT_INPUT_BYTES, NativeSnapshot,
    SnapshotFile, SnapshotImportMode, SnapshotImportPlan, SnapshotKind, decode_snapshot,
    decode_transfer,
};
pub use staging::{StagingFaultPoint, StagingReport};
pub use templates::IssueTemplate;
pub use time_entries::{
    MAX_TIME_ENTRY_BYTES, TimeEntry, TimeEntryAmendment, TimeEntryInput, TimeEntryRecord,
    TimeReport, TimeReportQuery,
};
pub use workflow::{Workflow, WorkflowCategory, WorkflowState};

pub use restore::{
    SnapshotRestorePlan, SnapshotRestoreReceipt, preview_snapshot_restore, restore_snapshot,
    resume_snapshot_restore, validate_restore_receipt,
};

pub use wiki::{WikiDocument, WriteWiki};

mod organization;
pub use organization::*;

mod saved_views;
pub use saved_views::{SavedViewDefinition, SavedViewRecord, SavedViewResult, WriteSavedView};

pub(crate) mod gates;
pub use gates::{
    CreateGate, CriterionDeclaration, CriterionOwner, CriterionRef, GateAssessment,
    GateAssessmentRequest, GateDefinition, GateEvidenceSelection, GateMutation, GateMutationResult,
    GateRecord, GateRequest, GateRequirement, GateRequirementAssessment, GateVerificationBasis,
    GreenGateRequirement, MAX_GATE_BYTES, RedGreenGateAssessment, RedGreenGateRequest,
    ResolvedCriterion, VerifiedGateAssessment, VerifiedGateRequest, VerifiedGateRequirement,
    criterion_definition_hash,
};
pub(crate) mod evidence;
pub use evidence::{
    CheckRef, DeclareEvidence, DeclaredProvenance, EvidenceLink, EvidenceProvenanceKind,
    EvidenceQuery, EvidenceRecord, EvidenceReference, EvidenceSupersession,
    EvidenceVerificationBasis, ExactSubject, ExactSubjectKind, MAX_EVIDENCE_BYTES, ProducerRef,
    RedGreenEvidenceAssessment, RedGreenEvidenceRequest, ResultRef, VerifiedEvidenceAssessment,
    VerifiedEvidenceRequest,
};

pub use features::{FeatureRelatedInput, FeatureRelatedOutcome, RelatedFeatureLink};

pub(crate) mod questions;
pub use questions::{
    CreateQuestion, MAX_QUESTION_BYTES, QuestionActionState, QuestionApplicability,
    QuestionFreshness, QuestionMetadata, QuestionMutation, QuestionMutationResult, QuestionQuery,
    QuestionReason, QuestionRecord, QuestionReferenceBlocker, QuestionRequest, QuestionState,
    QuestionSubject, QuestionSupersession, RecordedAnswer,
};

pub(crate) mod handoffs;
pub use handoffs::{
    CreateHandoff, HandoffEvidenceRef, HandoffMetadata, HandoffOperationRef, HandoffRecord,
    MAX_HANDOFF_BYTES,
};

pub(crate) mod completion;
pub use completion::{
    CompleteRedGreenIssue, CompleteVerifiedIssue, CompletionAuthority, CompletionCheckSelection,
    CompletionGateAssessment, CompletionGateSelection, RedGreenIssueCompletion,
    VerifiedIssueCompletion,
};
