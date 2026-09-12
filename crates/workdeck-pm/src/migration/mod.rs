//! Explicit prototype migration planning, application, and recovery. No source
//! annotation is executed. Preview is strictly read-only; apply requires the
//! reviewed plan and a stable request ID. See `PROTOCOL.md` in this directory.

use crate::{Config, ContentHash, ErrorCode, PmError, Result, SchemaVersion, Timestamp};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod app_config;
mod apply;
mod convert;
mod export;
pub(crate) use export::convert_export_row;
mod scan;

pub use apply::{MigrationFault, MigrationReceipt, apply, apply_with_faults, resume};
pub(crate) use apply::{check_access, check_root};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewOptions {
    pub config: Config,
    pub imported_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationKind {
    Issue,
    Project,
    Cycle,
    Labels,
    AppConfig,
    ImportedSession,
    ImportedEvents,
    ImportedHandoff,
    Extension,
    Disposable,
    Unknown,
    Configuration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationFile {
    pub path: PathBuf,
    pub kind: MigrationKind,
    pub content_hash: Option<ContentHash>,
    pub size: Option<u64>,
    pub destinations: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationDraft {
    pub kind: MigrationKind,
    pub source_path: Option<PathBuf>,
    pub source_content: Option<ContentHash>,
    pub destination_path: PathBuf,
    pub content_hash: ContentHash,
    #[serde(with = "bytes")]
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationNotice {
    pub code: String,
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPreview {
    pub schema: SchemaVersion,
    pub source_root: PathBuf,
    pub destination_root: PathBuf,
    pub options: PreviewOptions,
    pub fingerprint: ContentHash,
    /// Complete means all inputs are accounted for and no known blockers remain;
    /// it is not an assertion that anything has been applied or cut over.
    pub complete: bool,
    pub directories: Vec<PathBuf>,
    pub inventory: Vec<MigrationFile>,
    pub destination_inventory: Vec<DestinationFile>,
    pub drafts: Vec<MigrationDraft>,
    pub notices: Vec<MigrationNotice>,
    pub blockers: Vec<PmError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationFile {
    pub path: PathBuf,
    pub content_hash: Option<ContentHash>,
    pub size: Option<u64>,
}

/// The caller supplies the proposed repository identity and import time once and
/// preserves them with the reviewed plan. Repeated previews with the same input
/// bytes and context have identical fingerprints and conversion drafts.
pub fn preview(
    legacy_root: &Path,
    destination: &Path,
    options: &PreviewOptions,
) -> Result<MigrationPreview> {
    options.config.validate()?;
    let source_root = scan::source_root(legacy_root)?;
    let destination_root = scan::destination_root(destination)?;
    if source_root.starts_with(&destination_root) || destination_root.starts_with(&source_root) {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "legacy source and migration destination must be separate, non-nested roots",
        )
        .at(&destination_root));
    }
    let snapshot = scan::capture(&source_root)?;
    let source_prefix = source_root
        .strip_prefix(destination_root.parent().expect("destination has parent"))
        .ok();
    let mut result = MigrationPreview {
        schema: SchemaVersion::CURRENT,
        source_root: source_root.clone(),
        destination_root: destination_root.clone(),
        options: options.clone(),
        fingerprint: ContentHash::of(&[]),
        complete: false,
        directories: snapshot.directories.clone(),
        inventory: Vec::new(),
        destination_inventory: Vec::new(),
        drafts: Vec::new(),
        notices: Vec::new(),
        blockers: snapshot.errors.clone(),
    };
    if source_prefix.is_none() {
        result.blockers.push(PmError::new(ErrorCode::Unsupported,"external legacy roots require an explicit portable provenance mapping before conversion; input inventory remains available").at(&source_root));
    }
    for input in &snapshot.files {
        if input.bytes.is_some()
            && let Some(source_prefix) = source_prefix
        {
            match convert::convert(input, options, &source_prefix.join(&input.path)) {
                Ok(converted) => {
                    result.drafts.extend(converted.drafts);
                    result.notices.extend(converted.notices);
                    result.blockers.extend(converted.errors);
                }
                Err(error) => result.blockers.push(error),
            }
        }
        result.inventory.push(MigrationFile {
            path: input.path.clone(),
            kind: input.kind.clone(),
            content_hash: input.hash.clone(),
            size: input.size,
            destinations: result
                .drafts
                .iter()
                .filter(|draft| draft.source_path.as_ref() == Some(&input.path))
                .map(|draft| draft.destination_path.clone())
                .collect(),
        });
    }
    add_configuration(&mut result)?;
    add_ignores(&mut result)?;
    for draft in &result.drafts {
        if draft.content.len() > 32 * 1024 * 1024 {
            result.blockers.push(PmError::new(ErrorCode::Unsupported,"migration draft exceeds 32 MiB; a bounded explicit preservation plan is required before apply").at(&draft.destination_path));
        }
    }
    check_destinations(&mut result)?;
    check_references(&mut result)?;
    // A preview is a read snapshot, not a sequence of unrelated best-effort reads.
    // Cooperating apply will lock and revalidate these fingerprints again.
    if scan::capture(&source_root)? != snapshot {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "legacy source changed while migration preview was being prepared",
        )
        .at(&source_root));
    }
    for expected in &result.destination_inventory {
        let current = scan::destination_file(&destination_root, &expected.path)?;
        if current.as_deref().map(ContentHash::of) != expected.content_hash {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "migration destination changed during preview",
            )
            .at(destination_root.join(&expected.path)));
        }
    }
    result.drafts.sort_by(|left, right| {
        left.destination_path
            .cmp(&right.destination_path)
            .then(left.source_path.cmp(&right.source_path))
    });
    result
        .notices
        .sort_by(|left, right| left.path.cmp(&right.path).then(left.code.cmp(&right.code)));
    result.blockers.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.message.cmp(&right.message))
    });
    result.complete = result.blockers.is_empty();
    if let Err(error) = apply::validate_publication_sizes(&result) {
        result.blockers.push(error);
        result.complete = false;
    }
    result.fingerprint = fingerprint(&result)?;
    Ok(result)
}

