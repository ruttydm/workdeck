use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub paths: PathConfig,
    #[serde(default)]
    pub keys: KeyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub preview: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            preview: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

impl Default for PathConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyConfig {
    #[serde(default = "key_quit")]
    pub quit: String,
    #[serde(default = "key_refresh")]
    pub refresh: String,
    #[serde(default = "key_search")]
    pub search: String,
    #[serde(default = "key_help")]
    pub help: String,
    #[serde(default = "key_changes")]
    pub changes: String,
    #[serde(default = "key_files")]
    pub files: String,
    #[serde(default = "key_tasks")]
    pub tasks: String,
    #[serde(default = "key_agents")]
    pub agents: String,
    #[serde(default = "key_toggle_preview")]
    pub toggle_preview: String,
    #[serde(default = "key_group_changes")]
    pub group_changes: String,
    #[serde(default = "key_toggle_dirstat")]
    pub toggle_dirstat: String,
    #[serde(default = "key_open_editor")]
    pub open_editor: String,
    #[serde(default = "key_copy")]
    pub copy: String,
    #[serde(default = "key_new_issue")]
    pub new_issue: String,
    #[serde(default = "key_edit_issue")]
    pub edit_issue: String,
    #[serde(default = "key_status")]
    pub status: String,
    #[serde(default = "key_priority")]
    pub priority: String,
    #[serde(default = "key_labels")]
    pub labels: String,
    #[serde(default = "key_assign")]
    pub assign: String,
    #[serde(default = "key_jump")]
    pub jump: String,
    #[serde(default = "key_link_file")]
    pub link_file: String,
}

impl Default for KeyConfig {
    fn default() -> Self {
        Self {
            quit: key_quit(),
            refresh: key_refresh(),
            search: key_search(),
            help: key_help(),
            changes: key_changes(),
            files: key_files(),
            tasks: key_tasks(),
            agents: key_agents(),
            toggle_preview: key_toggle_preview(),
            group_changes: key_group_changes(),
            toggle_dirstat: key_toggle_dirstat(),
            open_editor: key_open_editor(),
            copy: key_copy(),
            new_issue: key_new_issue(),
            edit_issue: key_edit_issue(),
            status: key_status(),
            priority: key_priority(),
            labels: key_labels(),
            assign: key_assign(),
            jump: key_jump(),
            link_file: key_link_file(),
        }
    }
}

impl Config {
    pub fn load(repo_root: &Path) -> Result<Self> {
        Self::load_from_paths(
            &repo_root.join(default_data_dir()).join("config.toml"),
            user_config_path().as_deref(),
        )
    }

    pub fn load_from_paths(
        repo_config_path: &Path,
        user_config_path: Option<&Path>,
    ) -> Result<Self> {
        let mut merged = toml::Value::Table(Default::default());

        if let Some(path) = user_config_path.filter(|path| path.exists()) {
            let user = read_config_value(path)?;
            merge_toml_values(&mut merged, user);
        }

        if repo_config_path.exists() {
            let repo = read_config_value(repo_config_path)?;
            merge_toml_values(&mut merged, repo);
        }

        let config: Self = merged
            .try_into()
            .with_context(|| "failed to parse merged config")?;
        config
            .validate()
            .with_context(|| "invalid Workdeck config")?;
        Ok(config)
    }

    pub fn data_dir(&self, repo_root: &Path) -> PathBuf {
        if self.paths.data_dir.is_absolute() {
            self.paths.data_dir.clone()
        } else {
            repo_root.join(&self.paths.data_dir)
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.paths.data_dir.as_os_str().is_empty() {
            bail!("paths.data_dir cannot be empty");
        }
        self.keys.validate()
    }
}

fn read_config_value(path: &Path) -> Result<toml::Value> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn merge_toml_values(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge_toml_values(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn user_config_path() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".config/workdeck/config.toml"))
}

impl KeyConfig {
    pub fn validate(&self) -> Result<()> {
        let bindings = [
            ("quit", &self.quit),
            ("refresh", &self.refresh),
            ("search", &self.search),
            ("help", &self.help),
            ("changes", &self.changes),
            ("files", &self.files),
            ("tasks", &self.tasks),
            ("agents", &self.agents),
            ("toggle_preview", &self.toggle_preview),
            ("group_changes", &self.group_changes),
            ("toggle_dirstat", &self.toggle_dirstat),
            ("open_editor", &self.open_editor),
            ("copy", &self.copy),
            ("new_issue", &self.new_issue),
            ("edit_issue", &self.edit_issue),
            ("status", &self.status),
            ("priority", &self.priority),
            ("labels", &self.labels),
            ("assign", &self.assign),
            ("jump", &self.jump),
            ("link_file", &self.link_file),
        ];

        let mut seen = BTreeMap::<String, &str>::new();
        for (name, binding) in bindings {
            let normalized = normalize_key(binding)
                .with_context(|| format!("invalid key binding keys.{name} = {binding:?}"))?;
            if let Some(existing) = seen.insert(normalized.clone(), name) {
                bail!("duplicate key binding {normalized:?} for keys.{existing} and keys.{name}");
            }
        }
        Ok(())
    }
}

