use super::*;
use crate::transactions::{ChangedPath, MutationReceipt, canonical_hash};
use serde_json::json;
use std::path::{Path, PathBuf};
pub(crate) const OPERATION: &str = "review.import_contract";
pub(crate) fn path(id: &ContractReviewId) -> PathBuf {
    Path::new("contract-reviews").join(format!("{id}.json"))
}
pub(crate) fn validate_path(value: &Path) -> Result<ContractReviewId> {
    let id: ContractReviewId = value
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("invalid contract review filename"))?
        .parse()?;
    if path(&id) != value {
        return Err(invalid(
            "contract-reviews use contract-reviews/<CRVW-ID>.json",
        ));
    }
    Ok(id)
}
pub(super) fn authenticate(
    input: &ImportContractReviewRequest,
    now: Timestamp,
) -> Result<AuthenticatedContractReview> {
    if input.envelope.len() > MAX_IMPORTED_REVIEW_BYTES
        || input.actor.trim().is_empty()
        || input.actor.len() > 256
        || input.actor.chars().any(char::is_control)
    {
        return Err(invalid(
            "imported review exceeds bounds or actor attribution is invalid",
        ));
    }
    let envelope = SignedContractReview::from_json(input.envelope.as_bytes())?;
    let admission = crate::contract_reviews::authenticate(
        &envelope,
        &input.policy,
        &input.expected_policy,
        &input.baseline,
        now,
    )?;
    if admission.approval.head.commit != input.expected_commit {
        return Err(invalid(
            "signed review differs from expected candidate commit",
        ));
    }
    Ok(admission)
}
pub(crate) fn parse(
    path: &Path,
    bytes: &[u8],
    repository: &RepositoryId,
) -> Result<ImportedContractReviewRecord> {
    if bytes.len() > MAX_IMPORTED_REVIEW_BYTES {
        return Err(invalid("imported review record exceeds 1 MiB"));
    }
    let id = validate_path(path)?;
    let record: ImportedContractReview =
        serde_json::from_slice(bytes).map_err(|e| invalid(&e.to_string()))?;
    if record.id != id
        || &record.repository != repository
        || record.input.policy.repository != *repository
    {
        return Err(invalid(
            "imported review identity differs from its repository or path",
        ));
    }
    // Reconstruct historical consistency at its recorded admission time. This
    // embedded policy is not selected as the current trust root by any reader.
    let admitted = authenticate(&record.input, record.imported_at)?;
    if record.admission != admitted {
        return Err(invalid(
            "imported review admission differs from its retained signature proof",
        ));
    }
    Ok(ImportedContractReviewRecord {
        record,
        path: path.into(),
        content: ContentHash::of(bytes),
        document: std::str::from_utf8(bytes)
            .map_err(|_| invalid("contract review must be UTF-8"))?
            .into(),
    })
}
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != OPERATION {
        return Ok(());
    }
    crate::transactions::validate_receipt(receipt)?;
    let record: ImportedContractReviewRecord =
        serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(&e.to_string()))?;
    if parse(
        &record.path,
        record.document.as_bytes(),
        &record.record.repository,
    )? != record
        || receipt.repository.as_ref() != Some(&record.record.repository)
        || receipt.request_id != record.record.request_id
        || receipt.input_hash != canonical_hash(&json!(record.record.input))?
        || receipt.changed
            != vec![ChangedPath {
                path: record.path.clone(),
                before: None,
                after: Some(record.content.clone()),
            }]
    {
        return Err(invalid(
            "imported review receipt differs from its exact input or publication",
        ));
    }
    Ok(())
}
