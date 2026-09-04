use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use workdeck_core::StartupNotice;
use workdeck_diff::sanitize_terminal_line;
use workdeck_tui::{UserKeyBinding, UserKeyBindingEntry};

/// Resolved user-extension configuration for one Workdeck invocation.
///
/// Bundled VCS adapters are not controlled by this switch. `paths` come from the
/// trusted user layer, while `repo_paths` retain repository provenance for the
/// native-extension trust gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionsConfig {
    pub enabled: bool,
    pub paths: Vec<PathBuf>,
    pub repo_paths: Vec<PathBuf>,
    pub extension_configs: BTreeMap<String, serde_json::Value>,
}

impl Default for ExtensionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            paths: Vec::new(),
            repo_paths: Vec::new(),
            extension_configs: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub paths: PathConfig,
    #[serde(default)]
    pub git: GitConfig,
    #[serde(default)]
    pub refresh: RefreshConfig,
    #[serde(default)]
    pub review: ReviewConfig,
    #[serde(default)]
    pub keys: KeyConfig,
    /// Merged user/repository settings exposed only to the named native extension.
    #[serde(default)]
    pub extension: BTreeMap<String, serde_json::Value>,
    /// Resolved `[extensions]` and `[extension.<id>]` state for this invocation.
    #[serde(skip)]
    pub resolved_extensions: ExtensionsConfig,
    /// Config-derived notices that must reach the review footer.
    #[serde(skip)]
    pub startup_notices: Vec<StartupNotice>,
    /// Hunk-compatible command bindings from the global user layer only.
    #[serde(skip)]
    pub keybindings: Vec<UserKeyBindingEntry>,
    /// Unsupported values ignored while reading `[keybindings]`.
    #[serde(skip)]
    pub keybinding_notices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewConfig {
    #[serde(default = "default_vcs")]
    pub vcs: String,
    #[serde(default = "default_review_mode")]
    pub mode: String,
    #[serde(default)]
    pub watch: bool,
    #[serde(default)]
    pub exclude_untracked: bool,
    #[serde(default = "default_true")]
    pub line_numbers: bool,
    #[serde(default = "default_tab_width")]
    pub tab_width: u16,
    #[serde(default = "default_file_gap")]
    pub file_gap: u16,
    #[serde(default)]
    pub hunk_gap: u16,
    #[serde(default)]
    pub wrap_lines: bool,
    #[serde(default = "default_true")]
    pub hunk_headers: bool,
    #[serde(default)]
    pub sidebar: ReviewSidebar,
    #[serde(default)]
    pub agent_notes: bool,
    #[serde(default)]
    pub transparent_background: bool,
    #[serde(default)]
    pub color_moved: Option<bool>,
    #[serde(default = "default_cursor_line")]
    pub cursor_line: String,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            vcs: default_vcs(),
            mode: default_review_mode(),
            watch: false,
            exclude_untracked: false,
            line_numbers: true,
            tab_width: default_tab_width(),
            file_gap: default_file_gap(),
            hunk_gap: 0,
            wrap_lines: false,
            hunk_headers: true,
            sidebar: ReviewSidebar::Auto,
            agent_notes: false,
            transparent_background: false,
            color_moved: None,
            cursor_line: default_cursor_line(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReviewSidebar {
    #[default]
    Auto,
    Show,
    Hide,
}

impl Serialize for ReviewSidebar {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Show => serializer.serialize_bool(true),
            Self::Hide => serializer.serialize_bool(false),
        }
    }
}

impl<'de> Deserialize<'de> for ReviewSidebar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Boolean(bool),
            Text(String),
        }
        match Value::deserialize(deserializer)? {
            Value::Boolean(true) => Ok(Self::Show),
            Value::Boolean(false) => Ok(Self::Hide),
            Value::Text(value) if value == "auto" => Ok(Self::Auto),
            Value::Text(value) => Err(serde::de::Error::custom(format!(
                "review.sidebar must be auto, true, or false; got {value:?}"
            ))),
        }
    }
}