pub fn normalize_key(binding: &str) -> Result<String> {
    let value = binding.trim();
    if value.is_empty() {
        bail!("key binding cannot be empty");
    }
    let lower = value.to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "tab" => "tab".to_string(),
        "shift-tab" | "backtab" => "shift-tab".to_string(),
        "enter" => "enter".to_string(),
        "esc" | "escape" => "esc".to_string(),
        "space" => "space".to_string(),
        _ => {
            if value.chars().count() == 1 {
                value.to_string()
            } else {
                bail!("supported named keys are tab, shift-tab, enter, esc, and space");
            }
        }
    };
    Ok(normalized)
}

pub fn resolve_repo_data_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(default_data_dir())
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "auto".to_string()
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(".agents/workdeck")
}

fn key_quit() -> String {
    "q".to_string()
}

fn key_refresh() -> String {
    "r".to_string()
}

fn key_search() -> String {
    "/".to_string()
}

fn key_help() -> String {
    "?".to_string()
}

fn key_changes() -> String {
    "c".to_string()
}

fn key_files() -> String {
    "f".to_string()
}

fn key_tasks() -> String {
    "i".to_string()
}

fn key_agents() -> String {
    "a".to_string()
}

fn key_toggle_preview() -> String {
    "t".to_string()
}

fn key_group_changes() -> String {
    "g".to_string()
}

fn key_toggle_dirstat() -> String {
    "w".to_string()
}

fn key_open_editor() -> String {
    "o".to_string()
}

fn key_copy() -> String {
    "y".to_string()
}

fn key_new_issue() -> String {
    "n".to_string()
}

fn key_edit_issue() -> String {
    "e".to_string()
}

fn key_status() -> String {
    "s".to_string()
}

fn key_priority() -> String {
    "p".to_string()
}

fn key_labels() -> String {
    "l".to_string()
}

fn key_assign() -> String {
    "A".to_string()
}

fn key_jump() -> String {
    "space".to_string()
}

fn key_link_file() -> String {
    "L".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_agents_workdeck() {
        let root = Path::new("/tmp/repo");
        assert_eq!(
            Config::default().data_dir(root),
            PathBuf::from("/tmp/repo/.agents/workdeck")
        );
    }

    #[test]
    fn partial_config_keeps_default_keys() {
        let config: Config = toml::from_str(
            r#"
            [ui]
            preview = false
            "#,
        )
        .unwrap();

        assert!(!config.ui.preview);
        assert_eq!(config.ui.theme, "auto");
        assert_eq!(config.keys.quit, "q");
        assert_eq!(config.paths.data_dir, PathBuf::from(".agents/workdeck"));
    }

    #[test]
    fn repo_config_overrides_user_config_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(
            &user_config,
            r#"
            [ui]
            preview = false
            [keys]
            quit = "x"
            "#,
        )
        .unwrap();
        fs::write(
            &repo_config,
            r#"
            [ui]
            preview = true

            [keys]
            files = "F"
            "#,
        )
        .unwrap();

        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();

        assert!(config.ui.preview);
        assert_eq!(config.keys.quit, "x");
        assert_eq!(config.keys.files, "F");
    }

    #[test]
    fn user_config_loads_when_repo_config_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("missing-repo-config.toml");
        fs::write(
            &user_config,
            r#"
            [paths]
            data_dir = ".workdeck"
            "#,
        )
        .unwrap();

        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();

        assert_eq!(config.paths.data_dir, PathBuf::from(".workdeck"));
    }

    #[test]
    fn validates_default_config() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn rejects_duplicate_keybindings() {
        let mut config = Config::default();
        config.keys.files = config.keys.changes.clone();

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("duplicate key binding"));
    }

    #[test]
    fn rejects_invalid_keybindings() {
        let mut config = Config::default();
        config.keys.quit = "ctrl-q".to_string();

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("invalid key binding"));
    }

    #[test]
    fn normalizes_named_keybindings() {
        assert_eq!(normalize_key("Esc").unwrap(), "esc");
        assert_eq!(normalize_key("BackTab").unwrap(), "shift-tab");
        assert_eq!(normalize_key("L").unwrap(), "L");
    }
}
