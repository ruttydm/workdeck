use super::*;
use crate::transactions::{ChangedPath, MutationReceipt, canonical_hash};
use serde_json::json;
use std::path::{Path, PathBuf};
pub(crate) const OPERATION: &str = "evidence.import_report";
pub(crate) fn path(id: &AttestationId) -> PathBuf {
    Path::new("attestations").join(format!("{id}.json"))
}
pub(crate) fn validate_path(value: &Path) -> Result<AttestationId> {
    let id: AttestationId = value
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("invalid attestation filename"))?
        .parse()?;
    if path(&id) != value {
        return Err(invalid("attestations use attestations/<ATST-ID>.json"));
    }
    Ok(id)
}
pub(crate) fn authenticate(
    input: &ImportCheckReportRequest,
    now: Timestamp,
) -> Result<AuthenticatedCheckReport> {
    if input.envelope.len() > MAX_IMPORTED_REPORT_BYTES
        || input.actor.trim().is_empty()
        || input.actor.len() > 256
        || input.actor.chars().any(char::is_control)
    {
        return Err(invalid(
            "imported report exceeds bounds or actor attribution is invalid",
        ));
    }
    let envelope = SignedCheckReport::from_json(input.envelope.as_bytes())?;
    if let Some(proof) = &input.red_green {
        crate::red_green::validate_retained(&proof.request(input)?, proof.review.as_ref(), now)?;
    }
    authenticate_check_report(
        &envelope,
        &input.policy,
        &input.expected_policy,
        &input.expected_commit,
        now,
    )
}
pub(crate) fn parse(
    path: &Path,
    bytes: &[u8],
    repository: &RepositoryId,
) -> Result<ImportedCheckReportRecord> {
    if bytes.len() > MAX_IMPORTED_REPORT_BYTES {
        return Err(invalid("imported report record exceeds 8 MiB"));
    }
    let id = validate_path(path)?;
    let record: ImportedCheckReport =
        serde_json::from_slice(bytes).map_err(|e| invalid(&e.to_string()))?;
    if record.id != id
        || &record.repository != repository
        || record.input.policy.repository != *repository
    {
        return Err(invalid(
            "imported report identity differs from its repository or path",
        ));
    }
    // Reconstruct historical consistency at its recorded admission time. This
    // embedded policy is not selected as the current trust root by any reader.
    let admitted = authenticate(&record.input, record.imported_at)?;
    if record.admission != ImportedReportAdmission::from(&admitted) {
        return Err(invalid(
            "imported report admission differs from its retained signature proof",
        ));
    }
    Ok(ImportedCheckReportRecord {
        record,
        path: path.into(),
        content: ContentHash::of(bytes),
        document: std::str::from_utf8(bytes)
            .map_err(|_| invalid("attestation must be UTF-8"))?
            .into(),
    })
}
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != OPERATION {
        return Ok(());
    }
    crate::transactions::validate_receipt(receipt)?;
    let record: ImportedCheckReportRecord =
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
            "imported report receipt differs from its exact input or publication",
        ));
    }
    Ok(())
}
