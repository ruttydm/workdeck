use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use workdeck_core::{
    BUNDLED_SHIKI_THEME_IDS, CUSTOM_THEME_COLOR_KEYS, LEGACY_CUSTOM_SYNTAX_COLOR_KEYS,
    LEGACY_CUSTOM_SYNTAX_NOTICE, LEGACY_CUSTOM_THEME_ID, LEGACY_THEME_ID_ALIASES,
    NamedCustomThemeConfig, StartupNotice, UserKeyBinding, UserKeyBindingEntry,
    create_invalid_theme_id_notice, create_theme_collision_notice, describe_custom_theme_id_issue,
    describe_theme_color_issue, normalize_theme_color_value, resolve_bundled_shiki_theme_id,
    resolve_custom_syntax_scope_overrides,
};
use workdeck_diff::sanitize_terminal_line;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigReferenceDefault {
    String(&'static str),
    Integer(u16),
    Boolean(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigReferenceAlias {
    pub key: &'static str,
    pub deprecated: bool,
}

/// One Hunk-compatible flat preference shared by runtime parsing and generated
/// configuration-reference documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigReferenceOption {
    pub key: &'static str,
    pub property: &'static str,
    pub value_type: &'static str,
    pub accepted: &'static str,
    pub runtime_default: Option<ConfigReferenceDefault>,
    pub default_value: Option<&'static str>,
    pub description: &'static str,
    pub aliases: &'static [ConfigReferenceAlias],
    /// Ordered keys preserve the historical precedence of deprecated aliases.
    pub runtime_keys: &'static [&'static str],
}

const NO_ALIASES: &[ConfigReferenceAlias] = &[];

/// Authoritative catalog for Hunk-compatible root, command, and pager options.
pub const CONFIG_REFERENCE_OPTIONS: &[ConfigReferenceOption] = &[
    ConfigReferenceOption {
        key: "mode",
        property: "mode",
        value_type: "string",
        accepted: "auto, split, or stack",
        runtime_default: Some(ConfigReferenceDefault::String("auto")),
        default_value: None,
        description: "Choose responsive, side-by-side, or stacked diff layout.",
        aliases: NO_ALIASES,
        runtime_keys: &["mode"],
    },
    ConfigReferenceOption {
        key: "cursor_line",
        property: "cursor_line",
        value_type: "string",
        accepted: "row, number, or off",
        runtime_default: Some(ConfigReferenceDefault::String("row")),
        default_value: None,
        description: "Mark the current line by row or line number, or disable the marker.",
        aliases: NO_ALIASES,
        runtime_keys: &["cursor_line"],
    },
    ConfigReferenceOption {
        key: "vcs",
        property: "vcs",
        value_type: "string",
        accepted: "git, jj, sl, or an id registered by a loaded extension",
        runtime_default: None,
        default_value: Some("detected from the checkout (Git fallback)"),
        description: "Select a version-control adapter explicitly.",
        aliases: NO_ALIASES,
        runtime_keys: &["vcs"],
    },
    ConfigReferenceOption {
        key: "theme",
        property: "theme",
        value_type: "string",
        accepted: "a built-in theme id, a native-extension theme id, or custom",
        runtime_default: Some(ConfigReferenceDefault::String("github-dark-default")),
        default_value: None,
        description: "Select the active color theme.",
        aliases: NO_ALIASES,
        runtime_keys: &["theme"],
    },
    ConfigReferenceOption {
        key: "watch",
        property: "watch",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(false)),
        default_value: None,
        description: "Reload supported review inputs when their source changes.",
        aliases: NO_ALIASES,
        runtime_keys: &["watch"],
    },
    ConfigReferenceOption {
        key: "exclude_untracked",
        property: "exclude_untracked",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(false)),
        default_value: None,
        description: "Hide untracked files from working-tree reviews.",
        aliases: NO_ALIASES,
        runtime_keys: &["exclude_untracked"],
    },
    ConfigReferenceOption {
        key: "line_numbers",
        property: "line_numbers",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(true)),
        default_value: None,
        description: "Show old and new line-number columns.",
        aliases: NO_ALIASES,
        runtime_keys: &["line_numbers"],
    },
    ConfigReferenceOption {
        key: "tab_width",
        property: "tab_width",
        value_type: "integer",
        accepted: "1 through 16",
        runtime_default: Some(ConfigReferenceDefault::Integer(4)),
        default_value: None,
        description: "Set terminal-cell tab stops used for display and wrapping.",
        aliases: NO_ALIASES,
        runtime_keys: &["tab_width"],
    },
    ConfigReferenceOption {
        key: "file_gap",
        property: "file_gap",
        value_type: "integer",
        accepted: "0 through 8",
        runtime_default: Some(ConfigReferenceDefault::Integer(1)),
        default_value: None,
        description: "Set the rows between files in the continuous review stream.",
        aliases: NO_ALIASES,
        runtime_keys: &["file_gap"],
    },
    ConfigReferenceOption {
        key: "hunk_gap",
        property: "hunk_gap",
        value_type: "integer",
        accepted: "0 through 8",
        runtime_default: Some(ConfigReferenceDefault::Integer(0)),
        default_value: None,
        description: "Set blank rows before hunks after the first hunk in a file.",
        aliases: NO_ALIASES,
        runtime_keys: &["hunk_gap"],
    },
    ConfigReferenceOption {
        key: "wrap_lines",
        property: "wrap_lines",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(false)),
        default_value: None,
        description: "Wrap long diff lines instead of keeping one visual row.",
        aliases: NO_ALIASES,
        runtime_keys: &["wrap_lines"],
    },
    ConfigReferenceOption {
        key: "hunk_headers",
        property: "hunk_headers",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(true)),
        default_value: None,
        description: "Show hunk metadata rows in the review stream.",
        aliases: NO_ALIASES,
        runtime_keys: &["hunk_headers"],
    },
    ConfigReferenceOption {
        key: "menu_bar",
        property: "menu_bar",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(true)),
        default_value: None,
        description: "Show the top application menu bar.",
        aliases: NO_ALIASES,
        runtime_keys: &["menu_bar"],
    },
    ConfigReferenceOption {
        key: "sidebar",
        property: "sidebar",
        value_type: "string or boolean",
        accepted: "auto, true, or false",
        runtime_default: Some(ConfigReferenceDefault::String("auto")),
        default_value: None,
        description: "Show, hide, or responsively select the files pane.",
        aliases: NO_ALIASES,
        runtime_keys: &["sidebar"],
    },
    ConfigReferenceOption {
        key: "agent_notes",
        property: "agent_notes",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(false)),
        default_value: None,
        description: "Show agent notes when a review opens.",
        aliases: NO_ALIASES,
        runtime_keys: &["agent_notes"],
    },
    ConfigReferenceOption {
        key: "copy_decorations",
        property: "copy_decorations",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(false)),
        default_value: None,
        description: "Include diff signs and line numbers in copied selections.",
        aliases: NO_ALIASES,
        runtime_keys: &["copy_decorations"],
    },
    ConfigReferenceOption {
        key: "prompt_save_view_preferences",
        property: "prompt_save_view_preferences",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(true)),
        default_value: None,
        description: "Ask before discarding view changes that can be persisted.",
        aliases: NO_ALIASES,
        runtime_keys: &["prompt_save_view_preferences"],
    },
    ConfigReferenceOption {
        key: "transparent_background",
        property: "transparent_background",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: Some(ConfigReferenceDefault::Boolean(false)),
        default_value: None,
        description: "Let the terminal background show through Workdeck surfaces.",
        aliases: &[ConfigReferenceAlias {
            key: "transparentBackground",
            deprecated: true,
        }],
        runtime_keys: &["transparentBackground", "transparent_background"],
    },
    ConfigReferenceOption {
        key: "color_moved",
        property: "color_moved",
        value_type: "boolean",
        accepted: "true or false",
        runtime_default: None,
        default_value: None,
        description: "Enable moved-line coloring when the renderer supports it.",
        aliases: NO_ALIASES,
        runtime_keys: &["color_moved"],
    },
];

/// Command-specific TOML tables accepted by the runtime resolver.
pub const CONFIG_COMMAND_SECTIONS: &[(&str, &str)] = &[
    ("vcs", "working-tree and target reviews (workdeck diff)"),
    ("show", "commit and target display reviews (workdeck show)"),
    ("stash-show", "stash reviews (workdeck stash show)"),
    ("diff", "two-file comparisons (workdeck diff --files)"),
    (
        "patch",
        "patch-file and pager reviews (workdeck patch/pager)",
    ),
    ("difftool", "Git difftool pair reviews (workdeck difftool)"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigReferenceCustomTheme {
    pub table: &'static str,
    pub base_values: &'static [&'static str],
    pub default_base: &'static str,
    pub legacy_base_aliases: &'static [(&'static str, &'static str)],
    pub color_keys: &'static [&'static str],
    pub legacy_syntax_color_keys: &'static [&'static str],
    pub syntax_scopes_table: &'static str,
    pub legacy_syntax_table: &'static str,
    pub named_theme_table: &'static str,
}

