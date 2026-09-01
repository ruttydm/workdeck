//! Explicit, idempotent migration of Hunk user/repository configuration into Workdeck paths.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Deprecated semantic-role keys accepted during the Hunk migration window.
pub const LEGACY_CUSTOM_SYNTAX_COLOR_KEYS: [&str; 11] = [
    "default",
    "keyword",
    "string",
    "comment",
    "number",
    "function",
    "property",
    "type",
    "variable",
    "operator",
    "punctuation",
];

fn legacy_syntax_role_scopes(role: &str) -> &'static [&'static str] {
    match role {
        "default" => &["source"],
        "keyword" => &["keyword"],
        "string" => &["string"],
        "comment" => &["comment", "punctuation.definition.comment"],
        "number" => &["constant.numeric"],
        "function" => &[
            "entity.name.function",
            "support.function",
            "variable.function",
        ],
        "property" => &["variable.other.property", "support.variable.property"],
        "type" => &[
            "entity.name.type",
            "entity.name.class",
            "support.type",
            "support.class",
        ],
        "variable" => &["variable"],
        "operator" => &["keyword.operator"],
        "punctuation" => &["punctuation"],
        _ => &[],
    }
}

/// Translate deprecated semantic colors into approximate TextMate selectors.
#[must_use]
pub fn legacy_syntax_colors_to_scopes(
    syntax: Option<&BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let syntax = syntax?;
    let mut scopes = BTreeMap::new();
    for role in LEGACY_CUSTOM_SYNTAX_COLOR_KEYS {
        let Some(color) = syntax.get(role).filter(|color| !color.is_empty()) else {
            continue;
        };
        for scope in legacy_syntax_role_scopes(role) {
            scopes.insert((*scope).into(), color.clone());
        }
    }
    (!scopes.is_empty()).then_some(scopes)
}

