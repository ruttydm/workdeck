//! Policy exceptions require their exact durable decision; authored issue links
//! remain declarations. This proves recorded intent, not authenticated identity.
use super::*;
use crate::transactions::{MutationReceipt, Snapshot, canonical_hash};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphIntent {
    reference: String,
    expected: Option<SourceToken>,
    expected_graph: Option<ContentHash>,
    mutation: IssueGraphMutation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphResult {
    issue: IssueRecord,
    mutation: IssueGraphMutation,
    previous_graph: ContentHash,
    #[serde(default)]
    waivers: Vec<PrerequisiteWaiver>,
    #[serde(default)]
    intent: Option<GraphIntent>,
}

pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    validate(receipt, None)
}

pub(super) fn validate_replay(receipt: &MutationReceipt, input: &serde_json::Value) -> Result<()> {
    let intent: GraphIntent =
        serde_json::from_value(input.clone()).map_err(|error| corrupt(error.to_string()))?;
    validate(receipt, Some(&intent))
}

fn validate(receipt: &MutationReceipt, supplied: Option<&GraphIntent>) -> Result<()> {
    if receipt.operation != "issue.graph" {
        return Ok(());
    }
    crate::transactions::validate_receipt(receipt)?;
    let repository = receipt
        .repository
        .as_ref()
        .ok_or_else(|| corrupt("graph receipt has no repository identity"))?;
    let result: GraphResult = serde_json::from_value(receipt.result.clone())
        .map_err(|error| corrupt(error.to_string()))?;
    let issue = &result.issue;
    if issue.path != Path::new(&format!("issues/{}/item.md", issue.metadata.id))
        || issue.source.revision != issue.metadata.revision
        || issue.retirement.is_some()
    {
        return Err(corrupt(
            "graph result issue identity, revision, or retirement is invalid",
        ));
    }
    validate_metadata(&issue.metadata)?;
    // Old receipts omitted the full input. Retain them as historical records;
    // they can prove their stored mutation/waiver consistency, not reconstruct
    // omitted reference/precondition inputs. Replay always supplies the exact
    // caller intent, so compatibility never permits returning another mutation.
    for intent in result.intent.iter().chain(supplied) {
        if canonical_hash(
            &serde_json::to_value(intent).map_err(|error| corrupt(error.to_string()))?,
        )? != receipt.input_hash
            || intent.mutation != result.mutation
            || !reference_matches(&intent.reference, &issue.metadata.id, repository)?
            || intent
                .expected_graph
                .as_ref()
                .is_some_and(|value| value != &result.previous_graph)
        {
            return Err(corrupt(
                "graph result differs from its original request intent",
            ));
        }
        if let Some(expected) = &intent.expected {
            if let Some(change) = receipt
                .changed
                .iter()
                .find(|change| change.path == issue.path)
            {
                if change.before.as_ref() != Some(&expected.content)
                    || expected.revision.next()? != issue.source.revision
                {
                    return Err(corrupt("graph result differs from its inspected source"));
                }
            } else if expected != &issue.source {
                return Err(corrupt(
                    "unchanged graph item differs from its inspected source",
                ));
            }
        }
    }
    if let Some(change) = receipt
        .changed
        .iter()
        .find(|change| change.path == issue.path)
        && change.after.as_ref() != Some(&issue.source.content)
    {
        return Err(corrupt(
            "graph result source differs from its published item hash",
        ));
    }
    match &result.mutation {
        IssueGraphMutation::WaivePrerequisite {
            prerequisite,
            actor,
            reason,
        } => {
            let [waiver] = result.waivers.as_slice() else {
                return Err(corrupt("waiver mutation must retain one exact decision"));
            };
            if waiver.repository != *repository
                || waiver.request_id != receipt.request_id
                || waiver.issue != issue.metadata.id
                || !issue.metadata.prerequisites.contains(&waiver.prerequisite)
                || !reference_matches(prerequisite, &waiver.prerequisite, repository)?
                || waiver.actor != *actor
                || waiver.reason != *reason
                || waiver.requirement_hash != super::capture::requirement_hash(&issue.metadata)?
                || receipt.changed.len() != 1
            {
                return Err(corrupt(
                    "waiver decision differs from its recorded mutation or requirement",
                ));
            }
            text_value(actor, "waiver actor")?;
            text_value(reason, "waiver reason")?;
            let path = super::capture::waiver_path(&waiver.issue, &waiver.prerequisite);
            let bytes =
                serde_yaml_ng::to_string(waiver).map_err(|error| corrupt(error.to_string()))?;
            if receipt.changed[0].path != path
                || receipt.changed[0].after.as_ref() != Some(&ContentHash::of(bytes.as_bytes()))
            {
                return Err(corrupt("waiver decision differs from its published source"));
            }
        }
        _ if !result.waivers.is_empty() => {
            return Err(corrupt(
                "only a waiver mutation can publish a waiver decision",
            ));
        }
        _ => (),
    }
    Ok(())
}

