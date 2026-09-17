//! Live comparison uses the immutable candidate's evaluator selection. A dirty
//! recipe cannot shrink that selection and authenticate its own replacement.
use crate::transactions::Snapshot;
use crate::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(super) fn capture(
    root: &Path,
    snapshot: &Snapshot<'_>,
    committed: &CiEvaluationContract,
) -> Result<ReviewWorkingTree> {
    let config = crate::repository::config_from_snapshot(root, snapshot)?;
    // Compare planning independently from evaluator bytes. Keep the committed
    // manifests here so a local mismatch cannot fabricate Git object identities.
    let local =
        crate::ci_contracts::capture(snapshot, &config, |_| Ok(committed.evaluators.clone()))?;
    let mut files = std::collections::BTreeSet::new();
    let mut trees = std::collections::BTreeSet::new();
    let mut expected = BTreeMap::new();
    for manifest in &committed.evaluators {
        files.extend(manifest.selection.files.iter().cloned());
        trees.extend(manifest.selection.trees.iter().cloned());
        for entry in &manifest.entries {
            let value = match entry {
                CiEvaluatorEntry::Directory { .. } => json!({"kind":"directory"}),
                CiEvaluatorEntry::File {
                    content,
                    size,
                    executable,
                    ..
                } => json!({"kind":"file","content":content,"size":size,"executable":executable}),
            };
            expected.insert(entry.path().to_owned(), value);
        }
    }
    // Overlapping check selections share one bounded read; directory membership
    // includes untracked files and file modes, without consulting the Git index.
    let selection = InputSelection {
        files: files
            .into_iter()
            .filter(|path| !trees.contains(path))
            .collect(),
        trees: trees.into_iter().collect(),
        ..InputSelection::default()
    };
    let worktree = crate::execution::inputs::worktree(root)?;
    let limits = InputLimits::default();
    let live = crate::execution::input_fs::capture(&worktree, &selection, &limits)?;
    let mut actual: BTreeMap<_, Value> = BTreeMap::new();
    for entry in &live.entries {
        let (path, value) = match entry {
            InputEntry::Directory { path, .. } if path == Path::new(".") => continue,
            InputEntry::Directory { path, .. } => (path, json!({"kind":"directory"})),
            InputEntry::File {
                path,
                content,
                size,
                executable,
            } => (
                path,
                json!({"kind":"file","content":content,"size":size,"executable":executable}),
            ),
            InputEntry::Absent { path } => (path, json!({"kind":"absent"})),
        };
        actual.insert(path.clone(), value);
    }
    let result = ReviewWorkingTree {
        contract: local.fingerprint.clone(),
        evaluators: crate::transactions::canonical_hash(&json!(actual))?,
        matches_revision: local.fingerprint == committed.fingerprint && actual == expected,
    };
    // Recheck inode/root identities as well as bytes before admitting this live
    // observation. Context performs another full capture before packet return.
    if live != crate::execution::input_fs::capture(&worktree, &selection, &limits)? {
        return Err(crate::execution::input_fs::stale());
    }
    Ok(result)
}
