//! Complete Ratatui application-theme derivation from bundled and custom theme metadata.

use crate::{TerminalThemeMode, blend_hex, contrast_ratio, hex_color_distance, relative_luminance};
use ratatui::style::Color;
use std::sync::{Arc, LazyLock};
use workdeck_core::{
    BUNDLED_SHIKI_THEME_IDS, LEGACY_CUSTOM_THEME_ID, NamedCustomThemeConfig,
    get_bundled_shiki_theme_background, get_bundled_shiki_theme_diff_colors,
    get_bundled_shiki_theme_foreground, resolve_bundled_shiki_theme_id,
    resolve_custom_syntax_scope_overrides,
};
use workdeck_extension_api::{ExtensionPaintTheme, ExtensionThemeAppearance};

pub const TRANSPARENT_BACKGROUND: &str = "transparent";
pub const DEFAULT_DARK_THEME_ID: &str = "github-dark-default";
pub const DEFAULT_LIGHT_THEME_ID: &str = "github-light-default";
const MIN_GUTTER_CONTRAST: f64 = 4.5;
pub const MIN_DIFF_SIGN_CONTRAST: f64 = 3.0;
pub const MIN_EMPHASIS_SEPARATION: u16 = 28;

const DARK_FALLBACK_ADDED: &str = "#5ecc71";
const DARK_FALLBACK_REMOVED: &str = "#ff6762";
const DARK_FALLBACK_MODIFIED: &str = "#69b1ff";
const LIGHT_FALLBACK_ADDED: &str = "#0dbe4e";
const LIGHT_FALLBACK_REMOVED: &str = "#ff2e3f";
const LIGHT_FALLBACK_MODIFIED: &str = "#009fff";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeAppearance {
    Light,
    Dark,
}

