//! Declared evaluator inputs are separate from candidate application inputs.
//! Captured Git pins establish content, not trusted-producer or sandbox authority.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EvaluatorInputSelection {
    pub files: Vec<PathBuf>,
    pub trees: Vec<PathBuf>,
}
impl EvaluatorInputSelection {
    pub fn validate(&self) -> Result<()> {
        let mut paths = BTreeSet::new();
        let mut bytes = 0usize;
        for (values, tree) in [(&self.files, false), (&self.trees, true)] {
            for path in values {
                crate::commands::validation::relative(path, tree)?;
                if excluded(path) || path.as_os_str().len() > 4096 || !paths.insert(path) {
                    return Err(PmError::new(ErrorCode::InvalidSchema, "evaluator selectors must be unique bounded source paths, not Git or engine output").at(path));
                }
                bytes += path.as_os_str().len();
            }
        }
        if paths.len() > 4096 || bytes > 1024 * 1024 {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "evaluator selection exceeds 4096 paths or 1 MiB",
            ));
        }
        Ok(())
    }
    pub(crate) fn selects(&self, path: &Path) -> bool {
        self.files.iter().any(|file| file == path)
            || self.trees.iter().any(|tree| covers(tree, path))
    }
    pub(crate) fn validate_runtime_inputs(&self, inputs: &InputSelection) -> Result<()> {
        for file in &self.files {
            if !inputs.files.contains(file)
                && !inputs.dependency_files.contains(file)
                && !inputs.toolchain_files.contains(file)
                && !inputs.trees.iter().any(|tree| covers(tree, file))
            {
                return Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "evaluator files must also be mandatory command inputs",
                )
                .at(file));
            }
        }
        for tree in &self.trees {
            if !inputs.trees.iter().any(|runtime| covers(runtime, tree)) {
                return Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "evaluator trees must also be command input trees",
                )
                .at(tree));
            }
        }
        Ok(())
    }
}
pub(crate) fn covers(tree: &Path, path: &Path) -> bool {
    tree == Path::new(".") || path.starts_with(tree)
}
pub(crate) fn excluded(path: &Path) -> bool {
    crate::execution::input_fs::excluded(Path::new(&path.to_string_lossy().to_ascii_lowercase()))
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CiEvaluatorEntry {
    Directory {
        path: PathBuf,
    },
    File {
        path: PathBuf,
        oid: GitOid,
        content: ContentHash,
        size: usize,
        executable: bool,
    },
}
impl CiEvaluatorEntry {
    pub fn path(&self) -> &Path {
        match self {
            Self::Directory { path } | Self::File { path, .. } => path,
        }
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiEvaluatorManifest {
    pub check: String,
    pub selection: EvaluatorInputSelection,
    pub entries: Vec<CiEvaluatorEntry>,
    pub fingerprint: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiEvaluatorChange {
    pub check: String,
    /// Repository-relative, unlike planning-relative contract record paths.
    pub path: PathBuf,
    pub base: Option<CiEvaluatorEntry>,
    pub head: Option<CiEvaluatorEntry>,
}
pub(crate) fn changes(
    base: &[CiEvaluatorManifest],
    head: &[CiEvaluatorManifest],
) -> Vec<CiEvaluatorChange> {
    let records = |manifests: &[CiEvaluatorManifest]| -> BTreeMap<_, _> {
        manifests
            .iter()
            .flat_map(|manifest| {
                manifest.entries.iter().map(|entry| {
                    (
                        (manifest.check.clone(), entry.path().to_owned()),
                        entry.clone(),
                    )
                })
            })
            .collect()
    };
    let before = records(base);
    let after = records(head);
    let keys: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
    keys.into_iter()
        .filter_map(|(check, path)| {
            let key = (check.clone(), path.clone());
            let base = before.get(&key);
            let head = after.get(&key);
            (base != head).then(|| CiEvaluatorChange {
                check,
                path,
                base: base.cloned(),
                head: head.cloned(),
            })
        })
        .collect()
}