fn fingerprint(preview: &MigrationPreview) -> Result<ContentHash> {
    let mut unsigned = preview.clone();
    unsigned.fingerprint = ContentHash::of(&[]);
    serde_json::to_vec(&unsigned)
        .map(|bytes| ContentHash::of(&bytes))
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))
}

fn add_ignores(preview: &mut MigrationPreview) -> Result<()> {
    let path = PathBuf::from(".gitignore");
    let existing = match scan::destination_file(&preview.destination_root, &path) {
        Ok(value) => value,
        Err(error) => {
            preview.blockers.push(error);
            return Ok(());
        }
    };
    let mut text = match String::from_utf8(existing.clone().unwrap_or_default()) {
        Ok(text) => text,
        Err(_) => {
            preview.blockers.push(
                PmError::new(
                    ErrorCode::InvalidSchema,
                    "destination ignore file must be UTF-8",
                )
                .at(&path),
            );
            return Ok(());
        }
    };
    for entry in [
        "/.tmp/",
        "/.index/",
        "/.local/",
        "/config.local.yml",
        "/config.local.toml",
        "/settings.local.yml",
    ] {
        if !text.lines().any(|line| line == entry) {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(entry);
            text.push('\n');
        }
    }
    if existing.as_deref() == Some(text.as_bytes()) {
        preview.destination_inventory.push(DestinationFile {
            path,
            content_hash: existing.as_deref().map(ContentHash::of),
            size: existing.as_ref().map(|bytes| bytes.len() as u64),
        });
    } else {
        let content = text.into_bytes();
        preview.drafts.push(MigrationDraft {
            kind: MigrationKind::Configuration,
            source_path: None,
            source_content: None,
            destination_path: path,
            content_hash: ContentHash::of(&content),
            content,
        });
    }
    Ok(())
}

fn add_configuration(preview: &mut MigrationPreview) -> Result<()> {
    let path = PathBuf::from("config.yml");
    match scan::destination_file(&preview.destination_root, &path) {
        Ok(Some(bytes)) => {
            preview.destination_inventory.push(DestinationFile {
                path: path.clone(),
                content_hash: Some(ContentHash::of(&bytes)),
                size: Some(bytes.len() as u64),
            });
            match crate::repository::parse_config(&preview.destination_root.join(&path), &bytes) {
                Ok(config) if config == preview.options.config => {}
                Ok(_) => preview.blockers.push(
                    PmError::new(
                        ErrorCode::Conflict,
                        "destination PM configuration differs from the explicit preview context",
                    )
                    .at(preview.destination_root.join(path)),
                ),
                Err(error) => preview.blockers.push(error),
            }
        }
        Ok(None) => {
            // Import must not assign a fresh identity to an orphaned native
            // source. Unrelated destination files remain outside its ownership.
            for namespace in [
                "issues",
                "initiatives",
                "projects",
                "milestones",
                "cycles",
                "targets",
                "features",
                "gates",
                "questions",
                "relations",
                "wiki",
                "labels.yml",
                "users.yml",
                "schema.yml",
                "operations",
                "tombstones",
                "imported-sessions",
                "imported-history",
                "imported-handoffs",
                "claims",
                "runs",
                "commands",
                "checks",
                "check-profiles",
                "claims",
                "coordination.yml",
                "evidence",
                "templates",
                "views",
                "migrations",
                "migration.yml",
            ] {
                match std::fs::symlink_metadata(preview.destination_root.join(namespace)) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        let entries = std::fs::read_dir(preview.destination_root.join(namespace))
                            .map_err(|error| {
                            PmError::io(preview.destination_root.join(namespace), error)
                        })?;
                        if entries.count() == 0 {
                            continue;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(PmError::io(preview.destination_root.join(namespace), error));
                    }
                    Ok(_) => {}
                }
                preview.blockers.push(PmError::new(ErrorCode::RecoveryRequired,"destination planning records exist without config.yml; restore its repository identity before migration").at(preview.destination_root.join(namespace)));
            }
            let content = convert::yaml(&path, &preview.options.config)?;
            preview.drafts.push(MigrationDraft {
                kind: MigrationKind::Configuration,
                source_path: None,
                source_content: None,
                destination_path: path,
                content_hash: ContentHash::of(&content),
                content,
            });
        }
        Err(error) => preview.blockers.push(error),
    }
    Ok(())
}