impl From<TerminalThemeMode> for ThemeAppearance {
    fn from(value: TerminalThemeMode) -> Self {
        match value {
            TerminalThemeMode::Light => Self::Light,
            TerminalThemeMode::Dark => Self::Dark,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxColors {
    pub default: String,
    pub keyword: String,
    pub string: String,
    pub comment: String,
    pub number: String,
    pub function: String,
    pub property: String,
    pub r#type: String,
    pub variable: Option<String>,
    pub operator: Option<String>,
    pub punctuation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppTheme {
    pub id: String,
    pub label: String,
    pub appearance: ThemeAppearance,
    pub background: String,
    pub panel: String,
    pub panel_alt: String,
    pub border: String,
    pub accent: String,
    pub accent_muted: String,
    pub text: String,
    pub muted: String,
    pub added_bg: String,
    pub removed_bg: String,
    pub moved_added_bg: String,
    pub moved_removed_bg: String,
    pub context_bg: String,
    pub added_content_bg: String,
    pub removed_content_bg: String,
    pub context_content_bg: String,
    pub added_sign_color: String,
    pub removed_sign_color: String,
    pub line_number_bg: String,
    pub line_number_fg: String,
    pub selected_hunk: String,
    pub badge_added: String,
    pub badge_removed: String,
    pub badge_neutral: String,
    pub file_new: String,
    pub file_deleted: String,
    pub file_renamed: String,
    pub file_modified: String,
    pub file_untracked: String,
    pub note_border: String,
    pub note_background: String,
    pub note_title_background: String,
    pub note_title_text: String,
    pub syntax_theme: Option<String>,
    /// Declaration-ordered exact TextMate selector overrides.
    pub syntax_scope_overrides: Vec<(String, String)>,
    pub syntax_colors: Arc<SyntaxColors>,
}

fn readable_foreground(preferred: Option<&str>, background: &str) -> String {
    if let Some(preferred) = preferred
        && contrast_ratio(preferred, background) >= MIN_GUTTER_CONTRAST
    {
        return preferred.to_owned();
    }
    if relative_luminance(background) > 0.45 {
        "#000000".into()
    } else {
        "#ffffff".into()
    }
}

fn readable_dim_foreground(preferred: &str, background: &str) -> String {
    if contrast_ratio(preferred, background) >= MIN_GUTTER_CONTRAST {
        return preferred.to_owned();
    }
    if relative_luminance(background) > 0.45 {
        blend_hex("#000000", background, 0.62)
    } else {
        blend_hex("#ffffff", background, 0.62)
    }
}

#[must_use]
pub fn readable_diff_sign(preferred: &str, background: &str) -> String {
    if contrast_ratio(preferred, background) >= MIN_DIFF_SIGN_CONTRAST {
        return preferred.to_owned();
    }
    let mut anchor = if relative_luminance(background) > 0.45 {
        "#000000"
    } else {
        "#ffffff"
    };
    if contrast_ratio(anchor, background) < MIN_DIFF_SIGN_CONTRAST {
        anchor = if anchor == "#000000" {
            "#ffffff"
        } else {
            "#000000"
        };
    }
    let mut amount = 0.02;
    while amount < 1.0 {
        let candidate = blend_hex(anchor, preferred, amount);
        if contrast_ratio(&candidate, background) >= MIN_DIFF_SIGN_CONTRAST {
            return candidate;
        }
        amount += 0.02;
    }
    anchor.into()
}

fn build_syntax_colors(code_foreground: &str) -> Arc<SyntaxColors> {
    let color = code_foreground.to_owned();
    Arc::new(SyntaxColors {
        default: color.clone(),
        keyword: color.clone(),
        string: color.clone(),
        comment: color.clone(),
        number: color.clone(),
        function: color.clone(),
        property: color.clone(),
        r#type: color.clone(),
        variable: Some(color.clone()),
        operator: Some(color.clone()),
        punctuation: color,
    })
}

fn readable_tinted_background(
    tint_color: &str,
    background: &str,
    foreground: &str,
    preferred_amount: f64,
) -> String {
    let max_steps = (preferred_amount / 0.01).round() as u32;
    for step in (1..=max_steps).rev() {
        let candidate = blend_hex(tint_color, background, f64::from(step) * 0.01);
        if contrast_ratio(foreground, &candidate) >= MIN_GUTTER_CONTRAST {
            return candidate;
        }
    }
    background.to_owned()
}

fn readable_separated_row_background(
    tint_color: &str,
    background: &str,
    foreground: &str,
    preferred_amount: f64,
    content_background: &str,
) -> String {
    let mut readable_fallback = None;
    let max_steps = (preferred_amount / 0.02).round() as u32;
    for step in (1..=max_steps).rev() {
        let candidate = blend_hex(tint_color, background, f64::from(step) * 0.02);
        if contrast_ratio(foreground, &candidate) < MIN_GUTTER_CONTRAST {
            continue;
        }
        if hex_color_distance(&candidate, content_background) >= MIN_EMPHASIS_SEPARATION {
            return candidate;
        }
        if readable_fallback.is_none() {
            readable_fallback = Some(candidate);
        }
    }
    readable_fallback.unwrap_or_else(|| background.to_owned())
}

fn readable_chrome_color(preferred: &str, panel: &str, panel_alt: &str) -> String {
    if contrast_ratio(preferred, panel) >= MIN_GUTTER_CONTRAST
        && contrast_ratio(preferred, panel_alt) >= MIN_GUTTER_CONTRAST
    {
        return preferred.to_owned();
    }
    let anchor = if relative_luminance(panel_alt) > 0.45 {
        "#000000"
    } else {
        "#ffffff"
    };
    for amount in [0.35, 0.5, 0.65, 0.8, 1.0] {
        let candidate = blend_hex(anchor, preferred, amount);
        if contrast_ratio(&candidate, panel) >= MIN_GUTTER_CONTRAST
            && contrast_ratio(&candidate, panel_alt) >= MIN_GUTTER_CONTRAST
        {
            return candidate;
        }
    }
    anchor.into()
}

fn build_shiki_theme(theme_id: &str) -> AppTheme {
    let editor_background = get_bundled_shiki_theme_background(Some(theme_id)).unwrap_or("#0d1117");
    let editor_foreground = get_bundled_shiki_theme_foreground(Some(theme_id));
    let diff_colors = get_bundled_shiki_theme_diff_colors(Some(theme_id));
    let light = relative_luminance(editor_background) > 0.45;
    let (fallback_added, fallback_removed, fallback_modified) = if light {
        (
            LIGHT_FALLBACK_ADDED,
            LIGHT_FALLBACK_REMOVED,
            LIGHT_FALLBACK_MODIFIED,
        )
    } else {
        (
            DARK_FALLBACK_ADDED,
            DARK_FALLBACK_REMOVED,
            DARK_FALLBACK_MODIFIED,
        )
    };
    let row_tint = if light { 0.12 } else { 0.2 };
    let content_tint = if light { 0.18 } else { 0.28 };
    let selected_tint = if light { 0.18 } else { 0.25 };
    let code_foreground = readable_foreground(editor_foreground, editor_background);
    let neutral_panel = blend_hex(
        &code_foreground,
        editor_background,
        if light { 0.04 } else { 0.08 },
    );
    let neutral_panel_alt = blend_hex(
        &code_foreground,
        editor_background,
        if light { 0.08 } else { 0.12 },
    );
    let neutral_border = blend_hex(
        &code_foreground,
        editor_background,
        if light { 0.15 } else { 0.18 },
    );
    let text_foreground = readable_foreground(
        Some(editor_foreground.unwrap_or(&code_foreground)),
        &neutral_panel_alt,
    );
    let dim_preferred = blend_hex(&text_foreground, editor_background, 0.56);
    let line_number_foreground = readable_dim_foreground(&dim_preferred, editor_background);
    let muted_foreground = readable_dim_foreground(&dim_preferred, &neutral_panel_alt);
    let added_sign_color = readable_diff_sign(
        diff_colors
            .and_then(|colors| colors.added)
            .unwrap_or(fallback_added),
        editor_background,
    );
    let removed_sign_color = readable_diff_sign(
        diff_colors
            .and_then(|colors| colors.removed)
            .unwrap_or(fallback_removed),
        editor_background,
    );
    let modified_color = readable_diff_sign(
        diff_colors
            .and_then(|colors| colors.modified)
            .unwrap_or(fallback_modified),
        editor_background,
    );
    let added_content_bg = readable_tinted_background(
        &added_sign_color,
        editor_background,
        &text_foreground,
        content_tint,
    );
    let removed_content_bg = readable_tinted_background(
        &removed_sign_color,
        editor_background,
        &text_foreground,
        content_tint,
    );
    let added_bg = readable_separated_row_background(
        &added_sign_color,
        editor_background,
        &text_foreground,
        row_tint,
        &added_content_bg,
    );
    let removed_bg = readable_separated_row_background(
        &removed_sign_color,
        editor_background,
        &text_foreground,
        row_tint,
        &removed_content_bg,
    );
    let moved_bg = readable_tinted_background(
        &modified_color,
        editor_background,
        &text_foreground,
        row_tint,
    );
    let accent_muted = readable_tinted_background(
        &modified_color,
        editor_background,
        &text_foreground,
        selected_tint,
    );
    let badge_added = readable_chrome_color(&added_sign_color, &neutral_panel, &neutral_panel_alt);
    let badge_removed =
        readable_chrome_color(&removed_sign_color, &neutral_panel, &neutral_panel_alt);
    let badge_modified = readable_chrome_color(&modified_color, &neutral_panel, &neutral_panel_alt);
    let syntax_colors = build_syntax_colors(&text_foreground);
    AppTheme {
        id: theme_id.into(),
        label: theme_id.into(),
        appearance: if light {
            ThemeAppearance::Light
        } else {
            ThemeAppearance::Dark
        },
        background: editor_background.into(),
        panel: neutral_panel.clone(),
        panel_alt: neutral_panel_alt,
        border: neutral_border,
        accent: modified_color.clone(),
        accent_muted,
        text: text_foreground.clone(),
        muted: muted_foreground.clone(),
        added_bg,
        removed_bg,
        moved_added_bg: moved_bg.clone(),
        moved_removed_bg: moved_bg,
        context_bg: editor_background.into(),
        added_content_bg,
        removed_content_bg,
        context_content_bg: editor_background.into(),
        added_sign_color,
        removed_sign_color,
        line_number_bg: editor_background.into(),
        line_number_fg: line_number_foreground,
        selected_hunk: blend_hex(&modified_color, editor_background, selected_tint),
        badge_added: badge_added.clone(),
        badge_removed: badge_removed.clone(),
        badge_neutral: muted_foreground,
        file_new: badge_added.clone(),
        file_deleted: badge_removed,
        file_renamed: badge_modified.clone(),
        file_modified: badge_modified,
        file_untracked: badge_added,
        note_border: modified_color,
        note_background: neutral_panel.clone(),
        note_title_background: neutral_panel,
        note_title_text: text_foreground,
        syntax_theme: Some(theme_id.into()),
        syntax_scope_overrides: Vec::new(),
        syntax_colors,
    }
}

pub static THEMES: LazyLock<Vec<AppTheme>> = LazyLock::new(|| {
    BUNDLED_SHIKI_THEME_IDS
        .iter()
        .map(|theme_id| build_shiki_theme(theme_id))
        .collect()
});

fn built_in_theme_by_id(theme_id: Option<&str>) -> Option<&'static AppTheme> {
    let resolved = resolve_bundled_shiki_theme_id(theme_id)?;
    THEMES.iter().find(|theme| theme.id == resolved)
}

fn fallback_theme(theme_mode: Option<ThemeAppearance>) -> &'static AppTheme {
    let fallback_id = if theme_mode == Some(ThemeAppearance::Light) {
        DEFAULT_LIGHT_THEME_ID
    } else {
        DEFAULT_DARK_THEME_ID
    };
    built_in_theme_by_id(Some(fallback_id)).unwrap_or(&THEMES[0])
}

fn build_custom_theme(custom: &NamedCustomThemeConfig) -> AppTheme {
    let base = built_in_theme_by_id(custom.base.as_deref()).unwrap_or_else(|| fallback_theme(None));
    let mut theme = base.clone();
    theme.id.clone_from(&custom.id);
    theme.label = custom.label.clone().unwrap_or_else(|| {
        if custom.id == LEGACY_CUSTOM_THEME_ID {
            "Custom".into()
        } else {
            custom.id.clone()
        }
    });
    macro_rules! override_color {
        ($field:ident) => {
            if let Some(value) = &custom.$field {
                theme.$field.clone_from(value);
            }
        };
    }
    override_color!(background);
    override_color!(panel);
    override_color!(panel_alt);
    override_color!(border);
    override_color!(accent);
    override_color!(accent_muted);
    override_color!(text);
    override_color!(muted);
    override_color!(added_bg);
    override_color!(removed_bg);
    override_color!(moved_added_bg);
    override_color!(moved_removed_bg);
    override_color!(context_bg);
    override_color!(added_content_bg);
    override_color!(removed_content_bg);
    override_color!(context_content_bg);
    override_color!(added_sign_color);
    override_color!(removed_sign_color);
    override_color!(line_number_bg);
    override_color!(line_number_fg);
    override_color!(selected_hunk);
    override_color!(badge_added);
    override_color!(badge_removed);
    override_color!(badge_neutral);
    override_color!(file_new);
    override_color!(file_deleted);
    override_color!(file_renamed);
    override_color!(file_modified);
    override_color!(file_untracked);
    override_color!(note_border);
    override_color!(note_background);
    override_color!(note_title_background);
    override_color!(note_title_text);
    theme.syntax_scope_overrides =
        resolve_custom_syntax_scope_overrides(&custom.syntax, &custom.syntax_scopes)
            .into_iter()
            .collect();
    theme
}

#[must_use]
pub fn available_theme_ids(custom_themes: &[NamedCustomThemeConfig]) -> Vec<String> {
    THEMES
        .iter()
        .map(|theme| theme.id.clone())
        .chain(custom_themes.iter().map(|theme| theme.id.clone()))
        .collect()
}

#[must_use]
pub fn available_themes(custom_themes: &[NamedCustomThemeConfig]) -> Vec<AppTheme> {
    THEMES
        .iter()
        .cloned()
        .chain(custom_themes.iter().map(build_custom_theme))
        .collect()
}

#[must_use]
pub fn resolve_theme(
    requested: Option<&str>,
    theme_mode: Option<ThemeAppearance>,
    custom_themes: &[NamedCustomThemeConfig],
) -> AppTheme {
    if requested == Some("auto") {
        return fallback_theme(theme_mode).clone();
    }
    if let Some(custom) =
        requested.and_then(|requested| custom_themes.iter().find(|theme| theme.id == requested))
    {
        return build_custom_theme(custom);
    }
    built_in_theme_by_id(requested)
        .unwrap_or_else(|| fallback_theme(theme_mode))
        .clone()
}

#[must_use]
pub fn with_transparent_surfaces(theme: &AppTheme) -> AppTheme {
    let mut transparent = theme.clone();
    for surface in [
        &mut transparent.background,
        &mut transparent.panel,
        &mut transparent.panel_alt,
        &mut transparent.context_bg,
        &mut transparent.context_content_bg,
        &mut transparent.line_number_bg,
    ] {
        *surface = TRANSPARENT_BACKGROUND.into();
    }
    transparent
}

/// Project the active app theme onto the stable paint-only native extension palette.
#[must_use]
pub fn to_extension_paint_theme(theme: &AppTheme) -> ExtensionPaintTheme {
    ExtensionPaintTheme {
        appearance: match theme.appearance {
            ThemeAppearance::Light => ExtensionThemeAppearance::Light,
            ThemeAppearance::Dark => ExtensionThemeAppearance::Dark,
        },
        background: theme.background.clone(),
        panel: theme.panel.clone(),
        panel_alt: theme.panel_alt.clone(),
        border: theme.border.clone(),
        accent: theme.accent.clone(),
        accent_muted: theme.accent_muted.clone(),
        text: theme.text.clone(),
        muted: theme.muted.clone(),
        selected_hunk: theme.selected_hunk.clone(),
        badge_added: theme.badge_added.clone(),
        badge_removed: theme.badge_removed.clone(),
        badge_neutral: theme.badge_neutral.clone(),
        file_new: theme.file_new.clone(),
        file_deleted: theme.file_deleted.clone(),
        file_renamed: theme.file_renamed.clone(),
        file_modified: theme.file_modified.clone(),
        file_untracked: theme.file_untracked.clone(),
        note_border: theme.note_border.clone(),
    }
}

/// Convert a validated app-theme token into a Ratatui color.
#[must_use]
pub fn ratatui_theme_color(value: &str) -> Color {
    if value == TRANSPARENT_BACKGROUND {
        return Color::Reset;
    }
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return Color::Reset;
    }
    let nibble = |value: u8| match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    };
    let pair = |first, second| Some(nibble(first)? * 16 + nibble(second)?);
    match (
        pair(bytes[1], bytes[2]),
        pair(bytes[3], bytes[4]),
        pair(bytes[5], bytes[6]),
    ) {
        (Some(red), Some(green), Some(blue)) => Color::Rgb(red, green, blue),
        _ => Color::Reset,
    }
}

