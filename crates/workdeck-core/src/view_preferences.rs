//! Persistent review-view preferences shared by launch composition and the TUI.
//!
//! This is a clean-room Rust translation of the view-preference primitives in
//! Hunk's MIT-licensed `src/core/run/config.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Workdeck keeps the same ordered
//! TOML projection while using its own config path and product name.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{InputCursorLine, InputLayoutMode, resolve_global_config_path};

pub const VIEW_PREFERENCES_PROMPT_CONFIG_KEY: &str = "prompt_save_view_preferences";

/// The mutable view state offered for persistence when an interactive review exits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedViewPreferences {
    pub mode: InputLayoutMode,
    pub theme: Option<String>,
    pub show_line_numbers: bool,
    pub wrap_lines: bool,
    pub show_hunk_headers: bool,
    pub show_menu_bar: bool,
    pub show_agent_notes: bool,
    pub copy_decorations: bool,
    pub cursor_line: InputCursorLine,
}

impl Default for PersistedViewPreferences {
    fn default() -> Self {
        Self {
            mode: InputLayoutMode::Auto,
            theme: None,
            show_line_numbers: true,
            wrap_lines: false,
            show_hunk_headers: true,
            show_menu_bar: true,
            show_agent_notes: false,
            copy_decorations: false,
            cursor_line: InputCursorLine::Row,
        }
    }
}

/// One changed preference rendered and persisted in the canonical key order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPreferenceChange {
    pub config_key: &'static str,
    pub previous_value: String,
    pub next_value: String,
}

#[derive(Debug, Error)]
pub enum ViewPreferencePersistenceError {
    #[error("Could not resolve a config path because HOME/XDG_CONFIG_HOME is unset.")]
    MissingConfigPath,
    #[error("failed to create config directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read config at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write config at {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreferenceValue {
    String(String),
    Bool(bool),
}

impl PreferenceValue {
    fn serialize(&self) -> String {
        match self {
            Self::String(value) => serde_json::to_string(value)
                .expect("serializing one owned Rust string to JSON cannot fail"),
            Self::Bool(value) => value.to_string(),
        }
    }
}

fn mode_value(mode: InputLayoutMode) -> PreferenceValue {
    PreferenceValue::String(
        match mode {
            InputLayoutMode::Auto => "auto",
            InputLayoutMode::Split => "split",
            InputLayoutMode::Stack => "stack",
        }
        .into(),
    )
}

fn cursor_value(cursor: InputCursorLine) -> PreferenceValue {
    PreferenceValue::String(
        match cursor {
            InputCursorLine::Row => "row",
            InputCursorLine::Number => "number",
            InputCursorLine::Off => "off",
        }
        .into(),
    )
}

fn preference_values(
    preferences: &PersistedViewPreferences,
) -> [(&'static str, Option<PreferenceValue>); 9] {
    [
        (
            "theme",
            preferences
                .theme
                .as_ref()
                .map(|theme| PreferenceValue::String(theme.clone())),
        ),
        ("mode", Some(mode_value(preferences.mode))),
        (
            "line_numbers",
            Some(PreferenceValue::Bool(preferences.show_line_numbers)),
        ),
        (
            "wrap_lines",
            Some(PreferenceValue::Bool(preferences.wrap_lines)),
        ),
        (
            "hunk_headers",
            Some(PreferenceValue::Bool(preferences.show_hunk_headers)),
        ),
        (
            "menu_bar",
            Some(PreferenceValue::Bool(preferences.show_menu_bar)),
        ),
        (
            "agent_notes",
            Some(PreferenceValue::Bool(preferences.show_agent_notes)),
        ),
        (
            "copy_decorations",
            Some(PreferenceValue::Bool(preferences.copy_decorations)),
        ),
        ("cursor_line", Some(cursor_value(preferences.cursor_line))),
    ]
}

/// Diff two snapshots using the exact key order used for persistence.
#[must_use]
pub fn diff_persisted_view_preferences(
    previous: &PersistedViewPreferences,
    next: &PersistedViewPreferences,
) -> Vec<ViewPreferenceChange> {
    preference_values(previous)
        .into_iter()
        .zip(preference_values(next))
        .filter_map(|((config_key, previous), (next_key, next))| {
            debug_assert_eq!(config_key, next_key);
            (previous != next).then(|| ViewPreferenceChange {
                config_key,
                previous_value: previous.map_or_else(|| "unset".into(), |value| value.serialize()),
                next_value: next.map_or_else(|| "unset".into(), |value| value.serialize()),
            })
        })
        .collect()
}

fn upsert_top_level_toml_value(source: &str, key: &str, value: &PreferenceValue) -> String {
    let mut lines = if source.is_empty() {
        Vec::new()
    } else {
        source.split('\n').map(str::to_owned).collect::<Vec<_>>()
    };
    let serialized = value.serialize();
    let assignment = format!("{key} = {serialized}");
    let first_table = lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let assignment_key = |line: &str| {
        line.trim_start()
            .strip_prefix(key)
            .is_some_and(|suffix| suffix.trim_start().starts_with('='))
    };
    if let Some(index) = lines[..first_table]
        .iter()
        .position(|line| assignment_key(line))
    {
        lines[index] = assignment;
    } else {
        let mut insert_at = first_table;
        let has_table_spacer = insert_at > 0 && lines[insert_at - 1].is_empty();
        if has_table_spacer {
            insert_at -= 1;
        }
        lines.insert(insert_at, assignment);
        if !has_table_spacer && insert_at < lines.len().saturating_sub(1) {
            lines.insert(insert_at + 1, String::new());
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    format!("{}\n", lines.join("\n"))
}

fn writable_config_path(
    configured: Option<&Path>,
) -> Result<PathBuf, ViewPreferencePersistenceError> {
    configured
        .map(Path::to_owned)
        .or_else(resolve_global_config_path)
        .ok_or(ViewPreferencePersistenceError::MissingConfigPath)
}

fn read_config_source(path: &Path) -> Result<String, ViewPreferencePersistenceError> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(source),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(ViewPreferencePersistenceError::Read {
            path: path.to_owned(),
            source,
        }),
    }
}

fn write_config_source(path: &Path, source: &str) -> Result<(), ViewPreferencePersistenceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            ViewPreferencePersistenceError::CreateDirectory {
                path: parent.to_owned(),
                source,
            }
        })?;
    }
    fs::write(path, source).map_err(|source| ViewPreferencePersistenceError::Write {
        path: path.to_owned(),
        source,
    })
}