impl ReviewSidebar {
    pub fn is_visible(self) -> bool {
        self != Self::Hide
    }
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
pub struct GitConfig {
    #[serde(default)]
    pub base_branch: String,
    #[serde(default = "default_recent_commits")]
    pub recent_commits: usize,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            base_branch: String::new(),
            recent_commits: default_recent_commits(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshConfig {
    #[serde(default = "default_true")]
    pub auto: bool,
    #[serde(default = "default_refresh_interval_ms")]
    pub interval_ms: u64,
    #[serde(default = "default_refresh_debounce_ms")]
    pub debounce_ms: u64,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            auto: true,
            interval_ms: default_refresh_interval_ms(),
            debounce_ms: default_refresh_debounce_ms(),
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
    #[serde(default = "key_git")]
    pub git: String,
    #[serde(default = "key_files")]
    pub files: String,
    #[serde(default = "key_issues", alias = "tasks")]
    pub issues: String,
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
    #[serde(default = "key_base")]
    pub base: String,
    #[serde(default = "key_pull_requests")]
    pub pull_requests: String,
}

impl Default for KeyConfig {
    fn default() -> Self {
        Self {
            quit: key_quit(),
            refresh: key_refresh(),
            search: key_search(),
            help: key_help(),
            changes: key_changes(),
            git: key_git(),
            files: key_files(),
            issues: key_issues(),
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
            base: key_base(),
            pull_requests: key_pull_requests(),
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
        let mut keybindings = Vec::new();
        let mut keybinding_notices = Vec::new();
        let mut user_extensions = ExtensionsLayer::default();
        let mut repo_extensions = ExtensionsLayer::default();

        if let Some(path) = user_config_path.filter(|path| path.exists()) {
            (keybindings, keybinding_notices) = read_user_keybindings(path)?;
            let user = read_config_value(path)?;
            user_extensions = read_extensions_layer(&user)?;
            merge_toml_values(&mut merged, user);
        }

        if repo_config_path.exists() {
            let repo = read_config_value(repo_config_path)?;
            repo_extensions = read_extensions_layer(&repo)?;
            merge_toml_values(&mut merged, repo);
        }

        // `[extension.<id>]` is special: repository tables override user tables one
        // property at a time, but opaque nested extension values are not recursively
        // interpreted by Workdeck.
        let extension_configs = merge_extension_configs(
            &user_extensions.extension_configs,
            &repo_extensions.extension_configs,
        );
        merged
            .as_table_mut()
            .expect("a TOML document root is always a table")
            .insert("extension".into(), toml::Value::Table(extension_configs));

        let mut config: Self = merged
            .try_into()
            .with_context(|| "failed to parse merged config")?;
        config.keybindings = keybindings;
        config.keybinding_notices = keybinding_notices;
        config.resolved_extensions = ExtensionsConfig {
            enabled: repo_extensions
                .enabled
                .or(user_extensions.enabled)
                .unwrap_or(true),
            paths: user_extensions.paths,
            repo_paths: repo_extensions.paths,
            extension_configs: config.extension.clone(),
        };
        config.startup_notices = repo_extension_config_notice(&repo_extensions.extension_configs)
            .into_iter()
            .collect();
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

    pub fn extension_config(&self, id: &str) -> serde_json::Value {
        self.extension_configs()
            .get(id)
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(Default::default()))
    }

    /// Per-extension tables after layer resolution, with a fallback for callers
    /// that deserialize one standalone `Config` value directly.
    pub fn extension_configs(&self) -> &BTreeMap<String, serde_json::Value> {
        if self.resolved_extensions.extension_configs.is_empty() && !self.extension.is_empty() {
            &self.extension
        } else {
            &self.resolved_extensions.extension_configs
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.paths.data_dir.as_os_str().is_empty() {
            bail!("paths.data_dir cannot be empty");
        }
        if self.refresh.interval_ms == 0 {
            bail!("refresh.interval_ms must be greater than 0");
        }
        if !matches!(self.review.mode.as_str(), "auto" | "split" | "stack") {
            bail!("review.mode must be auto, split, or stack");
        }
        // Extension-owned backends are not known until after configuration has loaded. Preserve
        // any non-empty id here and reconcile it against the composed session catalog later.
        if self.review.vcs.trim().is_empty() {
            bail!("review.vcs cannot be empty");
        }
        if !matches!(self.review.cursor_line.as_str(), "row" | "number" | "off") {
            bail!("review.cursor_line must be row, number, or off");
        }
        if !(1..=16).contains(&self.review.tab_width) {
            bail!("review.tab_width must be between 1 and 16");
        }
        if self.review.file_gap > 8 {
            bail!("review.file_gap must be between 0 and 8");
        }
        if self.review.hunk_gap > 8 {
            bail!("review.hunk_gap must be between 0 and 8");
        }
        self.keys.validate()
    }
}

#[derive(Debug, Clone, Default)]
struct ExtensionsLayer {
    enabled: Option<bool>,
    paths: Vec<PathBuf>,
    extension_configs: toml::map::Map<String, toml::Value>,
}

fn read_extensions_layer(value: &toml::Value) -> Result<ExtensionsLayer> {
    let root = value
        .as_table()
        .context("Expected Workdeck config to contain a TOML table.")?;
    let extensions = match root.get("extensions") {
        Some(value) => Some(
            value
                .as_table()
                .context("Expected extensions to contain a TOML table.")?,
        ),
        None => None,
    };
    let paths = extensions
        .and_then(|table| table.get("paths"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect();

    let mut extension_configs = toml::map::Map::new();
    if let Some(value) = root.get("extension") {
        let tables = value
            .as_table()
            .context("Expected extension to contain per-extension TOML tables.")?;
        for (id, value) in tables {
            if !value.is_table() {
                bail!("Expected [extension.{id}] to contain a TOML table.");
            }
            extension_configs.insert(id.clone(), value.clone());
        }
    }

    Ok(ExtensionsLayer {
        enabled: extensions
            .and_then(|table| table.get("enabled"))
            .and_then(toml::Value::as_bool),
        paths,
        extension_configs,
    })
}

fn merge_extension_configs(
    base: &toml::map::Map<String, toml::Value>,
    overrides: &toml::map::Map<String, toml::Value>,
) -> toml::map::Map<String, toml::Value> {
    let mut merged = base.clone();
    for (id, override_value) in overrides {
        let override_table = override_value
            .as_table()
            .expect("extension layers were validated as tables");
        let target = merged
            .entry(id.clone())
            .or_insert_with(|| toml::Value::Table(Default::default()));
        let target = target
            .as_table_mut()
            .expect("extension layers were validated as tables");
        for (key, value) in override_table {
            target.insert(key.clone(), value.clone());
        }
    }
    merged
}

fn repo_extension_config_notice(
    extension_configs: &toml::map::Map<String, toml::Value>,
) -> Option<StartupNotice> {
    let ids = extension_configs
        .iter()
        .filter_map(|(id, value)| (!value.as_table()?.is_empty()).then_some(id.as_str()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return None;
    }
    let listed = sanitize_terminal_line(&ids.join(", "));
    Some(StartupNotice::new(
        format!("extension:repo-config:{listed}"),
        format!("Repo config overrides settings for extension(s): {listed}"),
    ))
}

fn read_user_keybindings(path: &Path) -> Result<(Vec<UserKeyBindingEntry>, Vec<String>)> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    let document = raw
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let Some(item) = document.get("keybindings") else {
        return Ok((Vec::new(), Vec::new()));
    };
    let Some(table) = item.as_table_like() else {
        bail!("Expected keybindings to contain a TOML table.");
    };
    let mut bindings = Vec::new();
    let mut unusable = Vec::new();
    for (command_id, item) in table.iter() {
        let Some(value) = item.as_value() else {
            unusable.push(command_id.to_owned());
            continue;
        };
        let binding = if let Some(chord) = value.as_str() {
            Some(UserKeyBinding::Chord(chord.into()))
        } else if value.as_bool() == Some(false) {
            Some(UserKeyBinding::Disabled)
        } else if let Some(chords) = value.as_array() {
            chords
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .map(UserKeyBinding::Chords)
        } else {
            None
        };
        match binding {
            Some(binding) => bindings.push(UserKeyBindingEntry::new(command_id, binding)),
            None => unusable.push(command_id.to_owned()),
        }
    }
    let notices = if unusable.is_empty() {
        Vec::new()
    } else {
        unusable.sort();
        let listed = sanitize_terminal_line(&unusable.join(", "));
        vec![format!(
            "Ignored [keybindings] entries with unsupported values: {listed}. Use a chord string, a list of chords, or false to unbind."
        )]
    };
    Ok((bindings, notices))
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
    workdeck_core::resolve_global_config_path()
}

impl KeyConfig {
    pub fn validate(&self) -> Result<()> {
        let bindings = [
            ("quit", &self.quit),
            ("refresh", &self.refresh),
            ("search", &self.search),
            ("help", &self.help),
            ("changes", &self.changes),
            ("git", &self.git),
            ("files", &self.files),
            ("issues", &self.issues),
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
            ("base", &self.base),
        ];

        let mut seen = BTreeMap::<String, &str>::new();
        for (name, binding) in bindings {
            let normalized = normalize_key(binding)
                .with_context(|| format!("invalid key binding keys.{name} = {binding:?}"))?;
            if let Some(existing) = seen.insert(normalized.clone(), name) {
                bail!("duplicate key binding {normalized:?} for keys.{existing} and keys.{name}");
            }
        }
        normalize_key(&self.pull_requests)
            .with_context(|| "invalid key binding keys.pull_requests".to_string())?;
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

fn default_review_mode() -> String {
    "auto".to_string()
}

fn default_vcs() -> String {
    "auto".to_string()
}

fn default_cursor_line() -> String {
    "row".to_string()
}

fn default_tab_width() -> u16 {
    4
}

fn default_file_gap() -> u16 {
    1
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(".agents/workdeck")
}

fn default_recent_commits() -> usize {
    30
}

fn default_refresh_interval_ms() -> u64 {
    1500
}

fn default_refresh_debounce_ms() -> u64 {
    250
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

fn key_git() -> String {
    "G".to_string()
}

fn key_files() -> String {
    "f".to_string()
}

fn key_issues() -> String {
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

fn key_base() -> String {
    "b".to_string()
}

fn key_pull_requests() -> String {
    "p".to_string()
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
        assert_eq!(config.keys.git, "G");
        assert_eq!(config.git.recent_commits, 30);
        assert!(config.refresh.auto);
        assert_eq!(config.refresh.interval_ms, 1500);
        assert_eq!(config.refresh.debounce_ms, 250);
        assert_eq!(config.paths.data_dir, PathBuf::from(".agents/workdeck"));
        assert_eq!(config.review.mode, "auto");
        assert_eq!(config.review.tab_width, 4);
        assert!(config.review.line_numbers);
    }

    #[test]
    fn legacy_tasks_keybinding_alias_still_loads_as_issues() {
        let config: Config = toml::from_str(
            r#"
            [keys]
            tasks = "I"
            "#,
        )
        .unwrap();

        assert_eq!(config.keys.issues, "I");
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
    fn extension_configuration_merges_top_level_keys_without_interpreting_nested_values() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(
            &user_config,
            r#"
            [extension."example.review"]
            threshold = 2
            source = "user"
            [extension."example.review".nested]
            keep = true
            replace = "user"
            "#,
        )
        .unwrap();
        fs::write(
            &repo_config,
            r#"
            [extension."example.review"]
            source = "repo"
            [extension."example.review".nested]
            replace = "repo"
            "#,
        )
        .unwrap();

        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(
            config.extension_config("example.review"),
            serde_json::json!({
                "threshold": 2,
                "source": "repo",
                "nested": { "replace": "repo" }
            })
        );
        assert_eq!(config.extension_config("missing"), serde_json::json!({}));
        assert_eq!(
            config.startup_notices,
            [StartupNotice::new(
                "extension:repo-config:example.review",
                "Repo config overrides settings for extension(s): example.review",
            )]
        );
    }

    #[test]
    fn standalone_deserialization_keeps_extension_config_access() {
        let config: Config = toml::from_str("[extension.tools]\nthreshold = 3\n").unwrap();
        assert_eq!(
            config.extension_config("tools"),
            serde_json::json!({ "threshold": 3 })
        );
    }

    #[test]
    fn extension_discovery_paths_retain_user_and_repository_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(
            &user_config,
            "[extensions]\npaths = ['~/trusted', './user-relative']\n",
        )
        .unwrap();
        fs::write(&repo_config, "[extensions]\npaths = ['./repo-relative']\n").unwrap();
        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(
            config.resolved_extensions.paths,
            [PathBuf::from("~/trusted"), PathBuf::from("./user-relative")]
        );
        assert_eq!(
            config.resolved_extensions.repo_paths,
            [PathBuf::from("./repo-relative")]
        );
        assert!(config.resolved_extensions.enabled);
    }

    #[test]
    fn extension_discovery_paths_ignore_empty_and_non_string_entries() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        fs::write(&user_config, "[extensions]\npaths = ['./ok', 3, '']\n").unwrap();
        let config =
            Config::load_from_paths(&dir.path().join("missing-repo.toml"), Some(&user_config))
                .unwrap();
        assert_eq!(config.resolved_extensions.paths, [PathBuf::from("./ok")]);
    }

    #[test]
    fn repository_extension_enablement_overrides_user_and_defaults_true() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(&user_config, "[extensions]\nenabled = true\n").unwrap();
        fs::write(&repo_config, "[extensions]\nenabled = false\n").unwrap();

        let disabled = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(!disabled.resolved_extensions.enabled);

        fs::write(&repo_config, "[extensions]\nenabled = true\n").unwrap();
        let enabled = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(enabled.resolved_extensions.enabled);
        assert!(Config::default().resolved_extensions.enabled);
    }

    #[test]
    fn empty_repo_extension_table_does_not_emit_an_override_notice() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(&user_config, "[extension.blame]\nmax_age_days = 30\n").unwrap();
        fs::write(&repo_config, "[extension.blame]\n").unwrap();

        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(config.startup_notices.is_empty());
        assert_eq!(
            config.extension_config("blame"),
            serde_json::json!({ "max_age_days": 30 })
        );
    }

    #[test]
    fn repository_extension_notice_lists_every_nonempty_id_in_sorted_order() {
        let dir = tempfile::tempdir().unwrap();
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(
            &repo_config,
            "[extension.zebra]\nbinary = '/tmp/zebra'\n[extension.alpha]\non = true\n",
        )
        .unwrap();

        let config = Config::load_from_paths(&repo_config, None).unwrap();
        assert_eq!(
            config.startup_notices,
            [StartupNotice::new(
                "extension:repo-config:alpha, zebra",
                "Repo config overrides settings for extension(s): alpha, zebra",
            )]
        );
    }

    #[test]
    fn extension_resolution_does_not_validate_review_theme_selection() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(
            &user_config,
            "[ui]\ntheme = 'missing-custom-theme'\n[extensions]\npaths = ['/user/tools']\n[extension.tools]\ntoken = 'user'\n",
        )
        .unwrap();
        fs::write(
            &repo_config,
            "[extensions]\nenabled = true\npaths = ['./repo-tools']\n[extension.tools]\ntoken = 'repo'\n",
        )
        .unwrap();

        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(config.ui.theme, "missing-custom-theme");
        assert!(config.resolved_extensions.enabled);
        assert_eq!(
            config.resolved_extensions.paths,
            [PathBuf::from("/user/tools")]
        );
        assert_eq!(
            config.resolved_extensions.repo_paths,
            [PathBuf::from("./repo-tools")]
        );
        assert_eq!(
            config.extension_config("tools"),
            serde_json::json!({ "token": "repo" })
        );
    }

    #[test]
    fn malformed_extension_sections_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("missing-repo.toml");

        for (source, expected) in [
            ("extensions = true\n", "extensions to contain a TOML table"),
            (
                "extension = 'copy-as'\n",
                "extension to contain per-extension TOML tables",
            ),
            (
                "[extension]\ncopy-as = 1\n",
                "[extension.copy-as] to contain a TOML table",
            ),
        ] {
            fs::write(&user_config, source).unwrap();
            let error = Config::load_from_paths(&repo_config, Some(&user_config))
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error:?}");
        }
    }

    fn extension_projection(config: &Config) -> serde_json::Value {
        serde_json::json!({
            "enabled": config.resolved_extensions.enabled,
            "paths": config
                .resolved_extensions
                .paths
                .iter()
                .map(|path| path.to_string_lossy())
                .collect::<Vec<_>>(),
            "repoPaths": config
                .resolved_extensions
                .repo_paths
                .iter()
                .map(|path| path.to_string_lossy())
                .collect::<Vec<_>>(),
            "extensionConfigs": config.resolved_extensions.extension_configs,
        })
    }

    #[test]
    fn native_extension_config_resolution_matches_both_pinned_hunk_oracles() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/config-extensions.json"
        ))
        .unwrap();
        let expected = &oracle["expected"];
        assert_eq!(
            extension_projection(&Config::default()),
            expected["default"]
        );

        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(
            &user_config,
            "[extensions]\npaths = ['~/dev/copy-as.ts', 7, '']\nunknown_key = true\n",
        )
        .unwrap();
        fs::write(
            &repo_config,
            "[extensions]\npaths = ['./tools/policy.ts']\n",
        )
        .unwrap();
        let paths = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(extension_projection(&paths), expected["paths"]);

