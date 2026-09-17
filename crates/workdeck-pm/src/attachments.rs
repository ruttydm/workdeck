//! Explicit, inert issue attachments. Metadata inspection never decodes, renders,
//! executes, or reads payload bytes. Explicit reads verify bounded byte content.
use crate::{
    ContentHash, ErrorCode, IssueId, PmError, Repository, RequestId, Result, SchemaVersion,
    SourceToken, Timestamp,
    documents::YamlDocument,
    issues::resolve_issue,
    repository::config_from_snapshot,
    transactions::{FileChange, MutationReceipt, PreparedOperation, Snapshot},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};

pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_NAME_BYTES: usize = 200;

#[derive(Clone)]
pub struct AttachmentInput {
    pub name: String,
    pub content: Vec<u8>,
    pub media_type: Option<String>,
    pub actor: String,
}

impl std::fmt::Debug for AttachmentInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AttachmentInput")
            .field("name", &self.name)
            .field("content_bytes", &self.content.len())
            .field("media_type", &self.media_type)
            .field("actor", &self.actor)
            .finish()
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentRecord {
    pub schema: SchemaVersion,
    pub id: IssueId,
    pub issue: IssueId,
    pub name: String,
    pub media_type: Option<String>,
    pub actor: String,
    pub created_at: Timestamp,
    pub content_hash: ContentHash,
    pub size: u64,
    pub path: PathBuf,
    pub content_path: PathBuf,
}

impl Repository {
    /// Add immutable payload and metadata through one recoverable change set.
    /// A missing source token applies the intent to the current locked issue.
    /// Existing request replay precedes issue resolution, identity, and time.
    pub fn attach_issue(
        &self,
        reference: &str,
        expected: Option<&SourceToken>,
        input: &AttachmentInput,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        if input.content.len() > MAX_ATTACHMENT_BYTES {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "attachment exceeds the 20 MiB content limit",
            ));
        }
        let content_hash = ContentHash::of(&input.content);
        let parameters = json!({
            "reference":reference, "expected":expected, "name":input.name,
            "media_type":input.media_type, "actor":input.actor,
            "content_hash":content_hash, "size":input.content.len(),
        });
        self.store()?
            .transact(request, "issue.attach", &parameters, |snapshot| {
                validate_fields(&input.name, input.media_type.as_deref(), &input.actor)?;
                crate::organization::validate_actor(snapshot, self.identity(), &input.actor)?;
                let config = config_from_snapshot(self.root(), snapshot)?;
                let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
                crate::retirement::ensure_writable(
                    self.root(),
                    snapshot,
                    &config,
                    &crate::RetirementTarget::new(
                        crate::RetirementKind::Issue,
                        issue.metadata.id.as_str(),
                    )?,
                )?;
                if expected.is_some_and(|expected| expected != &issue.source) {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "issue changed since the attachment request was prepared",
                    )
                    .at(&issue.path));
                }
                let id = IssueId::new("ATT")?;
                let directory = attachment_directory(&issue.metadata.id, &id);
                let record = AttachmentRecord {
                    schema: SchemaVersion::CURRENT,
                    id,
                    issue: issue.metadata.id.clone(),
                    name: input.name.clone(),
                    media_type: input.media_type.clone(),
                    actor: input.actor.clone(),
                    created_at: Utc::now().max(issue.metadata.created_at),
                    content_hash: content_hash.clone(),
                    size: input.content.len() as u64,
                    path: directory.join("metadata.yml"),
                    content_path: directory.join("content").join(&input.name),
                };
                let metadata = serde_yaml_ng::to_string(&record)
                    .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?;
                Ok(PreparedOperation {
                    changes: vec![
                        FileChange {
                            path: record.path.clone(),
                            expected: None,
                            content: Some(metadata.into_bytes()),
                        },
                        FileChange {
                            path: record.content_path.clone(),
                            expected: None,
                            content: Some(input.content.clone()),
                        },
                    ],
                    result: serde_json::to_value(&record).map_err(|error| {
                        PmError::new(ErrorCode::InvalidSchema, error.to_string())
                    })?,
                })
            })
    }

    pub fn list_attachments(&self, reference: &str) -> Result<Vec<AttachmentRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
            crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                &config,
                &crate::RetirementTarget::new(
                    crate::RetirementKind::Issue,
                    issue.metadata.id.as_str(),
                )?,
            )?;
            load_attachments(snapshot, &issue.metadata.id)
        })
    }

    /// Bytes are returned only on explicit request. The media type is descriptive
    /// metadata and never a request to launch a decoder, viewer, or executable.
    pub fn read_attachment(&self, reference: &str, id: &str) -> Result<Vec<u8>> {
        let id = parse_attachment_id(id)?;
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
            crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                &config,
                &crate::RetirementTarget::new(
                    crate::RetirementKind::Issue,
                    issue.metadata.id.as_str(),
                )?,
            )?;
            let record = load_attachments(snapshot, &issue.metadata.id)?
                .into_iter()
                .find(|record| record.id == id)
                .ok_or_else(|| {
                    PmError::new(
                        ErrorCode::NotFound,
                        "attachment was not found for this issue",
                    )
                })?;
            let content = snapshot
                .read_bounded(&record.content_path, MAX_ATTACHMENT_BYTES)?
                .ok_or_else(|| {
                    PmError::new(ErrorCode::CorruptStore, "attachment payload is missing")
                        .at(&record.content_path)
                })?;
            if content.len() as u64 != record.size
                || ContentHash::of(&content) != record.content_hash
            {
                return Err(PmError::new(
                    ErrorCode::CorruptStore,
                    "attachment payload does not match its recorded size and SHA-256 identity",
                )
                .at(&record.content_path));
            }
            Ok(content)
        })
    }
}

