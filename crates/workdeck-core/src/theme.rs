//! Provider-neutral theme catalog, custom-theme model, and registration validation.
//!
//! The catalog is pinned to Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Keeping it in
//! core gives configuration, extensions, syntax highlighting, and Ratatui one canonical source.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::StartupNotice;

/// Bundled Shiki/TextMate themes, in the selection order exposed by Hunk.
pub const BUNDLED_SHIKI_THEME_IDS: &[&str] = &[
    "andromeeda",
    "aurora-x",
    "ayu-dark",
    "ayu-light",
    "ayu-mirage",
    "catppuccin-frappe",
    "catppuccin-latte",
    "catppuccin-macchiato",
    "catppuccin-mocha",
    "dark-plus",
    "dracula",
    "dracula-soft",
    "everforest-dark",
    "everforest-light",
    "github-dark",
    "github-dark-default",
    "github-dark-dimmed",
    "github-dark-high-contrast",
    "github-light",
    "github-light-default",
    "github-light-high-contrast",
    "gruvbox-dark-hard",
    "gruvbox-dark-medium",
    "gruvbox-dark-soft",
    "gruvbox-light-hard",
    "gruvbox-light-medium",
    "gruvbox-light-soft",
    "horizon",
    "horizon-bright",
    "houston",
    "kanagawa-dragon",
    "kanagawa-lotus",
    "kanagawa-wave",
    "laserwave",
    "light-plus",
    "material-theme",
    "material-theme-darker",
    "material-theme-lighter",
    "material-theme-ocean",
    "material-theme-palenight",
    "min-dark",
    "min-light",
    "monokai",
    "night-owl",
    "night-owl-light",
    "nord",
    "one-dark-pro",
    "one-light",
    "plastic",
    "poimandres",
    "red",
    "rose-pine",
    "rose-pine-dawn",
    "rose-pine-moon",
    "slack-dark",
    "slack-ochin",
    "snazzy-light",
    "solarized-dark",
    "solarized-light",
    "synthwave-84",
    "tokyo-night",
    "vesper",
    "vitesse-black",
    "vitesse-dark",
    "vitesse-light",
];

/// Removed pre-refactor theme IDs and their closest bundled replacements.
pub const LEGACY_THEME_ID_ALIASES: &[(&str, &str)] = &[
    ("graphite", "github-dark-default"),
    ("midnight", "github-dark-dimmed"),
    ("paper", "github-light-default"),
    ("ember", "dark-plus"),
    ("zenburn", "everforest-dark"),
];

/// Resolve a removed theme ID, leaving current and unknown IDs unchanged.
#[must_use]
pub fn resolve_legacy_theme_id(theme_id: Option<&str>) -> Option<&str> {
    let theme_id = theme_id?;
    Some(
        LEGACY_THEME_ID_ALIASES
            .iter()
            .find_map(|(legacy, current)| (*legacy == theme_id).then_some(*current))
            .unwrap_or(theme_id),
    )
}