/// Apply exact TextMate scopes after translated legacy roles.
#[must_use]
pub fn resolve_syntax_scope_overrides(
    syntax: Option<&BTreeMap<String, String>>,
    syntax_scopes: Option<&BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let legacy = legacy_syntax_colors_to_scopes(syntax);
    match (legacy, syntax_scopes) {
        (None, None) => None,
        (None, Some(exact)) => Some(exact.clone()),
        (Some(legacy), None) => Some(legacy),
        (Some(mut legacy), Some(exact)) => {
            legacy.extend(exact.clone());
            Some(legacy)
        }
    }
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("migration source {path} could not be read: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("migration source {path} is invalid TOML: {source}")]
    Toml {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("migration could not write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "cannot resolve a user config directory; HOME, USERPROFILE, and XDG_CONFIG_HOME are unset"
    )]
    MissingConfigDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub global: Option<ConfigMigration>,
    pub repository: Option<ConfigMigration>,
    pub legacy_state: Option<PathBuf>,
    pub legacy_extensions: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigMigration {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub destination_exists: bool,
    pub already_imported: bool,
    pub imported_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationResult {
    pub changed: Vec<PathBuf>,
    pub backups: Vec<PathBuf>,
    pub legacy_extensions: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub fn plan(repo: &Path) -> Result<MigrationPlan, MigrationError> {
    let config_root = user_config_root().ok_or(MigrationError::MissingConfigDirectory)?;
    plan_with_roots(repo, &config_root)
}

pub fn plan_with_roots(repo: &Path, config_root: &Path) -> Result<MigrationPlan, MigrationError> {
    let hunk_root = config_root.join("hunk");
    let workdeck_root = config_root.join("workdeck");
    let global_source = hunk_root.join("config.toml");
    let repo_source = repo.join(".hunk/config.toml");
    let global = global_source
        .is_file()
        .then(|| inspect_config(&global_source, &workdeck_root.join("config.toml")))
        .transpose()?;
    let repository = repo_source
        .is_file()
        .then(|| inspect_config(&repo_source, &repo.join(".agents/workdeck/config.toml")))
        .transpose()?;
    let legacy_state = hunk_root
        .join("state.json")
        .is_file()
        .then(|| hunk_root.join("state.json"));
    let legacy_extensions = discover_legacy_extensions(&hunk_root.join("extensions"), repo);
    let mut warnings = Vec::new();
    if !legacy_extensions.is_empty() {
        warnings.push(format!(
            "{} TypeScript extension entries were inventoried but will not execute; rewrite them against workdeck-extension-api v1",
            legacy_extensions.len()
        ));
    }
    if legacy_state.is_some() {
        warnings.push(
            "Hunk trust decisions are imported as legacy provenance and require fresh native-extension consent"
                .into(),
        );
    }
    Ok(MigrationPlan {
        global,
        repository,
        legacy_state,
        legacy_extensions,
        warnings,
    })
}

pub fn apply(plan: &MigrationPlan) -> Result<MigrationResult, MigrationError> {
    let mut result = MigrationResult {
        changed: Vec::new(),
        backups: Vec::new(),
        legacy_extensions: plan.legacy_extensions.clone(),
        warnings: plan.warnings.clone(),
    };
    for migration in [&plan.global, &plan.repository].into_iter().flatten() {
        if migration.already_imported {
            continue;
        }
        let source = read_toml(&migration.source)?;
        let imported = map_hunk_config(&source);
        let mut destination = if migration.destination.is_file() {
            read_toml(&migration.destination)?
        } else {
            toml::Value::Table(Default::default())
        };
        merge_missing(&mut destination, imported);
        mark_imported(&mut destination, &migration.source);
        if migration.destination.is_file() {
            let backup = backup_path(&migration.destination);
            fs::copy(&migration.destination, &backup).map_err(|source| MigrationError::Write {
                path: backup.clone(),
                source,
            })?;
            result.backups.push(backup);
        }
        atomic_write(
            &migration.destination,
            &toml::to_string_pretty(&destination).expect("mapped config serializes as TOML"),
        )?;
        result.changed.push(migration.destination.clone());
    }

    if let Some(state) = &plan.legacy_state {
        let destination_root = plan
            .global
            .as_ref()
            .and_then(|migration| migration.destination.parent())
            .map(Path::to_owned)
            .or_else(|| user_config_root().map(|root| root.join("workdeck")))
            .ok_or(MigrationError::MissingConfigDirectory)?;
        let destination = destination_root.join("legacy-hunk-state.json");
        if !destination.exists() {
            let source = fs::read(state).map_err(|source| MigrationError::Read {
                path: state.clone(),
                source,
            })?;
            atomic_write_bytes(&destination, &source)?;
            result.changed.push(destination);
        }
    }
    Ok(result)
}

fn inspect_config(source: &Path, destination: &Path) -> Result<ConfigMigration, MigrationError> {
    let source_value = read_toml(source)?;
    let imported = map_hunk_config(&source_value);
    let imported_keys = flatten_keys(&imported);
    let already_imported = destination
        .is_file()
        .then(|| read_toml(destination))
        .transpose()?
        .as_ref()
        .is_some_and(is_imported);
    Ok(ConfigMigration {
        source: source.to_owned(),
        destination: destination.to_owned(),
        destination_exists: destination.is_file(),
        already_imported,
        imported_keys,
    })
}

fn map_hunk_config(source: &toml::Value) -> toml::Value {
    let mut destination = toml::map::Map::new();
    let Some(table) = source.as_table() else {
        return toml::Value::Table(destination);
    };

    if let Some(theme) = table.get("theme").cloned() {
        destination.insert(
            "ui".into(),
            toml::Value::Table(toml::map::Map::from_iter([("theme".into(), theme)])),
        );
    }
    let review_keys = [
        "vcs",
        "mode",
        "watch",
        "exclude_untracked",
        "line_numbers",
        "tab_width",
        "file_gap",
        "hunk_gap",
        "wrap_lines",
        "hunk_headers",
        "sidebar",
        "agent_notes",
        "transparent_background",
        "cursor_line",
    ];
    let review = review_keys
        .into_iter()
        .filter_map(|key| table.get(key).cloned().map(|value| (key.into(), value)))
        .collect::<toml::map::Map<_, _>>();
    if !review.is_empty() {
        destination.insert("review".into(), toml::Value::Table(review));
    }
    for key in ["themes", "keybindings", "extensions", "extension"] {
        if let Some(value) = table.get(key).cloned() {
            destination.insert(format!("hunk_{key}"), value);
        }
    }
    destination.insert(
        "migration".into(),
        toml::Value::Table(toml::map::Map::from_iter([(
            "hunk".into(),
            toml::Value::Table(toml::map::Map::from_iter([(
                "imported".into(),
                toml::Value::Boolean(true),
            )])),
        )])),
    );
    toml::Value::Table(destination)
}

fn mark_imported(destination: &mut toml::Value, source: &Path) {
    let table = destination
        .as_table_mut()
        .expect("mapped config root is a table");
    let migration = table
        .entry("migration")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .expect("migration is a table");
    let hunk = migration
        .entry("hunk")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .expect("migration.hunk is a table");
    hunk.insert("imported".into(), toml::Value::Boolean(true));
    hunk.insert(
        "source".into(),
        toml::Value::String(source.display().to_string()),
    );
}

fn is_imported(destination: &toml::Value) -> bool {
    destination
        .get("migration")
        .and_then(|value| value.get("hunk"))
        .and_then(|value| value.get("imported"))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

fn merge_missing(destination: &mut toml::Value, source: toml::Value) {
    if let (toml::Value::Table(destination), toml::Value::Table(source)) = (destination, source) {
        for (key, value) in source {
            match destination.get_mut(&key) {
                Some(existing) => merge_missing(existing, value),
                None => {
                    destination.insert(key, value);
                }
            }
        }
    }
}

fn read_toml(path: &Path) -> Result<toml::Value, MigrationError> {
    let source = fs::read_to_string(path).map_err(|source| MigrationError::Read {
        path: path.to_owned(),
        source,
    })?;
    toml::from_str(&source).map_err(|source| MigrationError::Toml {
        path: path.to_owned(),
        source,
    })
}

fn discover_legacy_extensions(global: &Path, repo: &Path) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    for directory in [global.to_owned(), repo.join(".hunk/extensions")] {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                || path.extension().is_some_and(|extension| {
                    matches!(extension.to_str(), Some("ts" | "tsx" | "js" | "mjs"))
                })
            {
                paths.insert(path);
            }
        }
    }
    paths.into_iter().collect()
}

fn flatten_keys(value: &toml::Value) -> Vec<String> {
    fn walk(prefix: &str, value: &toml::Value, keys: &mut Vec<String>) {
        if let Some(table) = value.as_table() {
            for (key, value) in table {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if value.is_table() {
                    walk(&path, value, keys);
                } else {
                    keys.push(path);
                }
            }
        }
    }
    let mut keys = Vec::new();
    walk("", value, &mut keys);
    keys.sort();
    keys
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), MigrationError> {
    atomic_write_bytes(path, contents.as_bytes())
}

fn atomic_write_bytes(path: &Path, contents: &[u8]) -> Result<(), MigrationError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| MigrationError::Write {
        path: parent.to_owned(),
        source,
    })?;
    let temporary = path.with_extension("tmp");
    let mut file = fs::File::create(&temporary).map_err(|source| MigrationError::Write {
        path: temporary.clone(),
        source,
    })?;
    file.write_all(contents)
        .and_then(|_| file.sync_all())
        .map_err(|source| MigrationError::Write {
            path: temporary.clone(),
            source,
        })?;
    fs::rename(&temporary, path).map_err(|source| MigrationError::Write {
        path: path.to_owned(),
        source,
    })
}

fn backup_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    path.with_extension(format!("toml.bak.{stamp}"))
}

