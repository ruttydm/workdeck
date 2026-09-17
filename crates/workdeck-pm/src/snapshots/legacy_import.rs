use super::*;
use crate::{
    LabelsMetadata, PlanningKind, RequestId, Timestamp,
    migration::{MigrationKind, PreviewOptions},
    transactions::{FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyImportContext {
    pub imported_at: Timestamp,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyImportPlan {
    #[serde(flatten)]
    pub plan: SnapshotImportPlan,
    pub imported_at: Timestamp,
    pub source_path: PathBuf,
    pub notices: Vec<String>,
}
impl Repository {
    pub fn preview_legacy_import(
        &self,
        input: &LegacyExport,
        context: Option<&LegacyImportContext>,
        mode: SnapshotImportMode,
    ) -> Result<LegacyImportPlan> {
        self.store()?.with_snapshot(|snapshot| {
            legacy_plan(self.root(), snapshot, input, context, mode).map(|(plan, _)| plan)
        })
    }
    pub fn import_legacy_export(
        &self,
        input: &LegacyExport,
        context: Option<&LegacyImportContext>,
        mode: SnapshotImportMode,
        expected_plan: Option<&ContentHash>,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.import_legacy_export_with_faults(input, context, mode, expected_plan, request, |_| {
            Ok(())
        })
    }
    #[doc(hidden)]
    pub fn import_legacy_export_with_faults(
        &self,
        input: &LegacyExport,
        context: Option<&LegacyImportContext>,
        mode: SnapshotImportMode,
        expected_plan: Option<&ContentHash>,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        // Unspecified import time remains null in the intent. Select actual time
        // inside the replay-first closure, never before durable request lookup.
        let parameters = json!({"source":input.hash,"format":input.format,"context":context,"mode":mode,"expected_plan":expected_plan});
        let receipt = self.store()?.transact_with_faults(request,"snapshot.import_legacy",&parameters,|snapshot|{
            if expected_plan.is_some() && context.is_none(){return Err(PmError::new(ErrorCode::InvalidInput,"applying a reviewed legacy import requires its explicit imported_at context").hint("Supply --imported-at from the preview when using --expected-plan."));}
            let (plan,changes)=legacy_plan(self.root(),snapshot,input,context,mode)?;
            if expected_plan.is_some_and(|expected|expected!=&plan.plan.fingerprint){return Err(PmError::new(ErrorCode::StaleSource,"legacy import source, context, destination bytes, or membership changed since preview"));}
            if !plan.plan.allowed{return Err(PmError::new(ErrorCode::PolicyBlocked,"legacy import has unresolved blockers").details(json!({"plan":plan})));}
            Ok(PreparedOperation{changes,result:serde_json::to_value(plan).map_err(|error|invalid(error.to_string()))?})
        },fault)?;
        let plan = validate_legacy_receipt(&receipt, input, self.identity())?;
        if plan.plan.mode != mode
            || context.is_some_and(|context| context.imported_at != plan.imported_at)
            || expected_plan.is_some_and(|expected| expected != &plan.plan.fingerprint)
        {
            return Err(invalid(
                "legacy import receipt does not describe the requested intent",
            ));
        }
        Ok(receipt)
    }
}

fn legacy_plan(
    root: &Path,
    snapshot: &Snapshot<'_>,
    input: &LegacyExport,
    context: Option<&LegacyImportContext>,
    mode: SnapshotImportMode,
) -> Result<(LegacyImportPlan, Vec<FileChange>)> {
    let current = capture(snapshot)?;
    let config = crate::repository::config_from_snapshot(root, snapshot)?;
    validation::validate_files(&current, &config.repository)?;
    let imported_at = match context {
        Some(context) => context.imported_at,
        None => previous_import_time(&current, input, &config.repository)?
            .unwrap_or_else(chrono::Utc::now),
    };
    let options = PreviewOptions {
        config: config.clone(),
        imported_at,
    };
    let source_path = input.source_path();
    let mut projected = current.clone();
    let mut allowed = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut blockers = Vec::new();
    let mut notices=vec!["Legacy JSON records remain historical declarations and annotations; no verification/check receipts or live sessions are created.".into()];
    if input.format == LegacyExportFormat::Jsonl {
        notices.push("Legacy JSONL has no declared record count; missing complete trailing records cannot be detected from this input.".into());
    }
    let mut incoming_events = Vec::new();
    for row in &input.rows {
        let key = if row.kind == MigrationKind::Issue {
            "key"
        } else {
            "id"
        };
        if row.kind != MigrationKind::ImportedEvents
            && let Some(id) = row.value.get(key).and_then(Value::as_str)
            && !seen.insert((format!("{:?}", row.kind), id.to_ascii_lowercase()))
        {
            blockers.push(collision(
                &source_path,
                format!(
                    "duplicate or case-colliding legacy record identity {id:?} at {}",
                    row.selector
                ),
            ));
            continue;
        }
        let converted = match crate::migration::convert_export_row(
            row.kind.clone(),
            &row.value,
            &row.selector,
            &source_path,
            &input.hash,
            &options,
        ) {
            Ok(converted) => converted,
            Err(error) => {
                blockers.push(error);
                continue;
            }
        };
        blockers.extend(converted.errors);
        notices.extend(converted.notices.into_iter().map(|notice| notice.message));
        for draft in converted.drafts {
            if draft.kind == MigrationKind::ImportedEvents {
                incoming_events.push(row.value.clone());
                continue;
            }
            if draft.kind == MigrationKind::Labels {
                let labels: LabelsMetadata = serde_yaml_ng::from_slice(&draft.content)
                    .map_err(|error| invalid(error.to_string()))?;
                for label in labels.labels {
                    let view = Snapshot::from_memory(root, &projected);
                    let existing =
                        crate::planning::store::list_planning(root, &view, PlanningKind::Label)?;
                    if let Some(old) = existing
                        .iter()
                        .find(|old| old.metadata.id.eq_ignore_ascii_case(&label.id))
                    {
                        if old.metadata != label {
                            blockers.push(collision(Path::new("labels.yml"),format!("legacy label {:?} collides with an existing identity; import cannot overwrite or revive it",label.id)));
                        }
                        continue;
                    }
                    match crate::planning::store::prepare_write(
                        root,
                        &view,
                        PlanningKind::Label,
                        None,
                        label,
                        "",
                    ) {
                        Ok(prepared) => {
                            for change in prepared.changes {
                                allowed.insert(change.path.clone());
                                projected
                                    .insert(change.path, change.content.expect("creation bytes"));
                            }
                        }
                        Err(error) => blockers.push(error),
                    }
                }
                continue;
            }
            let path = draft.destination_path;
            if let Some(before) = projected.get(&path) {
                if before != &draft.content {
                    blockers.push(collision(&path,"matching legacy record differs; import never overwrites native history, revisions, or retired identities"));
                }
                continue;
            }
            if let Some(existing) = projected.keys().find(|existing| {
                existing
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&path.to_string_lossy())
            }) {
                blockers.push(collision(
                    &path,
                    format!(
                        "legacy record path case-collides with existing identity {}",
                        existing.display()
                    ),
                ));
                continue;
            }
            // Only actual issue conversions can admit imported Done. A caller
            // cannot forge this set through NativeSnapshot or serialized input.
            if draft.kind == MigrationKind::Issue {
                allowed.insert(path.clone());
            }
            projected.insert(path, draft.content);
        }
    }
    if !incoming_events.is_empty() {
        let path = PathBuf::from("imported-history/events.jsonl");
        let bytes = merge_events(
            projected.get(&path).map(Vec::as_slice).unwrap_or_default(),
            &incoming_events,
        )?;
        projected.insert(path.clone(), bytes);
        allowed.insert(path);
    }
    if let Some(original) = projected.get(&source_path) {
        if original != &input.raw {
            blockers.push(collision(
                &source_path,
                "retained source artifact differs from its immutable hash",
            ));
        }
    } else {
        projected.insert(source_path.clone(), input.raw.clone());
    }
    // Even an empty legacy export has a real retained artifact. Marking that
    // created path selects projected semantic validation without a loose parser.
    allowed.insert(source_path.clone());
    let view = Snapshot::from_memory(root, &projected);
    if let Err(error) = validate_references(root, &view, &config, &allowed) {
        blockers.push(error);
    }
    let native = NativeSnapshot::build(config.repository.clone(), projected)?;
    let (plan, changes) = importing::plan_with_admission(root, snapshot, &native, mode, &allowed)?;
    let mut plan = LegacyImportPlan {
        plan,
        imported_at,
        source_path,
        notices,
    };
    plan.plan.blockers.extend(blockers);
    finalize(&mut plan, input)?;
    let prepared = PreparedOperation {
        changes: changes.clone(),
        result: serde_json::to_value(&plan).map_err(|error| invalid(error.to_string()))?,
    };
    if let Err(error) =
        snapshot.check_prepared_capacity(&prepared, &config.repository, "snapshot.import_legacy")
    {
        plan.plan.blockers.push(error);
        finalize(&mut plan, input)?;
    }
    Ok((plan, changes))
}
// An exact source artifact is immutable. Its first successful import receipt
// binds the historical context for later, independent merge requests. Current
// records must still match conversion bytes; this does not mask later edits.
fn previous_import_time(
    current: &BTreeMap<PathBuf, Vec<u8>>,
    input: &LegacyExport,
    repository: &crate::RepositoryId,
) -> Result<Option<Timestamp>> {
    let source = input.source_path();
    if !current.contains_key(&source) {
        return Ok(None);
    }
    let mut known = None;
    for (path, bytes) in current {
        if path.parent() != Some(Path::new("operations")) {
            continue;
        }
        let receipt: MutationReceipt = serde_yaml_ng::from_slice(bytes)
            .map_err(|error| invalid(error.to_string()).at(path))?;
        if receipt.operation != "snapshot.import_legacy"
            || !receipt.changed.iter().any(|change| {
                change.path == source
                    && change.before.is_none()
                    && change.after.as_ref() == Some(&input.hash)
            })
        {
            continue;
        }
        let plan =
            validate_legacy_receipt(&receipt, input, repository).map_err(|error| error.at(path))?;
        if known.is_some_and(|previous| previous != plan.imported_at) {
            return Err(invalid(
                "conflicting original import contexts for the same retained export",
            )
            .at(path));
        }
        known = Some(plan.imported_at);
    }
    Ok(known)
}
// Pure verification of the returned historical result, never today's records.
// This preserves replay after edits while rejecting a forged result/qualification.
pub(super) fn validate_legacy_receipt(
    receipt: &MutationReceipt,
    input: &LegacyExport,
    repository: &crate::RepositoryId,
) -> Result<LegacyImportPlan> {
    crate::transactions::validate_receipt(receipt)?;
    let plan: LegacyImportPlan = serde_json::from_value(receipt.result.clone())
        .map_err(|error| invalid(format!("invalid legacy import context receipt: {error}")))?;
    let mut validated = plan.clone();
    finalize(&mut validated, input)?;
    if receipt.operation != "snapshot.import_legacy"
        || receipt.repository.as_ref() != Some(repository)
        || &plan.plan.repository != repository
        || plan.source_path != input.source_path()
        || !plan.plan.allowed
        || !plan.plan.blockers.is_empty()
        || plan.plan.changes != receipt.changed
        || plan.plan.fingerprint != validated.plan.fingerprint
    {
        return Err(invalid(
            "retained legacy import context disagrees with its publication receipt",
        ));
    }
    Ok(plan)
}
fn finalize(plan: &mut LegacyImportPlan, input: &LegacyExport) -> Result<()> {
    plan.plan.allowed = plan.plan.blockers.is_empty();
    plan.plan.fingerprint = hash(&(
        &input.hash,
        &input.format,
        plan.imported_at,
        &plan.source_path,
        &plan.notices,
        &plan.plan.repository,
        &plan.plan.snapshot,
        plan.plan.mode,
        &plan.plan.destination,
        &plan.plan.changes,
        &plan.plan.blockers,
    ))?;
    Ok(())
}
fn validate_references(
    root: &Path,
    view: &Snapshot<'_>,
    config: &crate::Config,
    converted: &BTreeSet<PathBuf>,
) -> Result<()> {
    let projects = crate::planning::store::list_planning(root, view, PlanningKind::Project)?;
    let cycles = crate::planning::store::list_planning(root, view, PlanningKind::Cycle)?;
    let labels = crate::planning::store::list_planning(root, view, PlanningKind::Label)?;
    for path in converted {
        if validation::classify(path)? != Some(SnapshotKind::Issue) {
            continue;
        }
        let id = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|id| id.to_str())
            .expect("converted issue path");
        let issue = crate::issues::resolve_issue(root, view, config, id)?;
        for (kind, refs, known) in [
            (
                PlanningKind::Project,
                issue.metadata.project.iter().collect::<Vec<_>>(),
                &projects,
            ),
            (
                PlanningKind::Cycle,
                issue.metadata.cycle.iter().collect(),
                &cycles,
            ),
            (
                PlanningKind::Label,
                issue.metadata.labels.iter().collect(),
                &labels,
            ),
        ] {
            for reference in refs {
                if !known.iter().any(|record| &record.metadata.id == reference) {
                    return Err(PmError::new(
                        ErrorCode::NotFound,
                        format!(
                            "legacy issue {id} has unresolved {kind:?} reference {reference:?}"
                        ),
                    )
                    .at(path));
                }
                crate::retirement::ensure_writable(
                    root,
                    view,
                    config,
                    &crate::RetirementTarget::new(kind.into(), reference)?,
                )?;
            }
        }
    }
    Ok(())
}
// Match multiplicity rather than set-deduplicating event annotations, which
// have no stable legacy event identity. Preserve old bytes and incoming order.
fn merge_events(before: &[u8], incoming: &[Value]) -> Result<Vec<u8>> {
    let mut counts = BTreeMap::new();
    let text =
        std::str::from_utf8(before).map_err(|_| invalid("historical events must be UTF-8"))?;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value =
            serde_json::from_str(line).map_err(|error| invalid(error.to_string()))?;
        *counts.entry(hash(&value)?).or_insert(0usize) += 1;
    }
    let mut encountered = BTreeMap::new();
    let mut output = before.to_vec();
    for value in incoming {
        let key = hash(value)?;
        let count = encountered.entry(key.clone()).or_insert(0usize);
        *count += 1;
        if *count <= counts.get(&key).copied().unwrap_or(0) {
            continue;
        }
        if !output.is_empty() && output.last() != Some(&b'\n') {
            output.push(b'\n');
        }
        output.extend(serde_json::to_vec(value).map_err(|error| invalid(error.to_string()))?);
        output.push(b'\n');
    }
    Ok(output)
}
fn collision(path: &Path, message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::Conflict, message).at(path)
}