/// Resolve a current or legacy ID only when it names a bundled theme.
#[must_use]
pub fn resolve_bundled_shiki_theme_id(theme_id: Option<&str>) -> Option<&str> {
    let resolved = resolve_legacy_theme_id(theme_id)?;
    BUNDLED_SHIKI_THEME_IDS
        .contains(&resolved)
        .then_some(resolved)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BundledShikiThemeDiffColors {
    pub added: Option<&'static str>,
    pub removed: Option<&'static str>,
    pub modified: Option<&'static str>,
}

const BUNDLED_SHIKI_THEME_BACKGROUNDS: &[(&str, &str)] = &[
    ("andromeeda", "#23262e"),
    ("aurora-x", "#07090f"),
    ("ayu-dark", "#10141c"),
    ("ayu-light", "#fcfcfc"),
    ("ayu-mirage", "#242936"),
    ("catppuccin-frappe", "#303446"),
    ("catppuccin-latte", "#eff1f5"),
    ("catppuccin-macchiato", "#24273a"),
    ("catppuccin-mocha", "#1e1e2e"),
    ("dark-plus", "#1e1e1e"),
    ("dracula", "#282a36"),
    ("dracula-soft", "#282a36"),
    ("everforest-dark", "#2d353b"),
    ("everforest-light", "#fdf6e3"),
    ("github-dark", "#24292e"),
    ("github-dark-default", "#0d1117"),
    ("github-dark-dimmed", "#22272e"),
    ("github-dark-high-contrast", "#0a0c10"),
    ("github-light", "#ffffff"),
    ("github-light-default", "#ffffff"),
    ("github-light-high-contrast", "#ffffff"),
    ("gruvbox-dark-hard", "#1d2021"),
    ("gruvbox-dark-medium", "#282828"),
    ("gruvbox-dark-soft", "#32302f"),
    ("gruvbox-light-hard", "#f9f5d7"),
    ("gruvbox-light-medium", "#fbf1c7"),
    ("gruvbox-light-soft", "#f2e5bc"),
    ("horizon", "#1c1e26"),
    ("horizon-bright", "#fdf0ed"),
    ("houston", "#17191e"),
    ("kanagawa-dragon", "#181616"),
    ("kanagawa-lotus", "#f2ecbc"),
    ("kanagawa-wave", "#1f1f28"),
    ("laserwave", "#27212e"),
    ("light-plus", "#ffffff"),
    ("material-theme", "#263238"),
    ("material-theme-darker", "#212121"),
    ("material-theme-lighter", "#fafafa"),
    ("material-theme-ocean", "#0f111a"),
    ("material-theme-palenight", "#292d3e"),
    ("min-dark", "#1f1f1f"),
    ("min-light", "#ffffff"),
    ("monokai", "#272822"),
    ("night-owl", "#011627"),
    ("night-owl-light", "#fbfbfb"),
    ("nord", "#2e3440"),
    ("one-dark-pro", "#282c34"),
    ("one-light", "#fafafa"),
    ("plastic", "#21252b"),
    ("poimandres", "#1b1e28"),
    ("red", "#390000"),
    ("rose-pine", "#191724"),
    ("rose-pine-dawn", "#faf4ed"),
    ("rose-pine-moon", "#232136"),
    ("slack-dark", "#222222"),
    ("slack-ochin", "#ffffff"),
    ("snazzy-light", "#fafbfc"),
    ("solarized-dark", "#002b36"),
    ("solarized-light", "#fdf6e3"),
    ("synthwave-84", "#262335"),
    ("tokyo-night", "#1a1b26"),
    ("vesper", "#101010"),
    ("vitesse-black", "#000000"),
    ("vitesse-dark", "#121212"),
    ("vitesse-light", "#ffffff"),
];

const BUNDLED_SHIKI_THEME_FOREGROUNDS: &[(&str, &str)] = &[
    ("andromeeda", "#d5ced9"),
    ("ayu-dark", "#bfbdb6"),
    ("ayu-light", "#5c6166"),
    ("ayu-mirage", "#cccac2"),
    ("catppuccin-frappe", "#c6d0f5"),
    ("catppuccin-latte", "#4c4f69"),
    ("catppuccin-macchiato", "#cad3f5"),
    ("catppuccin-mocha", "#cdd6f4"),
    ("dark-plus", "#d4d4d4"),
    ("dracula", "#f8f8f2"),
    ("dracula-soft", "#f6f6f4"),
    ("everforest-dark", "#d3c6aa"),
    ("everforest-light", "#5c6a72"),
    ("github-dark", "#e1e4e8"),
    ("github-dark-default", "#e6edf3"),
    ("github-dark-dimmed", "#adbac7"),
    ("github-dark-high-contrast", "#f0f3f6"),
    ("github-light", "#24292e"),
    ("github-light-default", "#1f2328"),
    ("github-light-high-contrast", "#0e1116"),
    ("gruvbox-dark-hard", "#ebdbb2"),
    ("gruvbox-dark-medium", "#ebdbb2"),
    ("gruvbox-dark-soft", "#ebdbb2"),
    ("gruvbox-light-hard", "#3c3836"),
    ("gruvbox-light-medium", "#3c3836"),
    ("gruvbox-light-soft", "#3c3836"),
    ("houston", "#eef0f9"),
    ("kanagawa-dragon", "#c5c9c5"),
    ("kanagawa-lotus", "#545464"),
    ("kanagawa-wave", "#dcd7ba"),
    ("laserwave", "#ffffff"),
    ("light-plus", "#000000"),
    ("material-theme", "#eeffff"),
    ("material-theme-darker", "#eeffff"),
    ("material-theme-lighter", "#90a4ae"),
    ("material-theme-ocean", "#babed8"),
    ("material-theme-palenight", "#babed8"),
    ("min-light", "#212121"),
    ("monokai", "#f8f8f2"),
    ("night-owl", "#d6deeb"),
    ("night-owl-light", "#403f53"),
    ("nord", "#d8dee9"),
    ("one-dark-pro", "#abb2bf"),
    ("one-light", "#383a42"),
    ("plastic", "#a9b2c3"),
    ("poimandres", "#a6accd"),
    ("red", "#f8f8f8"),
    ("rose-pine", "#e0def4"),
    ("rose-pine-dawn", "#575279"),
    ("rose-pine-moon", "#e0def4"),
    ("slack-dark", "#e6e6e6"),
    ("slack-ochin", "#000000"),
    ("snazzy-light", "#565869"),
    ("solarized-dark", "#839496"),
    ("solarized-light", "#657b83"),
    ("tokyo-night", "#a9b1d6"),
    ("vesper", "#ffffff"),
    ("vitesse-black", "#dbd7ca"),
    ("vitesse-dark", "#dbd7ca"),
    ("vitesse-light", "#393a34"),
];

const BUNDLED_SHIKI_THEME_DIFF_COLORS: &[(&str, BundledShikiThemeDiffColors)] = &[
    ("andromeeda", diff("#9bc53d", "#fc644d", "#5bc0eb")),
    ("aurora-x", diff("#64d389", "#dd5074", "#c778db")),
    ("ayu-dark", diff("#70bf56", "#f26d78", "#73b8ff")),
    ("ayu-light", diff("#6cbf43", "#ff7383", "#478acc")),
    ("ayu-mirage", diff("#87d96c", "#f27983", "#80bfff")),
    ("catppuccin-frappe", diff("#a6d189", "#e78284", "#e5c890")),
    ("catppuccin-latte", diff("#40a02b", "#d20f39", "#df8e1d")),
    (
        "catppuccin-macchiato",
        diff("#a6da95", "#ed8796", "#eed49f"),
    ),
    ("catppuccin-mocha", diff("#a6e3a1", "#f38ba8", "#f9e2af")),
    ("dracula", diff("#50fa7b", "#ff5555", "#8be9fd")),
    ("dracula-soft", diff("#50fa7b", "#ff5555", "#8be9fd")),
    ("everforest-dark", diff("#899c40", "#da6362", "#5a93a2")),
    ("everforest-light", diff("#8da101", "#f1706f", "#3a94c5")),
    ("github-dark", diff("#28a745", "#ea4a5a", "#2188ff")),
    ("github-dark-default", diff("#2ea043", "#f85149", "#bb8009")),
    ("github-dark-dimmed", diff("#46954a", "#e5534b", "#ae7c14")),
    (
        "github-dark-high-contrast",
        diff("#09b43a", "#ff6a69", "#e09b13"),
    ),
    ("github-light", diff("#28a745", "#d73a49", "#2188ff")),
    (
        "github-light-default",
        diff("#116329", "#cf222e", "#9a6700"),
    ),
    (
        "github-light-high-contrast",
        diff("#26a148", "#ee5a5d", "#b58407"),
    ),
    ("gruvbox-dark-hard", diff("#b8bb26", "#fb4934", "#83a598")),
    ("gruvbox-dark-medium", diff("#b8bb26", "#fb4934", "#83a598")),
    ("gruvbox-dark-soft", diff("#b8bb26", "#fb4934", "#83a598")),
    ("gruvbox-light-hard", diff("#79740e", "#9d0006", "#076678")),
    (
        "gruvbox-light-medium",
        diff("#79740e", "#9d0006", "#076678"),
    ),
    ("gruvbox-light-soft", diff("#79740e", "#9d0006", "#076678")),
    ("horizon", diff("#09f7a0", "#f43e5c", "#21bfc2")),
    ("horizon-bright", diff("#29d398", "#f43e5c", "#af5427")),
    ("houston", diff("#4bf3c8", "#f06788", "#54b9ff")),
    ("kanagawa-dragon", diff("#76946a", "#c34043", "#dca561")),
    ("kanagawa-lotus", diff("#6e915f", "#d7474b", "#4d699b")),
    ("kanagawa-wave", diff("#76946a", "#c34043", "#dca561")),
    ("laserwave", diff("#74dfc4", "#eb64b9", "#40b4c4")),
    ("material-theme", diff("#c3e88d", "#f07178", "#82aaff")),
    (
        "material-theme-darker",
        diff("#c3e88d", "#f07178", "#82aaff"),
    ),
    (
        "material-theme-lighter",
        diff("#39adb5", "#e53935", "#6182b8"),
    ),
    (
        "material-theme-ocean",
        diff("#c3e88d", "#f07178", "#82aaff"),
    ),
    (
        "material-theme-palenight",
        diff("#c3e88d", "#f07178", "#82aaff"),
    ),
    (
        "min-light",
        BundledShikiThemeDiffColors {
            added: Some("#77cc00"),
            removed: Some("#d32f2f"),
            modified: None,
        },
    ),
    ("monokai", diff("#86b42b", "#c4265e", "#6a7ec8")),
    ("night-owl", diff("#9ccc65", "#ef5350", "#e2b93d")),
    ("night-owl-light", diff("#08916a", "#f76e6e", "#288ed7")),
    ("nord", diff("#a3be8c", "#bf616a", "#ebcb8b")),
    ("one-dark-pro", diff("#109868", "#e05561", "#948b60")),
    (
        "one-light",
        BundledShikiThemeDiffColors {
            added: Some("#00809b"),
            removed: None,
            modified: None,
        },
    ),
    ("plastic", diff("#98c379", "#e06c75", "#d19a66")),
    ("poimandres", diff("#5fb3a1", "#d0679d", "#add7ff")),
    ("rose-pine", diff("#9ccfd8", "#eb6f92", "#ebbcba")),
    ("rose-pine-dawn", diff("#56949f", "#b4637a", "#d7827e")),
    ("rose-pine-moon", diff("#9ccfd8", "#eb6f92", "#ea9a97")),
    ("slack-ochin", diff("#91b859", "#e53935", "#ecb22e")),
    ("snazzy-light", diff("#2dae58", "#ff5c57", "#00a39f")),
    ("solarized-dark", diff("#859900", "#dc322f", "#268bd2")),
    ("solarized-light", diff("#859900", "#dc322f", "#268bd2")),
    ("synthwave-84", diff("#0beb99", "#fa2e46", "#b893ce")),
    ("tokyo-night", diff("#41a6b5", "#db4b4b", "#6183bb")),
    ("vesper", diff("#99ffe4", "#ff8080", "#ffc799")),
    ("vitesse-black", diff("#4d9375", "#cb7676", "#6394bf")),
    ("vitesse-dark", diff("#4d9375", "#cb7676", "#6394bf")),
    ("vitesse-light", diff("#1e754f", "#ab5959", "#296aa3")),
];

const fn diff(
    added: &'static str,
    removed: &'static str,
    modified: &'static str,
) -> BundledShikiThemeDiffColors {
    BundledShikiThemeDiffColors {
        added: Some(added),
        removed: Some(removed),
        modified: Some(modified),
    }
}

fn lookup(table: &'static [(&'static str, &'static str)], theme_id: &str) -> Option<&'static str> {
    table
        .iter()
        .find_map(|(id, value)| (*id == theme_id).then_some(*value))
}

#[must_use]
pub fn get_bundled_shiki_theme_background(theme_id: Option<&str>) -> Option<&'static str> {
    lookup(BUNDLED_SHIKI_THEME_BACKGROUNDS, theme_id?)
}

#[must_use]
pub fn get_bundled_shiki_theme_foreground(theme_id: Option<&str>) -> Option<&'static str> {
    lookup(BUNDLED_SHIKI_THEME_FOREGROUNDS, theme_id?)
}