fn load_attachments(snapshot: &Snapshot<'_>, issue: &IssueId) -> Result<Vec<AttachmentRecord>> {
    let prefix = PathBuf::from(format!("issues/{issue}/attachments"));
    let mut groups = BTreeMap::<IssueId, Vec<PathBuf>>::new();
    for path in snapshot.list(&prefix)? {
        let relative = path.strip_prefix(&prefix).map_err(|_| {
            PmError::new(ErrorCode::UnsafePath, "attachment path escaped its issue").at(&path)
        })?;
        let id = relative
            .components()
            .next()
            .and_then(|part| part.as_os_str().to_str())
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::InvalidSchema,
                    "attachment has no directory identity",
                )
                .at(&path)
            })?;
        let id = parse_attachment_id(id)
            .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.message).at(&path))?;
        groups.entry(id).or_default().push(path);
    }
    let mut records = Vec::new();
    for (id, files) in groups {
        records.push(load_attachment(snapshot, issue, &id, &files)?);
    }
    records.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.id.cmp(&right.id))
    });
    Ok(records)
}

/// Validate one independent attachment's descriptor and declared layout without
/// reading, decoding, or executing its payload. Doctor supplies snapshot-listed
/// files for each group so one malformed descriptor does not hide other records.
pub(crate) fn load_attachment(
    snapshot: &Snapshot<'_>,
    issue: &IssueId,
    id: &IssueId,
    files: &[PathBuf],
) -> Result<AttachmentRecord> {
    let directory = attachment_directory(issue, id);
    let path = directory.join("metadata.yml");
    let bytes = snapshot
        .read_bounded(&path, MAX_METADATA_BYTES)?
        .ok_or_else(|| {
            PmError::new(ErrorCode::CorruptStore, "attachment metadata is missing").at(&path)
        })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        PmError::new(
            ErrorCode::InvalidSchema,
            "attachment metadata must be UTF-8",
        )
        .at(&path)
    })?;
    let document = YamlDocument::parse(&path, text)?;
    if let Some(schema) = document
        .metadata()
        .get("schema")
        .and_then(serde_yaml_ng::Value::as_u64)
    {
        SchemaVersion::try_from(schema).map_err(|error| error.at(&path))?;
    }
    let record: AttachmentRecord = document.deserialize()?;
    validate_fields(&record.name, record.media_type.as_deref(), &record.actor)
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.message).at(&path))?;
    if record.issue != *issue
        || record.id != *id
        || record.path != path
        || record.content_path != directory.join("content").join(&record.name)
        || record.size > MAX_ATTACHMENT_BYTES as u64
    {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "attachment identity, canonical paths, or declared size are invalid",
        )
        .at(&path));
    }
    if !files.contains(&record.content_path) {
        return Err(
            PmError::new(ErrorCode::CorruptStore, "attachment payload is missing")
                .at(&record.content_path),
        );
    }
    if files.len() != 2
        || files
            .iter()
            .any(|file| file != &path && file != &record.content_path)
    {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "attachment directory contains undeclared files",
        )
        .at(&directory));
    }
    Ok(record)
}

pub(crate) fn parse_attachment_id(value: &str) -> Result<IssueId> {
    let id: IssueId = value.parse()?;
    if !id.as_str().starts_with("ATT-") {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "attachment ID must be a full ATT-prefixed ULID",
        ));
    }
    Ok(id)
}

fn attachment_directory(issue: &IssueId, id: &IssueId) -> PathBuf {
    PathBuf::from(format!("issues/{issue}/attachments/{id}"))
}

fn validate_fields(name: &str, media_type: Option<&str>, actor: &str) -> Result<()> {
    // Portable ASCII leaves avoid Unicode normalization/case aliases on macOS
    // and Windows. Original content remains binary and completely unrestricted.
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || !name.is_ascii()
        || name.trim() != name
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.chars().any(|ch| {
            ch.is_control() || matches!(ch, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "attachment name must be a portable ASCII leaf of 1–200 bytes, without traversal, control characters, reserved punctuation, or trailing dots/spaces",
        ));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'));
    if device {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "attachment name is a reserved device name",
        ));
    }
    if actor.trim().is_empty() || actor.len() > 200 || actor.chars().any(char::is_control) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "attachment actor must be nonempty, at most 200 bytes, and contain no control characters",
        ));
    }
    if let Some(media_type) = media_type {
        let essence = media_type.split(';').next().unwrap_or_default().trim();
        let valid_token = |value: &str| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&byte))
        };
        if media_type.len() > 255
            || media_type.trim() != media_type
            || !media_type.is_ascii()
            || media_type.chars().any(char::is_control)
            || !essence
                .split_once('/')
                .is_some_and(|(kind, subtype)| valid_token(kind) && valid_token(subtype))
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "attachment media type must be a bounded type/subtype without control characters",
            ));
        }
    }
    Ok(())
}
