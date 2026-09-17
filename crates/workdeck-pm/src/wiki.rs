//! Plain Markdown authoring. Wiki bodies are inert UTF-8, without mandatory
//! frontmatter. Paths and exact content hashes are their public edit contract.
use crate::{
    ContentHash, ErrorCode, PmError, Repository, RequestId, Result, SourceLink,
    documents::MAX_DOCUMENT_BYTES,
    repository::config_from_snapshot,
    transactions::{
        ChangedPath, FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

const MAX_WIKI_ENTRIES: usize = 10_000;
const MAX_WIKI_BYTES: usize = 32 * 1024 * 1024;

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WikiDocument {
    /// Relative to the planning root, including `wiki/`.
    pub path: PathBuf,
    pub content_hash: ContentHash,
    pub body: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteWiki {
    /// Relative to `wiki/`, for example `architecture/overview.md`.
    pub path: String,
    pub body: String,
    /// None is create-only. Updates require the exact previously read hash.
    pub expected: Option<ContentHash>,
}

impl Repository {
    pub fn wiki_document(&self, path: &str) -> Result<WikiDocument> {
        let path = wiki_path(path)?;
        self.store()?.with_snapshot(|snapshot| {
            config_from_snapshot(self.root(), snapshot)?;
            read_document(snapshot, &path)
        })
    }

    pub fn wiki_documents(&self) -> Result<Vec<WikiDocument>> {
        self.store()?.with_snapshot(|snapshot| {
            config_from_snapshot(self.root(), snapshot)?;
            let mut documents = Vec::new();
            let mut total = 0usize;
            for path in snapshot.list_bounded(Path::new("wiki"), MAX_WIKI_ENTRIES)? {
                let document = read_document(snapshot, &path)?;
                total = total.checked_add(document.body.len()).ok_or_else(|| {
                    PmError::new(ErrorCode::Unsupported, "wiki content size overflow")
                })?;
                if total > MAX_WIKI_BYTES {
                    return Err(PmError::new(
                        ErrorCode::Unsupported,
                        "wiki listing exceeds 32 MiB",
                    ));
                }
                documents.push(document);
            }
            documents.sort_by(|left, right| left.path.cmp(&right.path));
            Ok(documents)
        })
    }

    pub fn write_wiki(&self, input: &WriteWiki, request: &RequestId) -> Result<MutationReceipt> {
        self.write_wiki_with_faults(input, request, |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn write_wiki_with_faults(
        &self,
        input: &WriteWiki,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let path = wiki_path(&input.path)?;
        validate_body(&path, input.body.as_bytes())?;
        let parameters = json!({"path":input.path,"body":input.body,"expected":input.expected});
        let receipt = self.store()?.transact_with_faults(request, "wiki.write", &parameters, |snapshot| {
            config_from_snapshot(self.root(), snapshot)?;
            // Enumeration also pins membership and rejects nonregular paths.
            // Canonical lowercase path components avoid cross-platform case aliases.
            for existing in snapshot.list_bounded(Path::new("wiki"), MAX_WIKI_ENTRIES)? {
                validate_stored_path(&existing)?;
            }
            let before = snapshot.read_bounded(&path, MAX_DOCUMENT_BYTES)?;
            let actual = before.as_deref().map(ContentHash::of);
            if actual != input.expected {
                let code = if input.expected.is_some() { ErrorCode::StaleSource } else { ErrorCode::Conflict };
                return Err(PmError::new(code, "wiki destination differs from the expected content; read it before updating").at(&path));
            }
            let record = WikiDocument {
                path: path.clone(),
                content_hash: ContentHash::of(input.body.as_bytes()),
                body: input.body.clone(),
            };
            let changes = if before.as_deref() == Some(input.body.as_bytes()) {
                Vec::new()
            } else {
                vec![FileChange {path: path.clone(), expected: actual, content: Some(input.body.as_bytes().to_vec())}]
            };
            let result = serde_json::to_value(record).map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?;
            let changed = changes.iter().map(|change| ChangedPath {path:change.path.clone(),before:change.expected.clone(),after:change.content.as_deref().map(ContentHash::of)}).collect::<Vec<_>>();
            validate_result(&result, &changed, &crate::transactions::canonical_hash(&parameters)?)?;
            Ok(PreparedOperation {changes, result})
        }, fault)?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
}

pub(crate) fn wiki_path(relative: &str) -> Result<PathBuf> {
    SourceLink {
        path: relative.into(),
        line: None,
        end_line: None,
    }
    .validate()?;
    if relative.len() > 512 || relative.split('/').count() > 16 {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "wiki path exceeds 512 bytes or 16 components",
        ));
    }
    let parts = relative.split('/').collect::<Vec<_>>();
    let (file, directories) = parts.split_last().expect("validated nonempty path");
    let stem = file
        .strip_suffix(".md")
        .ok_or_else(|| PmError::new(ErrorCode::InvalidInput, "wiki files must end in .md"))?;
    if !crate::identity::valid_slug(stem)
        || directories
            .iter()
            .any(|part| !crate::identity::valid_slug(part))
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "wiki names must be lowercase portable slugs",
        ));
    }
    Ok(Path::new("wiki").join(relative))
}

pub(crate) fn validate_stored_path(path: &Path) -> Result<()> {
    let relative = path
        .strip_prefix("wiki")
        .ok()
        .and_then(Path::to_str)
        .ok_or_else(|| {
            PmError::new(ErrorCode::InvalidSchema, "wiki paths belong below wiki/").at(path)
        })?;
    if wiki_path(relative)? != path {
        return Err(PmError::new(ErrorCode::InvalidSchema, "noncanonical wiki path").at(path));
    }
    Ok(())
}

fn validate_body(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_DOCUMENT_BYTES || bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
    {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "wiki Markdown must be UTF-8 without NUL and at most 2 MiB",
        )
        .at(path));
    }
    Ok(())
}

