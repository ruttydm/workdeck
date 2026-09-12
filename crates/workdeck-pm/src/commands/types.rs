use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArgumentToken {
    Literal { value: String },
    Parameter { name: String },
    Parameters { name: String },
    Artifact { id: String },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandRecipe {
    Argv {
        argv: Vec<ArgumentToken>,
    },
    /// Script text is authored and immutable; tokens become positional arguments.
    Shell {
        interpreter: String,
        script: String,
        args: Vec<ArgumentToken>,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterType {
    String { max_bytes: usize },
    Integer { minimum: i64, maximum: i64 },
    Boolean,
    Choice { values: Vec<String> },
    Path,
    Strings { max_items: usize, max_bytes: usize },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterDefinition {
    pub value_type: ParameterType,
    #[serde(default)]
    pub default: Option<Value>,
}
pub type ArgumentValues = BTreeMap<String, Value>;
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvironmentValue {
    Literal {
        value: String,
    },
    /// Values are resolved from this explicit name; public plans contain only pins.
    Inherit {
        source: String,
        #[serde(default = "yes")]
        required: bool,
    },
}
fn yes() -> bool {
    true
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequirement {
    /// Logical identity used as argv[0] or the shell interpreter.
    pub name: String,
    /// Explicit absolute/worktree-relative path, or a basename resolved using declared PATH.
    pub executable: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBounds {
    pub timeout_seconds: u64,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
}
impl Default for RunBounds {
    fn default() -> Self {
        Self {
            timeout_seconds: 300,
            stdout_bytes: 1024 * 1024,
            stderr_bytes: 1024 * 1024,
        }
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSpec {
    pub id: String,
    /// Portable leaf name within this invocation's owned local artifact directory.
    pub name: String,
    pub max_bytes: usize,
    #[serde(default)]
    pub required: bool,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclaredEffect {
    Read { path: PathBuf },
    Write { path: PathBuf },
    Network { description: String },
    Other { description: String },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDefinition {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub archived: bool,
    pub recipe: CommandRecipe,
    #[serde(default)]
    pub parameters: BTreeMap<String, ParameterDefinition>,
    pub cwd: PathBuf,
    #[serde(default)]
    pub environment: BTreeMap<String, EnvironmentValue>,
    pub tools: Vec<ToolRequirement>,
    pub inputs: InputSelection,
    #[serde(default)]
    pub bounds: RunBounds,
    #[serde(default)]
    pub artifacts: Vec<ArtifactSpec>,
    #[serde(default)]
    pub effects: Vec<DeclaredEffect>,
    #[serde(default)]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRecord {
    pub definition: CommandDefinition,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}