        fs::write(&user_config, "[extensions]\nenabled = true\n").unwrap();
        fs::write(&repo_config, "[extensions]\nenabled = false\n").unwrap();
        let repo_false = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        fs::write(&repo_config, "[extensions]\nenabled = true\n").unwrap();
        let repo_true = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(
            serde_json::json!({
                "repoFalse": repo_false.resolved_extensions.enabled,
                "repoTrue": repo_true.resolved_extensions.enabled,
            }),
            serde_json::json!({
                "repoFalse": expected["precedence"]["repoFalse"],
                "repoTrue": expected["precedence"]["repoTrue"],
            })
        );

        fs::write(
            &user_config,
            concat!(
                "[extension.copy-as]\n",
                "severity = 'nit'\n",
                "wrap = true\n",
                "[extension.copy-as.nested]\n",
                "keep = true\n",
                "replace = 'user'\n",
                "[extension.blame]\n",
                "max_age_days = 30\n",
            ),
        )
        .unwrap();
        fs::write(
            &repo_config,
            concat!(
                "[extension.copy-as]\n",
                "severity = 'blocking'\n",
                "[extension.copy-as.nested]\n",
                "replace = 'repo'\n",
            ),
        )
        .unwrap();
        let configs = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(
            serde_json::to_value(&configs.resolved_extensions.extension_configs).unwrap(),
            expected["configs"]
        );
        assert_eq!(
            serde_json::to_value(&configs.startup_notices).unwrap(),
            expected["notices"]
        );

