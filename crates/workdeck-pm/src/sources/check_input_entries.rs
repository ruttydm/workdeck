//! Compare complete selected membership, not only files that happen to be tracked.
use crate::*;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Directory,
    File(ContentHash, u64, bool),
}
pub(super) fn stale() -> PmError {
    PmError::new(
        ErrorCode::StaleSource,
        "selected check inputs differ from the committed revision",
    )
}
pub(super) fn local(manifest: &InputManifest) -> Result<BTreeMap<PathBuf, Entry>> {
    let mut entries = BTreeMap::new();
    for entry in &manifest.entries {
        let (path, value) = match entry {
            InputEntry::File {
                path,
                content,
                size,
                executable,
            } => (path, Entry::File(content.clone(), *size, *executable)),
            InputEntry::Directory { path, .. } => (path, Entry::Directory),
            InputEntry::Absent { .. } => continue,
        };
        if entries.insert(path.clone(), value).is_some() {
            return Err(stale());
        }
    }
    for tool in &manifest.tools {
        if let Some(ToolLocation::Worktree { path }) = &tool.resolved {
            let value = Entry::File(
                tool.content.clone().ok_or_else(stale)?,
                tool.size.ok_or_else(stale)?,
                tool.executable_bit,
            );
            if let Some(previous) = entries.insert(path.clone(), value)
                && entries.get(path) != Some(&previous)
            {
                return Err(stale());
            }
        }
    }
    Ok(entries)
}
pub(super) fn committed(manifest: &CiEvaluatorManifest) -> BTreeMap<PathBuf, Entry> {
    let mut entries: BTreeMap<_, _> = manifest
        .entries
        .iter()
        .map(|entry| match entry {
            CiEvaluatorEntry::File {
                path,
                content,
                size,
                executable,
                ..
            } => (
                path.clone(),
                Entry::File(content.clone(), *size as u64, *executable),
            ),
            CiEvaluatorEntry::Directory { path } => (path.clone(), Entry::Directory),
        })
        .collect();
    if manifest
        .selection
        .trees
        .iter()
        .any(|path| path == std::path::Path::new("."))
    {
        entries.insert(PathBuf::from("."), Entry::Directory);
    }
    entries
}
