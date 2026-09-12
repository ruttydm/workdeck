use super::*;
use crate::{
    IssueId, RepositoryId,
    documents::MAX_DOCUMENT_BYTES,
    transactions::{MutationReceipt, Snapshot},
};
use std::collections::BTreeSet;

pub(crate) fn file_limit(kind: SnapshotKind) -> usize {
    match kind {
        SnapshotKind::ContractReview => crate::MAX_IMPORTED_REVIEW_BYTES,
        SnapshotKind::Attestation => crate::MAX_IMPORTED_REPORT_BYTES,
        SnapshotKind::Claim => crate::MAX_CLAIM_BYTES,
        SnapshotKind::RunIntent | SnapshotKind::RunResult => crate::execution::MAX_RUN_RECORD_BYTES,
        SnapshotKind::CommandDefinition
        | SnapshotKind::CheckDefinition
        | SnapshotKind::CheckProfile => crate::commands::catalog::MAX_DEFINITION_BYTES,
        SnapshotKind::AttachmentContent => crate::MAX_ATTACHMENT_BYTES,
        SnapshotKind::Users | SnapshotKind::OrganizationSchema => crate::MAX_ORGANIZATION_BYTES,
        SnapshotKind::TimeEntry => crate::MAX_TIME_ENTRY_BYTES,
        SnapshotKind::Question => crate::MAX_QUESTION_BYTES,
        SnapshotKind::Handoff => crate::MAX_HANDOFF_BYTES,
        SnapshotKind::Gate => crate::MAX_GATE_BYTES,
        SnapshotKind::Evidence => crate::MAX_EVIDENCE_BYTES,
        SnapshotKind::Operation
        | SnapshotKind::Migration
        | SnapshotKind::ImportedHandoff
        | SnapshotKind::ImportedHistory => MAX_SNAPSHOT_CONTENT_BYTES,
        _ => MAX_DOCUMENT_BYTES,
    }
}