#[must_use]
pub fn get_bundled_shiki_theme_diff_colors(
    theme_id: Option<&str>,
) -> Option<BundledShikiThemeDiffColors> {
    let theme_id = theme_id?;
    BUNDLED_SHIKI_THEME_DIFF_COLORS
        .iter()
        .find_map(|(id, value)| (*id == theme_id).then_some(*value))
}

/// Classify a bundled theme from its declared editor surface.
#[must_use]
pub fn bundled_shiki_theme_is_light(theme_id: Option<&str>) -> Option<bool> {
    let theme_id = resolve_bundled_shiki_theme_id(theme_id)?;
    let background = get_bundled_shiki_theme_background(Some(theme_id))?;
    let channels = [
        u8::from_str_radix(&background[1..3], 16).ok()?,
        u8::from_str_radix(&background[3..5], 16).ok()?,
        u8::from_str_radix(&background[5..7], 16).ok()?,
    ];
    let linear = channels.map(|channel| {
        let component = f64::from(channel) / 255.0;
        if component <= 0.039_28 {
            component / 12.92
        } else {
            ((component + 0.055) / 1.055).powf(2.4)
        }
    });
    Some(0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2] > 0.5)
}

/// Every configurable render color, in Hunk declaration/validation order.
pub const CUSTOM_THEME_COLOR_KEYS: &[&str] = &[
    "background",
    "panel",
    "panelAlt",
    "border",
    "accent",
    "accentMuted",
    "text",
    "muted",
    "addedBg",
    "removedBg",
    "movedAddedBg",
    "movedRemovedBg",
    "contextBg",
    "addedContentBg",
    "removedContentBg",
    "contextContentBg",
    "addedSignColor",
    "removedSignColor",
    "lineNumberBg",
    "lineNumberFg",
    "selectedHunk",
    "badgeAdded",
    "badgeRemoved",
    "badgeNeutral",
    "fileNew",
    "fileDeleted",
    "fileRenamed",
    "fileModified",
    "fileUntracked",
    "noteBorder",
    "noteBackground",
    "noteTitleBackground",
    "noteTitleText",
];