pub const CONFIG_REFERENCE_CUSTOM_THEME: ConfigReferenceCustomTheme = ConfigReferenceCustomTheme {
    table: "custom_theme",
    base_values: BUNDLED_SHIKI_THEME_IDS,
    default_base: "github-dark-default",
    legacy_base_aliases: LEGACY_THEME_ID_ALIASES,
    color_keys: CUSTOM_THEME_COLOR_KEYS,
    legacy_syntax_color_keys: LEGACY_CUSTOM_SYNTAX_COLOR_KEYS,
    syntax_scopes_table: "custom_theme.syntax_scopes",
    legacy_syntax_table: "custom_theme.syntax",
    named_theme_table: "themes",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigReferenceSectionKey {
    pub key: &'static str,
    pub value_type: &'static str,
    pub accepted: &'static str,
    pub default_value: Option<&'static str>,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigReferenceExtensions {
    pub table: &'static str,
    pub per_extension_table: &'static str,
    pub keys: &'static [ConfigReferenceSectionKey],
}

pub const CONFIG_REFERENCE_EXTENSIONS: ConfigReferenceExtensions = ConfigReferenceExtensions {
    table: "extensions",
    per_extension_table: "extension",
    keys: &[
        ConfigReferenceSectionKey {
            key: "extensions.enabled",
            value_type: "boolean",
            accepted: "true or false",
            default_value: Some("true"),
            description: "Load user-native extensions; bundled VCS adapters remain loaded.",
        },
        ConfigReferenceSectionKey {
            key: "extensions.paths",
            value_type: "array of strings",
            accepted: "native extension manifest, executable, or directory paths",
            default_value: Some("[]"),
            description: "Extension entry points loaded at startup; repository paths are trust-gated.",
        },
    ],
};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_true")]
    pub prompt_save_view_preferences: bool,
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
    /// Layered `[custom_theme]` and `[themes.<id>]` declarations in selector order.
    #[serde(skip)]
    pub custom_themes: Vec<NamedCustomThemeConfig>,
    /// Existing repository config, otherwise the global config path, for view persistence.
    #[serde(skip)]
    pub view_preferences_config_path: Option<PathBuf>,
    /// Host-selected write admission; legacy repository layers require explicit migration.
    #[serde(skip)]
    pub view_preferences_write_policy: workdeck_core::ViewPreferenceWritePolicy,
    /// A VCS id named by configuration, distinct from the detected/default adapter.
    #[serde(skip)]
    pub explicit_vcs_id: Option<String>,
    /// Config path intent and the planning source chosen at repository load.
    /// Direct `load_from_paths` callers remain responsible for source selection.
    #[serde(skip)]
    pub explicit_data_dir: bool,
    #[serde(skip)]
    pub resolved_data_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prompt_save_view_preferences: true,
            ui: UiConfig::default(),
            paths: PathConfig::default(),
            git: GitConfig::default(),
            refresh: RefreshConfig::default(),
            review: ReviewConfig::default(),
            keys: KeyConfig::default(),
            extension: BTreeMap::new(),
            resolved_extensions: ExtensionsConfig::default(),
            startup_notices: Vec::new(),
            keybindings: Vec::new(),
            keybinding_notices: Vec::new(),
            custom_themes: Vec::new(),
            view_preferences_config_path: None,
            view_preferences_write_policy: workdeck_core::ViewPreferenceWritePolicy::Writable,
            explicit_vcs_id: None,
            explicit_data_dir: false,
            resolved_data_dir: None,
        }
    }
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
    #[serde(default = "default_true")]
    pub menu_bar: bool,
    #[serde(default)]
    pub sidebar: ReviewSidebar,
    #[serde(default)]
    pub agent_notes: bool,
    #[serde(default)]
    pub copy_decorations: bool,
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
            menu_bar: true,
            sidebar: ReviewSidebar::Auto,
            agent_notes: false,
            copy_decorations: false,
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
        Self::load_repository_with_context(repo_root, user_config_path().as_deref(), None, false)
    }

    /// Load configuration for one review input. Within each user/repository
    /// layer, flat root options are followed by the command section and then
    /// the pager section, matching Hunk's resolution order.
    pub fn load_for_review(
        repo_root: &Path,
        command_section: Option<&str>,
        pager: bool,
    ) -> Result<Self> {
        Self::load_repository_with_context(
            repo_root,
            user_config_path().as_deref(),
            command_section,
            pager,
        )
    }

    fn load_repository_with_context(
        repo_root: &Path,
        user_config: Option<&Path>,
        command_section: Option<&str>,
        pager: bool,
    ) -> Result<Self> {
        let source = repository_config_source(repo_root)?;
        let config = Self::load_from_paths_with_review_context(
            &source.path,
            user_config,
            command_section,
            pager,
        )?;
        apply_repository_source(config, repo_root, &source)
    }

    pub fn load_from_paths(
        repo_config_path: &Path,
        user_config_path: Option<&Path>,
    ) -> Result<Self> {
        Self::load_from_paths_with_review_context(repo_config_path, user_config_path, None, false)
    }

    pub fn load_from_paths_for_review(
        repo_config_path: &Path,
        user_config_path: Option<&Path>,
        command_section: Option<&str>,
        pager: bool,
    ) -> Result<Self> {
        Self::load_from_paths_with_review_context(
            repo_config_path,
            user_config_path,
            command_section,
            pager,
        )
    }

    fn load_from_paths_with_review_context(
        repo_config_path: &Path,
        user_config_path: Option<&Path>,
        command_section: Option<&str>,
        pager: bool,
    ) -> Result<Self> {
        Self::load_with_repo_candidate(
            repo_config_path,
            user_config_path,
            command_section,
            pager,
            None,
        )
    }

    fn load_with_repo_candidate(
        repo_config_path: &Path,
        user_config_path: Option<&Path>,
        command_section: Option<&str>,
        pager: bool,
        candidate: Option<&toml::Value>,
    ) -> Result<Self> {
        let mut merged = toml::Value::Table(Default::default());
        set_config_preference(
            merged
                .as_table_mut()
                .expect("a TOML document root is always a table"),
            "theme",
            toml::Value::String("github-dark-default".into()),
        );
        if pager {
            set_config_preference(
                merged
                    .as_table_mut()
                    .expect("a TOML document root is always a table"),
                "menu_bar",
                toml::Value::Boolean(false),
            );
        }
        let mut keybindings = Vec::new();
        let mut keybinding_notices = Vec::new();
        let mut user_extensions = ExtensionsLayer::default();
        let mut repo_extensions = ExtensionsLayer::default();
        let mut custom_themes = Vec::new();
        let mut uses_legacy_custom_syntax = false;
        let mut theme_notices = Vec::new();

        if let Some(path) = user_config_path.filter(|path| path.exists()) {
            (keybindings, keybinding_notices) = read_user_keybindings(path)?;
            let mut user = read_config_value(&path.canonicalize()?)?;
            apply_layered_review_preferences(&mut user, command_section, pager)?;
            user_extensions = read_extensions_layer(&user)?;
            let themes = read_custom_themes(&user)?;
            merge_custom_theme_layer(&mut custom_themes, themes.themes);
            uses_legacy_custom_syntax |= themes.uses_legacy_syntax;
            merge_startup_notices(&mut theme_notices, themes.notices);
            merge_toml_values(&mut merged, user);
        }

        if candidate.is_some() || repo_config_path.exists() {
            let mut repo = match candidate {
                Some(value) => value.clone(),
                None => read_config_value(repo_config_path)?,
            };
            apply_layered_review_preferences(&mut repo, command_section, pager)?;
            repo_extensions = read_extensions_layer(&repo)?;
            let themes = read_custom_themes(&repo)?;
            merge_custom_theme_layer(&mut custom_themes, themes.themes);
            uses_legacy_custom_syntax |= themes.uses_legacy_syntax;
            merge_startup_notices(&mut theme_notices, themes.notices);
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
            .clone()
            .try_into()
            .with_context(|| "failed to parse merged config")?;
        config.explicit_vcs_id = merged
            .get("review")
            .and_then(toml::Value::as_table)
            .and_then(|review| review.get("vcs"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        config.explicit_data_dir = merged
            .get("paths")
            .and_then(|paths| paths.get("data_dir"))
            .is_some();
        config.keybindings = keybindings;
        config.keybinding_notices = keybinding_notices;
        config.custom_themes = custom_themes;
        config.view_preferences_config_path = if repo_config_path.exists() {
            Some(repo_config_path.to_owned())
        } else {
            user_config_path.map(Path::to_owned)
        };
        config.resolved_extensions = ExtensionsConfig {
            enabled: repo_extensions
                .enabled
                .or(user_extensions.enabled)
                .unwrap_or(true),
            paths: user_extensions.paths,
            repo_paths: repo_extensions.paths,
            extension_configs: config.extension.clone(),
        };
        config.startup_notices = uses_legacy_custom_syntax
            .then(|| LEGACY_CUSTOM_SYNTAX_NOTICE.clone())
            .into_iter()
            .chain(theme_notices)
            .chain(repo_extension_config_notice(
                &repo_extensions.extension_configs,
            ))
            .collect();
        if config.ui.theme == LEGACY_CUSTOM_THEME_ID
            && !config
                .custom_themes
                .iter()
                .any(|theme| theme.id == LEGACY_CUSTOM_THEME_ID)
        {
            bail!("Expected a [custom_theme] table when config selects theme = \"custom\".");
        }
        config
            .validate()
            .with_context(|| "invalid Workdeck config")?;
        Ok(config)
    }

    /// Load only extension discovery/configuration. Review theme and preference
    /// validation is intentionally skipped so an extension-owned CLI command
    /// can bootstrap even when unrelated review configuration is incomplete.
    pub fn load_extension_bootstrap(repo_root: &Path) -> Result<Self> {
        let source = repository_config_source(repo_root)?;
        let config =
            Self::load_extension_bootstrap_from_paths(&source.path, user_config_path().as_deref())?;
        apply_repository_source(config, repo_root, &source)
    }

    pub fn load_extension_bootstrap_from_paths(
        repo_config_path: &Path,
        user_config_path: Option<&Path>,
    ) -> Result<Self> {
        let user_value = match user_config_path.filter(|path| path.exists()) {
            Some(path) => read_config_value(&path.canonicalize()?)?,
            None => toml::Value::Table(Default::default()),
        };
        let repo_value = if repo_config_path.exists() {
            read_config_value(repo_config_path)?
        } else {
            toml::Value::Table(Default::default())
        };
        let user_extensions = read_extensions_layer(&user_value)?;
        let repo_extensions = read_extensions_layer(&repo_value)?;
        let mut path_layer = toml::Value::Table(Default::default());
        for layer in [&user_value, &repo_value] {
            if let Some(paths) = layer.get("paths") {
                merge_toml_values(&mut path_layer, paths.clone());
            }
        }
        let explicit_data_dir = path_layer.get("data_dir").is_some();
        let paths: PathConfig = path_layer
            .try_into()
            .with_context(|| "invalid configured planning path")?;
        let extension_configs = merge_extension_configs(
            &user_extensions.extension_configs,
            &repo_extensions.extension_configs,
        );
        let resolved_extension_configs = extension_configs
            .iter()
            .map(|(id, value)| (id.clone(), toml_value_as_json(value)))
            .collect::<BTreeMap<_, _>>();
        let mut config = Self {
            paths,
            explicit_data_dir,
            extension: resolved_extension_configs.clone(),
            resolved_extensions: ExtensionsConfig {
                enabled: repo_extensions
                    .enabled
                    .or(user_extensions.enabled)
                    .unwrap_or(true),
                paths: user_extensions.paths,
                repo_paths: repo_extensions.paths,
                extension_configs: resolved_extension_configs,
            },
            startup_notices: repo_extension_config_notice(&repo_extensions.extension_configs)
                .into_iter()
                .collect(),
            ..Self::default()
        };
        config.view_preferences_config_path = if repo_config_path.exists() {
            Some(repo_config_path.to_owned())
        } else {
            user_config_path.map(Path::to_owned)
        };
        Ok(config)
    }

    pub fn data_dir(&self, repo_root: &Path) -> PathBuf {
        if let Some(resolved) = &self.resolved_data_dir {
            return resolved.clone();
        }
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

/// Apply one Hunk-compatible source layer to Workdeck's canonical nested model.
/// The source is cloned before mutation so command and pager sections cannot be
/// affected by values projected from an earlier level.
fn apply_layered_review_preferences(
    root: &mut toml::Value,
    command_section: Option<&str>,
    pager: bool,
) -> Result<()> {
    let source = root
        .as_table()
        .context("Expected Workdeck config to contain a TOML table.")?
        .clone();
    let destination = root
        .as_table_mut()
        .expect("the source was verified as a TOML table");

    // This key is part of Workdeck's canonical root model as well as the flat
    // compatibility model. Remove it first so an invalid scalar is ignored in
    // the same way Hunk ignores invalid non-numeric preference values.
    destination.remove("prompt_save_view_preferences");
    apply_flat_review_preferences(destination, &source)?;
    if let Some(section) = command_section
        .and_then(|section| source.get(section))
        .and_then(toml::Value::as_table)
    {
        apply_flat_review_preferences(destination, section)?;
    }
    if pager && let Some(section) = source.get("pager").and_then(toml::Value::as_table) {
        apply_flat_review_preferences(destination, section)?;
    }
    Ok(())
}

fn apply_flat_review_preferences(
    destination: &mut toml::map::Map<String, toml::Value>,
    source: &toml::map::Map<String, toml::Value>,
) -> Result<()> {
    for option in CONFIG_REFERENCE_OPTIONS {
        let mut normalized = None;
        for key in option.runtime_keys {
            if let Some(value) =
                normalize_config_reference_value(option.property, source.get(*key))?
            {
                normalized = Some(value);
                break;
            }
        }
        if let Some(value) = normalized {
            set_config_preference(destination, option.property, value);
        }
    }
    Ok(())
}

fn normalize_config_reference_value(
    property: &str,
    value: Option<&toml::Value>,
) -> Result<Option<toml::Value>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = match property {
        "mode" => value
            .as_str()
            .filter(|value| matches!(*value, "auto" | "split" | "stack"))
            .map(|value| toml::Value::String(value.to_owned())),
        "cursor_line" => value
            .as_str()
            .filter(|value| matches!(*value, "row" | "number" | "off"))
            .map(|value| toml::Value::String(value.to_owned())),
        "vcs" => value
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .map(|value| toml::Value::String(value.to_owned())),
        "theme" => value
            .as_str()
            .filter(|value| !value.is_empty())
            .map(|value| toml::Value::String(value.to_owned())),
        "sidebar" => match value {
            toml::Value::Boolean(value) => Some(toml::Value::Boolean(*value)),
            toml::Value::String(value) if value == "auto" => {
                Some(toml::Value::String(value.clone()))
            }
            _ => None,
        },
        "tab_width" => Some(normalize_bounded_integer(value, "tab_width", 1, 16)?),
        "file_gap" => Some(normalize_bounded_integer(value, "file_gap", 0, 8)?),
        "hunk_gap" => Some(normalize_bounded_integer(value, "hunk_gap", 0, 8)?),
        _ => value.as_bool().map(toml::Value::Boolean),
    };
    Ok(normalized)
}

fn normalize_bounded_integer(
    value: &toml::Value,
    key: &str,
    minimum: i64,
    maximum: i64,
) -> Result<toml::Value> {
    let Some(value) = value.as_integer() else {
        bail!("Expected {key} to be an integer from {minimum} to {maximum}.");
    };
    if !(minimum..=maximum).contains(&value) {
        bail!("Expected {key} to be an integer from {minimum} to {maximum}.");
    }
    Ok(toml::Value::Integer(value))
}

fn set_config_preference(
    destination: &mut toml::map::Map<String, toml::Value>,
    property: &str,
    value: toml::Value,
) {
    match property {
        "theme" => set_config_table_value(destination, "ui", "theme", value),
        "prompt_save_view_preferences" => {
            destination.insert(property.to_owned(), value);
        }
        property => set_config_table_value(destination, "review", property, value),
    }
}

fn set_config_table_value(
    destination: &mut toml::map::Map<String, toml::Value>,
    table: &str,
    key: &str,
    value: toml::Value,
) {
    let table = destination
        .entry(table)
        .or_insert_with(|| toml::Value::Table(Default::default()));
    if let Some(table) = table.as_table_mut() {
        table.insert(key.to_owned(), value);
    }
}

#[derive(Debug, Clone, Default)]
struct CustomThemeLayer {
    themes: Vec<NamedCustomThemeConfig>,
    uses_legacy_syntax: bool,
    notices: Vec<StartupNotice>,
}

fn read_custom_themes(value: &toml::Value) -> Result<CustomThemeLayer> {
    let root = value
        .as_table()
        .context("Expected Workdeck config to contain a TOML table.")?;
    let mut layer = CustomThemeLayer::default();

    if let Some(value) = root.get("custom_theme") {
        let table = value
            .as_table()
            .context("Expected custom_theme to contain a TOML table.")?;
        let (theme, uses_legacy_syntax) =
            read_custom_theme_table(table, LEGACY_CUSTOM_THEME_ID, "custom_theme")?;
        layer.themes.push(theme);
        layer.uses_legacy_syntax |= uses_legacy_syntax;
    }

    if let Some(value) = root.get("themes") {
        let themes = value
            .as_table()
            .context("Expected themes to contain named TOML tables.")?;
        for (id, value) in themes {
            let table = value
                .as_table()
                .with_context(|| format!("Expected [themes.{id}] to contain a TOML table."))?;
            let id_value = serde_json::Value::String(id.clone());
            if let Some(reason) = describe_custom_theme_id_issue(&id_value) {
                layer
                    .notices
                    .push(create_invalid_theme_id_notice("config", id, reason));
                continue;
            }
            if layer.themes.iter().any(|theme| theme.id == *id) {
                layer.notices.push(create_theme_collision_notice(
                    "config",
                    id,
                    "[custom_theme]",
                ));
                continue;
            }
            let (theme, uses_legacy_syntax) =
                read_custom_theme_table(table, id, &format!("themes.{id}"))?;
            layer.themes.push(theme);
            layer.uses_legacy_syntax |= uses_legacy_syntax;
        }
    }
    Ok(layer)
}

fn read_custom_theme_table(
    table: &toml::map::Map<String, toml::Value>,
    id: &str,
    key_path: &str,
) -> Result<(NamedCustomThemeConfig, bool)> {
    let legacy_syntax = optional_theme_table(table, "syntax", key_path)?;
    let exact_scopes = optional_theme_table(table, "syntax_scopes", key_path)?;
    let mut theme = serde_json::Map::new();
    theme.insert("id".into(), serde_json::Value::String(id.into()));

    if let Some(value) = table.get("base") {
        let resolved = value
            .as_str()
            .and_then(|value| resolve_bundled_shiki_theme_id(Some(value)))
            .with_context(|| {
                format!(
                    "Expected {key_path}.base to be a built-in theme id. Known themes: {}.",
                    BUNDLED_SHIKI_THEME_IDS.join(", ")
                )
            })?;
        theme.insert("base".into(), serde_json::Value::String(resolved.into()));
    }
    if let Some(label) = table
        .get("label")
        .and_then(toml::Value::as_str)
        .filter(|label| !label.is_empty())
    {
        theme.insert("label".into(), serde_json::Value::String(label.into()));
    }
    for key in CUSTOM_THEME_COLOR_KEYS {
        if let Some(value) = table.get(*key) {
            let json = toml_value_as_json(value);
            if describe_theme_color_issue(&json).is_some() {
                bail!("Expected {key_path}.{key} to be a hex color like #112233.");
            }
            let color = value.as_str().expect("validated theme colors are strings");
            theme.insert(
                (*key).into(),
                serde_json::Value::String(normalize_theme_color_value(color)),
            );
        }
    }

    let legacy = read_theme_color_table(
        legacy_syntax,
        LEGACY_CUSTOM_SYNTAX_COLOR_KEYS,
        &format!("{key_path}.syntax"),
    )?;
    let exact = read_exact_syntax_scopes(exact_scopes, key_path)?;
    let scopes = resolve_custom_syntax_scope_overrides(&legacy, &exact);
    if !scopes.is_empty() {
        theme.insert(
            "syntaxScopes".into(),
            serde_json::to_value(scopes).expect("syntax scopes are JSON serializable"),
        );
    }

    let theme = serde_json::from_value(serde_json::Value::Object(theme))
        .expect("normalized config theme has the provider-neutral schema");
    Ok((theme, !legacy.is_empty()))
}

fn optional_theme_table<'a>(
    parent: &'a toml::map::Map<String, toml::Value>,
    key: &str,
    key_path: &str,
) -> Result<Option<&'a toml::map::Map<String, toml::Value>>> {
    parent
        .get(key)
        .map(|value| {
            value
                .as_table()
                .with_context(|| format!("Expected {key_path}.{key} to contain a TOML table."))
        })
        .transpose()
}

fn read_theme_color_table(
    table: Option<&toml::map::Map<String, toml::Value>>,
    recognized: &[&str],
    key_path: &str,
) -> Result<indexmap::IndexMap<String, String>> {
    let mut colors = indexmap::IndexMap::new();
    let Some(table) = table else {
        return Ok(colors);
    };
    for key in recognized {
        let Some(value) = table.get(*key) else {
            continue;
        };
        let json = toml_value_as_json(value);
        if describe_theme_color_issue(&json).is_some() {
            bail!("Expected {key_path}.{key} to be a hex color like #112233.");
        }
        colors.insert(
            (*key).to_owned(),
            normalize_theme_color_value(value.as_str().expect("validated color is a string")),
        );
    }
    Ok(colors)
}

fn read_exact_syntax_scopes(
    table: Option<&toml::map::Map<String, toml::Value>>,
    key_path: &str,
) -> Result<indexmap::IndexMap<String, String>> {
    let mut colors = indexmap::IndexMap::new();
    let Some(table) = table else {
        return Ok(colors);
    };
    for (scope, value) in table {
        if scope.trim().is_empty() {
            bail!("Expected {key_path}.syntax_scopes keys to be non-empty Shiki scopes.");
        }
        let json = toml_value_as_json(value);
        if describe_theme_color_issue(&json).is_some() {
            bail!("Expected {key_path}.syntax_scopes.{scope} to be a hex color like #112233.");
        }
        colors.insert(
            scope.clone(),
            normalize_theme_color_value(value.as_str().expect("validated color is a string")),
        );
    }
    Ok(colors)
}

fn toml_value_as_json(value: &toml::Value) -> serde_json::Value {
    serde_json::to_value(value).expect("TOML values are JSON serializable")
}

fn merge_custom_theme_layer(
    merged: &mut Vec<NamedCustomThemeConfig>,
    overrides: Vec<NamedCustomThemeConfig>,
) {
    for theme in overrides {
        if let Some(index) = merged.iter().position(|candidate| candidate.id == theme.id) {
            merged[index] = merge_custom_theme(&merged[index], &theme);
        } else {
            merged.push(theme);
        }
    }
}

fn merge_custom_theme(
    base: &NamedCustomThemeConfig,
    overrides: &NamedCustomThemeConfig,
) -> NamedCustomThemeConfig {
    let mut base = serde_json::to_value(base)
        .expect("custom themes are JSON serializable")
        .as_object()
        .expect("custom themes serialize as objects")
        .clone();
    let mut overrides = serde_json::to_value(overrides)
        .expect("custom themes are JSON serializable")
        .as_object()
        .expect("custom themes serialize as objects")
        .clone();
    let override_scopes = overrides.remove("syntaxScopes");
    let base_scopes = base.remove("syntaxScopes");
    for (key, value) in overrides {
        if key != "id" {
            base.insert(key, value);
        }
    }
    if !base.contains_key("base") {
        base.insert(
            "base".into(),
            serde_json::Value::String("github-dark-default".into()),
        );
    }
    if base_scopes.is_some() || override_scopes.is_some() {
        let mut scopes = base_scopes
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        if let Some(overrides) = override_scopes.and_then(|value| value.as_object().cloned()) {
            for (scope, color) in overrides {
                scopes.insert(scope, color);
            }
        }
        base.insert("syntaxScopes".into(), serde_json::Value::Object(scopes));
    }
    serde_json::from_value(serde_json::Value::Object(base))
        .expect("merged custom theme has the provider-neutral schema")
}

fn merge_startup_notices(merged: &mut Vec<StartupNotice>, layer: Vec<StartupNotice>) {
    for notice in layer {
        if let Some(existing) = merged
            .iter_mut()
            .find(|candidate| candidate.key == notice.key)
        {
            *existing = notice;
        } else {
            merged.push(notice);
        }
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
    let mut ids = extension_configs
        .iter()
        .filter_map(|(id, value)| (!value.as_table()?.is_empty()).then_some(id.as_str()))
        .collect::<Vec<_>>();
    ids.sort_unstable();
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
    // User-selected configuration may be a dotfiles symlink. Resolve that
    // explicit user source, then still require a bounded regular descriptor.
    let raw = read_app_config_text(&path.canonicalize()?)?;
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
    let raw = read_app_config_text(path)?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

/// Read the selected repository layer without following a leaf symlink or
/// blocking on a special file. Used by config inspection as well as startup.
pub fn read_repository_config_value(path: &Path) -> Result<toml::Value> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(Default::default()))
        }
        Err(error) => Err(error.into()),
        Ok(_) => read_config_value(path),
    }
}