/// Every accepted path has a specific authority meaning. App preferences and
/// extension source are separate scopes; unknown PM namespaces are diagnostics.
pub(crate) fn classify(path: &Path) -> Result<Option<SnapshotKind>> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid("snapshot paths must be UTF-8"))?;
    if text.len() > 1024 {
        return Err(invalid("snapshot path exceeds 1024 bytes").at(path));
    }
    crate::SourceLink {
        path: text.into(),
        line: None,
        end_line: None,
    }
    .validate()?;
    let parts = text.split('/').collect::<Vec<_>>();
    if text == ".gitignore" || text == "config.toml" || parts[0] == "extensions" {
        return Ok(None);
    }
    let kind = match parts.as_slice() {
        ["contract-reviews", _] => {
            crate::retained_reviews::records::validate_path(path)?;
            SnapshotKind::ContractReview
        }
        ["attestations", _] => {
            crate::attestations::records::validate_path(path)?;
            SnapshotKind::Attestation
        }
        ["coordination.yml"] => SnapshotKind::CoordinationMarker,
        ["claims", name] if name.ends_with(".yml") => {
            let issue: crate::IssueId = name.trim_end_matches(".yml").parse()?;
            if path != crate::claims::validation::path(&issue) {
                return Err(invalid("noncanonical claim path").at(path));
            }
            SnapshotKind::Claim
        }
        ["runs", ..] => match crate::execution::records::validate_path(path)?.1 {
            crate::execution::records::RunDocumentKind::Intent => SnapshotKind::RunIntent,
            crate::execution::records::RunDocumentKind::Result => SnapshotKind::RunResult,
        },
        [namespace @ ("commands" | "checks" | "check-profiles"), name]
            if name.ends_with(".yml") =>
        {
            if !crate::identity::valid_slug(name.trim_end_matches(".yml")) {
                return Err(
                    invalid("execution definition requires a canonical slug filename").at(path),
                );
            }
            match *namespace {
                "commands" => SnapshotKind::CommandDefinition,
                "checks" => SnapshotKind::CheckDefinition,
                _ => SnapshotKind::CheckProfile,
            }
        }
        ["questions", ..] => {
            crate::questions::validation::validate_path(path)?;
            SnapshotKind::Question
        }
        ["issues", _, "handoffs", ..] => {
            crate::handoffs::validate_path(path)?;
            SnapshotKind::Handoff
        }
        ["features", ..] => {
            crate::features::validate_path(path)?;
            SnapshotKind::Feature
        }
        ["relations", "features", ..] => {
            crate::features::relations::validate_path(path)?;
            SnapshotKind::FeatureRelation
        }
        ["gates", _] => {
            crate::gates::store::validate_path(path)?;
            SnapshotKind::Gate
        }
        ["evidence", _] => {
            crate::evidence::store::validate_path(path)?;
            SnapshotKind::Evidence
        }
        ["wiki", ..] => {
            crate::wiki::validate_stored_path(path)?;
            SnapshotKind::Wiki
        }
        ["views", name] if name.ends_with(".yml") => {
            if crate::saved_views::path(name.trim_end_matches(".yml"))? != path {
                return Err(invalid("saved view identity must match its filename").at(path));
            }
            SnapshotKind::SavedView
        }
        ["relations", "issues", ..] => {
            crate::graph::validate_path(path)?;
            SnapshotKind::IssueRelation
        }
        ["relations", "waivers", ..] => {
            crate::graph::validate_path(path)?;
            SnapshotKind::PrerequisiteWaiver
        }
        ["config.yml"] => SnapshotKind::Configuration,
        ["users.yml"] => SnapshotKind::Users,
        ["schema.yml"] => SnapshotKind::OrganizationSchema,
        ["labels.yml"] => SnapshotKind::Labels,
        ["issues", id, "item.md"] => {
            id.parse::<IssueId>()?;
            SnapshotKind::Issue
        }
        ["issues", issue, "comments", comment] if comment.ends_with(".md") => {
            issue.parse::<IssueId>()?;
            SnapshotKind::Comment
        }
        ["issues", issue, "time", name] if name.ends_with(".yml") => {
            issue.parse::<IssueId>()?;
            crate::time_entries::parse_time_id(name.trim_end_matches(".yml"))?;
            SnapshotKind::TimeEntry
        }
        ["issues", issue, "attachments", id, "metadata.yml"] => {
            issue.parse::<IssueId>()?;
            crate::attachments::parse_attachment_id(id)?;
            SnapshotKind::AttachmentMetadata
        }
        ["issues", issue, "attachments", id, "content", _] => {
            issue.parse::<IssueId>()?;
            crate::attachments::parse_attachment_id(id)?;
            SnapshotKind::AttachmentContent
        }
        ["initiatives", id, "item.md"] => {
            crate::planning::validate_id(id)?;
            SnapshotKind::Initiative
        }
        ["milestones", id, "item.md"] => {
            crate::planning::validate_id(id)?;
            SnapshotKind::Milestone
        }
        ["targets", id, "item.md"] => {
            crate::planning::validate_id(id)?;
            SnapshotKind::Target
        }
        ["projects", id, "item.md"] => {
            crate::planning::validate_id(id)?;
            SnapshotKind::Project
        }
        ["cycles", id, "item.md"] => {
            crate::planning::validate_id(id)?;
            SnapshotKind::Cycle
        }
        ["templates", "issues", name] if name.ends_with(".md") => SnapshotKind::IssueTemplate,
        ["tombstones", kind, name]
            if [
                "issues",
                "features",
                "gates",
                "initiatives",
                "projects",
                "milestones",
                "cycles",
                "targets",
                "labels",
            ]
            .contains(kind)
                && name.ends_with(".yml") =>
        {
            SnapshotKind::Tombstone
        }
        ["operations", name] if name.ends_with(".yml") => SnapshotKind::Operation,
        ["migration.yml"] => SnapshotKind::Migration,
        ["migrations", operation, name] if ["plan.json", "manifest.yml"].contains(name) => {
            operation.parse::<crate::OperationId>()?;
            SnapshotKind::Migration
        }
        ["imported-sessions", name] if name.ends_with(".toml") => SnapshotKind::ImportedSession,
        ["imported-history", "exports", name]
            if name.ends_with(".json") || name.ends_with(".jsonl") =>
        {
            let stem = Path::new(name)
                .file_stem()
                .and_then(|name| name.to_str())
                .expect("UTF-8 path");
            stem.parse::<ContentHash>()?;
            SnapshotKind::ImportedHistory
        }
        ["imported-history", "events.jsonl"] => SnapshotKind::ImportedHistory,
        ["imported-history", name]
            if [
                "projects-metadata.yml",
                "cycles-metadata.yml",
                "labels-metadata.yml",
            ]
            .contains(name) =>
        {
            SnapshotKind::ImportedHistory
        }
        ["imported-history", "deleted-sessions", name] if name.ends_with(".yml") => {
            SnapshotKind::ImportedHistory
        }
        ["imported-handoffs", ..] if parts.len() > 1 => SnapshotKind::ImportedHandoff,
        _ => {
            return Err(unsupported(
                "unsupported authoritative snapshot kind or noncanonical path",
            )
            .at(path));
        }
    };
    Ok(Some(kind))
}

