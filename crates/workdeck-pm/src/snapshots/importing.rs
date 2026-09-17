use super::*;
use crate::{
    Config, RepositoryId, RequestId, SchemaVersion, WorkflowCategory,
    transactions::{
        ChangedPath, FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotImportMode {
    /// Create missing records and accept byte-identical existing records.
    Merge,
    /// Replace eligible matching active records; retain unrelated records and
    /// every immutable history item. This never means replacing the root/set.
    ReplaceMatching,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotImportPlan {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub snapshot: ContentHash,
    pub mode: SnapshotImportMode,
    pub destination: ContentHash,
    pub fingerprint: ContentHash,
    pub allowed: bool,
    pub changes: Vec<ChangedPath>,
    pub blockers: Vec<PmError>,
}

impl Repository {
    pub fn preview_snapshot_import(
        &self,
        input: &NativeSnapshot,
        mode: SnapshotImportMode,
    ) -> Result<SnapshotImportPlan> {
        self.store()?.with_snapshot(|snapshot| {
            plan(self.root(), snapshot, input, mode).map(|(plan, _)| plan)
        })
    }

    pub fn import_snapshot(
        &self,
        input: &NativeSnapshot,
        mode: SnapshotImportMode,
        expected_plan: Option<&ContentHash>,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.import_snapshot_with_faults(input, mode, expected_plan, request, |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn import_snapshot_with_faults(
        &self,
        input: &NativeSnapshot,
        mode: SnapshotImportMode,
        expected_plan: Option<&ContentHash>,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        // Include actual payload hashes as well as declarations. A forged
        // fingerprint cannot make changed input masquerade as an old request.
        let bytes = input
            .files
            .iter()
            .map(|file| {
                (
                    &file.path,
                    file.kind,
                    &file.content_hash,
                    ContentHash::of(&file.content),
                )
            })
            .collect::<Vec<_>>();
        let parameters = json!({"snapshot":hash(&(&input.format,input.version,&input.repository,&input.fingerprint,bytes))?,"mode":mode,"expected_plan":expected_plan});
        self.store()?.transact_with_faults(request, "snapshot.import", &parameters, |snapshot| {
            let (plan, changes) = plan(self.root(), snapshot, input, mode)?;
            if expected_plan.is_some_and(|expected| expected != &plan.fingerprint) {
                return Err(PmError::new(ErrorCode::StaleSource, "snapshot import source, destination bytes, or membership changed since preview"));
            }
            if !plan.allowed { return Err(PmError::new(ErrorCode::PolicyBlocked, "snapshot import has unresolved blockers").details(json!({"plan":plan}))); }
            Ok(PreparedOperation { changes, result: serde_json::to_value(plan).map_err(|error| invalid(error.to_string()))? })
        }, fault)
    }
}

fn plan(
    root: &Path,
    snapshot: &Snapshot<'_>,
    input: &NativeSnapshot,
    mode: SnapshotImportMode,
) -> Result<(SnapshotImportPlan, Vec<FileChange>)> {
    plan_with_admission(
        root,
        snapshot,
        input,
        mode,
        &std::collections::BTreeSet::new(),
    )
}

// Only the private converter can admit new historical completion or composed
// add-only aggregates. Public NativeSnapshot inputs never receive this grant.
pub(super) fn plan_with_admission(
    root: &Path,
    snapshot: &Snapshot<'_>,
    input: &NativeSnapshot,
    mode: SnapshotImportMode,
    converted: &std::collections::BTreeSet<PathBuf>,
) -> Result<(SnapshotImportPlan, Vec<FileChange>)> {
    if converted.is_empty() {
        input.validate()?;
    } else {
        input.checked_files()?;
    }
    let current = capture(snapshot)?;
    let config = crate::repository::config_from_snapshot(root, snapshot)?;
    validation::validate_files(&current, &config.repository)?;
    let destination = hash(
        &current
            .iter()
            .map(|(path, bytes)| (path, ContentHash::of(bytes)))
            .collect::<Vec<_>>(),
    )?;
    let mut plan = SnapshotImportPlan {
        schema: SchemaVersion::CURRENT,
        repository: config.repository.clone(),
        snapshot: input.fingerprint.clone(),
        mode,
        destination,
        fingerprint: ContentHash::of(&[]),
        allowed: false,
        changes: Vec::new(),
        blockers: Vec::new(),
    };
    if input.repository != config.repository {
        plan.blockers.push(unsupported("native import currently requires the same repository identity; foreign-source conversion is not implemented"));
    }
    let mut projected = current.clone();
    let mut changes = Vec::new();
    for file in &input.files {
        let before = current.get(&file.path);
        if before == Some(&file.content) {
            continue;
        }
        if matches!(
            file.kind,
            SnapshotKind::Configuration | SnapshotKind::Operation | SnapshotKind::Migration
        ) {
            let error = if before.is_none() {
                unsupported(
                    "snapshot restoration requires missing configuration, operation receipts, or migration authority; receipt/provenance restoration is not implemented",
                )
            } else {
                PmError::new(
                    ErrorCode::Conflict,
                    "snapshot configuration, operation receipt, or migration authority differs from the destination",
                )
            };
            plan.blockers.push(error.at(&file.path));
            continue;
        }
        if before.is_some()
            && !converted.contains(&file.path)
            && (mode == SnapshotImportMode::Merge
                || !matches!(
                    file.kind,
                    SnapshotKind::Users
                        | SnapshotKind::OrganizationSchema
                        | SnapshotKind::Issue
                        | SnapshotKind::Initiative
                        | SnapshotKind::Milestone
                        | SnapshotKind::Target
                        | SnapshotKind::Project
                        | SnapshotKind::Cycle
                        | SnapshotKind::Labels
                        | SnapshotKind::IssueTemplate
                        | SnapshotKind::Wiki
                        | SnapshotKind::SavedView
                        | SnapshotKind::Feature
                        | SnapshotKind::FeatureRelation
                        | SnapshotKind::Question
                        | SnapshotKind::Gate
                        | SnapshotKind::CommandDefinition
                        | SnapshotKind::CheckDefinition
                        | SnapshotKind::CheckProfile
                ))
        {
            plan.blockers.push(PmError::new(ErrorCode::Conflict, "matching source differs; merge and immutable-history records cannot overwrite it").at(&file.path));
            continue;
        }
        projected.insert(file.path.clone(), file.content.clone());
        changes.push(FileChange {
            path: file.path.clone(),
            expected: before.map(|bytes| ContentHash::of(bytes)),
            content: Some(file.content.clone()),
        });
    }
    // Shared parsers, workflow, completion, retirement, attachment integrity, and
    // cross-record validation inspect the entire projected store as one source.
    let projected_view = Snapshot::from_memory(root, &projected);
    if let Err(error) = validation::validate_files(&projected, &config.repository) {
        plan.blockers.push(error);
    }
    for change in &changes {
        if let Err(error) = validate_change(
            root,
            snapshot,
            &projected_view,
            &config,
            &change.path,
            current.contains_key(&change.path),
            converted.contains(&change.path),
        ) {
            plan.blockers.push(error.at(&change.path));
        }
    }
    plan.changes = changes
        .iter()
        .map(|change| ChangedPath {
            path: change.path.clone(),
            before: change.expected.clone(),
            after: change.content.as_deref().map(ContentHash::of),
        })
        .collect();
    plan.allowed = plan.blockers.is_empty();
    plan.fingerprint = hash(&(
        &plan.repository,
        &plan.snapshot,
        mode,
        &plan.destination,
        &plan.changes,
        &plan.blockers,
    ))?;
    let prepared = PreparedOperation {
        changes: changes.clone(),
        result: serde_json::to_value(&plan).map_err(|error| invalid(error.to_string()))?,
    };
    if let Err(error) =
        snapshot.check_prepared_capacity(&prepared, &config.repository, "snapshot.import")
    {
        plan.blockers.push(error);
        plan.allowed = false;
        plan.fingerprint = hash(&(
            &plan.repository,
            &plan.snapshot,
            mode,
            &plan.destination,
            &plan.changes,
            &plan.blockers,
        ))?;
    }
    Ok((plan, changes))
}

fn validate_change(
    root: &Path,
    current: &Snapshot<'_>,
    projected: &Snapshot<'_>,
    config: &Config,
    path: &Path,
    existed: bool,
    converted: bool,
) -> Result<()> {
    let kind = validation::classify(path)?.expect("classified import path");
    match kind {
        SnapshotKind::ContractReview => {
            let before = current.read_bounded(path, crate::MAX_IMPORTED_REVIEW_BYTES)?;
            let after = projected
                .read_bounded(path, crate::MAX_IMPORTED_REVIEW_BYTES)?
                .ok_or_else(|| invalid("imported contract review disappeared"))?;
            if before.as_ref().is_some_and(|bytes| bytes != &after) {
                return Err(invalid("imported contract reviews are immutable"));
            }
            crate::retained_reviews::records::parse(path, &after, &config.repository)?;
        }
        SnapshotKind::Attestation => {
            let before = current.read_bounded(path, crate::MAX_IMPORTED_REPORT_BYTES)?;
            let after = projected
                .read_bounded(path, crate::MAX_IMPORTED_REPORT_BYTES)?
                .ok_or_else(|| invalid("imported attestation disappeared"))?;
            if before.as_ref().is_some_and(|bytes| bytes != &after) {
                return Err(invalid("imported attestations are immutable"));
            }
            crate::attestations::records::parse(path, &after, &config.repository)?;
        }

        SnapshotKind::CoordinationMarker => {
            let bytes = projected
                .read_bounded(path, 4096)?
                .ok_or_else(|| invalid("coordination marker disappeared"))?;
            crate::sources::parse_coordination_marker(path, &bytes, &config.repository)?;
        }
        SnapshotKind::Claim => {
            let bytes = projected
                .read_bounded(path, crate::MAX_CLAIM_BYTES)?
                .ok_or_else(|| invalid("imported claim disappeared"))?;
            crate::claims::validation::parse(path, &bytes, &config.repository)?;
            crate::claims::load_claims(projected, config)?;
        }
        SnapshotKind::RunIntent | SnapshotKind::RunResult => {
            let before = current.read_bounded(path, crate::execution::MAX_RUN_RECORD_BYTES)?;
            let after = projected
                .read_bounded(path, crate::execution::MAX_RUN_RECORD_BYTES)?
                .ok_or_else(|| invalid("imported run record disappeared"))?;
            crate::execution::records::parse(path, &after, &config.repository)?;
            crate::execution::records::validate_import(
                root,
                projected,
                path,
                before.as_deref(),
                &after,
            )?;
        }
        SnapshotKind::Question => {
            crate::questions::validate_import(root, current, projected, config, path)?
        }
        SnapshotKind::Handoff => crate::handoffs::validate_import(root, projected, config, path)?,
        SnapshotKind::IssueRelation => {
            crate::graph::validate_related_import(root, projected, config, path)?
        }
        SnapshotKind::Feature => {
            crate::features::validate_import(root, current, projected, config, path)?
        }
        SnapshotKind::FeatureRelation => {
            crate::features::relations::validate_import(root, projected, config, path)?
        }
        SnapshotKind::SavedView => crate::saved_views::validate_import(projected, config, path)?,
        SnapshotKind::Gate => {
            let id = crate::gates::store::validate_path(path)?;
            let before = if existed {
                Some(crate::gates::load_gate(current, config, &id)?)
            } else {
                None
            };
            let next = crate::gates::load_gate(projected, config, &id)?;
            crate::gates::store::validate_change(root, projected, config, before.as_ref(), &next)?;
        }
        SnapshotKind::Evidence => {
            let bytes = projected
                .read_bounded(path, crate::MAX_EVIDENCE_BYTES)?
                .ok_or_else(|| invalid("imported evidence disappeared"))?;
            let record = crate::evidence::store::parse(path, &bytes, &config.repository)?;
            crate::organization::validate_evidence_change(
                projected,
                config,
                &record.reference.declaration,
            )?;
        }

        SnapshotKind::Users | SnapshotKind::OrganizationSchema => {
            crate::organization::validate_import(root, current, projected, config, path)?
        }
        SnapshotKind::IssueTemplate if !converted => {
            let id = path
                .file_stem()
                .and_then(|id| id.to_str())
                .expect("canonical template path");
            crate::templates::load_template(root, projected, config, id)?;
        }
        SnapshotKind::Comment if !converted => {
            let issue = path
                .components()
                .nth(1)
                .and_then(|part| part.as_os_str().to_str())
                .expect("canonical issue path")
                .parse::<crate::IssueId>()?;
            let comment = crate::issues::load_comment(root, projected, path, &issue)?;
            crate::organization::validate_actor(projected, &config.repository, &comment.author)?;
        }
        SnapshotKind::AttachmentMetadata | SnapshotKind::TimeEntry if !converted => {
            let bytes = projected
                .read_bounded(path, crate::documents::MAX_DOCUMENT_BYTES)?
                .ok_or_else(|| invalid("imported attribution record disappeared"))?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| invalid("attribution record must be UTF-8"))?;
            let document = crate::documents::YamlDocument::parse(path, text)?;
            if kind == SnapshotKind::AttachmentMetadata {
                let record: crate::AttachmentRecord = document.deserialize()?;
                crate::organization::validate_actor(projected, &config.repository, &record.actor)?;
            } else {
                let record: crate::TimeEntry = document.deserialize()?;
                crate::organization::validate_actor(projected, &config.repository, &record.actor)?;
                // An amendment retains the superseded attribution exactly;
                // its newly supplied actor is always checked above.
                let retained_user = if let Some(previous) = &record.supersedes {
                    let previous_path =
                        PathBuf::from(format!("issues/{}/time/{previous}.yml", record.issue));
                    let bytes = projected
                        .read_bounded(&previous_path, crate::MAX_TIME_ENTRY_BYTES)?
                        .ok_or_else(|| invalid("superseded time entry is missing"))?;
                    let source = std::str::from_utf8(&bytes)
                        .map_err(|_| invalid("time entry must be UTF-8"))?;
                    let previous: crate::TimeEntry =
                        crate::documents::YamlDocument::parse(&previous_path, source)?
                            .deserialize()?;
                    previous.user == record.user
                } else {
                    false
                };
                if !retained_user {
                    crate::organization::validate_actor(
                        projected,
                        &config.repository,
                        &record.user,
                    )?;
                }
            }
        }
        SnapshotKind::Issue => {
            let id = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|id| id.to_str())
                .expect("canonical issue path");
            crate::retirement::ensure_writable(
                root,
                current,
                config,
                &crate::RetirementTarget::new(crate::RetirementKind::Issue, id)?,
            )?;
            let next = crate::issues::resolve_issue(root, projected, config, id)?;
            if existed {
                let original = crate::issues::resolve_issue(root, current, config, id)?;
                validate_issue_replacement(config, &original, &next)?;
                if !converted {
                    crate::graph::validate_issue_change(
                        root,
                        projected,
                        config,
                        Some(&original.metadata),
                        &next.metadata,
                    )?;
                    crate::features::validate_issue_associations(
                        root,
                        projected,
                        config,
                        Some(&original.metadata),
                        &next.metadata,
                    )?;
                    crate::gates::validate_issue_associations(
                        root,
                        projected,
                        config,
                        Some(&original.metadata),
                        &next.metadata,
                    )?;
                    if config.workflow.state(&next.metadata.status)?.category
                        == WorkflowCategory::Completed
                    {
                        let report = crate::issues::completion(root, projected, config, &next)?;
                        if !report.allowed {
                            return Err(PmError::new(
                                ErrorCode::PolicyBlocked,
                                "imported replacement violates current completion requirements",
                            )
                            .details(serde_json::json!(report)));
                        }
                    }
                    crate::planning::hierarchy::validate_issue_change(
                        root,
                        projected,
                        config,
                        Some(&original.metadata),
                        &next.metadata,
                    )?;
                    crate::organization::validate_issue_change(
                        projected,
                        config,
                        Some(&original.metadata),
                        &next.metadata,
                        true,
                    )?;
                }
            } else if !converted
                && matches!(
                    config.workflow.state(&next.metadata.status)?.category,
                    WorkflowCategory::Completed | WorkflowCategory::Canceled
                )
            {
                return Err(unsupported(
                    "new terminal history requires the explicit restoration/conversion contract; import cannot manufacture completion provenance",
                ));
            }
            if !existed && !converted {
                crate::graph::validate_issue_change(root, projected, config, None, &next.metadata)?;
                crate::features::validate_issue_associations(
                    root,
                    projected,
                    config,
                    None,
                    &next.metadata,
                )?;
                crate::gates::validate_issue_associations(
                    root,
                    projected,
                    config,
                    None,
                    &next.metadata,
                )?;
                crate::planning::hierarchy::validate_issue_change(
                    root,
                    projected,
                    config,
                    None,
                    &next.metadata,
                )?;
                crate::organization::validate_issue_change(
                    projected,
                    config,
                    None,
                    &next.metadata,
                    true,
                )?;
            }
        }
        SnapshotKind::Initiative
        | SnapshotKind::Project
        | SnapshotKind::Milestone
        | SnapshotKind::Cycle
        | SnapshotKind::Target
        | SnapshotKind::Labels => {
            let kind = match kind {
                SnapshotKind::Initiative => crate::PlanningKind::Initiative,
                SnapshotKind::Milestone => crate::PlanningKind::Milestone,
                SnapshotKind::Target => crate::PlanningKind::Target,
                SnapshotKind::Project => crate::PlanningKind::Project,
                SnapshotKind::Cycle => crate::PlanningKind::Cycle,
                _ => crate::PlanningKind::Label,
            };
            let originals = crate::planning::store::list_planning(root, current, kind)?;
            let next = crate::planning::store::list_planning(root, projected, kind)?;
            for old in &originals {
                let Some(new) = next.iter().find(|new| new.metadata.id == old.metadata.id) else {
                    return Err(PmError::new(
                        ErrorCode::PolicyBlocked,
                        "replace_matching cannot remove a planning identity; retire it explicitly",
                    ));
                };
                if old.metadata == new.metadata && old.body == new.body {
                    continue;
                }
                crate::retirement::ensure_writable(
                    root,
                    current,
                    config,
                    &crate::RetirementTarget::new(kind.into(), &old.metadata.id)?,
                )?;
                if new.metadata.revision <= old.metadata.revision
                    || new.metadata.created_at != old.metadata.created_at
                    || new.metadata.updated_at < old.metadata.updated_at
                    || new.metadata.imported != old.metadata.imported
                {
                    return Err(PmError::new(
                        ErrorCode::PolicyBlocked,
                        "replacement must advance revision, retain creation/import provenance, and not roll back update time",
                    ));
                }
            }
            for new in &next {
                if !converted {
                    let old = originals
                        .iter()
                        .find(|old| old.metadata.id == new.metadata.id);
                    crate::planning::hierarchy::validate_planning_change(
                        root,
                        projected,
                        config,
                        kind,
                        old.map(|old| &old.metadata),
                        &new.metadata,
                    )?;
                    if old.is_none_or(|old| old.metadata != new.metadata || old.body != new.body) {
                        crate::organization::validate_planning_change(
                            projected,
                            config,
                            kind,
                            old.map(|old| &old.metadata),
                            &new.metadata,
                            true,
                        )?;
                    }
                }
                if originals
                    .iter()
                    .all(|old| old.metadata.id != new.metadata.id)
                {
                    crate::retirement::ensure_writable(
                        root,
                        current,
                        config,
                        &crate::RetirementTarget::new(kind.into(), &new.metadata.id)?,
                    )?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_issue_replacement(
    config: &Config,
    old: &crate::IssueRecord,
    new: &crate::IssueRecord,
) -> Result<()> {
    let original = &old.metadata;
    let next = &new.metadata;
    if next.revision <= original.revision
        || next.updated_at < original.updated_at
        || next.created_at != original.created_at
        || next.completed_at != original.completed_at
        || next.canceled_at != original.canceled_at
        || next.manual_acceptance != original.manual_acceptance
        || next.imported_completion != original.imported_completion
    {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "snapshot replacement must advance revision and preserve managed provenance; completion/reopen use their semantic commands",
        ));
    }
    config.workflow.transition(&original.status, &next.status)?;
    if config.workflow.state(&original.status)?.category == WorkflowCategory::Completed {
        let mut comparable = next.clone();
        comparable.revision = original.revision;
        comparable.updated_at = original.updated_at;
        comparable.archived = original.archived;
        if comparable != *original || old.body != new.body {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "reopen completed work before changing its accepted content",
            ));
        }
    }
    Ok(())
}