fn read_document(snapshot: &Snapshot<'_>, path: &Path) -> Result<WikiDocument> {
    validate_stored_path(path)?;
    let bytes = snapshot
        .read_bounded(path, MAX_DOCUMENT_BYTES)?
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "wiki document was not found").at(path))?;
    validate_body(path, &bytes)?;
    Ok(WikiDocument {
        path: path.to_owned(),
        content_hash: ContentHash::of(&bytes),
        body: String::from_utf8(bytes).expect("validated UTF-8"),
    })
}

pub(crate) fn inspect_snapshot(snapshot: &Snapshot<'_>) -> (usize, Vec<PmError>) {
    let paths = match snapshot.list_bounded(Path::new("wiki"), MAX_WIKI_ENTRIES) {
        Ok(paths) => paths,
        Err(error) => return (0, vec![error]),
    };
    let count = paths.len();
    let mut errors = Vec::new();
    let mut total = 0usize;
    for path in paths {
        match read_document(snapshot, &path) {
            Ok(document) => {
                total = total.saturating_add(document.body.len());
                if total > MAX_WIKI_BYTES {
                    errors.push(PmError::new(
                        ErrorCode::Unsupported,
                        "wiki inspection exceeds 32 MiB",
                    ));
                    break;
                }
            }
            Err(error) => errors.push(error),
        }
    }
    (count, errors)
}

/// Pure historical proof: never compare a replay to today's wiki contents.
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != "wiki.write" {
        return Ok(());
    }
    validate_result(&receipt.result, &receipt.changed, &receipt.input_hash)
}

fn validate_result(
    result: &serde_json::Value,
    changed: &[ChangedPath],
    input_hash: &ContentHash,
) -> Result<()> {
    let corrupt = || {
        PmError::new(
            ErrorCode::CorruptStore,
            "wiki receipt content, path, changes, or original input proof is invalid",
        )
    };
    let record: WikiDocument = serde_json::from_value(result.clone()).map_err(|_| corrupt())?;
    validate_stored_path(&record.path).map_err(|_| corrupt())?;
    validate_body(&record.path, record.body.as_bytes()).map_err(|_| corrupt())?;
    if ContentHash::of(record.body.as_bytes()) != record.content_hash {
        return Err(corrupt());
    }
    let expected = match changed {
        [] => Some(record.content_hash.clone()),
        [change]
            if change.path == record.path
                && change.after.as_ref() == Some(&record.content_hash)
                && change.before != change.after =>
        {
            change.before.clone()
        }
        _ => return Err(corrupt()),
    };
    let relative = record
        .path
        .strip_prefix("wiki")
        .ok()
        .and_then(Path::to_str)
        .ok_or_else(corrupt)?;
    let parameters = json!({"path":relative,"body":record.body,"expected":expected});
    if &crate::transactions::canonical_hash(&parameters)? != input_hash {
        return Err(corrupt());
    }
    Ok(())
}