        let mut malformed = Vec::new();
        for source in [
            "extensions = true\n",
            "extension = 'copy-as'\n",
            "[extension]\ncopy-as = 1\n",
        ] {
            fs::write(&user_config, source).unwrap();
            malformed.push(
                Config::load_from_paths(&dir.path().join("missing.toml"), Some(&user_config))
                    .unwrap_err()
                    .to_string(),
            );
        }
        assert_eq!(serde_json::json!(malformed), expected["malformed"]);
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
    fn command_keybindings_load_from_user_layer_only_in_declaration_order() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(
            &user_config,
            r#"
            [keybindings]
            "workdeck.app.quit" = "ctrl+q"
            "workdeck.review.nextHunk" = ["n", "ctrl+n"]
            "workdeck.view.toggleFilesPane" = false
            "bad.boolean" = true
            "bad.array" = ["q", 1]
            "#,
        )
        .unwrap();
        fs::write(
            &repo_config,
            r#"
            [keybindings]
            "workdeck.app.quit" = "x"
            "repo.command" = "y"
            "#,
        )
        .unwrap();

        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(
            config.keybindings,
            [
                UserKeyBindingEntry::new(
                    "workdeck.app.quit",
                    UserKeyBinding::Chord("ctrl+q".into()),
                ),
                UserKeyBindingEntry::new(
                    "workdeck.review.nextHunk",
                    UserKeyBinding::Chords(vec!["n".into(), "ctrl+n".into()]),
                ),
                UserKeyBindingEntry::new("workdeck.view.toggleFilesPane", UserKeyBinding::Disabled,),
            ]
        );
        assert_eq!(config.keybinding_notices.len(), 1);
        assert!(config.keybinding_notices[0].contains("bad.array, bad.boolean"));
        assert!(!config.keybindings.iter().any(|entry| {
            entry.command_id == "repo.command"
                || matches!(&entry.binding, UserKeyBinding::Chord(chord) if chord == "x")
        }));
    }