fn read_app_config_text(path: &Path) -> Result<String> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .context("application config has no filename")?;
    let read = crate::bounded_files::read(parent, Path::new(name), 2 * 1024 * 1024)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    if read.truncated {
        bail!(
            "application configuration exceeds 2 MiB: {}",
            path.display()
        );
    }
    String::from_utf8(read.bytes)
        .with_context(|| format!("configuration must be UTF-8: {}", path.display()))
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

struct RepositoryConfigSource {
    path: PathBuf,
    native_pm: bool,
    legacy_pm: bool,
    legacy_config: bool,
    planning_diagnostic: Option<String>,
}

/// One repository app-settings resolver for reads and explicit config commands.
/// Application preferences remain usable when PM records need repair/recovery;
/// PM commands independently enforce full Repository admission.
pub fn resolve_repo_config_path(repo_root: &Path) -> Result<PathBuf> {
    Ok(repository_config_source(repo_root)?.path)
}

/// Validate the proposed repository layer using the same global/repository
/// merge, preference semantics, extension provenance, and source pin as load.
/// The candidate never needs to be written to the filesystem for validation.
pub fn validate_repo_config_candidate(repo_root: &Path, candidate: &toml::Value) -> Result<()> {
    validate_repo_config_candidate_with_user(repo_root, candidate, user_config_path().as_deref())
}