/// Persist all defined view preferences while preserving unrelated top-level text and tables.
pub fn save_global_view_preferences(
    preferences: &PersistedViewPreferences,
    configured_path: Option<&Path>,
) -> Result<PathBuf, ViewPreferencePersistenceError> {
    let path = writable_config_path(configured_path)?;
    let mut source = read_config_source(&path)?;
    for (key, value) in preference_values(preferences) {
        if let Some(value) = value {
            source = upsert_top_level_toml_value(&source, key, &value);
        }
    }
    write_config_source(&path, &source)?;
    Ok(path)
}

/// Persist only the future-prompt policy, leaving all view settings untouched.
pub fn save_view_preferences_prompt_preference(
    prompt: bool,
    configured_path: Option<&Path>,
) -> Result<PathBuf, ViewPreferencePersistenceError> {
    let path = writable_config_path(configured_path)?;
    let source = upsert_top_level_toml_value(
        &read_config_source(&path)?,
        VIEW_PREFERENCES_PROMPT_CONFIG_KEY,
        &PreferenceValue::Bool(prompt),
    );
    write_config_source(&path, &source)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn diff_uses_persistence_order_and_toml_values() {
        let previous = PersistedViewPreferences {
            theme: Some("github-dark-default".into()),
            ..PersistedViewPreferences::default()
        };
        let next = PersistedViewPreferences {
            theme: Some("github-dark-dimmed".into()),
            show_line_numbers: false,
            wrap_lines: true,
            ..previous.clone()
        };
        assert_eq!(
            diff_persisted_view_preferences(&previous, &next),
            [
                ViewPreferenceChange {
                    config_key: "theme",
                    previous_value: "\"github-dark-default\"".into(),
                    next_value: "\"github-dark-dimmed\"".into(),
                },
                ViewPreferenceChange {
                    config_key: "line_numbers",
                    previous_value: "true".into(),
                    next_value: "false".into(),
                },
                ViewPreferenceChange {
                    config_key: "wrap_lines",
                    previous_value: "false".into(),
                    next_value: "true".into(),
                },
            ]
        );
    }

    #[test]
    fn save_preserves_comments_and_tables_and_replaces_only_top_level_values() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(
            &path,
            "# keep me\ntheme = \"old\"\n\n[review]\nwrap_lines = false\n",
        )
        .unwrap();
        let preferences = PersistedViewPreferences {
            theme: Some("new".into()),
            wrap_lines: true,
            ..PersistedViewPreferences::default()
        };
        assert_eq!(
            save_global_view_preferences(&preferences, Some(&path)).unwrap(),
            path
        );
        let source = fs::read_to_string(&path).unwrap();
        assert!(source.contains("# keep me"));
        assert!(source.contains("theme = \"new\""));
        assert!(source.contains("wrap_lines = true"));
        assert!(source.contains("cursor_line = \"row\"\n\n[review]"));
        assert!(source.contains("[review]\nwrap_lines = false"));
    }

    #[test]
    fn prompt_save_changes_only_the_policy_key() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "# keep me\n").unwrap();
        save_view_preferences_prompt_preference(false, Some(&path)).unwrap();
        let source = fs::read_to_string(path).unwrap();
        assert!(source.contains("# keep me"));
        assert!(source.contains("prompt_save_view_preferences = false"));
        assert!(!source.contains("theme ="));
    }
}