fn reference_matches(reference: &str, id: &IssueId, repository: &RepositoryId) -> Result<bool> {
    if reference.contains("::") {
        let qualified: QualifiedRef = reference
            .parse()
            .map_err(|_| corrupt("invalid qualified graph reference"))?;
        return Ok(&qualified.repository == repository && qualified.record == *id);
    }
    Ok(reference.len() >= 4 && id.as_str().starts_with(reference))
}

pub(super) fn validate_waivers(
    snapshot: &Snapshot<'_>,
    config: &Config,
    waivers: &[PrerequisiteWaiver],
    sources: &mut BTreeMap<PathBuf, ContentHash>,
) -> Result<BTreeMap<RequestId, SourcePin>> {
    if waivers.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut receipts = Vec::new();
    let mut total = 0usize;
    let mut requests = BTreeSet::new();
    for path in snapshot.list_bounded(Path::new("operations"), MAX_NODES)? {
        let bytes = snapshot
            .read_bounded(&path, MAX_BYTES.saturating_sub(total))?
            .ok_or_else(|| corrupt("waiver decision receipt disappeared"))?;
        total += bytes.len();
        let receipt: MutationReceipt =
            serde_yaml_ng::from_slice(&bytes).map_err(|e| corrupt(e.to_string()).at(&path))?;
        crate::transactions::validate_receipt(&receipt)?;
        validate_receipt(&receipt)?;
        if receipt.repository.as_ref() != Some(&config.repository)
            || path != Path::new(&format!("operations/{}.yml", receipt.operation_id))
            || !requests.insert(receipt.request_id.clone())
        {
            return Err(corrupt(
                "waiver receipt has invalid repository, path, or duplicate request identity",
            )
            .at(path));
        }
        receipts.push((receipt, path, ContentHash::of(&bytes)));
    }
    let mut proofs = BTreeMap::new();
    for waiver in waivers {
        let path = super::capture::waiver_path(&waiver.issue, &waiver.prerequisite);
        let hash = &sources[&path];
        let Some((receipt, receipt_path, receipt_hash)) = receipts
            .iter()
            .find(|(r, _, _)| r.request_id == waiver.request_id)
        else {
            return Err(corrupt("waiver has no matching durable decision").at(path));
        };
        let recorded: Vec<PrerequisiteWaiver> =
            serde_json::from_value(receipt.result.get("waivers").cloned().unwrap_or(json!([])))
                .map_err(|e| corrupt(e.to_string()).at(receipt_path))?;
        if receipt.operation != "issue.graph"
            || !recorded.contains(waiver)
            || !receipt
                .changed
                .iter()
                .any(|c| c.path == path && c.after.as_ref() == Some(hash))
        {
            return Err(corrupt("waiver content differs from its durable decision").at(path));
        }
        if receipts.iter().any(|(r, _, _)| {
            r.operation_id != receipt.operation_id
                && r.changed.iter().any(|c| {
                    c.path == path
                        && c.before.as_ref() == Some(hash)
                        && c.after.as_ref() != Some(hash)
                })
        }) {
            return Err(corrupt(
                "waiver was revoked or superseded; restoring old bytes cannot reactivate it",
            )
            .at(path));
        }
        sources.insert(receipt_path.clone(), receipt_hash.clone());
        proofs.insert(
            waiver.request_id.clone(),
            SourcePin {
                path: receipt_path.clone(),
                content: receipt_hash.clone(),
            },
        );
    }
    Ok(proofs)
}
fn corrupt(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::CorruptStore, message)
}