pub(crate) fn validate_files(
    files: &BTreeMap<PathBuf, Vec<u8>>,
    repository: &RepositoryId,
) -> Result<()> {
    validate_source_files(
        files,
        repository,
        MAX_SNAPSHOT_FILES,
        MAX_SNAPSHOT_CONTENT_BYTES,
    )
}

/// Validate the same authority and historical proof closure for a bounded,
/// immutable Git source. Portable transfers retain their smaller public limits.
pub(crate) fn validate_source_files(
    files: &BTreeMap<PathBuf, Vec<u8>>,
    repository: &RepositoryId,
    max_files: usize,
    max_bytes: usize,
) -> Result<()> {
    inspect_source_files(files, repository, max_files, max_bytes).map(|_| ())
}

/// The projection retains these same source diagnostics without running a
/// second doctor pass or weakening the portable-transfer validation boundary.
pub(crate) fn inspect_source_files(
    files: &BTreeMap<PathBuf, Vec<u8>>,
    repository: &RepositoryId,
    max_files: usize,
    max_bytes: usize,
) -> Result<crate::DoctorReport> {
    // Incoming and destination manifests can each be portable while their
    // union collides. Apply the same layout contract to the projected source.
    let mut portable = BTreeSet::new();
    let mut total = 0usize;
    for (path, bytes) in files {
        if !portable.insert(path.to_string_lossy().to_lowercase()) {
            return Err(invalid("projected snapshot has case-colliding paths").at(path));
        }
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("projected snapshot size overflow"))?;
    }
    if files.len() > max_files || total > max_bytes {
        let message = if max_files == MAX_SNAPSHOT_FILES && max_bytes == MAX_SNAPSHOT_CONTENT_BYTES
        {
            "projected snapshot exceeds the 4096 file or 32 MiB decoded content limit".to_owned()
        } else {
            format!(
                "planning source exceeds the {max_files} file or {max_bytes} byte decoded content limit"
            )
        };
        return Err(unsupported(message));
    }
    for path in &portable {
        if Path::new(path).ancestors().skip(1).any(|parent| {
            parent
                .to_str()
                .is_some_and(|parent| portable.contains(parent))
        }) {
            return Err(invalid("projected snapshot file is also another file's parent").at(path));
        }
    }
    let root = Path::new("snapshot");
    let snapshot = Snapshot::from_memory(root, files);
    let config = crate::repository::config_from_snapshot(root, &snapshot)?;
    if &config.repository != repository {
        return Err(invalid(
            "snapshot configuration and repository identity disagree",
        ));
    }
    crate::migration::check_access(root, &snapshot, None)?;
    let report = crate::repository::inspect_snapshot(root, &snapshot)?;
    if !report.valid {
        return Err(invalid("snapshot contains invalid authoritative records")
            .details(serde_json::json!({"errors":report.errors})));
    }
    let mut requests = BTreeSet::new();
    for (path, bytes) in files {
        match classify(path)?.expect("snapshot files were classified") {
            SnapshotKind::CoordinationMarker => {
                crate::sources::parse_coordination_marker(path, bytes, repository)?;
            }
            SnapshotKind::Operation => {
                let receipt: MutationReceipt = serde_yaml_ng::from_slice(bytes)
                    .map_err(|error| invalid(error.to_string()).at(path))?;
                crate::transactions::validate_receipt(&receipt)?;
                if receipt.operation == "snapshot.restore" {
                    crate::restore::validate_restore_receipt(&receipt)?;
                }
                if matches!(
                    receipt.operation.as_str(),
                    "issue.log_time" | "issue.amend_time"
                ) {
                    crate::time_entries::validate_receipt_result(&receipt, repository)?;
                }
                if receipt.repository.as_ref() != Some(repository)
                    || path != Path::new(&format!("operations/{}.yml", receipt.operation_id))
                    || !requests.insert(receipt.request_id.clone())
                {
                    return Err(invalid(
                        "operation repository, filename, or unique request identity is invalid",
                    )
                    .at(path));
                }
                validate_result_source(&receipt).map_err(|error| error.at(path))?;
            }
            SnapshotKind::AttachmentMetadata => {
                let record: crate::AttachmentRecord = serde_yaml_ng::from_slice(bytes)
                    .map_err(|error| invalid(error.to_string()).at(path))?;
                let payload = files
                    .get(&record.content_path)
                    .ok_or_else(|| invalid("attachment content is missing").at(path))?;
                if payload.len() as u64 != record.size
                    || ContentHash::of(payload) != record.content_hash
                {
                    return Err(invalid(
                        "snapshot attachment bytes differ from their declared content identity",
                    )
                    .at(&record.content_path));
                }
            }
            _ => {}
        }
    }
    Ok(report)
}