    #[test]
    fn command_keybindings_require_a_table() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("missing-repo-config.toml");
        fs::write(&user_config, "keybindings = 'ctrl+q'\n").unwrap();

        let error = Config::load_from_paths(&repo_config, Some(&user_config))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Expected keybindings to contain a TOML table.");
    }

    #[test]
    fn validates_default_config() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn preserves_a_non_empty_extension_owned_vcs_id() {
        let mut config = Config::default();
        config.review.vcs = "fossil-tools".into();
        config.validate().unwrap();

        config.review.vcs = " \t".into();
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("review.vcs cannot be empty"));
    }

    #[test]
    fn loads_an_extension_owned_vcs_id_before_the_extension_catalog_exists() {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("missing-repo-config.toml");
        fs::write(&user_config, "[review]\nvcs = 'mercurial-native'\n").unwrap();

        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(config.review.vcs, "mercurial-native");
    }

    #[test]
    fn rejects_duplicate_keybindings() {
        let mut config = Config::default();
        config.keys.files = config.keys.changes.clone();

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("duplicate key binding"));
    }

    #[test]
    fn rejects_duplicate_git_keybinding() {
        let mut config = Config::default();
        config.keys.git = config.keys.files.clone();

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("duplicate key binding"));
        assert!(error.contains("keys.git"));
    }

    #[test]
    fn rejects_invalid_keybindings() {
        let mut config = Config::default();
        config.keys.quit = "ctrl-q".to_string();

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("invalid key binding"));
    }

    #[test]
    fn rejects_zero_refresh_interval() {
        let mut config = Config::default();
        config.refresh.interval_ms = 0;

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("refresh.interval_ms"));
    }

    #[test]
    fn normalizes_named_keybindings() {
        assert_eq!(normalize_key("Esc").unwrap(), "esc");
        assert_eq!(normalize_key("BackTab").unwrap(), "shift-tab");
        assert_eq!(normalize_key("L").unwrap(), "L");
    }
}
