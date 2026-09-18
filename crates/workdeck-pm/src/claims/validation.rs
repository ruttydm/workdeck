use super::*;
use crate::transactions::{ChangedPath, MutationReceipt, canonical_hash};
use std::path::{Path, PathBuf};

pub(crate) fn path(issue: &IssueId) -> PathBuf {
    PathBuf::from(format!("claims/{issue}.yml"))
}

pub(crate) fn validate_metadata(value: &ClaimMetadata) -> Result<()> {
    value.contract.validate()?;
    transition::text(&value.actor, "claim actor")?;
    if value.generation == 0
        || value.contract.issue != value.issue
        || value.contract.accepted_source.repository != value.repository
        || !matches!(
            value.contract.accepted_source.role,
            SourceRole::Local | SourceRole::Accepted
        )
        || value.acquired_at > value.updated_at
        || value.expires_at <= value.acquired_at
        || (value.state == ClaimState::Active && value.expires_at <= value.updated_at)
    {
        return Err(invalid(
            "claim identity, contract, generation or timestamps are inconsistent",
        ));
    }
    if let Some(reason) = &value.reason {
        transition::text(reason, "claim reason")?;
    }
    if value.state != ClaimState::Active && value.reason.is_none() {
        return Err(invalid("terminated claims require an explicit reason"));
    }
    if !matches!(
        value.last_operation.as_str(),
        "claim.acquire"
            | "claim.recover"
            | "claim.renew"
            | "claim.revalidate"
            | "claim.release"
            | "claim.cancel"
            | "claim.supersede"
    ) {
        return Err(invalid("unknown claim operation"));
    }
    let source = &value.contract.accepted_source;
    if source.role == SourceRole::Accepted
        && (source.ref_name.is_none() || source.commit.is_none() || source.tree.is_none())
    {
        return Err(invalid(
            "accepted work contracts require exact Git ref, commit and tree identities",
        ));
    }
    Ok(())
}

pub(crate) fn parse(
    record_path: &Path,
    bytes: &[u8],
    repository: &RepositoryId,
) -> Result<ClaimRecord> {
    if bytes.len() > MAX_CLAIM_BYTES {
        return Err(invalid("claim exceeds 128 KiB").at(record_path));
    }
    let metadata: ClaimMetadata =
        serde_yaml_ng::from_slice(bytes).map_err(|e| invalid(e.to_string()).at(record_path))?;
    validate_metadata(&metadata).map_err(|e| e.at(record_path))?;
    if &metadata.repository != repository || record_path != path(&metadata.issue) {
        return Err(invalid("claim repository or canonical path disagrees").at(record_path));
    }
    let source = SourceToken::new(Revision::new(metadata.generation)?, bytes);
    let document = String::from_utf8(bytes.to_vec())
        .map_err(|_| invalid("claim is not UTF-8").at(record_path))?;
    Ok(ClaimRecord {
        metadata,
        source,
        path: record_path.to_path_buf(),
        document,
    })
}

pub(crate) fn serialize(metadata: ClaimMetadata) -> Result<ClaimRecord> {
    let bytes = serde_yaml_ng::to_string(&metadata).map_err(|e| invalid(e.to_string()))?;
    parse(
        &path(&metadata.issue),
        bytes.as_bytes(),
        &metadata.repository,
    )
}

pub(crate) fn validate_record(record: &ClaimRecord) -> Result<()> {
    if parse(
        &record.path,
        record.document.as_bytes(),
        &record.metadata.repository,
    )? != *record
    {
        return Err(invalid(
            "claim descriptor does not match its exact document",
        ));
    }
    Ok(())
}

pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if !receipt.operation.starts_with("claim.") {
        return Ok(());
    }
    let change: ClaimChange =
        serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
    validate_record(&change.after)?;
    if let Some(before) = &change.before {
        validate_record(before)?;
    }
    let after = &change.after;
    if receipt.operation != change.request.operation()
        || receipt.request_id != after.metadata.last_request
        || receipt.repository.as_ref() != Some(&after.metadata.repository)
        || receipt.input_hash != canonical_hash(&serde_json::json!({"request":change.request}))?
        || change.before.as_ref().is_some_and(|before| {
            before.metadata.repository != after.metadata.repository
                || before.metadata.issue != after.metadata.issue
        })
        || receipt.changed
            != vec![ChangedPath {
                path: after.path.clone(),
                before: change.before.as_ref().map(|r| r.source.content.clone()),
                after: Some(after.source.content.clone()),
            }]
    {
        return Err(invalid(
            "claim receipt identity, original request or publication proof is inconsistent",
        ));
    }
    let derived = transition::apply(
        &change.request,
        change.before.as_ref(),
        &change.accepted_contract,
        &change.policy,
        after.metadata.updated_at,
        &receipt.request_id,
        after.metadata.token.clone(),
    )?;
    if derived != *after {
        return Err(invalid(
            "claim receipt contradicts its ownership transition",
        ));
    }
    Ok(())
}