/// Strengthen custom word-emphasis colors only when their row separation is too subtle.
#[must_use]
pub fn resolve_word_diff_highlight_bg(content_bg: &str, line_bg: &str, sign_color: &str) -> String {
    if content_bg == TRANSPARENT_BACKGROUND || line_bg == TRANSPARENT_BACKGROUND {
        return content_bg.into();
    }
    let is_hex = |color: &str| {
        color.len() == 7
            && color.starts_with('#')
            && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    if !is_hex(content_bg) || !is_hex(line_bg) {
        return content_bg.into();
    }
    if hex_color_distance(content_bg, line_bg) >= MIN_EMPHASIS_SEPARATION {
        return content_bg.into();
    }
    let mut strongest = line_bg.to_owned();
    for step in 1..=40 {
        let candidate = blend_hex(sign_color, line_bg, f64::from(step) * 0.005);
        strongest = candidate;
        if hex_color_distance(&strongest, line_bg) >= MIN_EMPHASIS_SEPARATION {
            return strongest;
        }
    }
    strongest
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN_READABLE_TEXT_CONTRAST: f64 = 4.5;
    const MAX_RESCUE_HUE_DRIFT: f64 = 2.0;

    fn custom_theme(id: &str, base: Option<&str>) -> NamedCustomThemeConfig {
        NamedCustomThemeConfig {
            id: id.into(),
            base: base.map(str::to_owned),
            ..NamedCustomThemeConfig::default()
        }
    }

    fn theme_contrast_failure(
        label: &str,
        foreground: &str,
        background: &str,
        minimum: f64,
    ) -> Option<String> {
        let ratio = contrast_ratio(foreground, background);
        (ratio + 0.005 < minimum)
            .then(|| format!("{label}: {ratio:.2} ({foreground} on {background})"))
    }

    fn hex_hue_degrees(hex: &str) -> Option<f64> {
        let red = f64::from(u8::from_str_radix(&hex[1..3], 16).unwrap());
        let green = f64::from(u8::from_str_radix(&hex[3..5], 16).unwrap());
        let blue = f64::from(u8::from_str_radix(&hex[5..7], 16).unwrap());
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        if max == min {
            return None;
        }
        let chroma = max - min;
        let segment = if max == red {
            ((green - blue) / chroma) % 6.0
        } else if max == green {
            (blue - red) / chroma + 2.0
        } else {
            (red - green) / chroma + 4.0
        };
        Some((segment * 60.0 + 360.0) % 360.0)
    }

    fn hue_distance(left: f64, right: f64) -> f64 {
        let delta = (left - right).abs() % 360.0;
        if delta > 180.0 { 360.0 - delta } else { delta }
    }

    fn bundled_diff_sign_slots(theme_id: &str) -> (&'static str, Vec<(&'static str, String)>) {
        let background = get_bundled_shiki_theme_background(Some(theme_id)).unwrap_or("#0d1117");
        let colors = get_bundled_shiki_theme_diff_colors(Some(theme_id));
        let theme = resolve_theme(Some(theme_id), None, &[]);
        let mut slots = Vec::new();
        if let Some(source) = colors.and_then(|colors| colors.added) {
            slots.push((source, theme.added_sign_color));
        }
        if let Some(source) = colors.and_then(|colors| colors.removed) {
            slots.push((source, theme.removed_sign_color));
        }
        if let Some(source) = colors.and_then(|colors| colors.modified) {
            slots.push((source, theme.accent));
        }
        (background, slots)
    }

    #[test]
    fn defaults_and_auto_choose_github_dark_or_light() {
        assert_eq!(resolve_theme(None, None, &[]).id, DEFAULT_DARK_THEME_ID);
        assert_eq!(
            resolve_theme(Some("missing"), None, &[]).id,
            DEFAULT_DARK_THEME_ID
        );
        assert_eq!(
            resolve_theme(Some("auto"), Some(ThemeAppearance::Dark), &[]).id,
            DEFAULT_DARK_THEME_ID
        );
        assert_eq!(
            resolve_theme(Some("auto"), Some(ThemeAppearance::Light), &[]).id,
            DEFAULT_LIGHT_THEME_ID
        );
        assert_eq!(
            resolve_theme(Some("missing"), Some(ThemeAppearance::Light), &[]).id,
            DEFAULT_LIGHT_THEME_ID
        );
        assert_eq!(
            resolve_theme(Some("missing"), Some(ThemeAppearance::Dark), &[]).id,
            DEFAULT_DARK_THEME_ID
        );
    }

    #[test]
    fn ui_lib_custom_theme_preserves_semantic_syntax_projection() {
        assert_eq!(resolve_theme(Some("dracula"), None, &[]).id, "dracula");
        let mut custom = custom_theme("custom", Some("github-light-default"));
        custom.label = Some("My Theme".into());
        custom.accent = Some("#7755aa".into());
        custom
            .syntax_scopes
            .insert("keyword.control".into(), "#123456".into());
        let theme = resolve_theme(Some("custom"), None, &[custom]);

        assert_eq!(theme.id, "custom");
        assert_eq!(theme.label, "My Theme");
        assert_eq!(theme.appearance, ThemeAppearance::Light);
        assert_eq!(theme.accent, "#7755aa");
        assert_eq!(
            theme.syntax_scope_overrides,
            [("keyword.control".into(), "#123456".into())]
        );
        assert_eq!(theme.syntax_colors.default, "#1f2328");
        assert_eq!(
            resolve_theme(Some("custom"), None, &[]).id,
            DEFAULT_DARK_THEME_ID
        );
    }

    #[test]
    fn maps_every_removed_theme_id_to_its_bundled_replacement() {
        for (requested, expected) in [
            ("graphite", "github-dark-default"),
            ("paper", "github-light-default"),
            ("midnight", "github-dark-dimmed"),
            ("ember", "dark-plus"),
            ("zenburn", "everforest-dark"),
        ] {
            assert_eq!(resolve_theme(Some(requested), None, &[]).id, expected);
        }
    }

    #[test]
    fn exposes_every_bundled_theme_in_catalog_order() {
        assert_eq!(
            available_theme_ids(&[]),
            BUNDLED_SHIKI_THEME_IDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            available_themes(&[])
                .iter()
                .map(|theme| theme.id.as_str())
                .collect::<Vec<_>>(),
            BUNDLED_SHIKI_THEME_IDS
        );
        for theme_id in BUNDLED_SHIKI_THEME_IDS {
            let theme = resolve_theme(Some(theme_id), None, &[]);
            assert_eq!(&theme.id, theme_id);
            assert_eq!(&theme.label, theme_id);
            assert_eq!(theme.syntax_theme.as_deref(), Some(*theme_id));
            assert!(!theme.syntax_colors.default.is_empty());
        }
    }

    #[test]
    fn derives_github_default_surfaces_from_catalog_metadata() {
        let dark = resolve_theme(Some("github-dark-default"), None, &[]);
        let light = resolve_theme(Some("github-light-default"), None, &[]);
        assert_eq!(dark.background, "#0d1117");
        assert_eq!(dark.syntax_colors.default, "#e6edf3");
        assert_eq!(dark.added_sign_color, "#2ea043");
        assert_eq!(dark.removed_sign_color, "#f85149");
        assert_eq!(dark.added_bg, blend_hex("#2ea043", "#0d1117", 0.14));
        assert_eq!(dark.removed_bg, blend_hex("#f85149", "#0d1117", 0.2));
        assert_eq!(light.background, "#ffffff");
        assert_eq!(light.syntax_colors.default, "#1f2328");
        assert_eq!(light.added_sign_color, "#116329");
        assert_eq!(light.removed_sign_color, "#cf222e");
        assert_eq!(light.added_bg, blend_hex("#116329", "#ffffff", 0.12));
        assert_eq!(light.removed_bg, blend_hex("#cf222e", "#ffffff", 0.12));
    }

    #[test]
    fn every_bundled_diff_row_and_gutter_remains_readable_and_distinct() {
        let mut failures = Vec::new();
        for theme_id in BUNDLED_SHIKI_THEME_IDS {
            let theme = resolve_theme(Some(theme_id), None, &[]);
            for (label, foreground, background, minimum) in [
                ("text/context", &theme.text, &theme.context_bg, 4.5),
                ("text/added", &theme.text, &theme.added_bg, 4.5),
                ("text/removed", &theme.text, &theme.removed_bg, 4.5),
                (
                    "text/added-content",
                    &theme.text,
                    &theme.added_content_bg,
                    4.5,
                ),
                (
                    "text/removed-content",
                    &theme.text,
                    &theme.removed_content_bg,
                    4.5,
                ),
                ("added-sign", &theme.added_sign_color, &theme.added_bg, 2.4),
                (
                    "removed-sign",
                    &theme.removed_sign_color,
                    &theme.removed_bg,
                    2.4,
                ),
                (
                    "line-number",
                    &theme.line_number_fg,
                    &theme.line_number_bg,
                    4.5,
                ),
            ] {
                if let Some(failure) = theme_contrast_failure(
                    &format!("{theme_id} {label}"),
                    foreground,
                    background,
                    minimum,
                ) {
                    failures.push(failure);
                }
            }
            if theme.added_bg == theme.context_bg {
                failures.push(format!("{theme_id} added bg matches context"));
            }
            if theme.removed_bg == theme.context_bg {
                failures.push(format!("{theme_id} removed bg matches context"));
            }
        }
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn every_fallback_syntax_role_is_readable_on_diff_rows() {
        let mut failures = Vec::new();
        for theme_id in BUNDLED_SHIKI_THEME_IDS {
            let theme = resolve_theme(Some(theme_id), None, &[]);
            let colors = [
                &theme.syntax_colors.default,
                &theme.syntax_colors.keyword,
                &theme.syntax_colors.string,
                &theme.syntax_colors.comment,
                &theme.syntax_colors.number,
                &theme.syntax_colors.function,
                &theme.syntax_colors.property,
                &theme.syntax_colors.r#type,
                theme.syntax_colors.variable.as_ref().unwrap(),
                theme.syntax_colors.operator.as_ref().unwrap(),
                &theme.syntax_colors.punctuation,
            ];
            for color in colors {
                for background in [&theme.context_bg, &theme.added_bg, &theme.removed_bg] {
                    if let Some(failure) = theme_contrast_failure(theme_id, color, background, 4.5)
                    {
                        failures.push(failure);
                    }
                }
            }
        }
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn every_bundled_chrome_color_is_readable() {
        let mut failures = Vec::new();
        for theme_id in BUNDLED_SHIKI_THEME_IDS {
            let theme = resolve_theme(Some(theme_id), None, &[]);
            for foreground in [
                &theme.badge_added,
                &theme.badge_removed,
                &theme.badge_neutral,
                &theme.file_new,
                &theme.file_deleted,
                &theme.file_renamed,
                &theme.file_modified,
                &theme.file_untracked,
            ] {
                for background in [&theme.panel, &theme.panel_alt] {
                    if let Some(failure) =
                        theme_contrast_failure(theme_id, foreground, background, 4.5)
                    {
                        failures.push(failure);
                    }
                }
            }
            for (foreground, background) in [
                (&theme.text, &theme.panel),
                (&theme.text, &theme.panel_alt),
                (&theme.muted, &theme.panel),
                (&theme.muted, &theme.panel_alt),
                (&theme.text, &theme.accent_muted),
            ] {
                if let Some(failure) = theme_contrast_failure(theme_id, foreground, background, 4.5)
                {
                    failures.push(failure);
                }
            }
        }
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn catppuccin_rows_remain_semantically_distinct() {
        for theme_id in [
            "catppuccin-latte",
            "catppuccin-frappe",
            "catppuccin-macchiato",
            "catppuccin-mocha",
        ] {
            let theme = resolve_theme(Some(theme_id), None, &[]);
            assert_ne!(theme.added_bg, theme.removed_bg);
            assert!(hex_color_distance(&theme.added_bg, &theme.context_bg) > 0);
            assert!(hex_color_distance(&theme.removed_bg, &theme.context_bg) > 0);
            assert!(
                hex_color_distance(&theme.added_content_bg, &theme.context_bg)
                    > hex_color_distance(&theme.added_bg, &theme.context_bg)
            );
            assert!(
                hex_color_distance(&theme.removed_content_bg, &theme.context_bg)
                    > hex_color_distance(&theme.removed_bg, &theme.context_bg)
            );
        }
    }

    #[test]
    fn readable_catalog_accents_are_not_changed() {
        let mut failures = Vec::new();
        for theme_id in BUNDLED_SHIKI_THEME_IDS {
            let (background, slots) = bundled_diff_sign_slots(theme_id);
            for (source, derived) in slots {
                if contrast_ratio(source, background) >= MIN_DIFF_SIGN_CONTRAST && derived != source
                {
                    failures.push(format!("{theme_id}: {source} rescued to {derived}"));
                }
            }
        }
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn rescued_accents_keep_hue_clear_contrast_and_use_a_minimal_blend() {
        let mut failures = Vec::new();
        for theme_id in BUNDLED_SHIKI_THEME_IDS {
            let (background, slots) = bundled_diff_sign_slots(theme_id);
            for (source, derived) in slots {
                if contrast_ratio(source, background) >= MIN_DIFF_SIGN_CONTRAST {
                    continue;
                }
                if contrast_ratio(&derived, background) < MIN_DIFF_SIGN_CONTRAST {
                    failures.push(format!("{theme_id} {derived} remains unreadable"));
                }
                if let (Some(source_hue), Some(derived_hue)) =
                    (hex_hue_degrees(source), hex_hue_degrees(&derived))
                    && hue_distance(source_hue, derived_hue) > MAX_RESCUE_HUE_DRIFT
                {
                    failures.push(format!("{theme_id} {source} hue drifted to {derived}"));
                }
                let mut minimal = Vec::new();
                for anchor in ["#000000", "#ffffff"] {
                    let mut amount = 0.02;
                    while amount < 1.0 {
                        let candidate = blend_hex(anchor, source, amount);
                        if contrast_ratio(&candidate, background) >= MIN_DIFF_SIGN_CONTRAST {
                            minimal.push(candidate);
                            break;
                        }
                        amount += 0.02;
                    }
                }
                if !minimal.contains(&derived) {
                    failures.push(format!("{theme_id} {derived} is not a minimal rescue"));
                }
            }
        }
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn catppuccin_latte_green_is_nudged_without_washout() {
        assert_eq!(
            resolve_theme(Some("catppuccin-latte"), None, &[]).added_sign_color,
            "#3f9d2a"
        );
    }

    #[test]
    fn readable_diff_sign_handles_mid_luminance_backgrounds() {
        let rescued = readable_diff_sign("#b0b0b0", "#aaaaaa");
        assert!(contrast_ratio(&rescued, "#aaaaaa") >= MIN_DIFF_SIGN_CONTRAST);
    }

    #[test]
    fn rendered_word_emphasis_is_separated_and_readable_for_every_theme() {
        let mut failures = Vec::new();
        for theme_id in BUNDLED_SHIKI_THEME_IDS {
            let theme = resolve_theme(Some(theme_id), None, &[]);
            for (row, content, sign) in [
                (
                    &theme.added_bg,
                    &theme.added_content_bg,
                    &theme.added_sign_color,
                ),
                (
                    &theme.removed_bg,
                    &theme.removed_content_bg,
                    &theme.removed_sign_color,
                ),
            ] {
                let rendered = resolve_word_diff_highlight_bg(content, row, sign);
                if rendered != *content {
                    failures.push(format!("{theme_id} rewrote {content} to {rendered}"));
                }
                if hex_color_distance(row, &rendered) < MIN_EMPHASIS_SEPARATION {
                    failures.push(format!("{theme_id} emphasis separation"));
                }
                if contrast_ratio(&theme.text, &rendered) + 0.005 < MIN_READABLE_TEXT_CONTRAST {
                    failures.push(format!("{theme_id} emphasis contrast"));
                }
            }
        }
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn custom_theme_layers_over_base_and_preserves_syntax_identity() {
        let mut custom = custom_theme("custom", Some("catppuccin-mocha"));
        custom.label = Some("My Theme".into());
        custom.text = Some("#ffffff".into());
        custom
            .syntax_scopes
            .insert("keyword.control".into(), "#ff00ff".into());
        let theme = resolve_theme(Some("custom"), None, &[custom]);
        let base = resolve_theme(Some("catppuccin-mocha"), None, &[]);
        assert_eq!(theme.id, "custom");
        assert_eq!(theme.label, "My Theme");
        assert_eq!(theme.background, base.background);
        assert_eq!(theme.text, "#ffffff");
        assert_eq!(theme.syntax_theme.as_deref(), Some("catppuccin-mocha"));
        assert_eq!(
            theme.syntax_scope_overrides,
            [("keyword.control".into(), "#ff00ff".into())]
        );
        assert!(Arc::ptr_eq(&theme.syntax_colors, &base.syntax_colors));
    }

    #[test]
    fn custom_themes_list_and_resolve_in_declaration_order() {
        let mut custom = custom_theme("custom", Some("nord"));
        let mut ocean = custom_theme("ocean", Some("nord"));
        ocean.label = Some("Ocean".into());
        ocean.accent = Some("#123456".into());
        let mut sunset = custom_theme("sunset", Some("github-light-default"));
        sunset.accent = Some("#654321".into());
        let custom_themes = [custom.clone(), ocean, sunset];
        let ids = available_theme_ids(&custom_themes);
        assert_eq!(
            &ids[BUNDLED_SHIKI_THEME_IDS.len()..],
            ["custom", "ocean", "sunset"]
        );
        let themes = available_themes(&custom_themes);
        assert_eq!(themes[BUNDLED_SHIKI_THEME_IDS.len()].label, "Custom");
        assert_eq!(themes[BUNDLED_SHIKI_THEME_IDS.len() + 1].label, "Ocean");
        assert_eq!(
            resolve_theme(Some("ocean"), None, &custom_themes).accent,
            "#123456"
        );
        assert_eq!(
            resolve_theme(Some("sunset"), None, &custom_themes).accent,
            "#654321"
        );
        assert_eq!(
            resolve_theme(Some("missing"), None, &custom_themes).id,
            DEFAULT_DARK_THEME_ID
        );
        assert_eq!(
            resolve_theme(Some("auto"), Some(ThemeAppearance::Light), &custom_themes).id,
            DEFAULT_LIGHT_THEME_ID
        );
        custom.id = "midnight".into();
        custom.accent = Some("#123456".into());
        assert_eq!(
            resolve_theme(Some("midnight"), None, &[custom]).id,
            "midnight"
        );
        assert_eq!(
            resolve_theme(Some("midnight"), None, &[]).id,
            "github-dark-dimmed"
        );
    }

    #[test]
    fn transparent_surfaces_keep_semantic_tints_and_syntax_identity() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let transparent = with_transparent_surfaces(&theme);
        assert_eq!(transparent.background, TRANSPARENT_BACKGROUND);
        assert_eq!(transparent.panel, TRANSPARENT_BACKGROUND);
        assert_eq!(transparent.panel_alt, TRANSPARENT_BACKGROUND);
        assert_eq!(transparent.context_bg, TRANSPARENT_BACKGROUND);
        assert_eq!(transparent.context_content_bg, TRANSPARENT_BACKGROUND);
        assert_eq!(transparent.line_number_bg, TRANSPARENT_BACKGROUND);
        assert_eq!(transparent.added_bg, theme.added_bg);
        assert_eq!(transparent.removed_bg, theme.removed_bg);
        assert_eq!(transparent.moved_added_bg, theme.moved_added_bg);
        assert_eq!(transparent.moved_removed_bg, theme.moved_removed_bg);
        assert_eq!(transparent.added_content_bg, theme.added_content_bg);
        assert_eq!(transparent.removed_content_bg, theme.removed_content_bg);
        assert!(Arc::ptr_eq(
            &transparent.syntax_colors,
            &theme.syntax_colors
        ));
    }

    #[test]
    fn legacy_and_exact_syntax_scopes_preserve_precedence_order() {
        let mut custom = custom_theme("custom", Some("nord"));
        custom.syntax.insert("comment".into(), "#111111".into());
        custom.syntax.insert("string".into(), "#222222".into());
        custom
            .syntax_scopes
            .insert("comment".into(), "#333333".into());
        custom
            .syntax_scopes
            .insert("comment, string".into(), "#444444".into());
        let theme = resolve_theme(Some("custom"), None, &[custom]);
        assert_eq!(
            theme.syntax_scope_overrides,
            [
                ("string".into(), "#222222".into()),
                ("comment".into(), "#333333".into()),
                ("punctuation.definition.comment".into(), "#111111".into()),
                ("comment, string".into(), "#444444".into()),
            ]
        );
    }

    #[test]
    fn extension_paint_projection_contains_only_the_public_live_palette() {
        let theme = resolve_theme(Some("rose-pine-dawn"), None, &[]);
        let paint = to_extension_paint_theme(&theme);
        assert_eq!(paint.appearance, ExtensionThemeAppearance::Light);
        assert_eq!(paint.background, theme.background);
        assert_eq!(paint.panel, theme.panel);
        assert_eq!(paint.panel_alt, theme.panel_alt);
        assert_eq!(paint.border, theme.border);
        assert_eq!(paint.accent, theme.accent);
        assert_eq!(paint.accent_muted, theme.accent_muted);
        assert_eq!(paint.text, theme.text);
        assert_eq!(paint.muted, theme.muted);
        assert_eq!(paint.selected_hunk, theme.selected_hunk);
        assert_eq!(paint.badge_added, theme.badge_added);
        assert_eq!(paint.badge_removed, theme.badge_removed);
        assert_eq!(paint.badge_neutral, theme.badge_neutral);
        assert_eq!(paint.file_new, theme.file_new);
        assert_eq!(paint.file_deleted, theme.file_deleted);
        assert_eq!(paint.file_renamed, theme.file_renamed);
        assert_eq!(paint.file_modified, theme.file_modified);
        assert_eq!(paint.file_untracked, theme.file_untracked);
        assert_eq!(paint.note_border, theme.note_border);
    }

    #[test]
    fn ratatui_colors_preserve_rgb_and_treat_transparency_or_malformed_input_as_reset() {
        assert_eq!(ratatui_theme_color("#aBcD09"), Color::Rgb(0xab, 0xcd, 9));
        assert_eq!(ratatui_theme_color(TRANSPARENT_BACKGROUND), Color::Reset);
        assert_eq!(ratatui_theme_color("#12345"), Color::Reset);
        assert_eq!(ratatui_theme_color("#12zz89"), Color::Reset);
        assert_eq!(ratatui_theme_color("#\u{e9}1234"), Color::Reset);
    }
}