pub const LEGACY_CUSTOM_THEME_ID: &str = "custom";

pub const LEGACY_CUSTOM_SYNTAX_COLOR_KEYS: &[&str] = &[
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

/// Translate deprecated semantic syntax roles into approximate TextMate selectors.
#[must_use]
pub fn legacy_custom_syntax_colors_to_scopes(
    syntax: &IndexMap<String, String>,
) -> IndexMap<String, String> {
    let mut scopes = IndexMap::new();
    for role in LEGACY_CUSTOM_SYNTAX_COLOR_KEYS {
        let Some(color) = syntax.get(*role).filter(|color| !color.is_empty()) else {
            continue;
        };
        for scope in legacy_syntax_role_scopes(role) {
            scopes.insert((*scope).to_owned(), color.clone());
        }
    }
    scopes
}

/// Layer exact scopes after translated legacy roles without changing declaration positions.
#[must_use]
pub fn resolve_custom_syntax_scope_overrides(
    syntax: &IndexMap<String, String>,
    syntax_scopes: &IndexMap<String, String>,
) -> IndexMap<String, String> {
    let mut resolved = legacy_custom_syntax_colors_to_scopes(syntax);
    for (scope, color) in syntax_scopes {
        resolved.insert(scope.clone(), color.clone());
    }
    resolved
}

/// One normalized `[themes.<id>]` table or native extension registration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NamedCustomThemeConfig {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_alt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_muted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_added_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_removed_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_content_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_content_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_content_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_sign_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_sign_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_number_bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_number_fg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_hunk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge_added: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge_removed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge_neutral: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_new: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_deleted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_renamed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_untracked: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_border: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_title_background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_title_text: Option<String>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub syntax: IndexMap<String, String>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub syntax_scopes: IndexMap<String, String>,
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

/// Build the single named custom-theme list used by translated theme tests.
///
/// This is the Rust equivalent of Hunk's `createTestCustomThemes` helper from
/// `test/helpers/theme-helpers.ts` (2c00f435, MIT, Modem Labs Inc.; see
/// `THIRD_PARTY_NOTICES`). The caller's complete theme payload is retained and
/// only its test identifier is replaced.
#[must_use]
pub fn create_test_custom_themes(
    mut theme: NamedCustomThemeConfig,
    id: impl Into<String>,
) -> Vec<NamedCustomThemeConfig> {
    theme.id = id.into();
    vec![theme]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCustomTheme {
    pub extension_id: String,
    pub theme: Value,
}

impl RegisteredCustomTheme {
    #[must_use]
    pub fn new(extension_id: impl Into<String>, theme: impl Serialize) -> Self {
        let mut theme = serde_json::to_value(theme).expect("custom theme is JSON serializable");
        if let Some(theme) = theme.as_object_mut() {
            theme.retain(|_, value| !value.is_null());
        }
        Self {
            extension_id: extension_id.into(),
            theme,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomThemeFieldIssue {
    pub key: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionCustomThemes {
    pub themes: Vec<NamedCustomThemeConfig>,
    pub notices: Vec<StartupNotice>,
}

#[must_use]
pub fn describe_custom_theme_id_issue(id: &Value) -> Option<&'static str> {
    let Some(id) = id.as_str().filter(|id| !id.is_empty()) else {
        return Some("theme ids must be non-empty strings");
    };
    let mut previous_separator = true;
    for byte in id.bytes() {
        let separator = matches!(byte, b'-' | b'_');
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || separator)
            || (separator && previous_separator)
        {
            return Some("theme ids must be lowercase words separated by - or _");
        }
        previous_separator = separator;
    }
    if previous_separator {
        return Some("theme ids must be lowercase words separated by - or _");
    }
    if id == "auto" || BUNDLED_SHIKI_THEME_IDS.contains(&id) {
        return Some("that id belongs to a built-in theme");
    }
    None
}

#[must_use]
pub fn describe_theme_color_issue(value: &Value) -> Option<&'static str> {
    value
        .as_str()
        .filter(|value| {
            value.len() == 7
                && value.starts_with('#')
                && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .map_or(Some("must be a hex color like #112233"), |_| None)
}

#[must_use]
pub fn normalize_theme_color_value(value: &str) -> String {
    value.to_ascii_lowercase()
}

pub fn resolve_theme_base(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .and_then(|value| resolve_bundled_shiki_theme_id(Some(value)))
        .ok_or_else(|| {
            format!(
                "must be a built-in theme id. Known themes: {}",
                BUNDLED_SHIKI_THEME_IDS.join(", ")
            )
        })
}

#[must_use]
pub fn describe_custom_theme_field_issue(theme: &Value) -> Option<CustomThemeFieldIssue> {
    let Some(theme) = theme.as_object() else {
        return Some(CustomThemeFieldIssue {
            key: "theme".into(),
            reason: "must be an object".into(),
        });
    };
    if theme.get("label").is_some_and(|value| !value.is_string()) {
        return Some(issue("label", "must be a string"));
    }
    if let Some(base) = theme.get("base")
        && let Err(reason) = resolve_theme_base(base)
    {
        return Some(issue("base", reason));
    }
    for key in CUSTOM_THEME_COLOR_KEYS {
        if let Some(value) = theme.get(*key)
            && let Some(reason) = describe_theme_color_issue(value)
        {
            return Some(issue(key, reason));
        }
    }
    if let Some(syntax) = theme.get("syntax") {
        let Some(syntax) = syntax.as_object() else {
            return Some(issue("syntax", "must be an object"));
        };
        for key in LEGACY_CUSTOM_SYNTAX_COLOR_KEYS {
            if let Some(value) = syntax.get(*key)
                && let Some(reason) = describe_theme_color_issue(value)
            {
                return Some(issue(&format!("syntax.{key}"), reason));
            }
        }
    }
    if let Some(scopes) = theme.get("syntaxScopes") {
        let Some(scopes) = scopes.as_object() else {
            return Some(issue("syntaxScopes", "must be an object"));
        };
        for (scope, color) in scopes {
            if scope.trim().is_empty() {
                return Some(issue("syntaxScopes", "keys must be non-empty Shiki scopes"));
            }
            if let Some(reason) = describe_theme_color_issue(color) {
                return Some(issue(&format!("syntaxScopes.{scope}"), reason));
            }
        }
    }
    None
}

fn issue(key: &str, reason: impl Into<String>) -> CustomThemeFieldIssue {
    CustomThemeFieldIssue {
        key: key.into(),
        reason: reason.into(),
    }
}

#[must_use]
pub fn create_invalid_theme_field_notice(
    source: &str,
    id: &str,
    issue: &CustomThemeFieldIssue,
) -> StartupNotice {
    StartupNotice::new(
        format!("theme:invalid-field:{source}:{id}:{}", issue.key),
        format!(
            "Skipped theme \"{id}\" from {source} • {} {}",
            issue.key, issue.reason
        ),
    )
}

#[must_use]
pub fn create_invalid_theme_id_notice(source: &str, id: &str, reason: &str) -> StartupNotice {
    StartupNotice::new(
        format!("theme:invalid-id:{source}:{id}"),
        format!("Skipped theme \"{id}\" from {source} • {reason}"),
    )
}

#[must_use]
pub fn create_theme_collision_notice(source: &str, id: &str, winner: &str) -> StartupNotice {
    StartupNotice::new(
        format!("theme:collision:{source}:{id}"),
        format!("Skipped theme \"{id}\" from {source} • {winner} already defines it"),
    )
}

#[must_use]
pub fn collect_session_custom_themes(
    config_themes: &[NamedCustomThemeConfig],
    extension_themes: &[RegisteredCustomTheme],
) -> SessionCustomThemes {
    let mut themes = config_themes.to_vec();
    let mut notices = Vec::new();
    let mut claimed_by = config_themes
        .iter()
        .map(|theme| (theme.id.clone(), "config".to_owned()))
        .collect::<BTreeMap<_, _>>();

    for registration in extension_themes {
        let source = format!("extension {}", registration.extension_id);
        let id_value = registration
            .theme
            .as_object()
            .and_then(|theme| theme.get("id"))
            .unwrap_or(&Value::Null);
        if let Some(reason) = describe_custom_theme_id_issue(id_value) {
            notices.push(create_invalid_theme_id_notice(
                &source,
                &javascript_string(id_value),
                reason,
            ));
            continue;
        }
        let id = id_value.as_str().expect("validated theme id");
        if let Some(field_issue) = describe_custom_theme_field_issue(&registration.theme) {
            notices.push(create_invalid_theme_field_notice(&source, id, &field_issue));
            continue;
        }
        if let Some(owner) = claimed_by.get(id) {
            notices.push(create_theme_collision_notice(&source, id, owner));
            continue;
        }
        let normalized = normalize_validated_theme(&registration.theme);
        claimed_by.insert(id.to_owned(), source);
        themes.push(normalized);
    }
    SessionCustomThemes { themes, notices }
}

fn normalize_validated_theme(theme: &Value) -> NamedCustomThemeConfig {
    let mut normalized = theme.clone();
    let object = normalized
        .as_object_mut()
        .expect("validated custom theme is an object");
    if let Some(base) = object.get_mut("base")
        && let Some(resolved) = base
            .as_str()
            .and_then(|value| resolve_bundled_shiki_theme_id(Some(value)))
    {
        *base = Value::String(resolved.to_owned());
    }
    for key in CUSTOM_THEME_COLOR_KEYS {
        lowercase_value(object.get_mut(*key));
    }
    for table_name in ["syntax", "syntaxScopes"] {
        if let Some(table) = object.get_mut(table_name).and_then(Value::as_object_mut) {
            for value in table.values_mut() {
                lowercase_value(Some(value));
            }
        }
    }
    serde_json::from_value(normalized).expect("validated custom theme has typed fields")
}

fn lowercase_value(value: Option<&mut Value>) {
    if let Some(Value::String(value)) = value {
        value.make_ascii_lowercase();
    }
}

fn javascript_string(value: &Value) -> String {
    match value {
        Value::Null => "undefined".into(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) => "".into(),
        Value::Object(_) => "[object Object]".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hue_and_saturation(hex: &str) -> (f64, f64) {
        let red = f64::from(u8::from_str_radix(&hex[1..3], 16).unwrap()) / 255.0;
        let green = f64::from(u8::from_str_radix(&hex[3..5], 16).unwrap()) / 255.0;
        let blue = f64::from(u8::from_str_radix(&hex[5..7], 16).unwrap()) / 255.0;
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        let delta = max - min;
        if delta == 0.0 {
            return (0.0, 0.0);
        }
        let lightness = (max + min) / 2.0;
        let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
        let hue = if max == red {
            60.0 * (((green - blue) / delta + 6.0) % 6.0)
        } else if max == green {
            60.0 * ((blue - red) / delta + 2.0)
        } else {
            60.0 * ((red - green) / delta + 4.0)
        };
        (hue, saturation)
    }

    fn all_diff_colors(entry: BundledShikiThemeDiffColors) -> impl Iterator<Item = &'static str> {
        [entry.added, entry.removed, entry.modified]
            .into_iter()
            .flatten()
    }

    #[test]
    fn diff_catalog_only_names_bundled_ids_and_preserves_catalog_order() {
        let actual = BUNDLED_SHIKI_THEME_DIFF_COLORS
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let expected = BUNDLED_SHIKI_THEME_IDS
            .iter()
            .copied()
            .filter(|id| {
                BUNDLED_SHIKI_THEME_DIFF_COLORS
                    .iter()
                    .any(|(candidate, _)| candidate == id)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn diff_catalog_accents_are_lowercase_six_digit_hex_tokens() {
        for (theme_id, entry) in BUNDLED_SHIKI_THEME_DIFF_COLORS {
            for color in all_diff_colors(*entry) {
                assert_eq!(color.len(), 7, "{theme_id} {color}");
                assert!(color.starts_with('#'), "{theme_id} {color}");
                assert!(
                    color[1..]
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                    "{theme_id} {color}"
                );
            }
        }
    }

    #[test]
    fn diff_catalog_accents_are_saturated() {
        for (theme_id, entry) in BUNDLED_SHIKI_THEME_DIFF_COLORS {
            for color in all_diff_colors(*entry) {
                let (_, saturation) = hue_and_saturation(color);
                assert!(saturation > 0.1, "{theme_id} {color} {saturation}");
            }
        }
    }

    #[test]
    fn added_accents_are_green_or_teal() {
        for (theme_id, entry) in BUNDLED_SHIKI_THEME_DIFF_COLORS {
            if let Some(color) = entry.added {
                let (hue, _) = hue_and_saturation(color);
                assert!((50.0..=200.0).contains(&hue), "{theme_id} {color} {hue}");
            }
        }
    }

    #[test]
    fn removed_accents_are_red_or_pink() {
        for (theme_id, entry) in BUNDLED_SHIKI_THEME_DIFF_COLORS {
            if let Some(color) = entry.removed {
                let (hue, _) = hue_and_saturation(color);
                assert!(hue >= 300.0 || hue <= 30.0, "{theme_id} {color} {hue}");
            }
        }
    }

    #[test]
    fn added_and_removed_accents_never_match() {
        for (theme_id, entry) in BUNDLED_SHIKI_THEME_DIFF_COLORS {
            if let (Some(added), Some(removed)) = (entry.added, entry.removed) {
                assert_ne!(added, removed, "{theme_id}");
            }
        }
    }

    #[test]
    fn every_bundled_theme_has_a_background() {
        assert_eq!(BUNDLED_SHIKI_THEME_BACKGROUNDS.len(), 65);
        for theme_id in BUNDLED_SHIKI_THEME_IDS {
            assert!(
                get_bundled_shiki_theme_background(Some(theme_id)).is_some(),
                "{theme_id}"
            );
        }
    }

    #[test]
    fn resolves_legacy_and_current_bundled_ids() {
        assert_eq!(
            resolve_bundled_shiki_theme_id(Some("graphite")),
            Some("github-dark-default")
        );
        assert_eq!(
            resolve_bundled_shiki_theme_id(Some("dracula")),
            Some("dracula")
        );
        assert_eq!(resolve_bundled_shiki_theme_id(Some("unknown")), None);
        assert_eq!(resolve_bundled_shiki_theme_id(None), None);
    }

    fn registered(extension_id: &str, theme: Value) -> RegisteredCustomTheme {
        RegisteredCustomTheme {
            extension_id: extension_id.into(),
            theme,
        }
    }

    fn config_theme(id: &str) -> NamedCustomThemeConfig {
        NamedCustomThemeConfig {
            id: id.into(),
            ..NamedCustomThemeConfig::default()
        }
    }

    #[test]
    fn create_test_custom_themes_replaces_only_the_identifier() {
        let mut theme = config_theme("source");
        theme.base = Some("github-dark-default".into());
        theme.label = Some("Test theme".into());
        theme.accent = Some("#7755aa".into());
        theme.syntax.insert("keyword".into(), "#ff00aa".into());
        theme.extra.insert("customField".into(), json!("retained"));
        let expected = {
            let mut value = theme.clone();
            value.id = "custom".into();
            value
        };
        assert_eq!(create_test_custom_themes(theme, "custom"), vec![expected]);
        assert_eq!(
            create_test_custom_themes(config_theme("source"), "named")[0].id,
            "named"
        );
    }

    #[test]
    fn custom_theme_ids_accept_lowercase_kebab_and_word_ids() {
        for id in ["custom", "ocean-dark", "team_theme2"] {
            assert_eq!(describe_custom_theme_id_issue(&json!(id)), None);
        }
    }

    #[test]
    fn custom_theme_ids_reject_non_lowercase_word_shapes() {
        for id in ["", "Ocean", "ocean dark", "ocean.dark", "-ocean", "ocean-"] {
            assert_eq!(
                describe_custom_theme_id_issue(&json!(id)),
                Some(if id.is_empty() {
                    "theme ids must be non-empty strings"
                } else {
                    "theme ids must be lowercase words separated by - or _"
                })
            );
        }
    }

    #[test]
    fn custom_theme_ids_reject_bundled_and_auto_ids() {
        for id in ["dracula", "github-dark-default", "auto"] {
            assert_eq!(
                describe_custom_theme_id_issue(&json!(id)),
                Some("that id belongs to a built-in theme")
            );
        }
    }

    #[test]
    fn session_custom_themes_keep_config_then_registry_order() {
        let result = collect_session_custom_themes(
            &[config_theme("custom"), config_theme("team")],
            &[
                registered("pack", json!({"id": "ocean"})),
                registered("pack", json!({"id": "sunset"})),
            ],
        );
        assert_eq!(
            result
                .themes
                .iter()
                .map(|theme| theme.id.as_str())
                .collect::<Vec<_>>(),
            ["custom", "team", "ocean", "sunset"]
        );
        assert!(result.notices.is_empty());
    }

    #[test]
    fn config_theme_wins_over_extension_collision() {
        let mut config = config_theme("ocean");
        config.accent = Some("#123456".into());
        let result = collect_session_custom_themes(
            &[config.clone()],
            &[registered(
                "pack",
                json!({"id": "ocean", "accent": "#654321"}),
            )],
        );
        assert_eq!(result.themes, [config]);
        assert_eq!(
            result.notices,
            [StartupNotice::new(
                "theme:collision:extension pack:ocean",
                "Skipped theme \"ocean\" from extension pack • config already defines it"
            )]
        );
    }

    #[test]
    fn first_extension_theme_wins_collision() {
        let result = collect_session_custom_themes(
            &[],
            &[
                registered("first", json!({"id": "ocean", "accent": "#111111"})),
                registered("second", json!({"id": "ocean", "accent": "#222222"})),
            ],
        );
        assert_eq!(result.themes.len(), 1);
        assert_eq!(result.themes[0].accent.as_deref(), Some("#111111"));
        assert_eq!(
            result.notices[0].message,
            "Skipped theme \"ocean\" from extension second • extension first already defines it"
        );
    }

    #[test]
    fn unusable_extension_ids_are_skipped_without_stopping_siblings() {
        let result = collect_session_custom_themes(
            &[],
            &[
                registered("pack", json!({"id": "Ocean"})),
                registered("pack", json!({"id": "nord"})),
                registered("pack", json!({"id": "ocean"})),
            ],
        );
        assert_eq!(result.themes[0].id, "ocean");
        assert_eq!(result.notices.len(), 2);
        assert_eq!(
            result.notices[0].message,
            "Skipped theme \"Ocean\" from extension pack • theme ids must be lowercase words separated by - or _"
        );
        assert_eq!(
            result.notices[1].message,
            "Skipped theme \"nord\" from extension pack • that id belongs to a built-in theme"
        );
    }

    #[test]
    fn empty_theme_collection_is_empty() {
        assert_eq!(
            collect_session_custom_themes(&[], &[]),
            SessionCustomThemes::default()
        );
    }

    #[test]
    fn valid_extension_theme_is_normalized_like_config() {
        let result = collect_session_custom_themes(
            &[],
            &[registered(
                "paint-ext",
                json!({
                    "id": "midnight-review",
                    "label": "Midnight Review",
                    "base": "graphite",
                    "accent": "#7FD1FF",
                    "syntaxScopes": {"keyword.operator": "#7FD1FF"}
                }),
            )],
        );
        assert!(result.notices.is_empty());
        assert_eq!(
            result.themes[0].base.as_deref(),
            Some("github-dark-default")
        );
        assert_eq!(result.themes[0].accent.as_deref(), Some("#7fd1ff"));
        assert_eq!(
            result.themes[0].syntax_scopes.get("keyword.operator"),
            Some(&"#7fd1ff".to_owned())
        );
    }

    #[test]
    fn invalid_extension_color_types_and_literals_are_skipped() {
        for (theme, fragment) in [
            (
                json!({"id": "bad-theme", "background": 12345}),
                "background must be a hex color like #112233",
            ),
            (
                json!({"id": "bad-theme", "accent": "#xyz"}),
                "accent must be a hex color like #112233",
            ),
            (
                json!({"id": "bad-theme", "text": {"not": "a string"}}),
                "text must be a hex color like #112233",
            ),
        ] {
            let result = collect_session_custom_themes(&[], &[registered("paint-ext", theme)]);
            assert!(result.themes.is_empty());
            assert!(result.notices[0].message.contains(fragment));
        }
    }

    #[test]
    fn invalid_label_and_base_are_skipped() {
        for (theme, fragment) in [
            (
                json!({"id": "bad-theme", "label": 7}),
                "label must be a string",
            ),
            (
                json!({"id": "bad-theme", "base": "not-a-real-theme"}),
                "base must be a built-in theme id",
            ),
        ] {
            let result = collect_session_custom_themes(&[], &[registered("paint-ext", theme)]);
            assert!(result.themes.is_empty());
            assert!(result.notices[0].message.contains(fragment));
        }
    }

    #[test]
    fn malformed_scope_tables_and_colors_are_skipped() {
        for (theme, fragment) in [
            (
                json!({"id": "bad-theme", "syntaxScopes": {"keyword.operator": "cyan"}}),
                "syntaxScopes.keyword.operator must be a hex color like #112233",
            ),
            (
                json!({"id": "bad-theme", "syntaxScopes": ["#112233"]}),
                "syntaxScopes must be an object",
            ),
            (
                json!({"id": "bad-theme", "syntax": {"keyword": 42}}),
                "syntax.keyword must be a hex color like #112233",
            ),
            (
                json!({"id": "bad-theme", "syntaxScopes": {"  ": "#112233"}}),
                "syntaxScopes keys must be non-empty Shiki scopes",
            ),
        ] {
            let result = collect_session_custom_themes(&[], &[registered("paint-ext", theme)]);
            assert!(result.themes.is_empty());
            assert!(result.notices[0].message.contains(fragment));
        }
    }

    #[test]
    fn non_object_theme_reports_id_first() {
        let result = collect_session_custom_themes(&[], &[registered("paint-ext", json!("nope"))]);
        assert!(result.themes.is_empty());
        assert_eq!(result.notices.len(), 1);
        assert!(result.notices[0].key.contains("invalid-id"));
    }

    #[test]
    fn malformed_registration_does_not_remove_valid_sibling() {
        let result = collect_session_custom_themes(
            &[],
            &[
                registered("paint-ext", json!({"id": "bad-theme", "accent": 1})),
                registered(
                    "paint-ext",
                    json!({"id": "good-theme", "accent": "#112233"}),
                ),
            ],
        );
        assert_eq!(result.themes[0].id, "good-theme");
        assert_eq!(result.notices.len(), 1);
    }

    #[test]
    fn typed_registration_constructor_omits_absent_optional_fields() {
        let registration = RegisteredCustomTheme::new("pack", config_theme("ocean"));
        assert_eq!(registration.theme, json!({"id": "ocean"}));
        let result = collect_session_custom_themes(&[], &[registration]);
        assert_eq!(result.themes[0].id, "ocean");
        assert!(result.notices.is_empty());
    }

    #[test]
    fn normalization_preserves_unknown_runtime_fields_like_object_spread() {
        let result = collect_session_custom_themes(
            &[],
            &[registered(
                "pack",
                json!({"id": "ocean", "futureColor": "untouched", "accent": "#ABCDEF"}),
            )],
        );
        assert_eq!(
            serde_json::to_value(&result.themes[0]).unwrap(),
            json!({"id": "ocean", "accent": "#abcdef", "futureColor": "untouched"})
        );
    }
}
