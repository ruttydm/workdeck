//! Generated schemas describe the actual serializable Rust contracts. Repository
//! policy and cross-record validation remain explicit application operations.
use crate::*;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema, schema_for};
use serde_json::{Value, json};
use std::borrow::Cow;

macro_rules! primitive_schema {
    ($type:ty, $definition:tt) => {
        impl JsonSchema for $type {
            fn schema_name() -> Cow<'static, str> {
                stringify!($type).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!($definition)
            }
        }
    };
}
primitive_schema!(SchemaVersion, {"type":"integer", "const":1});
primitive_schema!(Revision, {"type":"integer", "minimum":1, "maximum":18446744073709551615_u64});
primitive_schema!(ContentHash, {"type":"string", "pattern":"^[0-9a-f]{64}$"});
primitive_schema!(RequestId, {"type":"string", "pattern":"^[A-Za-z0-9_-]{1,96}$"});
primitive_schema!(RepositoryId, {"type":"string", "pattern":"^repo-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(OperationId, {"type":"string", "pattern":"^OP-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(IssueId, {"type":"string", "pattern":"^([A-Z]{1,12}-[0-7][0-9A-HJKMNP-TV-Z]{25}|WD-[1-9][0-9]{0,19})$", "description":"Full canonical ULID, or preserved legacy WD-N where N is a positive u64."});
primitive_schema!(FeatureId, {"type":"string", "pattern":"^FEAT-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(GateId, {"type":"string", "pattern":"^GATE-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(QuestionId, {"type":"string", "pattern":"^Q-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(ClaimToken, {"type":"string", "pattern":"^CLM-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(HandoffId, {"type":"string", "pattern":"^H-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(ContractReviewId, {"type":"string", "pattern":"^CRVW-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(AttestationId, {"type":"string", "pattern":"^ATST-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(EvidenceId, {"type":"string", "pattern":"^EVD-[0-7][0-9A-HJKMNP-TV-Z]{25}$"});
primitive_schema!(QualifiedRef, {"type":"string", "pattern":"^repo-[0-7][0-9A-HJKMNP-TV-Z]{25}::([A-Z]{1,12}-[0-7][0-9A-HJKMNP-TV-Z]{25}|WD-[1-9][0-9]{0,19})$"});

/// The same name-to-type mapping drives listing and schema lookup.
macro_rules! schemas {
    ($($name:literal => $type:ty),+ $(,)?) => {
        pub const SCHEMA_NAMES: &[&str] = &[$($name),+];
        pub fn schema(name: &str) -> Result<Value> {
            let value = match name { $($name => schema_for!($type)),+,
                _ => return Err(PmError::new(ErrorCode::NotFound, format!("unknown planning schema {name:?}"))
                    .hint("Run workdeck schema --json to list supported schemas.")),
            };
            let mut value = serde_json::to_value(value).map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?;
            constrain_authoring(&mut value);
            constrain_extensions(&mut value);
            value["x-workdeck-validation"] = json!({"schema_version":1,"semantic_validator":"workdeck doctor","note":"Document syntax, repository workflow, cross-record references, stale-source preconditions, and completion policy also require application validation."});
            Ok(value)
        }
    };
}

fn constrain_authoring(schema: &mut Value) {
    constrain_fields(
        schema,
        &["CreateIssue", "UpdateIssue", "TemplateIssueInput"],
        serde_json::to_value(schema_for!(IssueMetadata)).expect("metadata schema is JSON"),
        crate::issues::ISSUE_WRITABLE_FIELDS,
    );
    constrain_fields(
        schema,
        &["CreatePlanning", "SavePlanning", "PlanningMutation"],
        serde_json::to_value(schema_for!(PlanningMetadata)).expect("metadata schema is JSON"),
        crate::planning::store::PLANNING_WRITABLE_FIELDS,
    );
}

fn constrain_fields(schema: &mut Value, types: &[&str], metadata: Value, writable: &[&str]) {
    let authoring = schema["title"]
        .as_str()
        .is_some_and(|name| types.contains(&name))
        || schema
            .get("$defs")
            .is_some_and(|defs| types.iter().any(|name| defs.get(name).is_some()));
    if !authoring {
        return;
    }
    let properties = metadata["properties"]
        .as_object()
        .expect("metadata has properties");
    let fields = json!({"type":"object", "properties":properties.iter()
        .filter(|(name,_)|writable.contains(&name.as_str()))
        .map(|(name,value)|(name.clone(),value.clone())).collect::<serde_json::Map<_,_>>(),
        "additionalProperties":false,"patternProperties":{"^x-[a-z0-9][a-z0-9_-]{0,95}$":true}});
    fn patch(value: &mut Value, fields: &Value) {
        if value
            .get("properties")
            .is_some_and(|properties| properties.get("fields").is_some())
        {
            value["properties"]["fields"] = fields.clone();
        }
        if let Some(array) = value.as_array_mut() {
            for child in array {
                patch(child, fields);
            }
        } else if let Some(object) = value.as_object_mut() {
            for child in object.values_mut() {
                patch(child, fields);
            }
        }
    }
    if schema["title"]
        .as_str()
        .is_some_and(|name| types.contains(&name))
    {
        patch(schema, &fields);
    }
    if schema.get("$defs").is_none() {
        schema["$defs"] = json!({});
    }
    let definitions = schema["$defs"]
        .as_object_mut()
        .expect("definitions are objects");
    for name in types {
        if let Some(definition) = definitions.get_mut(*name) {
            patch(definition, &fields);
        }
    }
    if let Some(metadata_defs) = metadata["$defs"].as_object() {
        for (name, definition) in metadata_defs {
            definitions
                .entry(name)
                .or_insert_with(|| definition.clone());
        }
    }
}

schemas! {
    "projection-limits" => crate::projection::ProjectionLimits,
    "projection-view" => crate::projection::ProjectionViewId,
    "projection-query" => crate::projection::ProjectionQuery,
    "projection-query-handle" => crate::projection::ProjectionQueryHandle,
    "projection-row-token" => crate::projection::ProjectionRowToken,
    "projection-page" => crate::projection::ProjectionPage,
    "projection-detail" => crate::projection::ProjectionDetail,
    "projection-status" => crate::projection::ProjectionStatus,
    "projection-board-request" => crate::projection::ProjectionBoardRequest,
    "projection-board-column" => crate::projection::ProjectionBoardColumn,
    "command-definition" => CommandDefinition,
    "command-record" => CommandRecord,
    "command-catalog" => CommandCatalogSnapshot,
    "command-plan-request" => CommandPlanRequest,
    "check-definition" => CheckDefinition,
    "check-record" => CheckRecord,
    "check-profile" => CheckProfileDefinition,
    "check-profile-record" => CheckProfileRecord,
    "check-plan-request" => CheckPlanRequest,
    "check-plan" => CheckPlan,
    "input-manifest" => InputManifest,
    "check-run-request" => CheckRunRequest,
    "run-intent" => RunIntent,
    "run-result" => RunResult,
    "run-outcome" => RunOutcome,
    "run-query" => RunQuery,
    "run-record" => RunRecord,
    "run-result-record" => RunResultRecord,
    "run-document" => RunDocument,
    "run-publication" => RunPublication,
    "junit-case-identity" => JUnitCaseIdentity,
    "junit-case-report" => JUnitCaseReport,
    "red-green-requirement" => RedGreenRequirement,
    "red-green-request" => RedGreenRequest,
    "red-green-assessment" => RedGreenAssessment,
    "red-green-evidence-request" => RedGreenEvidenceRequest,
    "red-green-evidence-assessment" => RedGreenEvidenceAssessment,
    "retained-red-green-proof" => RetainedRedGreenProof,
    "verify-imported-check" => VerifyImportedCheck,
    "retained-red-green-authority" => RetainedRedGreenAuthority,
    "retained-red-green-assessment" => RetainedRedGreenAssessment,
    "red-green-baseline-review" => RedGreenBaselineReview,
    "reviewed-red-green-assessment" => ReviewedRedGreenAssessment,
    "check-report" => CheckReport,
    "producer-trust-policy" => ProducerTrustPolicy,
    "signed-check-report" => SignedCheckReport,
    "authenticated-check-report" => AuthenticatedCheckReport,
    "attestation-id" => AttestationId,
    "contract-review-id" => ContractReviewId,
    "review-coverage-request" => ReviewCoverageRequest,
    "review-coverage" => ReviewCoverage,
    "review-coverage-row" => ReviewCoverageRow,
    "import-contract-review-request" => ImportContractReviewRequest,
    "imported-contract-review" => ImportedContractReview,
    "imported-contract-review-record" => ImportedContractReviewRecord,
    "imported-contract-review-summary" => ImportedContractReviewSummary,
    "import-check-report-request" => ImportCheckReportRequest,
    "imported-check-report" => ImportedCheckReport,
    "imported-check-report-record" => ImportedCheckReportRecord,
    "imported-check-report-summary" => ImportedCheckReportSummary,
    "report-assessment" => ReportAssessment,
    "local-result-assessment" => LocalResultAssessment,
    "context-check-run" => ContextCheckRun,
    "context-check-failure" => ContextCheckFailure,

    "feature" => FeatureMetadata,
    "feature-record" => FeatureRecord,
    "feature-create" => CreateFeature,
    "feature-mutation" => FeatureMutation,
    "policy-acceptance" => PolicyAcceptance,
    "policy-assessment" => PolicyAssessment,
    "feature-intent" => FeatureIntent,
    "feature-outcome" => FeatureOutcome,
    "feature-coverage-query" => FeatureCoverageQuery,
    "feature-coverage" => FeatureCoverage,
    "feature-related" => RelatedFeatureLink,
    "feature-related-input" => FeatureRelatedInput,
    "feature-related-outcome" => FeatureRelatedOutcome,
    "context-packet" => ContextPacket,
    "context-anchor" => ContextAnchor,
    "context-request" => ContextRequest,
    "next-action-request" => NextActionRequest,
    "next-actions" => NextActions,
    "next-issue-request" => NextIssueRequest,
    "next-issue-selection" => NextIssueSelection,
    "next-issue-cursor" => NextIssueCursor,
    "question-create" => CreateQuestion,
    "question" => QuestionMetadata,
    "question-record" => QuestionRecord,
    "question-mutation" => QuestionMutation,
    "question-mutation-result" => QuestionMutationResult,
    "question-query" => QuestionQuery,
    "question-applicability" => QuestionApplicability,
    "handoff-create" => CreateHandoff,
    "handoff" => HandoffMetadata,
    "handoff-record" => HandoffRecord,
    "gate" => GateDefinition,
    "gate-record" => GateRecord,
    "gate-create" => CreateGate,
    "gate-mutation" => GateMutation,
    "gate-request" => GateRequest,
    "gate-mutation-result" => GateMutationResult,
    "gate-assessment-request" => GateAssessmentRequest,
    "gate-assessment" => GateAssessment,
    "complete-red-green-issue" => CompleteRedGreenIssue,
    "red-green-issue-completion" => RedGreenIssueCompletion,
    "completion-authority" => CompletionAuthority,
    "complete-verified-issue" => CompleteVerifiedIssue,
    "verified-issue-completion" => VerifiedIssueCompletion,
    "red-green-gate-request" => RedGreenGateRequest,
    "red-green-gate-assessment" => RedGreenGateAssessment,
    "verified-gate-request" => VerifiedGateRequest,
    "verified-gate-assessment" => VerifiedGateAssessment,
    "verified-evidence-request" => VerifiedEvidenceRequest,
    "verified-evidence-assessment" => VerifiedEvidenceAssessment,
    "criterion-ref" => CriterionRef,
    "resolved-criterion" => ResolvedCriterion,
    "exact-subject" => ExactSubject,
    "evidence-declare" => DeclareEvidence,
    "evidence-reference" => EvidenceReference,
    "evidence-record" => EvidenceRecord,
    "evidence-query" => EvidenceQuery,
    "config" => Config,
    "users-registry" => UsersRegistry,
    "users-record" => UsersRecord,
    "user-definition" => UserDefinition,
    "user-mutation" => UserMutation,
    "organization-schema" => OrganizationSchema,
    "organization-schema-record" => OrganizationSchemaRecord,
    "schema-change" => SchemaChange,
    "schema-change-plan" => SchemaChangePlan,
    "organization-compliance" => OrganizationCompliance,
    "custom-patch" => CustomPatch,
    "estimate" => Estimate,
    "estimate-report" => EstimateReport,
    "issue" => IssueMetadata,
    "issue-record" => IssueRecord,
    "issue-query" => IssueQuery,
    "issue-create" => CreateIssue,
    "issue-update" => UpdateIssue,
    "issue-mutation" => IssueMutation,
    "issue-template-input" => TemplateIssueInput,
    "issue-template" => IssueTemplate,
    "saved-view" => SavedViewRecord,
    "saved-view-definition" => SavedViewDefinition,
    "saved-view-write" => WriteSavedView,
    "saved-view-result" => SavedViewResult,
    "wiki-document" => WikiDocument,
    "wiki-write" => WriteWiki,
    "time-entry-input" => TimeEntryInput,
    "time-entry-amendment" => TimeEntryAmendment,
    "time-entry" => TimeEntry,
    "time-entry-record" => TimeEntryRecord,
    "time-report-query" => TimeReportQuery,
    "time-report" => TimeReport,
    "comment" => CommentRecord,
    "attachment" => AttachmentRecord,
    "completion" => CompletionReport,
    "completion-condition" => CompletionCondition,
    "subject-ref" => SubjectRef,
    "issue-graph" => IssueGraphSnapshot,
    "issue-graph-mutation" => IssueGraphMutation,
    "issue-relations" => IssueRelations,
    "issue-readiness" => IssueReadiness,
    "issue-dependency-path" => IssueDependencyPath,
    "issue-related-link" => RelatedIssueLink,
    "prerequisite-waiver" => PrerequisiteWaiver,
    "source-token" => SourceToken,
    "planning-source-identity" => PlanningSourceIdentity,
    "source-observation" => SourceObservation,
    "source-selector" => SourceSelector,
    "source-status" => SourceStatus,
    "source-fetch-request" => SourceFetchRequest,
    "source-fetch-outcome" => SourceFetchOutcome,
    "shared-sources" => SharedSources,
    "coordination-marker" => CoordinationMarker,
    "publication-outcome" => PublicationOutcome,
    "claim-policy" => ClaimPolicy,
    "claim-contract" => ClaimWorkContract,
    "registered-checkout" => registry::RegisteredCheckout,
    "repository-registry" => registry::RegistrySnapshot,
    "repository-registration-request" => registry::RegistryRequest,
    "repository-registration-outcome" => registry::RegistryOutcome,
    "my-work-request" => registry::MyWorkRequest,
    "my-work-report" => registry::MyWorkReport,
    "registered-work-row" => registry::RegisteredWorkRow,
    "claim-precondition" => ClaimPrecondition,
    "claim-request" => ClaimRequest,
    "claim-record" => ClaimRecord,
    "claim-status" => ClaimStatus,
    "claim-change" => ClaimChange,
    "claim-operation-outcome" => ClaimOperationOutcome,
    "claimed-completion-input" => CompleteClaimedIssue,
    "claimed-verified-completion-input" => CompleteClaimedVerifiedIssue,
    "claimed-completion-verification" => ClaimedCompletionVerification,
    "claimed-completion-confirmation" => ClaimedCompletionConfirmation,
    "claimed-completion-proof" => ClaimedCompletionProof,
    "claimed-completion-outcome" => ClaimedCompletionOutcome,
    "hook-plan" => HookPlan,
    "hook-apply" => HookApply,
    "hook-receipt" => HookReceipt,
    "hook-status" => HookStatus,
    "ci-prepared-check" => CiPreparedCheck,
    "ci-check-run-request" => CiCheckRunRequest,
    "ci-check-input-binding" => CiCheckInputBinding,
    "ci-validate-request" => CiValidateRequest,
    "ci-baseline-pin" => CiBaselinePin,
    "contract-review-policy" => ContractReviewPolicy,
    "ci-contract-approval" => CiContractApproval,
    "signed-contract-review" => SignedContractReview,
    "authenticated-contract-review" => AuthenticatedContractReview,
    "ci-reviewed-validation" => CiReviewedValidation,
    "ci-validation-report" => CiValidationReport,
    "ci-evaluation-contract" => CiEvaluationContract,
    "ci-subject-contract" => CiSubjectContract,
    "evaluator-input-selection" => EvaluatorInputSelection,
    "ci-evaluator-manifest" => CiEvaluatorManifest,
    "staged-doctor-request" => StagedDoctorRequest,
    "staged-doctor-report" => StagedDoctorReport,
    "proposal-request" => sources::ProposalRequest,
    "proposal-plan" => sources::ProposalPlan,
    "proposal-outcome" => sources::ProposalOutcome,
    "receipt" => transactions::MutationReceipt,
    "staging" => StagingReport,
    "pending-operation" => transactions::PendingOperation,
    "error" => PmError,
    "planning-record" => PlanningRecord,
    "planning-metadata" => PlanningMetadata,
    "planning-criterion" => PlanningCriterion,
    "planning-membership-query" => PlanningMembershipQuery,
    "planning-membership" => PlanningMembership,
    "cycle-carryover-request" => crate::CycleCarryoverRequest,
    "cycle-carryover-plan" => crate::CycleCarryoverPlan,
    "planning-create" => CreatePlanning,
    "planning-save" => SavePlanning,
    "planning-mutation" => PlanningMutation,
    "labels" => LabelsMetadata,
    "retirement-target" => RetirementTarget,
    "retirement-preview" => RetirementPreview,
    "retirement-input" => RetirementInput,
    "retirement-outcome" => RetirementOutcome,
    "reference-retirement-change" => ReferenceRetirementChange,
    "reference-retirement-plan" => ReferenceRetirementPlan,
    "reference-retirement-outcome" => ReferenceRetirementOutcome,
    "tombstone" => Tombstone,
    "native-snapshot" => NativeSnapshot,
    "snapshot-file" => SnapshotFile,
    "snapshot-import-plan" => SnapshotImportPlan,
    "snapshot-import-mode" => SnapshotImportMode,
    "snapshot-restore-plan" => SnapshotRestorePlan,
    "snapshot-restore-receipt" => SnapshotRestoreReceipt,
    "legacy-import-context" => LegacyImportContext,
    "legacy-import-plan" => LegacyImportPlan,
    "legacy-export-format" => LegacyExportFormat,
}

fn constrain_extensions(schema: &mut Value) {
    if let Some(object) = schema.as_object_mut() {
        if matches!(
            object.get("title").and_then(Value::as_str),
            Some(
                "Config"
                    | "IssueMetadata"
                    | "PlanningMetadata"
                    | "LabelsMetadata"
                    | "CommandDefinition"
                    | "CheckDefinition"
                    | "CheckProfileDefinition"
            )
        ) {
            object.insert("additionalProperties".into(), json!(false));
            object.insert(
                "patternProperties".into(),
                json!({"^x-[a-z0-9][a-z0-9_-]{0,95}$":true}),
            );
        }
        // Definitions may omit title, so enforce the same known type contract.
        if let Some(definitions) = object.get_mut("$defs").and_then(Value::as_object_mut) {
            for name in [
                "Config",
                "IssueMetadata",
                "PlanningMetadata",
                "LabelsMetadata",
                "CommandDefinition",
                "CheckDefinition",
                "CheckProfileDefinition",
            ] {
                if let Some(definition) = definitions.get_mut(name) {
                    definition["additionalProperties"] = json!(false);
                    definition["patternProperties"] = json!({"^x-[a-z0-9][a-z0-9_-]{0,95}$":true});
                }
            }
        }
        for child in object.values_mut() {
            constrain_extensions(child);
        }
    } else if let Some(array) = schema.as_array_mut() {
        for child in array {
            constrain_extensions(child);
        }
    }
}
