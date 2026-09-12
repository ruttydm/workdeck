use super::{GitOid, SourceCaptureLimits, git::BoundGit};
use crate::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn limit() -> PmError {
    PmError::new(
        ErrorCode::Unsupported,
        "combined evaluator input capture exceeds source bounds",
    )
}
fn missing(path: &Path) -> PmError {
    PmError::new(
        ErrorCode::PolicyBlocked,
        "declared evaluator input is missing or has the wrong Git type",
    )
    .at(path)
}

pub(super) fn capture(
    git: &BoundGit,
    tree: &GitOid,
    checks: &[CheckRecord],
    limits: &SourceCaptureLimits,
) -> Result<Vec<CiEvaluatorManifest>> {
    let mut selections = Vec::new();
    let mut paths = BTreeSet::new();
    for check in checks {
        let selection = check.definition.evaluator_inputs.as_ref().ok_or_else(|| {
            PmError::new(
                ErrorCode::PolicyBlocked,
                "CI checks require an explicit evaluator_inputs declaration; omitted is not empty",
            )
            .at(&check.path)
        })?;
        selection.validate()?;
        paths.extend(selection.files.iter().chain(&selection.trees).cloned());
        selections.push((check, selection));
    }
    if paths.len() > 4096
        || paths
            .iter()
            .map(|path| path.as_os_str().len())
            .sum::<usize>()
            > 1024 * 1024
    {
        return Err(limit());
    }
    // Collapse nested literal pathspecs, then bound each argv separately.
    let mut minimal: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !minimal
            .iter()
            .any(|parent| crate::ci_evaluators::covers(parent, &path))
        {
            minimal.push(path);
        }
    }
    let mut groups: Vec<Vec<PathBuf>> = Vec::new();
    let mut bytes = 0;
    for path in minimal {
        if groups.is_empty() || bytes + path.as_os_str().len() > 32 * 1024 {
            groups.push(Vec::new());
            bytes = 0;
        }
        bytes += path.as_os_str().len() + 1;
        groups.last_mut().unwrap().push(path);
    }
    let mut metadata = BTreeMap::new();
    let mut portable = BTreeMap::new();
    for group in groups {
        for entry in git.evaluator_tree_entries(tree, &group)? {
            if crate::ci_evaluators::excluded(&entry.path) {
                continue;
            }
            crate::commands::validation::relative(&entry.path, false)?;
            if entry.path.as_os_str().len() > 4096 {
                return Err(limit());
            }
            // Git emits ancestor trees even for literal file selections. Check their
            // spelling before filtering so Tests/a and tests/b cannot alias on disk.
            let folded = entry.path.to_string_lossy().to_lowercase();
            if let Some(previous) = portable.get(&folded) {
                if previous != &entry.path {
                    return Err(PmError::new(
                        ErrorCode::UnsafePath,
                        "case-colliding evaluator input paths",
                    )
                    .at(&entry.path));
                }
            } else {
                if portable.len() >= limits.max_entries {
                    return Err(limit());
                }
                portable.insert(folded, entry.path.clone());
            }
            if !selections
                .iter()
                .any(|(_, selection)| selection.selects(&entry.path))
                || metadata.contains_key(&entry.path)
            {
                continue;
            }
            if !matches!(entry.mode.as_str(), "040000" | "100644" | "100755") {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "evaluator inputs must be regular Git files or trees",
                )
                .at(&entry.path));
            }
            metadata.insert(entry.path.clone(), entry);
        }
    }
    for (_, selection) in &selections {
        for file in &selection.files {
            if metadata
                .get(file)
                .is_none_or(|entry| !matches!(entry.mode.as_str(), "100644" | "100755"))
            {
                return Err(missing(file));
            }
        }
        for directory in &selection.trees {
            if directory != Path::new(".")
                && metadata
                    .get(directory)
                    .is_none_or(|entry| entry.mode != "040000")
            {
                return Err(missing(directory));
            }
        }
    }
    let files: Vec<_> = metadata
        .values()
        .filter(|entry| entry.mode != "040000")
        .collect();
    let oids = files
        .iter()
        .map(|entry| entry.oid.clone().ok_or_else(|| missing(&entry.path)))
        .collect::<Result<Vec<_>>>()?;
    let blobs = super::batch::read(git, &oids, limits)?;
    let mut captured = BTreeMap::new();
    for (entry, bytes) in files.into_iter().zip(blobs) {
        captured.insert(
            entry.path.clone(),
            CiEvaluatorEntry::File {
                path: entry.path.clone(),
                oid: entry.oid.clone().unwrap(),
                content: ContentHash::of(&bytes),
                size: bytes.len(),
                executable: entry.mode == "100755",
            },
        );
    }
    for entry in metadata.values().filter(|entry| entry.mode == "040000") {
        captured.insert(
            entry.path.clone(),
            CiEvaluatorEntry::Directory {
                path: entry.path.clone(),
            },
        );
    }
    let mut manifests = Vec::new();
    let mut memberships = 0usize;
    for (check, selection) in selections {
        let mut entries = Vec::new();
        for entry in captured
            .values()
            .filter(|entry| selection.selects(entry.path()))
        {
            memberships += 1;
            if memberships > limits.max_entries {
                return Err(limit());
            }
            entries.push(entry.clone());
        }
        let fingerprint = crate::transactions::canonical_hash(&serde_json::json!({
            "domain":"workdeck.ci-evaluator-inputs.v1", "check":check.definition.id, "selection":selection, "entries":entries,
        }))?;
        manifests.push(CiEvaluatorManifest {
            check: check.definition.id.clone(),
            selection: selection.clone(),
            entries,
            fingerprint,
        });
    }
    Ok(manifests)
}
