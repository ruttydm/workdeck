use super::*;
use crate::transactions::{ChangedPath, FileChange, MutationReceipt, canonical_hash};

pub(super) fn changed_paths(changes: &[FileChange]) -> Vec<ChangedPath> {
    changes
        .iter()
        .map(|change| ChangedPath {
            path: change.path.clone(),
            before: change.expected.clone(),
            after: change.content.as_deref().map(ContentHash::of),
        })
        .collect()
}
/// Consistency of the original intent, retained source bytes and exact result.
/// This is historical validation, not authentication of user-editable receipts.
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation == "feature.related" {
        return relations::validate_receipt(receipt);
    }
    if !matches!(
        receipt.operation.as_str(),
        "feature.create" | "feature.mutate"
    ) {
        return Ok(());
    }
    validate_result(
        &receipt.operation,
        receipt.repository.as_ref().ok_or_else(corrupt)?,
        &receipt.input_hash,
        &receipt.result,
        &receipt.changed,
    )
}
pub(super) fn validate_result(
    operation: &str,
    repository: &RepositoryId,
    input_hash: &ContentHash,
    result: &Value,
    changed: &[ChangedPath],
) -> Result<()> {
    validate_inner(operation, repository, input_hash, result, changed).map_err(|error| {
        PmError::new(
            ErrorCode::CorruptStore,
            format!(
                "feature receipt source/result/input proof is inconsistent: {}",
                error.message
            ),
        )
    })
}
fn validate_inner(
    operation: &str,
    repository: &RepositoryId,
    input_hash: &ContentHash,
    result: &Value,
    changed: &[ChangedPath],
) -> Result<()> {
    let outcome: FeatureOutcome = serde_json::from_value(result.clone()).map_err(|_| corrupt())?;
    if &canonical_hash(&serde_json::json!(outcome.intent))? != input_hash
        || outcome.record.retirement.is_some()
    {
        return Err(corrupt());
    }
    validate_record(&outcome.record, repository)?;
    let (metadata, body, path) = match &outcome.intent {
        FeatureIntent::Create { input } => {
            if operation != "feature.create" || outcome.before.is_some() {
                return Err(corrupt());
            }
            (
                store::initial(
                    repository.clone(),
                    outcome.record.metadata.id.clone(),
                    input,
                    outcome.record.metadata.created_at,
                )?,
                input.body.clone(),
                store::feature_path(
                    &outcome.record.metadata.id,
                    input.directory.as_deref().unwrap_or_default(),
                )?,
            )
        }
        FeatureIntent::Mutate {
            reference,
            expected,
            mutation,
        } => {
            if operation != "feature.mutate" {
                return Err(corrupt());
            }
            let before = outcome.before.as_ref().ok_or_else(corrupt)?;
            validate_record(before, repository)?;
            if before.retirement.is_some()
                || !store::matches_reference(&before.metadata.id, reference)
                || expected
                    .as_ref()
                    .is_some_and(|source| source != &before.source)
            {
                return Err(corrupt());
            }
            let (mut metadata, body, path) = store::candidate(before, mutation)?;
            if metadata != before.metadata || body != before.body || path != before.path {
                metadata.revision = metadata.revision.next()?;
                if outcome.record.metadata.updated_at < metadata.updated_at {
                    return Err(corrupt());
                }
                metadata.updated_at = outcome.record.metadata.updated_at;
            }
            (metadata, body, path)
        }
    };
    let (record, expected_changes) =
        prepare_record(outcome.before.as_ref(), metadata, &body, path)?;
    let mut actual_changes = changed.to_vec();
    actual_changes.sort_by(|a, b| a.path.cmp(&b.path));
    let mut expected_changes = changed_paths(&expected_changes);
    expected_changes.sort_by(|a, b| a.path.cmp(&b.path));
    if record != outcome.record || actual_changes != expected_changes {
        return Err(corrupt());
    }
    Ok(())
}
fn corrupt() -> PmError {
    PmError::new(
        ErrorCode::CorruptStore,
        "feature source or original intent differs from its receipt",
    )
}