fn user_config_root() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|home| home.join(".config"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn translates_every_deprecated_semantic_role_into_textmate_selectors() {
        let syntax = BTreeMap::from([
            ("default".into(), "#000001".into()),
            ("keyword".into(), "#000002".into()),
            ("string".into(), "#000003".into()),
            ("comment".into(), "#000004".into()),
            ("number".into(), "#000005".into()),
            ("function".into(), "#000006".into()),
            ("property".into(), "#000007".into()),
            ("type".into(), "#000008".into()),
            ("variable".into(), "#000009".into()),
            ("operator".into(), "#00000a".into()),
            ("punctuation".into(), "#00000b".into()),
        ]);
        assert_eq!(
            legacy_syntax_colors_to_scopes(Some(&syntax)),
            Some(BTreeMap::from([
                ("source".into(), "#000001".into()),
                ("keyword".into(), "#000002".into()),
                ("string".into(), "#000003".into()),
                ("comment".into(), "#000004".into()),
                ("punctuation.definition.comment".into(), "#000004".into()),
                ("constant.numeric".into(), "#000005".into()),
                ("entity.name.function".into(), "#000006".into()),
                ("support.function".into(), "#000006".into()),
                ("variable.function".into(), "#000006".into()),
                ("variable.other.property".into(), "#000007".into()),
                ("support.variable.property".into(), "#000007".into()),
                ("entity.name.type".into(), "#000008".into()),
                ("entity.name.class".into(), "#000008".into()),
                ("support.type".into(), "#000008".into()),
                ("support.class".into(), "#000008".into()),
                ("variable".into(), "#000009".into()),
                ("keyword.operator".into(), "#00000a".into()),
                ("punctuation".into(), "#00000b".into()),
            ]))
        );
    }

    #[test]
    fn exact_scope_configuration_overrides_translated_compatibility_rules() {
        let syntax = BTreeMap::from([("comment".into(), "#111111".into())]);
        let exact = BTreeMap::from([
            ("comment".into(), "#222222".into()),
            ("comment.block".into(), "#333333".into()),
        ]);
        assert_eq!(
            resolve_syntax_scope_overrides(Some(&syntax), Some(&exact)),
            Some(BTreeMap::from([
                ("comment".into(), "#222222".into()),
                ("punctuation.definition.comment".into(), "#111111".into()),
                ("comment.block".into(), "#333333".into()),
            ]))
        );
    }

    #[test]
    fn plans_and_applies_idempotent_config_migration() {
        let directory = TempDir::new().unwrap();
        let config = directory.path().join("config");
        let repo = directory.path().join("repo");
        fs::create_dir_all(config.join("hunk/extensions/demo")).unwrap();
        fs::create_dir_all(repo.join(".hunk")).unwrap();
        fs::write(
            config.join("hunk/config.toml"),
            "theme = 'github-dark-default'\nmode = 'split'\nline_numbers = false\n[keybindings]\n\"hunk.app.quit\" = 'q'\n",
        )
        .unwrap();
        fs::write(repo.join(".hunk/config.toml"), "file_gap = 2\n").unwrap();
        let first = plan_with_roots(&repo, &config).unwrap();
        assert_eq!(first.legacy_extensions.len(), 1);
        let result = apply(&first).unwrap();
        assert_eq!(result.changed.len(), 2);
        let user_target = fs::read_to_string(config.join("workdeck/config.toml")).unwrap();
        assert!(user_target.contains("theme = \"github-dark-default\""));
        assert!(user_target.contains("mode = \"split\""));
        assert!(user_target.contains("imported = true"));
        let second = plan_with_roots(&repo, &config).unwrap();
        assert!(second.global.as_ref().unwrap().already_imported);
        assert!(second.repository.as_ref().unwrap().already_imported);
        assert!(apply(&second).unwrap().changed.is_empty());
    }

    #[test]
    fn existing_workdeck_values_win_over_imported_defaults() {
        let directory = TempDir::new().unwrap();
        let config = directory.path().join("config");
        let repo = directory.path().join("repo");
        fs::create_dir_all(config.join("hunk")).unwrap();
        fs::create_dir_all(config.join("workdeck")).unwrap();
        fs::create_dir_all(&repo).unwrap();
        fs::write(config.join("hunk/config.toml"), "theme = 'hunk-theme'\n").unwrap();
        fs::write(
            config.join("workdeck/config.toml"),
            "[ui]\ntheme = 'workdeck-theme'\n",
        )
        .unwrap();
        apply(&plan_with_roots(&repo, &config).unwrap()).unwrap();
        let target = fs::read_to_string(config.join("workdeck/config.toml")).unwrap();
        assert!(target.contains("theme = \"workdeck-theme\""));
        assert!(!target.contains("theme = \"hunk-theme\""));
    }
}
