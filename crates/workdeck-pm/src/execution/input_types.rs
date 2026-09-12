use crate::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputSelection {
    pub files: Vec<PathBuf>,
    pub trees: Vec<PathBuf>,
    pub optional_files: Vec<PathBuf>,
    pub dependency_files: Vec<PathBuf>,
    pub toolchain_files: Vec<PathBuf>,
}
impl InputSelection {
    pub fn validate(&self) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for (paths, tree) in [
            (&self.files, false),
            (&self.trees, true),
            (&self.optional_files, false),
            (&self.dependency_files, false),
            (&self.toolchain_files, false),
        ] {
            for path in paths {
                crate::commands::validation::relative(path, tree)?;
                if !seen.insert(path) {
                    return Err(PmError::new(
                        ErrorCode::InvalidSchema,
                        "input selectors contain duplicate paths",
                    ));
                }
            }
        }
        if seen.len() > 4096 {
            return Err(PmError::new(
                ErrorCode::InvalidSchema,
                "input selection exceeds4096 paths",
            ));
        }
        Ok(())
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputLimits {
    pub max_entries: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}
impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_file_bytes: 64 * 1024 * 1024,
            max_total_bytes: 1024 * 1024 * 1024,
        }
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEntry {
    File {
        path: PathBuf,
        content: ContentHash,
        size: u64,
        executable: bool,
    },
    Directory {
        path: PathBuf,
        membership: ContentHash,
    },
    Absent {
        path: PathBuf,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentPin {
    pub name: String,
    pub source: Option<String>,
    pub present: bool,
    pub content: Option<ContentHash>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPin {
    pub name: String,
    pub executable: String,
    pub resolved: Option<ToolLocation>,
    pub content: Option<ContentHash>,
    pub size: Option<u64>,
    pub executable_bit: bool,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolLocation {
    Worktree { path: PathBuf },
    Absolute { path: PathBuf },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputManifest {
    pub schema: SchemaVersion,
    pub selection: InputSelection,
    pub limits: InputLimits,
    /// Complete selected inputs; this is not a claim of hermetic execution.
    pub complete: bool,
    pub entries: Vec<InputEntry>,
    pub environment: Vec<EnvironmentPin>,
    pub tools: Vec<ToolPin>,
    /// Only Git metadata and Workdeck-owned internal paths are excluded.
    pub excluded_internal_paths: Vec<PathBuf>,
    pub total_bytes: u64,
    pub fingerprint: ContentHash,
}