// A historical receipt need not describe the current source revision. Its own
// reported source must nevertheless agree with the file identity it published.
fn validate_result_source(receipt: &MutationReceipt) -> Result<()> {
    crate::graph::validate_receipt(receipt)?;
    crate::wiki::validate_receipt(receipt)?;
    crate::saved_views::validate_receipt(receipt)?;
    crate::features::validate_receipt(receipt)?;
    crate::gates::store::validate_receipt(receipt)?;
    crate::evidence::store::validate_receipt(receipt)?;
    crate::attestations::records::validate_receipt(receipt)?;
    crate::retained_reviews::records::validate_receipt(receipt)?;
    crate::questions::validate_receipt(receipt)?;
    crate::handoffs::validate_receipt(receipt)?;
    crate::execution::records::validate_receipt(receipt)?;
    crate::claims::validate_receipt(receipt)?;
    crate::completion::validate_receipt(receipt)?;
    let result = &receipt.result;
    if let (Some(path), Some(content)) = (
        result.get("path").and_then(serde_json::Value::as_str),
        result.pointer("/source/content"),
    ) {
        let content: ContentHash =
            serde_json::from_value(content.clone()).map_err(|error| invalid(error.to_string()))?;
        if !receipt.changed.is_empty()
            && !receipt.changed.iter().any(|change| {
                change.path == Path::new(path) && change.after.as_ref() == Some(&content)
            })
        {
            return Err(invalid(
                "receipt result source differs from its published file hash",
            ));
        }
    }
    Ok(())
}