fn check_destinations(preview: &mut MigrationPreview) -> Result<()> {
    let mut paths = std::collections::BTreeSet::new();
    for draft in &preview.drafts {
        let path = &draft.destination_path;
        if !paths.insert(path.to_string_lossy().to_lowercase()) {
            preview.blockers.push(
                PmError::new(
                    ErrorCode::Conflict,
                    "multiple migration records map to the same portable destination path",
                )
                .at(path),
            );
        }
        match scan::destination_file(&preview.destination_root, path) {
            Ok(bytes) => {
                preview.destination_inventory.push(DestinationFile {
                    path: path.clone(),
                    content_hash: bytes.as_deref().map(ContentHash::of),
                    size: bytes.as_ref().map(|bytes| bytes.len() as u64),
                });
                if bytes.is_some()
                    && !(draft.kind == MigrationKind::Configuration
                        && path == Path::new(".gitignore"))
                {
                    preview.blockers.push(PmError::new(ErrorCode::Conflict,"migration destination already exists; preview will not overwrite or assume ownership of it").at(preview.destination_root.join(path)));
                }
            }
            Err(error) => preview.blockers.push(error),
        }
    }
    preview
        .destination_inventory
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(())
}

fn check_references(preview: &mut MigrationPreview) -> Result<()> {
    use crate::{
        IssueMetadata, LabelsMetadata,
        documents::{MarkdownDocument, YamlDocument},
    };
    let mut projects = std::collections::BTreeSet::new();
    let mut cycles = std::collections::BTreeSet::new();
    let mut labels = std::collections::BTreeSet::new();
    for draft in &preview.drafts {
        match draft.kind {
            MigrationKind::Project | MigrationKind::Cycle
                if draft
                    .destination_path
                    .file_name()
                    .is_some_and(|name| name == "item.md") =>
            {
                let id = draft
                    .destination_path
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .expect("converted reference path")
                    .to_owned();
                if draft.kind == MigrationKind::Project {
                    projects.insert(id);
                } else {
                    cycles.insert(id);
                }
            }
            MigrationKind::Labels => {
                let document = YamlDocument::parse(
                    &draft.destination_path,
                    std::str::from_utf8(&draft.content).expect("generated UTF-8"),
                )?;
                let metadata: LabelsMetadata = document.deserialize()?;
                labels.extend(metadata.labels.into_iter().map(|label| label.id));
            }
            _ => {}
        }
    }
    for draft in &preview.drafts {
        if draft.kind != MigrationKind::Issue {
            continue;
        }
        let document = MarkdownDocument::parse(
            &draft.destination_path,
            std::str::from_utf8(&draft.content).expect("generated UTF-8"),
        )?;
        let issue: IssueMetadata = document.deserialize()?;
        for (kind, references, known) in [
            (
                "project",
                issue.project.iter().collect::<Vec<_>>(),
                &projects,
            ),
            ("cycle", issue.cycle.iter().collect::<Vec<_>>(), &cycles),
            ("label", issue.labels.iter().collect::<Vec<_>>(), &labels),
        ] {
            for reference in references {
                if !known.contains(reference) {
                    preview.blockers.push(PmError::new(ErrorCode::NotFound,format!("legacy issue {} has unresolved {kind} reference {reference:?}; it remains preserved in the conversion draft",issue.id)).at(draft.source_path.as_ref().expect("issue source")));
                }
            }
        }
    }
    Ok(())
}

mod bytes {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        STANDARD
            .decode(String::deserialize(deserializer)?)
            .map_err(D::Error::custom)
    }
}
