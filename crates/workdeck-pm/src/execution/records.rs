use super::*;
use crate::{
    transactions::{ChangedPath, MutationReceipt, Snapshot, canonical_hash},
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub(crate) const RESERVE: &str = "execution.reserve";
pub(crate) const FINISH: &str = "execution.finish";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunDocumentKind {
    Intent,
    Result,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "record",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RunDocument {
    Intent(Box<RunRecord>),
    Result(Box<RunResultRecord>),
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunPublication {
    pub intent: RunRecord,
    pub result: RunResultRecord,
}
pub(crate) fn intent_path(id: &LocalRunId) -> PathBuf {
    Path::new("runs").join(id.as_str()).join("intent.yml")
}
pub(crate) fn result_path(id: &LocalRunId) -> PathBuf {
    Path::new("runs").join(id.as_str()).join("result.yml")
}
pub(crate) fn validate_path(path: &Path) -> Result<(LocalRunId, RunDocumentKind)> {
    let id = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("run path is malformed"))?
        .parse()?;
    if path == intent_path(&id) {
        Ok((id, RunDocumentKind::Intent))
    } else if path == result_path(&id) {
        Ok((id, RunDocumentKind::Result))
    } else {
        Err(invalid("run records use runs/<RUN-ID>/intent.yml or result.yml").at(path))
    }
}
pub(super) fn json_value(value: &impl Serialize) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|e| invalid(e.to_string()))
}
pub(super) fn bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    let bytes = serde_yaml_ng::to_string(value)
        .map_err(|e| invalid(e.to_string()))?
        .into_bytes();
    if bytes.len() > MAX_RUN_RECORD_BYTES {
        return Err(invalid("run record exceeds 8 MiB"));
    }
    Ok(bytes)
}
pub(super) fn record(intent: RunIntent) -> Result<RunRecord> {
    let bytes = bytes(&intent)?;
    Ok(RunRecord {
        path: intent_path(&intent.id),
        content: ContentHash::of(&bytes),
        document: String::from_utf8(bytes).expect("YAML UTF8"),
        intent,
    })
}
pub(super) fn result_record(result: RunResult) -> Result<RunResultRecord> {
    let bytes = bytes(&result)?;
    Ok(RunResultRecord {
        path: result_path(&result.id),
        content: ContentHash::of(&bytes),
        document: String::from_utf8(bytes).expect("YAML UTF8"),
        result,
    })
}
pub(crate) fn parse(path: &Path, bytes: &[u8], repository: &RepositoryId) -> Result<RunDocument> {
    if bytes.len() > MAX_RUN_RECORD_BYTES {
        return Err(invalid("run record exceeds 8 MiB").at(path));
    }
    let (id, kind) = validate_path(path)?;
    let raw: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(bytes).map_err(|e| invalid(e.to_string()).at(path))?;
    SchemaVersion::try_from(
        raw.get("schema")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| invalid("run schema must be an integer").at(path))?,
    )?;
    let document = std::str::from_utf8(bytes)
        .map_err(|_| invalid("run document must be UTF8"))?
        .to_owned();
    let content = ContentHash::of(bytes);
    match kind {
        RunDocumentKind::Intent => {
            let intent: RunIntent =
                serde_yaml_ng::from_value(raw).map_err(|e| invalid(e.to_string()).at(path))?;
            if intent.id != id || &intent.repository != repository {
                return Err(invalid("run record identity differs from repository/path").at(path));
            }
            let record = RunRecord {
                intent,
                path: path.into(),
                content,
                document,
            };
            validate_intent(&record)?;
            Ok(RunDocument::Intent(Box::new(record)))
        }
        RunDocumentKind::Result => {
            let result: RunResult =
                serde_yaml_ng::from_value(raw).map_err(|e| invalid(e.to_string()).at(path))?;
            if result.id != id || &result.repository != repository {
                return Err(invalid("run result identity differs from repository/path").at(path));
            }
            let record = RunResultRecord {
                result,
                path: path.into(),
                content,
                document,
            };
            validate_result_shape(&record)?;
            Ok(RunDocument::Result(Box::new(record)))
        }
    }
}
pub(super) fn validate_intent(record: &RunRecord) -> Result<()> {
    let intent = &record.intent;
    if record.path != intent_path(&intent.id)
        || ContentHash::of(record.document.as_bytes()) != record.content
        || serde_yaml_ng::from_str::<RunIntent>(&record.document)
            .map_err(|e| invalid(e.to_string()))?
            != *intent
        || intent.input.plan.repository != intent.repository
        || intent.input.expected_plan != intent.input.plan.fingerprint
        || intent.input.actor.trim().is_empty()
        || intent.input.actor.len() > 256
        || intent.input.actor.chars().any(char::is_control)
        || !intent.worktree.is_absolute()
        || intent.invocations.len() != intent.input.plan.invocations.len()
        || intent.invocations.is_empty()
        || !intent.input.plan.blockers.is_empty()
    {
        return Err(invalid(
            "run intent identity, source or input proof is inconsistent",
        ));
    }
    crate::checks::validate_plan(&intent.input.plan)?;
    validate_run_bounds(&intent.input.plan)?;
    if let Some(revision) = &intent.revision {
        revision.validate(&intent.input.plan)?;
        crate::ci_execution::require_evaluator_declarations(&intent.input.plan)?;
    }
    for (index, resolved) in intent.invocations.iter().enumerate() {
        let invocation = &intent.input.plan.invocations[index];
        if resolved.fingerprint != invocation.fingerprint
            || resolved.cwd != intent.worktree.join(&invocation.cwd)
            || resolved.argv.is_empty()
        {
            return Err(invalid("run invocation differs from its captured plan"));
        }
        let expected = resolved_arguments(intent, index)?;
        if resolved.argv[1..] != expected {
            return Err(invalid(
                "run argv differs from exact planned argument elements",
            ));
        }
        let tool = invocation
            .inputs
            .tools
            .iter()
            .find(|pin| pin.name == invocation.tool)
            .and_then(|pin| pin.resolved.as_ref())
            .ok_or_else(|| invalid("run executable has no captured tool binding"))?;
        let expected_tool = match tool {
            crate::ToolLocation::Worktree { path } => intent.worktree.join(path),
            crate::ToolLocation::Absolute { path } => path.clone(),
        };
        if !expected_tool.is_absolute() || Path::new(&resolved.argv[0]) != expected_tool {
            return Err(invalid("run executable differs from captured tool binding"));
        }
    }
    Ok(())
}
pub(super) fn artifact_relative(intent: &RunIntent, index: usize, name: &str) -> PathBuf {
    Path::new(".local/runs")
        .join(intent.id.as_str())
        .join(format!("invocation-{index}"))
        .join("artifacts")
        .join(name)
}
pub(super) fn resolved_arguments(intent: &RunIntent, index: usize) -> Result<Vec<String>> {
    let invocation = &intent.input.plan.invocations[index];
    invocation
        .args
        .iter()
        .map(|arg| match arg {
            PlannedArgument::Literal { value } => Ok(value.clone()),
            PlannedArgument::ArtifactPath { artifact } => {
                let spec = invocation
                    .artifacts
                    .iter()
                    .find(|a| &a.id == artifact)
                    .ok_or_else(|| invalid("planned artifact token has no definition"))?;
                Ok(intent
                    .worktree
                    .join(".workdeck")
                    .join(artifact_relative(intent, index, &spec.name))
                    .to_string_lossy()
                    .into_owned())
            }
        })
        .collect()
}
pub(super) fn validate_result_shape(record: &RunResultRecord) -> Result<()> {
    let result = &record.result;
    if record.path != result_path(&result.id)
        || ContentHash::of(record.document.as_bytes()) != record.content
        || serde_yaml_ng::from_str::<RunResult>(&record.document)
            .map_err(|e| invalid(e.to_string()))?
            != *result
        || result.basis != "local_feedback"
        || result.finished_at < result.started_at
        || result.state == RunState::Running
        || result.invocations.len() > 256
        || result.checks.len() > 256
    {
        return Err(invalid(
            "run result source or classification is inconsistent",
        ));
    }
    for (index, outcome) in result.invocations.iter().enumerate() {
        if outcome.index != index
            || outcome.process.finished_at > result.finished_at
            || outcome
                .process
                .started_at
                .is_some_and(|at| at > outcome.process.finished_at)
        {
            return Err(invalid(
                "run process observation sequence or timestamps are invalid",
            ));
        }
        let process = &outcome.process;
        if (process.exit_code.is_some() && process.signal.is_some())
            || (process.termination == ProcessTermination::Exited
                && (process.started_at.is_none()
                    || (process.exit_code.is_none() && process.signal.is_none())))
            || (process.termination == ProcessTermination::NotRun
                && (process.started_at.is_some()
                    || process.exit_code.is_some()
                    || process.signal.is_some()
                    || process.elapsed_millis != 0))
            || (process.termination == ProcessTermination::SpawnFailed
                && (process.exit_code.is_some() || process.signal.is_some()))
        {
            return Err(invalid(
                "process exit, signal and spawn observations are contradictory",
            ));
        }
        for (log, name) in [
            (&outcome.process.stdout, "stdout.log"),
            (&outcome.process.stderr, "stderr.log"),
        ] {
            let expected = Path::new(".local/runs")
                .join(result.id.as_str())
                .join(format!("invocation-{index}"));
            if log.path != expected.join(name)
                || log.retained_bytes > MAX_LOCAL_LOG_BYTES
                || log.observed_bytes < log.retained_bytes
                || log.truncated != (log.observed_bytes > log.retained_bytes)
            {
                return Err(invalid("run log identity or bounds are invalid"));
            }
        }
    }
    Ok(())
}
pub(crate) fn validate_pair(intent: &RunRecord, result: &RunResultRecord) -> Result<()> {
    validate_intent(intent)?;
    validate_result_shape(result)?;
    if result.result.id != intent.intent.id
        || result.result.repository != intent.intent.repository
        || result.result.request_id != intent.intent.request_id
        || result.result.intent_content != intent.content
        || result.result.invocations.len() != intent.intent.invocations.len()
        || result.result.checks.len() != intent.intent.input.plan.checks.len()
    {
        return Err(invalid(
            "run result is not bound to the retained invocation",
        ));
    }
    for (index, outcome) in result.result.invocations.iter().enumerate() {
        if outcome.invocation != intent.intent.invocations[index].fingerprint {
            return Err(invalid("run result invocation fingerprint differs"));
        }
        let expected = &intent.intent.input.plan.invocations[index].artifacts;
        let bounds = &intent.intent.input.plan.invocations[index].bounds;
        if outcome.process.stdout.retained_bytes > bounds.stdout_bytes as u64
            || outcome.process.stderr.retained_bytes > bounds.stderr_bytes as u64
        {
            return Err(invalid("retained logs exceed exact planned bounds"));
        }
        if outcome.artifacts.len() != expected.len() {
            return Err(invalid("run artifact membership differs"));
        }
        for (artifact, spec) in outcome.artifacts.iter().zip(expected) {
            if artifact.id != spec.id
                || artifact.path != artifact_relative(&intent.intent, index, &spec.name)
                || artifact.bytes > MAX_RUN_ARTIFACT_BYTES
                || (artifact.availability == ArtifactAvailability::Present)
                    != artifact.content.is_some()
            {
                return Err(invalid("run artifact source proof differs"));
            }
        }
    }
    for (outcome, planned) in result
        .result
        .checks
        .iter()
        .zip(&intent.intent.input.plan.checks)
    {
        if outcome.check.id != planned.id
            || outcome.check.definition != planned.definition
            || intent
                .intent
                .input
                .plan
                .invocations
                .get(outcome.invocation)
                .is_none_or(|i| i.id != planned.invocation)
        {
            return Err(invalid("run check result mapping differs"));
        }
    }
    assessment::result_states(&intent.intent, &result.result)?;
    Ok(())
}
pub(super) fn reservation_input(
    input: &CheckRunRequest,
    revision: Option<&CiCheckInputBinding>,
) -> Result<serde_json::Value> {
    match revision {
        None => json_value(input),
        Some(revision) => Ok(json!({"input":input,"revision":revision})),
    }
}

pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if !matches!(receipt.operation.as_str(), RESERVE | FINISH) {
        return Ok(());
    }
    crate::transactions::validate_receipt(receipt)?;
    let (repository, path, content, input) = if receipt.operation == RESERVE {
        let record: RunRecord =
            serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
        validate_intent(&record)?;
        if record.intent.request_id != receipt.request_id {
            return Err(invalid("run reservation request proof differs"));
        }
        (
            record.intent.repository.clone(),
            record.path,
            record.content,
            reservation_input(&record.intent.input, record.intent.revision.as_ref())?,
        )
    } else {
        let publication: RunPublication =
            serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
        validate_pair(&publication.intent, &publication.result)?;
        if receipt.request_id != finish_request(&publication.intent.intent.id)? {
            return Err(invalid("run result publication request differs"));
        }
        let input = finish_input(&publication.intent, &publication.result);
        (
            publication.intent.intent.repository.clone(),
            publication.result.path,
            publication.result.content,
            input,
        )
    };
    if receipt.repository.as_ref() != Some(&repository)
        || receipt.input_hash != canonical_hash(&input)?
        || receipt.changed
            != vec![ChangedPath {
                path,
                before: None,
                after: Some(content),
            }]
    {
        return Err(invalid(
            "run receipt source or immutable publication proof differs",
        ));
    }
    Ok(())
}
pub(super) fn finish_request(id: &LocalRunId) -> Result<RequestId> {
    format!("execution-finish-{id}").parse()
}
pub(super) fn finish_input(intent: &RunRecord, result: &RunResultRecord) -> serde_json::Value {
    json!({"run":intent.intent.id,"intent":intent.content,"result":result.content})
}
pub(crate) type ReceiptCatalog = BTreeMap<String, MutationReceipt>;
pub(crate) fn receipt_catalog(snapshot: &Snapshot<'_>) -> Result<ReceiptCatalog> {
    let mut found = BTreeMap::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("operations"), 10_000)? {
        let bytes = snapshot
            .read_bounded(&path, (64 * 1024 * 1024usize).saturating_sub(total))?
            .ok_or_else(|| invalid("operation receipt disappeared"))?;
        total += bytes.len();
        let receipt: MutationReceipt =
            serde_yaml_ng::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
        if path != Path::new("operations").join(format!("{}.yml", receipt.operation_id)) {
            return Err(invalid("operation receipt canonical path differs"));
        }
        if found
            .insert(receipt.request_id.to_string(), receipt)
            .is_some()
        {
            return Err(invalid("operation request identity is duplicated"));
        }
    }
    Ok(found)
}
pub(super) fn receipts_for_record(
    catalog: &ReceiptCatalog,
    intent: &RunRecord,
) -> Result<BTreeMap<String, MutationReceipt>> {
    let mut found = BTreeMap::new();
    for (request, operation) in [
        (&intent.intent.request_id, RESERVE),
        (&finish_request(&intent.intent.id)?, FINISH),
    ] {
        if let Some(receipt) = catalog.get(&request.to_string()) {
            validate_receipt(receipt)?;
            if receipt.operation != operation
                || receipt.repository.as_ref() != Some(&intent.intent.repository)
            {
                return Err(invalid("run receipt operation or repository differs"));
            }
            found.insert(operation.into(), receipt.clone());
        }
    }
    Ok(found)
}
fn validate_source_proof(
    intent: &RunRecord,
    result: Option<&RunResultRecord>,
    catalog: &ReceiptCatalog,
) -> Result<()> {
    let receipts = receipts_for_record(catalog, intent)?;
    let reserve = receipts
        .get(RESERVE)
        .ok_or_else(|| invalid("run intent lacks original reservation receipt"))?;
    if serde_json::from_value::<RunRecord>(reserve.result.clone())
        .map_err(|e| invalid(e.to_string()))?
        != *intent
    {
        return Err(invalid(
            "run intent differs from original reservation proof",
        ));
    }
    if let Some(result) = result {
        validate_pair(intent, result)?;
        let finish = receipts
            .get(FINISH)
            .ok_or_else(|| invalid("run result lacks publication receipt"))?;
        let publication: RunPublication =
            serde_json::from_value(finish.result.clone()).map_err(|e| invalid(e.to_string()))?;
        if publication.intent != *intent || &publication.result != result {
            return Err(invalid("run result differs from publication proof"));
        }
    } else if receipts.contains_key(FINISH) {
        return Err(invalid("published result is missing"));
    }
    Ok(())
}
fn load_raw(
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
    id: &LocalRunId,
) -> Result<(RunRecord, Option<RunResultRecord>)> {
    let path = intent_path(id);
    let source = snapshot
        .read_bounded(&path, MAX_RUN_RECORD_BYTES)?
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "run was not found").at(&path))?;
    let RunDocument::Intent(intent) = parse(&path, &source, repository)? else {
        unreachable!()
    };
    let path = result_path(id);
    let result = snapshot
        .read_bounded(&path, MAX_RUN_RECORD_BYTES)?
        .map(|bytes| parse(&path, &bytes, repository))
        .transpose()?
        .map(|document| match document {
            RunDocument::Result(result) => *result,
            _ => unreachable!(),
        });
    Ok((*intent, result))
}
pub(crate) fn load_run(
    root: &Path,
    snapshot: &Snapshot<'_>,
    id: &LocalRunId,
) -> Result<(RunRecord, Option<RunResultRecord>)> {
    let config = crate::repository::config_from_snapshot(root, snapshot)?;
    let (intent, result) = load_raw(snapshot, &config.repository, id)?;
    validate_source_proof(&intent, result.as_ref(), &receipt_catalog(snapshot)?)?;
    Ok((intent, result))
}
pub(crate) fn load_runs(
    root: &Path,
    snapshot: &Snapshot<'_>,
) -> Result<Vec<(RunRecord, Option<RunResultRecord>)>> {
    let config = crate::repository::config_from_snapshot(root, snapshot)?;
    let mut ids = BTreeSet::new();
    let mut bytes = 0usize;
    for path in snapshot.list_bounded(Path::new("runs"), 12_288)? {
        let (id, _) = validate_path(&path)?;
        ids.insert(id);
        let source = snapshot
            .read_bounded(&path, MAX_RUN_RECORD_BYTES)?
            .ok_or_else(|| invalid("run record disappeared"))?;
        bytes += source.len();
        if bytes > 64 * 1024 * 1024 {
            return Err(invalid("run catalog exceeds 64 MiB"));
        }
    }
    let catalog = receipt_catalog(snapshot)?;
    // Immutable source deletion must not erase the inventory of completed or
    // ambiguous runs. Their original receipts remain replay authority.
    let mut reservations = BTreeSet::new();
    for receipt in catalog.values() {
        let id = match receipt.operation.as_str() {
            RESERVE => {
                validate_receipt(receipt)?;
                let run: RunRecord = serde_json::from_value(receipt.result.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                if !reservations.insert(run.intent.id.clone()) {
                    return Err(invalid("run has multiple original reservation receipts"));
                }
                run.intent.id
            }
            FINISH => {
                validate_receipt(receipt)?;
                let publication: RunPublication = serde_json::from_value(receipt.result.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                publication.intent.intent.id
            }
            _ => continue,
        };
        ids.insert(id);
    }
    if ids.len() > 4096 {
        return Err(invalid("run catalog exceeds 4096 identities"));
    }
    ids.into_iter()
        .map(|id| {
            let (intent, result) = load_raw(snapshot, &config.repository, &id)?;
            validate_source_proof(&intent, result.as_ref(), &catalog)?;
            Ok((intent, result))
        })
        .collect()
}
pub(crate) fn validate_import(
    _root: &Path,
    _snapshot: &Snapshot<'_>,
    _path: &Path,
    before: Option<&[u8]>,
    after: &[u8],
) -> Result<()> {
    if before.is_some_and(|bytes| bytes != after) {
        return Err(PmError::new(
            ErrorCode::Conflict,
            "run intent/results are immutable; import cannot overwrite retained execution history",
        ));
    }
    Ok(())
}