fn validate_repo_config_candidate_with_user(
    repo_root: &Path,
    candidate: &toml::Value,
    user_config: Option<&Path>,
) -> Result<()> {
    let source = repository_config_source(repo_root)?;
    let config =
        Config::load_with_repo_candidate(&source.path, user_config, None, false, Some(candidate))?;
    apply_repository_source(config, repo_root, &source)?;
    Ok(())
}

fn path_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

fn has_native_record_evidence(root: &Path) -> Result<bool> {
    for namespace in [
        "restore.yml",
        "users.yml",
        "schema.yml",
        "issues",
        "initiatives",
        "projects",
        "milestones",
        "targets",
        "cycles",
        "features",
        "gates",
        "relations",
        "wiki",
        "labels.yml",
        "operations",
        "tombstones",
        "imported-sessions",
        "imported-history",
        "imported-handoffs",
        "claims",
        "runs",
        "commands",
        "checks",
        "check-profiles",
        "claims",
        "coordination.yml",
        "evidence",
        "questions",
        "templates",
        "views",
        "migrations",
    ] {
        let path = root.join(namespace);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                if let Some(entry) = fs::read_dir(&path)
                    .with_context(|| format!("failed to inspect {}", path.display()))?
                    .next()
                {
                    entry?;
                    return Ok(true);
                }
            }
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("failed to inspect {}", path.display()));
            }
        }
    }
    Ok(false)
}

fn repository_config_source(repo_root: &Path) -> Result<RepositoryConfigSource> {
    let native = repo_root.join(".workdeck");
    let legacy = repo_root.join(".agents/workdeck");
    let marker = path_present(&native.join("migration.yml"))?;
    let native_pm =
        marker || path_present(&native.join("config.yml"))? || has_native_record_evidence(&native)?;
    let mut legacy_pm = false;
    for relative in ["issues", "projects.toml", "cycles.toml", "labels.toml"] {
        legacy_pm |= path_present(&legacy.join(relative))?;
    }
    if native_pm && legacy_pm && !marker {
        return Err(workdeck_pm::PmError::new(workdeck_pm::ErrorCode::AmbiguousSource,"native and legacy planning sources coexist without an accepted migration marker; select or migrate the source explicitly").at(&native).into());
    }
    let planning_diagnostic = if native_pm {
        workdeck_pm::Repository::open_source(&native)
            .err()
            .map(|error| error.to_string())
    } else {
        None
    };
    let native_config = native.join("config.toml");
    let legacy_config = legacy.join("config.toml");
    let use_legacy = !native_pm && !path_present(&native_config)? && path_present(&legacy_config)?;
    Ok(RepositoryConfigSource {
        path: if use_legacy {
            legacy_config
        } else {
            native_config
        },
        native_pm,
        legacy_pm,
        legacy_config: use_legacy,
        planning_diagnostic,
    })
}

