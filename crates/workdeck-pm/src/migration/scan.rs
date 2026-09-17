use super::MigrationKind;
use crate::{ContentHash, ErrorCode, PmError, Result};
use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

pub(super) const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 32;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Input {
    pub path: PathBuf,
    pub kind: MigrationKind,
    pub hash: Option<ContentHash>,
    pub size: Option<u64>,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub files: Vec<Input>,
    pub directories: Vec<PathBuf>,
    pub errors: Vec<PmError>,
}

pub(super) fn source_root(path: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PmError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "legacy source must be a real directory",
        )
        .at(path));
    }
    path.canonicalize()
        .map_err(|error| PmError::io(path, error))
}

pub(super) fn destination_root(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => path
            .canonicalize()
            .map_err(|error| PmError::io(path, error)),
        Ok(_) => Err(PmError::new(
            ErrorCode::UnsafePath,
            "migration destination must be a real directory",
        )
        .at(path)),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let name = path.file_name().ok_or_else(|| {
                PmError::new(
                    ErrorCode::UnsafePath,
                    "destination requires a directory name",
                )
                .at(path)
            })?;
            Ok(destination_root(parent)?.join(name))
        }
        Err(error) => Err(PmError::io(path, error)),
    }
}

pub(super) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PmError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "migration inputs must be regular files; links and special files are not followed",
        )
        .at(path));
    }
    if metadata.len() > limit as u64 {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            format!("migration input exceeds the {limit}-byte limit"),
        )
        .at(path));
    }
    let file = fs::File::open(path).map_err(|error| PmError::io(path, error))?;
    if !file
        .metadata()
        .map_err(|error| PmError::io(path, error))?
        .is_file()
    {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "migration input is no longer a regular file",
        )
        .at(path));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PmError::io(path, error))?;
    if bytes.len() > limit {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            format!("migration input exceeds the {limit}-byte limit"),
        )
        .at(path));
    }
    Ok(bytes)
}

pub(super) fn destination_file(root: &Path, relative: &Path) -> Result<Option<Vec<u8>>> {
    let mut path = root.to_owned();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(
                PmError::new(ErrorCode::UnsafePath, "invalid migration destination path")
                    .at(relative),
            );
        }
        path.push(component.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "migration destination contains a symlink",
                )
                .at(&path));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(PmError::io(&path, error)),
        }
    }
    read(&path, MAX_INPUT_BYTES).map(Some)
}

pub(super) fn capture(root: &Path) -> Result<Snapshot> {
    let mut snapshot = Snapshot {
        files: Vec::new(),
        directories: Vec::new(),
        errors: Vec::new(),
    };
    let mut pending = vec![(PathBuf::new(), 0)];
    let mut total = 0usize;
    let mut entries = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        let absolute = root.join(&directory);
        if depth > MAX_DEPTH {
            snapshot.errors.push(
                PmError::new(
                    ErrorCode::InvalidSchema,
                    "migration input exceeds 32 directory levels",
                )
                .at(&absolute),
            );
            continue;
        }
        let mut children = match fs::read_dir(&absolute) {
            Ok(children) => children
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|error| PmError::io(&absolute, error))?,
            Err(error) => {
                snapshot.errors.push(PmError::io(&absolute, error));
                continue;
            }
        };
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            entries += 1;
            if entries > MAX_ENTRIES {
                return Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "migration source exceeds 10000 entries; split or explicitly reduce its scope",
                )
                .at(root));
            }
            let relative = directory.join(entry.file_name());
            let path = root.join(&relative);
            let metadata =
                fs::symlink_metadata(&path).map_err(|error| PmError::io(&path, error))?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                snapshot.directories.push(relative.clone());
                pending.push((relative, depth + 1));
                continue;
            }
            let kind = classify(&relative);
            let size = metadata.is_file().then_some(metadata.len());
            let bytes = match read(
                &path,
                MAX_INPUT_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(total)),
            ) {
                Ok(bytes) => {
                    total += bytes.len();
                    Some(bytes)
                }
                Err(error) => {
                    snapshot.errors.push(error);
                    None
                }
            };
            snapshot.files.push(Input {
                path: relative,
                kind,
                hash: bytes.as_deref().map(ContentHash::of),
                size,
                bytes,
            });
        }
    }
    snapshot
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    snapshot.directories.sort();
    snapshot.errors.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.message.cmp(&right.message))
    });
    Ok(snapshot)
}

fn classify(path: &Path) -> MigrationKind {
    match path.to_str() {
        Some("projects.toml") => MigrationKind::Project,
        Some("cycles.toml") => MigrationKind::Cycle,
        Some("labels.toml") => MigrationKind::Labels,
        Some("config.toml") => MigrationKind::AppConfig,
        Some("events.jsonl") => MigrationKind::ImportedEvents,
        _ if path.starts_with("issues") => MigrationKind::Issue,
        _ if path.starts_with("agents") => MigrationKind::ImportedSession,
        _ if path.starts_with("handoffs") => MigrationKind::ImportedHandoff,
        _ if path.starts_with("extensions") => MigrationKind::Extension,
        _ if path.starts_with("index") => MigrationKind::Disposable,
        _ => MigrationKind::Unknown,
    }
}