fn apply_repository_source(
    mut config: Config,
    repo_root: &Path,
    source: &RepositoryConfigSource,
) -> Result<Config> {
    config.view_preferences_write_policy = if source.legacy_config {
        workdeck_core::ViewPreferenceWritePolicy::LegacyReadOnly
    } else {
        workdeck_core::ViewPreferenceWritePolicy::Writable
    };
    let selected = if source.native_pm {
        Some(repo_root.join(".workdeck"))
    } else if source.legacy_pm {
        Some(repo_root.join(".agents/workdeck"))
    } else {
        None
    };
    if let Some(selected) = selected {
        if config.explicit_data_dir
            && workdeck_core::resolve_canonical_path(config.data_dir(repo_root))?
                != workdeck_core::resolve_canonical_path(&selected)?
        {
            bail!(
                "paths.data_dir conflicts with the selected planning source at {}; preserve the configured path and migrate or select its source explicitly",
                selected.display()
            );
        }
        // `config show` should describe the effective compatibility default,
        // while explicitly authored path spelling remains intact.
        if !config.explicit_data_dir && source.legacy_pm && !source.native_pm {
            config.paths.data_dir = PathBuf::from(".agents/workdeck");
        }
        config.resolved_data_dir = Some(selected);
    }
    if source.legacy_config || (source.legacy_pm && !source.native_pm) {
        config.startup_notices.push(StartupNotice::new("migration:legacy-source","Using existing .agents/workdeck compatibility source; run workdeck migrate legacy to review migration into .workdeck."));
    }
    if let Some(diagnostic) = &source.planning_diagnostic {
        config.startup_notices.push(StartupNotice::new(
            "planning:source-unavailable",
            format!(
                "Project management unavailable: {}",
                sanitize_terminal_line(diagnostic)
            ),
        ));
    }
    let extension_directory = workdeck_extension_host::repository_extension_directory(repo_root);
    if extension_directory == repo_root.join(".agents/workdeck/extensions")
        && extension_directory.exists()
    {
        config.startup_notices.push(StartupNotice::new("migration:legacy-extensions","Repository extensions use the legacy .agents/workdeck/extensions directory with existing repository trust; migrate or configure the native .workdeck directory explicitly."));
    } else if repo_root.join(".agents/workdeck/extensions").exists() {
        config.startup_notices.push(StartupNotice::new(
            "migration:legacy-extension-paths",
            "Canonical extension discovery uses .workdeck/extensions. Legacy files remain in .agents/workdeck/extensions; move them into the canonical directory or retain their paths explicitly in repository [extensions].paths. Configured repository paths still require repository trust.",
        ));
    }
    Ok(config)
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
    PathBuf::from(".workdeck")
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
    use std::collections::BTreeSet;

    #[test]
    fn frozen_config_resolution_oracle_maps_both_pins_and_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/config-resolution.json"
        ))
        .unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert_eq!(
            baselines[0]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            baselines[0]["source_blob"],
            "68ebc02b234a5ebcae4881aeaded9a8bc557cf93"
        );
        assert_eq!(baselines[0]["source_bytes"], 48_848);
        assert_eq!(baselines[0]["source_lines"], 1_357);
        assert_eq!(
            baselines[0]["test_blob"],
            "9b5f312e7fc85568d5d1aa129f1a3b3283254540"
        );
        assert_eq!(baselines[0]["test_bytes"], 48_592);
        assert_eq!(baselines[0]["test_lines"], 1_483);
        assert_eq!(baselines[0]["passed"], 56);
        assert_eq!(baselines[0]["failed"], 0);
        assert_eq!(baselines[0]["expect_calls"], 124);
        assert_eq!(baselines[1]["passed"], 54);
        assert_eq!(baselines[1]["failed"], 0);
        assert_eq!(baselines[1]["expect_calls"], 110);

        let expected_source_intervals = [
            (2_549, 5_320, 73, 145),
            (5_645, 19_036, 154, 529),
            (32_998, 43_321, 949, 1_237),
            (43_475, 44_068, 1_241, 1_253),
            (44_455, 44_924, 1_262, 1_271),
            (44_981, 45_368, 1_273, 1_281),
            (45_425, 47_082, 1_283, 1_313),
            (47_529, 48_141, 1_322, 1_339),
            (48_181, 48_522, 1_341, 1_351),
            (48_841, 48_848, 1_356, 1_357),
        ];
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source_mapping = oracle["new_source_mapping"].as_array().unwrap();
        assert_eq!(source_mapping.len(), expected_source_intervals.len());
        for (mapping, (byte_start, byte_end, line_start, line_end)) in
            source_mapping.iter().zip(expected_source_intervals)
        {
            assert_eq!(mapping["bytes"], serde_json::json!([byte_start, byte_end]));
            assert_eq!(mapping["lines"], serde_json::json!([line_start, line_end]));
            assert!(!mapping["semantics"].as_str().unwrap().is_empty());
            for destination in mapping["destinations"].as_array().unwrap() {
                let destination = destination.as_str().unwrap();
                assert!(
                    repo_root.join(destination).is_file(),
                    "missing {destination}"
                );
            }
        }

        assert_eq!(
            oracle["new_test_intervals"],
            serde_json::json!([
                { "bytes": [0, 12392], "lines": [1, 386] },
                { "bytes": [23668, 36599], "lines": [736, 1115] },
                { "bytes": [38008, 40280], "lines": [1166, 1237] },
            ])
        );
        let source_tests = oracle["source_tests"].as_array().unwrap();
        assert_eq!(source_tests.len(), 49);
        let mut baseline_cases = 0_u64;
        let mut stable_cases = 0_u64;
        let mut source_numbers = BTreeSet::new();
        let mut titles = BTreeSet::new();
        for (index, source_test) in source_tests.iter().enumerate() {
            let number = source_test["number"].as_u64().unwrap();
            assert_eq!(number, index as u64 + 1);
            assert!(source_numbers.insert(number));
            assert!(titles.insert(source_test["title"].as_str().unwrap()));
            let cases = source_test["case_count"].as_u64().unwrap_or(1);
            baseline_cases += cases;
            if source_test["stable_present"].as_bool().unwrap_or(true) {
                stable_cases += cases;
            }
        }
        assert_eq!(baseline_cases, 56);
        assert_eq!(stable_cases, 54);

        let rust_sources = [
            include_str!("config.rs"),
            include_str!("main.rs"),
            include_str!("../../workdeck-core/src/view_preferences.rs"),
            include_str!("../../workdeck-core/src/paths.rs"),
            include_str!("../../workdeck-vcs/src/catalog.rs"),
        ];
        let mut mapped_numbers = BTreeSet::new();
        for mapping in oracle["test_mapping"].as_array().unwrap() {
            for number in mapping["source_numbers"].as_array().unwrap() {
                let number = number.as_u64().unwrap();
                assert!(source_numbers.contains(&number));
                assert!(mapped_numbers.insert(number), "test {number} mapped twice");
            }
            let rust_tests = mapping["rust_tests"].as_array().unwrap();
            assert!(!rust_tests.is_empty());
            for rust_test in rust_tests {
                let name = rust_test.as_str().unwrap();
                let needle = format!("fn {name}(");
                assert!(
                    rust_sources.iter().any(|source| source.contains(&needle)),
                    "missing mapped Rust test {name}"
                );
            }
        }
        assert_eq!(mapped_numbers, source_numbers);
    }

    #[test]
    fn defaults_to_root_workdeck() {
        let root = Path::new("/tmp/repo");
        assert_eq!(
            Config::default().data_dir(root),
            PathBuf::from("/tmp/repo/.workdeck")
        );
    }

    #[test]
    fn cutover_config_only_preserves_layers_and_does_not_initialize_pm() {
        let temp = tempfile::tempdir().unwrap();
        let native = temp.path().join(".workdeck");
        fs::create_dir(&native).unwrap();
        let user = temp.path().join("user.toml");
        fs::write(&user, "file_gap=2\n[review]\nline_numbers=false\n").unwrap();
        fs::write(
            native.join("config.toml"),
            "file_gap=3\n[diff]\nfile_gap=4\n[pager]\nfile_gap=5\n",
        )
        .unwrap();
        let config =
            Config::load_repository_with_context(temp.path(), Some(&user), Some("diff"), true)
                .unwrap();
        assert_eq!(config.review.file_gap, 5);
        assert!(!config.review.line_numbers);
        assert_eq!(
            config.view_preferences_config_path,
            Some(native.join("config.toml"))
        );
        assert_eq!(config.data_dir(temp.path()), native);
        assert!(!native.join("config.yml").exists());
        assert!(!native.join(".tmp").exists());
    }

    #[test]
    fn cutover_extension_bootstrap_checks_path_intent_without_validating_review_theme() {
        let temp = tempfile::tempdir().unwrap();
        workdeck_pm::Repository::init(temp.path(), "WD").unwrap();
        let path = temp.path().join(".workdeck/config.toml");
        fs::write(&path,"[ui]\ntheme='missing-custom-theme'\n[paths]\ndata_dir='../external'\n[extensions]\npaths=['repo-tools']\n").unwrap();
        let config = Config::load_extension_bootstrap_from_paths(&path, None).unwrap();
        assert_eq!(
            config.resolved_extensions.repo_paths,
            [PathBuf::from("repo-tools")]
        );
        let source = repository_config_source(temp.path()).unwrap();
        assert!(
            apply_repository_source(config, temp.path(), &source)
                .unwrap_err()
                .to_string()
                .contains("paths.data_dir conflicts")
        );
        fs::write(
            &path,
            "[ui]\ntheme='missing-custom-theme'\n[extensions]\npaths=['repo-tools']\n",
        )
        .unwrap();
        let config = Config::load_extension_bootstrap_from_paths(&path, None).unwrap();
        assert!(apply_repository_source(config, temp.path(), &source).is_ok());
    }

    #[test]
    fn cutover_sole_legacy_pm_remains_selected_with_native_app_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join(".agents/workdeck");
        fs::create_dir_all(legacy.join("issues")).unwrap();
        fs::write(legacy.join("config.toml"), "file_gap=6\n").unwrap();
        let old = Config::load_repository_with_context(temp.path(), None, None, false).unwrap();
        assert_eq!(old.data_dir(temp.path()), legacy);
        assert_eq!(old.paths.data_dir, Path::new(".agents/workdeck"));
        assert_eq!(
            old.view_preferences_config_path,
            Some(legacy.join("config.toml"))
        );
        assert_eq!(old.review.file_gap, 6);
        assert_eq!(
            old.view_preferences_write_policy,
            workdeck_core::ViewPreferenceWritePolicy::LegacyReadOnly
        );
        let native = temp.path().join(".workdeck");
        fs::create_dir(&native).unwrap();
        fs::write(native.join("config.toml"), "file_gap=7\n").unwrap();
        let config = Config::load_repository_with_context(temp.path(), None, None, false).unwrap();
        assert_eq!(config.review.file_gap, 7);
        assert_eq!(
            config.view_preferences_write_policy,
            workdeck_core::ViewPreferenceWritePolicy::Writable
        );
        assert_eq!(config.data_dir(temp.path()), legacy);
        assert_eq!(config.paths.data_dir, Path::new(".agents/workdeck"));
        assert_eq!(
            config.view_preferences_config_path,
            Some(native.join("config.toml"))
        );
        assert!(
            config
                .startup_notices
                .iter()
                .any(|notice| notice.key == "migration:legacy-source")
        );
        assert!(!native.join("config.yml").exists());
    }

    #[test]
    fn cutover_custom_path_intent_is_preserved_or_reports_a_source_conflict() {
        let temp = tempfile::tempdir().unwrap();
        let native = temp.path().join(".workdeck");
        fs::create_dir(&native).unwrap();
        fs::write(
            native.join("config.toml"),
            "[paths]\ndata_dir='../external-planning'\n",
        )
        .unwrap();
        let config = Config::load_repository_with_context(temp.path(), None, None, false).unwrap();
        assert_eq!(
            config.data_dir(temp.path()),
            temp.path().join("../external-planning")
        );
        assert!(!temp.path().join("../external-planning").exists());
        workdeck_pm::Repository::init(temp.path(), "WD").unwrap();
        let error =
            Config::load_repository_with_context(temp.path(), None, None, false).unwrap_err();
        assert!(error.to_string().contains("paths.data_dir conflicts"));
        fs::write(
            native.join("config.toml"),
            "[paths]\ndata_dir='.agents/workdeck'\n",
        )
        .unwrap();
        assert!(
            Config::load_repository_with_context(temp.path(), None, None, false)
                .unwrap_err()
                .to_string()
                .contains("paths.data_dir conflicts")
        );
    }

    #[test]
    fn explicit_preference_move_reports_changed_extension_discovery_and_preserves_trust() {
        use workdeck_extension_host::{
            ManifestOrigin, TrustDecision, TrustStore, discover_manifests_with_config,
        };
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let legacy = root.join(".agents/workdeck");
        let manifest = legacy.join("extensions/example/workdeck-extension.toml");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, "id='example'\nname='Example'\nversion='1.0.0'\napi_version=1\nexecutable='never-executed'\n").unwrap();
        let original = "# untouched legacy preferences\nmode='split'\n";
        fs::write(legacy.join("config.toml"), original).unwrap();
        let discover = |trust: &TrustStore, config: &Config| {
            discover_manifests_with_config(
                None,
                Some(&root),
                trust,
                &[],
                &[],
                &config.resolved_extensions.repo_paths,
                &root,
            )
            .unwrap()
        };
        let before = Config::load_repository_with_context(&root, None, None, false).unwrap();
        assert!(
            discover(&TrustStore::default(), &before)
                .manifests
                .is_empty()
        );
        let mut trust = TrustStore::default();
        trust.grant(&root, TrustDecision::Trusted);
        assert_eq!(
            discover(&trust, &before).manifests,
            std::slice::from_ref(&manifest)
        );
        assert!(crate::config_edit::set(&root, "tab_width", "0").is_err());
        assert!(
            !root.join(".workdeck").exists(),
            "invalid preferences must not suppress legacy extension discovery"
        );
        let rejected = Config::load_repository_with_context(&root, None, None, false).unwrap();
        assert_eq!(
            rejected.view_preferences_config_path,
            Some(legacy.join("config.toml"))
        );
        assert_eq!(
            discover(&trust, &rejected).manifests,
            std::slice::from_ref(&manifest)
        );
        crate::config_edit::initialize(&root).unwrap();
        let after = Config::load_repository_with_context(&root, None, None, false).unwrap();
        assert!(discover(&trust, &after).manifests.is_empty());
        let notice = after
            .startup_notices
            .iter()
            .find(|notice| notice.key == "migration:legacy-extension-paths")
            .expect("explicit preference migration must explain the extension discovery change");
        assert!(notice.message.contains(".workdeck/extensions"));
        assert!(notice.message.contains("[extensions].paths"));
        crate::config_edit::set(&root, "extensions.paths", "['.agents/workdeck/extensions']")
            .unwrap();
        let retained = Config::load_repository_with_context(&root, None, None, false).unwrap();
        let trusted = discover(&trust, &retained);
        assert_eq!(trusted.manifests, std::slice::from_ref(&manifest));
        assert_eq!(trusted.origin(&manifest), Some(ManifestOrigin::Repository));
        let untrusted = discover(&TrustStore::default(), &retained);
        assert!(untrusted.manifests.is_empty());
        assert_eq!(untrusted.pending_trust_repo_root, Some(root));
        assert_eq!(
            fs::read_to_string(legacy.join("config.toml")).unwrap(),
            original
        );
    }

    #[test]
    fn preference_write_policy_follows_selected_layer_and_cannot_be_configured() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("user.toml");
        fs::write(&user, "mode='stack'\n").unwrap();
        let global =
            Config::load_repository_with_context(temp.path(), Some(&user), None, false).unwrap();
        assert_eq!(global.view_preferences_config_path, Some(user.clone()));
        assert_eq!(
            global.view_preferences_write_policy,
            workdeck_core::ViewPreferenceWritePolicy::Writable
        );
        let legacy = temp.path().join(".agents/workdeck/config.toml");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(
            &legacy,
            "mode='split'\nview_preferences_write_policy='Writable'\n",
        )
        .unwrap();
        let old =
            Config::load_repository_with_context(temp.path(), Some(&user), None, false).unwrap();
        assert_eq!(old.view_preferences_config_path, Some(legacy));
        assert_eq!(
            old.view_preferences_write_policy,
            workdeck_core::ViewPreferenceWritePolicy::LegacyReadOnly
        );
        assert!(
            !serde_json::to_value(old)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("view_preferences_write_policy")
        );
    }

    #[test]
    fn cutover_candidate_validation_checks_settings_layers_and_source_before_publication() {
        let temp = tempfile::tempdir().unwrap();
        workdeck_pm::Repository::init(temp.path(), "WD").unwrap();
        let path = temp.path().join(".workdeck/config.toml");
        let original = "file_gap=2\n";
        fs::write(&path, original).unwrap();
        let user = temp.path().join("user.toml");
        fs::write(
            &user,
            "[review]\ntab_width=4\n[extension.demo]\nuser_value=1\n",
        )
        .unwrap();
        for candidate in [
            "[review]\ntab_width=0\n",
            "[paths]\ndata_dir='../conflicting-root'\n",
        ] {
            let candidate: toml::Value = toml::from_str(candidate).unwrap();
            assert!(
                validate_repo_config_candidate_with_user(temp.path(), &candidate, Some(&user))
                    .is_err()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
        let candidate: toml::Value = toml::from_str("[extension.demo]\nrepo_value=2\n").unwrap();
        validate_repo_config_candidate_with_user(temp.path(), &candidate, Some(&user)).unwrap();
        let source = repository_config_source(temp.path()).unwrap();
        let loaded = Config::load_with_repo_candidate(
            &source.path,
            Some(&user),
            None,
            false,
            Some(&candidate),
        )
        .unwrap();
        assert_eq!(loaded.review.tab_width, 4);
        assert_eq!(
            loaded.extension_config("demo"),
            serde_json::json!({"user_value":1,"repo_value":2})
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(!temp.path().join("../conflicting-root").exists());
    }

    #[test]
    fn cutover_ambiguous_authorities_error_while_malformed_pm_keeps_review_usable() {
        let temp = tempfile::tempdir().unwrap();
        let native = temp.path().join(".workdeck");
        fs::create_dir(&native).unwrap();
        fs::write(native.join("config.toml"), "file_gap=8\n").unwrap();
        fs::write(native.join("config.yml"), "malformed: [\n").unwrap();
        let config = Config::load_repository_with_context(temp.path(), None, None, false).unwrap();
        assert_eq!(config.review.file_gap, 8);
        assert_eq!(config.data_dir(temp.path()), native);
        assert!(
            config
                .startup_notices
                .iter()
                .any(|notice| notice.key == "planning:source-unavailable")
        );
        fs::create_dir_all(temp.path().join(".agents/workdeck/issues")).unwrap();
        assert!(
            resolve_repo_config_path(temp.path())
                .unwrap_err()
                .to_string()
                .contains("native and legacy")
        );
    }

    #[test]
    fn cutover_orphan_native_records_are_diagnosed_and_cannot_fall_back_to_legacy() {
        let temp = tempfile::tempdir().unwrap();
        let native = temp.path().join(".workdeck");
        fs::create_dir_all(native.join("issues/WD-1")).unwrap();
        fs::write(native.join("issues/WD-1/item.md"), "orphan native record").unwrap();
        let config = Config::load_repository_with_context(temp.path(), None, None, false).unwrap();
        assert_eq!(config.data_dir(temp.path()), native);
        assert!(
            config
                .startup_notices
                .iter()
                .any(|notice| notice.key == "planning:source-unavailable")
        );
        fs::create_dir_all(temp.path().join(".agents/workdeck/issues")).unwrap();
        assert!(
            resolve_repo_config_path(temp.path())
                .unwrap_err()
                .to_string()
                .contains("native and legacy")
        );
        assert!(!native.join("config.yml").exists());
    }

    #[test]
    fn cutover_pending_and_completed_markers_never_restore_retained_legacy_settings() {
        use workdeck_pm::{
            RequestId, Timestamp,
            migration::{MigrationFault, PreviewOptions, apply_with_faults, preview, resume},
        };
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join(".git")).unwrap();
        let legacy = temp.path().join(".agents/workdeck");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(
            legacy.join("config.toml"),
            "file_gap=6\n[paths]\ndata_dir='.agents/workdeck'\n",
        )
        .unwrap();
        let native = temp.path().join(".workdeck");
        let context = PreviewOptions {
            config: workdeck_pm::Config::new("WD").unwrap(),
            imported_at: "2026-09-09T00:00:00Z".parse::<Timestamp>().unwrap(),
        };
        let plan = preview(&legacy, &native, &context).unwrap();
        assert!(plan.complete, "{:?}", plan.blockers);
        let request = RequestId::new();
        apply_with_faults(&plan, &request, |point| {
            if point == MigrationFault::AfterBootstrap {
                Err(workdeck_pm::PmError::new(
                    workdeck_pm::ErrorCode::Canceled,
                    "test stop",
                ))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(
            resolve_repo_config_path(temp.path()).unwrap(),
            native.join("config.toml")
        );
        let pending = Config::load_repository_with_context(temp.path(), None, None, false).unwrap();
        assert_ne!(pending.review.file_gap, 6);
        assert_eq!(pending.data_dir(temp.path()), native);
        assert!(
            pending
                .startup_notices
                .iter()
                .any(|notice| notice.key == "planning:source-unavailable")
        );
        resume(&native, &request).unwrap();
        fs::write(
            legacy.join("config.toml"),
            "file_gap=10\n[paths]\ndata_dir='../external'\n",
        )
        .unwrap();
        let complete =
            Config::load_repository_with_context(temp.path(), None, None, false).unwrap();
        assert_eq!(complete.review.file_gap, 6);
        assert_eq!(complete.data_dir(temp.path()), native);
        assert!(
            !complete
                .startup_notices
                .iter()
                .any(|notice| notice.key == "planning:source-unavailable")
        );
        assert_eq!(
            complete.view_preferences_config_path,
            Some(native.join("config.toml"))
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
        assert_eq!(config.paths.data_dir, PathBuf::from(".workdeck"));
        assert_eq!(config.review.mode, "auto");
        assert_eq!(config.review.tab_width, 4);
        assert!(config.review.line_numbers);
        assert!(config.review.menu_bar);
        assert!(!config.review.copy_decorations);
        assert!(config.prompt_save_view_preferences);
    }

    #[test]
    fn prompt_save_view_preferences_is_a_layered_top_level_policy() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("user.toml");
        let repo_config = directory.path().join("repo.toml");
        fs::write(&user_config, "prompt_save_view_preferences = false\n").unwrap();
        let user = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(!user.prompt_save_view_preferences);

        fs::write(&repo_config, "prompt_save_view_preferences = true\n").unwrap();
        let layered = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(layered.prompt_save_view_preferences);
        assert_eq!(
            layered.view_preferences_config_path.as_deref(),
            Some(repo_config.as_path())
        );
    }

    #[test]
    fn interactive_view_save_round_trips_into_the_nested_runtime_model() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("config.toml");
        let repo_config = directory.path().join("missing-repo.toml");
        fs::write(
            &user_config,
            "[ui]\ntheme = \"old\"\n\n[review]\nwrap_lines = false\n",
        )
        .unwrap();
        workdeck_core::save_global_view_preferences(
            &workdeck_core::PersistedViewPreferences {
                mode: workdeck_core::InputLayoutMode::Stack,
                theme: Some("github-light-default".into()),
                show_line_numbers: false,
                wrap_lines: true,
                show_hunk_headers: false,
                show_menu_bar: false,
                show_agent_notes: true,
                copy_decorations: true,
                cursor_line: workdeck_core::InputCursorLine::Number,
            },
            Some(&user_config),
        )
        .unwrap();

        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(config.ui.theme, "github-light-default");
        assert_eq!(config.review.mode, "stack");
        assert!(!config.review.line_numbers);
        assert!(config.review.wrap_lines);
        assert!(!config.review.hunk_headers);
        assert!(!config.review.menu_bar);
        assert!(config.review.agent_notes);
        assert!(config.review.copy_decorations);
        assert_eq!(config.review.cursor_line, "number");

        fs::write(&repo_config, "[review]\nwrap_lines = false\n").unwrap();
        let layered = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(!layered.review.wrap_lines);
        assert_eq!(layered.review.mode, "stack");
    }

    #[test]
    fn config_reference_catalog_matches_the_pinned_runtime_surface() {
        assert_eq!(
            CONFIG_REFERENCE_OPTIONS
                .iter()
                .map(|option| option.key)
                .collect::<Vec<_>>(),
            [
                "mode",
                "cursor_line",
                "vcs",
                "theme",
                "watch",
                "exclude_untracked",
                "line_numbers",
                "tab_width",
                "file_gap",
                "hunk_gap",
                "wrap_lines",
                "hunk_headers",
                "menu_bar",
                "sidebar",
                "agent_notes",
                "copy_decorations",
                "prompt_save_view_preferences",
                "transparent_background",
                "color_moved",
            ]
        );
        assert_eq!(
            CONFIG_COMMAND_SECTIONS
                .iter()
                .map(|(section, _)| *section)
                .collect::<Vec<_>>(),
            ["vcs", "show", "stash-show", "diff", "patch", "difftool"]
        );
        let transparent = CONFIG_REFERENCE_OPTIONS
            .iter()
            .find(|option| option.key == "transparent_background")
            .unwrap();
        assert_eq!(
            transparent.runtime_keys,
            ["transparentBackground", "transparent_background"]
        );
        assert_eq!(
            transparent.aliases,
            [ConfigReferenceAlias {
                key: "transparentBackground",
                deprecated: true,
            }]
        );
        assert_eq!(CONFIG_REFERENCE_CUSTOM_THEME.table, "custom_theme");
        assert_eq!(CONFIG_REFERENCE_CUSTOM_THEME.named_theme_table, "themes");
        assert_eq!(
            CONFIG_REFERENCE_CUSTOM_THEME.default_base,
            "github-dark-default"
        );
        assert_eq!(
            CONFIG_REFERENCE_CUSTOM_THEME.base_values,
            BUNDLED_SHIKI_THEME_IDS
        );
        assert_eq!(CONFIG_REFERENCE_EXTENSIONS.table, "extensions");
        assert_eq!(CONFIG_REFERENCE_EXTENSIONS.per_extension_table, "extension");
        assert_eq!(
            CONFIG_REFERENCE_EXTENSIONS
                .keys
                .iter()
                .map(|key| key.key)
                .collect::<Vec<_>>(),
            ["extensions.enabled", "extensions.paths"]
        );
    }

    #[test]
    fn review_config_layers_root_command_pager_and_repository_in_order() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("user.toml");
        let repo_config = directory.path().join("repo.toml");
        fs::write(
            &user_config,
            concat!(
                "mode = 'split'\n",
                "line_numbers = false\n",
                "watch = true\n",
                "[show]\nmode = 'stack'\ntab_width = 8\n",
                "[pager]\nmode = 'auto'\nmenu_bar = true\n",
            ),
        )
        .unwrap();
        fs::write(
            &repo_config,
            concat!(
                "mode = 'stack'\n",
                "[show]\nline_numbers = true\n",
                "[pager]\nmode = 'split'\n",
            ),
        )
        .unwrap();

        let config = Config::load_from_paths_for_review(
            &repo_config,
            Some(&user_config),
            Some("show"),
            true,
        )
        .unwrap();
        assert_eq!(config.review.mode, "split");
        assert!(config.review.line_numbers);
        assert!(config.review.watch);
        assert_eq!(config.review.tab_width, 8);
        assert!(config.review.menu_bar);

        let plain = Config::load_from_paths_for_review(
            &directory.path().join("missing.toml"),
            None,
            Some("patch"),
            false,
        )
        .unwrap();
        let pager = Config::load_from_paths_for_review(
            &directory.path().join("missing.toml"),
            None,
            Some("patch"),
            true,
        )
        .unwrap();
        assert!(plain.review.menu_bar);
        assert_eq!(plain.ui.theme, "github-dark-default");
        assert!(!pager.review.menu_bar);
    }

    #[test]
    fn flat_scalar_preferences_ignore_invalid_values_but_keep_nested_schema_strict() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("user.toml");
        let repo_config = directory.path().join("missing.toml");
        fs::write(
            &user_config,
            concat!(
                "mode = 7\n",
                "cursor_line = false\n",
                "vcs = 7\n",
                "theme = false\n",
                "watch = 'yes'\n",
                "sidebar = 'always'\n",
                "prompt_save_view_preferences = 'yes'\n",
                "transparent_background = 'yes'\n",
            ),
        )
        .unwrap();
        let config = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(config.review.mode, "auto");
        assert_eq!(config.review.cursor_line, "row");
        assert_eq!(config.review.vcs, "auto");
        assert_eq!(config.explicit_vcs_id, None);
        assert_eq!(config.ui.theme, "github-dark-default");
        assert!(!config.review.watch);
        assert_eq!(config.review.sidebar, ReviewSidebar::Auto);
        assert!(config.prompt_save_view_preferences);
        assert!(!config.review.transparent_background);

        fs::write(&user_config, "[review]\nmode = 7\n").unwrap();
        assert!(Config::load_from_paths(&repo_config, Some(&user_config)).is_err());
    }

    #[test]
    fn flat_numeric_preferences_reject_wrong_types_and_bounds_at_every_level() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("user.toml");
        let repo_config = directory.path().join("missing.toml");
        for (source, key) in [
            ("tab_width = 0\n", "tab_width"),
            ("tab_width = 17\n", "tab_width"),
            ("tab_width = '4'\n", "tab_width"),
            ("file_gap = -1\n", "file_gap"),
            ("file_gap = 9\n", "file_gap"),
            ("hunk_gap = '2'\n", "hunk_gap"),
            ("[show]\nhunk_gap = 9\n", "hunk_gap"),
            ("[pager]\nfile_gap = '2'\n", "file_gap"),
        ] {
            fs::write(&user_config, source).unwrap();
            let error = Config::load_from_paths_for_review(
                &repo_config,
                Some(&user_config),
                Some("show"),
                true,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(key), "{source:?}: {error}");
        }
    }

    #[test]
    fn deprecated_transparent_background_alias_keeps_historical_precedence() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("user.toml");
        let repo_config = directory.path().join("missing.toml");
        fs::write(
            &user_config,
            "transparentBackground = true\ntransparent_background = false\n",
        )
        .unwrap();
        let legacy = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(legacy.review.transparent_background);

        fs::write(
            &user_config,
            "transparentBackground = 'invalid'\ntransparent_background = false\n",
        )
        .unwrap();
        let current = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(!current.review.transparent_background);
    }

    #[test]
    fn explicit_vcs_provenance_is_distinct_from_the_git_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("user.toml");
        let repo_config = directory.path().join("repo.toml");

        let detected = Config::load_from_paths(&repo_config, None).unwrap();
        assert_eq!(detected.review.vcs, "auto");
        assert_eq!(detected.explicit_vcs_id, None);

        fs::write(&user_config, "vcs = 'hg'\n").unwrap();
        fs::write(&repo_config, "vcs = 7\n").unwrap();
        let configured = Config::load_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert_eq!(configured.review.vcs, "hg");
        assert_eq!(configured.explicit_vcs_id.as_deref(), Some("hg"));
    }

    #[test]
    fn extension_bootstrap_ignores_unrelated_legacy_custom_theme_failure() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("user.toml");
        let repo_config = directory.path().join("repo.toml");
        fs::write(
            &user_config,
            concat!(
                "theme = 'custom'\n",
                "[extensions]\npaths = ['/user/tools']\n",
                "[extension.tools]\ntoken = 'user'\n",
            ),
        )
        .unwrap();
        fs::write(
            &repo_config,
            concat!(
                "[extensions]\nenabled = true\npaths = ['./repo-tools']\n",
                "[extension.tools]\ntoken = 'repo'\n",
            ),
        )
        .unwrap();

        let bootstrap =
            Config::load_extension_bootstrap_from_paths(&repo_config, Some(&user_config)).unwrap();
        assert!(bootstrap.resolved_extensions.enabled);
        assert_eq!(
            bootstrap.resolved_extensions.paths,
            [PathBuf::from("/user/tools")]
        );
        assert_eq!(
            bootstrap.resolved_extensions.repo_paths,
            [PathBuf::from("./repo-tools")]
        );
        assert_eq!(
            bootstrap.extension_config("tools"),
            serde_json::json!({ "token": "repo" })
        );
        assert!(Config::load_from_paths(&repo_config, Some(&user_config)).is_err());
    }

    #[test]
    fn review_menu_and_copy_preferences_deserialize_explicitly() {
        let config: Config = toml::from_str(
            r#"
            [review]
            menu_bar = false
            copy_decorations = true
            "#,
        )
        .unwrap();

        assert!(!config.review.menu_bar);
        assert!(config.review.copy_decorations);
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

    fn load_theme_projection(user: &str, repo: Option<&str>) -> Result<serde_json::Value> {
        let dir = tempfile::tempdir().unwrap();
        let user_config = dir.path().join("user-config.toml");
        let repo_config = dir.path().join("repo-config.toml");
        fs::write(&user_config, user).unwrap();
        if let Some(repo) = repo {
            fs::write(&repo_config, repo).unwrap();
        }
        let config = Config::load_from_paths(&repo_config, Some(&user_config))?;
        let theme = match config.ui.theme.as_str() {
            // This fixture isolates Hunk's custom-theme parser. Workdeck's pre-existing
            // terminal-adaptive default remains auto until startup detection is ported.
            "auto" => "github-dark-default",
            theme => theme,
        };
        Ok(serde_json::json!({
            "theme": theme,
            "customThemes": config.custom_themes,
            "startupNotices": if config.startup_notices.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::to_value(config.startup_notices).unwrap()
            },
            "viewPreferencesConfigPath": if config.view_preferences_config_path.as_deref()
                == Some(repo_config.as_path())
            {
                "repo"
            } else {
                "user"
            },
        }))
    }

    #[test]
    fn migrated_theme_guide_examples_load_with_workdeck_configuration_semantics() {
        let guide = include_str!("../../../docs/themes.md");
        let examples: Vec<_> = guide
            .split("```toml\n")
            .skip(1)
            .map(|section| section.split_once("```").unwrap().0)
            .collect();
        assert_eq!(examples.len(), 3);
        let built_in = load_theme_projection(examples[0], None).unwrap();
        assert_eq!(built_in["theme"], "github-dark-default");
        let custom = load_theme_projection(examples[1], None).unwrap();
        assert_eq!(custom["theme"], "custom");
        assert_eq!(custom["customThemes"][0]["id"], "custom");
        assert_eq!(custom["customThemes"][0]["base"], "catppuccin-mocha");
        assert_eq!(custom["customThemes"][0]["accent"], "#7fd1ff");
        assert_eq!(
            custom["customThemes"][0]["syntaxScopes"]["entity.name.function"],
            "#8ed4ff"
        );
        let named = load_theme_projection(examples[2], None).unwrap();
        assert_eq!(named["theme"], "ocean");
        assert_eq!(named["customThemes"][0]["id"], "ocean");
        assert_eq!(named["customThemes"][1]["id"], "paper-review");
        for projection in [&built_in, &custom, &named] {
            assert!(projection["startupNotices"].is_null());
        }
    }

    #[test]
    fn custom_theme_config_matches_both_pinned_hunk_oracles() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/config-themes.json"
        ))
        .unwrap();
        assert_eq!(oracle["pinsAgree"], true);
        let expected = &oracle["expected"];

        assert_eq!(
            load_theme_projection(
                concat!(
                    "[ui]\ntheme = 'custom'\n",
                    "[custom_theme]\nbase = 'github-dark-default'\n",
                    "label = 'Global Custom'\naccent = '#123456'\n",
                    "[custom_theme.syntax_scopes]\n'keyword.control' = '#abcdef'\n",
                ),
                Some(concat!(
                    "[ui]\ntheme = 'custom'\n",
                    "[custom_theme]\nlabel = 'Repo Custom'\npanel = '#654321'\n",
                    "[custom_theme.syntax_scopes]\n'string.quoted' = '#fedcba'\n",
                )),
            )
            .unwrap(),
            expected["layered"],
        );
        assert_eq!(
            load_theme_projection(
                concat!(
                    "[ui]\ntheme = 'ocean'\n",
                    "[custom_theme]\nbase = 'github-dark-default'\n",
                    "[themes.ocean]\nbase = 'nord'\nlabel = 'Ocean'\naccent = '#ABCDEF'\n",
                    "[themes.ocean.syntax_scopes]\n'keyword.control' = '#ABCDEF'\n",
                    "[themes.team_theme]\nbase = 'dracula'\n",
                ),
                None,
            )
            .unwrap(),
            expected["declarationOrder"],
        );
        assert_eq!(
            load_theme_projection(
                "[custom_theme]\naccent = '#123456'\n[themes.custom]\naccent = '#654321'\n",
                None,
            )
            .unwrap(),
            expected["collision"],
        );
        assert_eq!(
            load_theme_projection(
                concat!(
                    "[themes.'Ocean Dark']\nbase = 'nord'\n",
                    "[themes.dracula]\nbase = 'nord'\n",
                    "[themes.ocean]\nbase = 'nord'\n",
                ),
                None,
            )
            .unwrap(),
            expected["invalidIds"],
        );
        assert_eq!(
            load_theme_projection(
                concat!(
                    "[custom_theme.syntax]\ncomment = '#FFFFFF'\n",
                    "[custom_theme.syntax_scopes]\ncomment = '#EEEEEE'\n",
                ),
                None,
            )
            .unwrap(),
            expected["legacySyntax"],
        );
        assert_eq!(
            load_theme_projection(
                "[themes.ocean]\naccent = '#123456'\n",
                Some("[themes.ocean]\npanel = '#654321'\n"),
            )
            .unwrap(),
            expected["defaultBaseOnMerge"],
        );
        assert_eq!(
            load_theme_projection(
                concat!(
                    "[themes.ocean]\nbase = 'nord'\nlabel = 'Ocean'\naccent = '#123456'\n",
                    "[themes.ocean.syntax_scopes]\n'keyword.control' = '#abcdef'\n",
                ),
                Some(concat!(
                    "[themes.ocean]\nlabel = 'Repo Ocean'\npanel = '#654321'\n",
                    "[themes.ocean.syntax_scopes]\n'string.quoted' = '#fedcba'\n",
                    "[themes.repo-only]\nbase = 'dracula'\n",
                )),
            )
            .unwrap()["customThemes"],
            expected["namedLayered"],
        );
        assert_eq!(
            load_theme_projection(
                "[themes.'Ocean Dark']\nbase = 'nord'\n",
                Some("[themes.'Ocean Dark']\nbase = 'dracula'\n"),
            )
            .unwrap(),
            expected["noticeReplacement"],
        );
    }

    #[test]
    fn custom_theme_config_errors_match_the_pinned_hunk_oracle() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/config-themes.json"
        ))
        .unwrap();
        let expected = &oracle["expected"]["errors"];
        for (source, key) in [
            ("[ui]\ntheme = 'custom'\n", "selectedCustomWithoutTable"),
            ("[custom_theme]\nbase = 'unknown'\n", "invalidBase"),
            ("[custom_theme]\naccent = 'blue'\n", "invalidColor"),
            ("[themes.ocean]\naccent = 'blue'\n", "invalidNamedColor"),
            (
                "[custom_theme.syntax_scopes]\n'comment.line' = 'white'\n",
                "invalidScope",
            ),
            ("[custom_theme]\nsyntax = 'white'\n", "invalidSyntaxTable"),
            ("themes = 'ocean'\n", "invalidThemesTable"),
            ("[themes]\nocean = 'nord'\n", "invalidNamedTable"),
        ] {
            let error = load_theme_projection(source, None).unwrap_err().to_string();
            assert_eq!(error, expected[key], "fixture {key}");
        }
    }

    #[test]
    fn custom_theme_bases_accept_every_oracle_case_and_normalize_legacy_ids() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/config-themes.json"
        ))
        .unwrap();
        let expected = &oracle["expected"];
        for base in [
            "github-dark-default",
            "github-light-default",
            "dracula",
            "catppuccin-mocha",
        ] {
            let config =
                load_theme_projection(&format!("[custom_theme]\nbase = '{base}'\n"), None).unwrap();
            assert_eq!(config["customThemes"], expected["acceptedBases"][base],);
        }
        let legacy = load_theme_projection("[custom_theme]\nbase = 'graphite'\n", None).unwrap();
        assert_eq!(legacy["customThemes"], expected["legacyBase"]);
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
