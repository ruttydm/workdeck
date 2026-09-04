//! Native theme-selector state derived from Hunk's React controller.
//!
//! This is a clean-room Rust translation of Hunk's MIT-licensed
//! `src/ui/hooks/useThemeSelectorController.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Ratatui owns rendering, while
//! this module preserves the source controller's committed, preview, catalog,
//! bootstrap, and transparent-surface semantics.

use crate::{
    AppTheme, NamedCustomThemeConfig, ThemeAppearance, ThemeSelectorItem, ThemeSelectorWindowState,
    available_themes, resolve_theme, with_transparent_surfaces,
};

/// Generation-based theme state shared by the review shell and selector.
///
/// The raw committed identity deliberately survives catalog replacement. This
/// lets a temporarily unavailable custom theme fall back safely and regain its
/// palette if the same identity reappears later.
#[derive(Debug)]
pub struct ThemeController {
    pub(crate) committed: String,
    pub(crate) active: String,
    pub(crate) requested: String,
    preview_theme_id: Option<String>,
    pub(crate) selected_theme_id: Option<String>,
    pub(crate) selector_open: bool,
    pub(crate) window: Option<ThemeSelectorWindowState>,
    pub(crate) generation: u64,
    pub(crate) cursor_palette: Option<String>,
    pub(crate) cursor_updates: u64,
    detected_theme_mode: Option<ThemeAppearance>,
    custom_themes: Vec<NamedCustomThemeConfig>,
    transparent_background: bool,
}

impl ThemeController {
    /// Construct from an already-resolved theme identity.
    pub fn new(theme: String) -> Self {
        Self::from_resolved(theme, None, Vec::new(), false)
    }

    /// Resolve the launch preference exactly once against terminal appearance.
    pub fn from_options(
        initial_theme: Option<&str>,
        initial_theme_mode: Option<ThemeAppearance>,
        custom_themes: Vec<NamedCustomThemeConfig>,
        transparent_background: bool,
    ) -> Self {
        let committed = resolve_theme(initial_theme, initial_theme_mode, &custom_themes).id;
        Self::from_resolved(
            committed,
            initial_theme_mode,
            custom_themes,
            transparent_background,
        )
    }

    /// Retain a composition-root theme that has already undergone launch resolution.
    pub fn from_resolved(
        committed: String,
        detected_theme_mode: Option<ThemeAppearance>,
        custom_themes: Vec<NamedCustomThemeConfig>,
        transparent_background: bool,
    ) -> Self {
        let active = resolve_theme(Some(&committed), detected_theme_mode, &custom_themes).id;
        Self {
            committed,
            active: active.clone(),
            requested: active,
            preview_theme_id: None,
            selected_theme_id: None,
            selector_open: false,
            window: None,
            generation: 0,
            cursor_palette: None,
            cursor_updates: 0,
            detected_theme_mode,
            custom_themes,
            transparent_background,
        }
    }

    /// Replace soft-bootstrap inputs without reinterpreting launch detection or
    /// an in-session committed choice.
    pub fn replace_options(
        &mut self,
        custom_themes: Vec<NamedCustomThemeConfig>,
        _initial_theme: Option<&str>,
        _initial_theme_mode: Option<ThemeAppearance>,
        transparent_background: bool,
    ) {
        self.custom_themes = custom_themes;
        self.transparent_background = transparent_background;
        let catalog = self.catalog();
        let available = |id: &str| catalog.iter().any(|theme| theme.id == id);
        let committed = resolve_theme(
            Some(&self.committed),
            self.detected_theme_mode,
            &self.custom_themes,
        );

        if self.selector_open && !self.selected_theme_id.as_deref().is_some_and(&available) {
            self.selected_theme_id = Some(committed.id.clone());
        }
        if !self.preview_theme_id.as_deref().is_some_and(&available) {
            self.preview_theme_id = None;
        }
        self.active = self
            .preview_theme_id
            .as_deref()
            .map_or_else(|| committed.id.clone(), ToOwned::to_owned);
        self.requested.clone_from(&self.active);
    }

    #[must_use]
    pub fn catalog(&self) -> Vec<AppTheme> {
        available_themes(&self.custom_themes)
    }

    #[must_use]
    pub fn theme_id(&self) -> &str {
        &self.committed
    }

    #[must_use]
    pub fn base_theme(&self) -> AppTheme {
        resolve_theme(
            Some(&self.active),
            self.detected_theme_mode,
            &self.custom_themes,
        )
    }

    #[must_use]
    pub fn active_theme(&self) -> AppTheme {
        let base = self.base_theme();
        if self.transparent_background {
            with_transparent_surfaces(&base)
        } else {
            base
        }
    }

    pub fn request_preview(&mut self, theme: impl Into<String>) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.requested = theme.into();
        self.generation
    }

    pub fn commit_preview(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.active.clone_from(&self.requested);
        true
    }

    pub fn set_cursor_palette(&mut self, palette: Option<String>) -> bool {
        if self.cursor_palette == palette {
            return false;
        }
        self.cursor_palette = palette;
        self.cursor_updates = self.cursor_updates.saturating_add(1);
        true
    }

    pub(crate) fn open_selector(&mut self, catalog: &[AppTheme]) {
        let committed = resolve_theme(
            Some(&self.committed),
            self.detected_theme_mode,
            &self.custom_themes,
        );
        let selected = catalog
            .iter()
            .find(|theme| theme.id == committed.id)
            .or_else(|| catalog.first())
            .map(|theme| theme.id.clone());
        self.selector_open = true;
        self.preview_theme_id = None;
        self.selected_theme_id = selected;
        self.active = committed.id;
        self.requested.clone_from(&self.active);
        self.window = None;
    }

    pub(crate) fn close_selector(&mut self) -> String {
        self.generation = self.generation.saturating_add(1);
        self.selector_open = false;
        self.preview_theme_id = None;
        self.selected_theme_id = None;
        self.window = None;
        let committed = resolve_theme(
            Some(&self.committed),
            self.detected_theme_mode,
            &self.custom_themes,
        )
        .id;
        self.active.clone_from(&committed);
        self.requested = committed;
        self.committed.clone()
    }

    pub(crate) fn selected_index(&self, catalog: &[AppTheme]) -> usize {
        self.selected_theme_id
            .as_ref()
            .and_then(|selected| catalog.iter().position(|theme| &theme.id == selected))
            .or_else(|| {
                let committed = resolve_theme(
                    Some(&self.committed),
                    self.detected_theme_mode,
                    &self.custom_themes,
                );
                catalog.iter().position(|theme| theme.id == committed.id)
            })
            .unwrap_or(0)
    }

    pub(crate) fn preview_index(&mut self, catalog: &[AppTheme], index: usize) -> Option<String> {
        let theme = catalog.get(index)?;
        self.preview_theme_id = Some(theme.id.clone());
        self.selected_theme_id = Some(theme.id.clone());
        let generation = self.request_preview(&theme.id);
        self.commit_preview(generation);
        Some(theme.id.clone())
    }

    pub(crate) fn move_selector(&mut self, catalog: &[AppTheme], delta: isize) -> Option<String> {
        if catalog.is_empty() {
            self.preview_theme_id = None;
            self.selected_theme_id = None;
            let committed = resolve_theme(
                Some(&self.committed),
                self.detected_theme_mode,
                &self.custom_themes,
            )
            .id;
            self.requested.clone_from(&committed);
            self.active = committed;
            return None;
        }
        let anchor = self.selected_index(catalog);
        let count = isize::try_from(catalog.len()).unwrap_or(isize::MAX);
        let anchor = isize::try_from(anchor).unwrap_or_default();
        let next = usize::try_from((anchor + delta).rem_euclid(count)).unwrap_or_default();
        self.preview_index(catalog, next)
    }

    pub(crate) fn accept_selector(&mut self, catalog: &[AppTheme]) -> Option<(String, String)> {
        let selected = self.selected_theme_id.as_ref()?;
        let theme = catalog.iter().find(|theme| &theme.id == selected)?;
        self.committed.clone_from(&theme.id);
        self.active.clone_from(&theme.id);
        self.requested.clone_from(&theme.id);
        self.selector_open = false;
        self.preview_theme_id = None;
        self.selected_theme_id = Some(theme.id.clone());
        self.window = None;
        Some((theme.id.clone(), theme.label.clone()))
    }

    pub(crate) fn accept_selector_index(
        &mut self,
        catalog: &[AppTheme],
        index: usize,
    ) -> Option<(String, String)> {
        let theme = catalog.get(index)?;
        self.selected_theme_id = Some(theme.id.clone());
        self.accept_selector(catalog)
    }

    pub(crate) fn items(&self, catalog: &[AppTheme]) -> Vec<ThemeSelectorItem> {
        let active = self.base_theme().id;
        catalog
            .iter()
            .map(|theme| ThemeSelectorItem {
                id: theme.id.clone(),
                label: theme.label.clone(),
                description: if theme.id == active {
                    "active".into()
                } else {
                    String::new()
                },
                active: theme.id == active,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_DARK_THEME_ID, DEFAULT_LIGHT_THEME_ID, TRANSPARENT_BACKGROUND};

    #[test]
    fn frozen_oracle_records_the_baseline_controller_and_stable_absence() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../port/hunk/oracles/theme-selector-controller.json"
        )))
        .unwrap();
        assert_eq!(
            oracle["source"]["blob"],
            "7db6223da5edf57cf0c1fcf92e79b2307f369308"
        );
        assert_eq!(oracle["source"]["bytes"], 7_553);
        assert_eq!(oracle["sourceTest"]["bytes"], 13_602);
        assert_eq!(oracle["baselineOracle"]["passed"], 8);
        assert_eq!(oracle["baselineOracle"]["assertions"], 56);
        assert_eq!(oracle["stableOracle"]["status"], "absent");
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 8);
    }

    fn custom_theme(id: &str, label: &str, accent: &str) -> NamedCustomThemeConfig {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "label": label,
            "base": DEFAULT_DARK_THEME_ID,
            "accent": accent,
        }))
        .unwrap()
    }

    #[test]
    fn resolves_auto_initialization_from_detected_light_or_dark_terminal_mode() {
        let light = ThemeController::from_options(
            Some("auto"),
            Some(ThemeAppearance::Light),
            Vec::new(),
            false,
        );
        let dark = ThemeController::from_options(
            Some("auto"),
            Some(ThemeAppearance::Dark),
            Vec::new(),
            false,
        );
        assert_eq!(light.theme_id(), DEFAULT_LIGHT_THEME_ID);
        assert_eq!(light.base_theme().appearance, ThemeAppearance::Light);
        assert_eq!(dark.theme_id(), DEFAULT_DARK_THEME_ID);
        assert_eq!(dark.base_theme().appearance, ThemeAppearance::Dark);
    }

    #[test]
    fn opens_on_committed_theme_and_wraps_keyboard_preview_movement() {
        let mut controller = ThemeController::from_options(None, None, Vec::new(), false);
        let catalog = controller.catalog();
        let first = catalog[0].id.clone();
        controller.committed.clone_from(&first);
        controller.active.clone_from(&first);
        controller.open_selector(&catalog);
        assert!(controller.selector_open);
        assert_eq!(catalog[controller.selected_index(&catalog)].id, first);

        controller.move_selector(&catalog, -1);
        assert_eq!(controller.selected_index(&catalog), catalog.len() - 1);
        assert_eq!(controller.theme_id(), first);
        assert_eq!(controller.base_theme().id, catalog.last().unwrap().id);
        assert!(!controller.items(&catalog)[0].active);
        assert!(controller.items(&catalog).last().unwrap().active);

        controller.move_selector(&catalog, 1);
        assert_eq!(controller.selected_index(&catalog), 0);
        assert_eq!(controller.base_theme().id, first);
    }

    #[test]
    fn pointer_preview_is_transient_cancel_restores_and_invalid_indexes_are_safe() {
        let mut controller =
            ThemeController::from_options(Some(DEFAULT_DARK_THEME_ID), None, Vec::new(), false);
        let catalog = controller.catalog();
        controller.open_selector(&catalog);
        let preview = catalog[2].id.clone();
        controller.preview_index(&catalog, 2);
        assert_eq!(controller.base_theme().id, preview);
        assert_eq!(controller.theme_id(), DEFAULT_DARK_THEME_ID);
        assert!(controller.items(&catalog)[2].active);
        assert!(controller.preview_index(&catalog, usize::MAX).is_none());
        assert!(
            controller
                .accept_selector_index(&catalog, usize::MAX)
                .is_none()
        );
        assert_eq!(controller.base_theme().id, preview);
        controller.close_selector();
        assert!(!controller.selector_open);
        assert_eq!(controller.base_theme().id, DEFAULT_DARK_THEME_ID);
        assert_eq!(controller.theme_id(), DEFAULT_DARK_THEME_ID);
    }

    #[test]
    fn pointer_and_keyboard_acceptance_commit_atomically_and_return_notices() {
        let mut controller =
            ThemeController::from_options(Some(DEFAULT_DARK_THEME_ID), None, Vec::new(), false);
        let catalog = controller.catalog();
        controller.open_selector(&catalog);
        let pointer = catalog[2].clone();
        controller.preview_index(&catalog, 2);
        let accepted = controller.accept_selector_index(&catalog, 2).unwrap();
        assert_eq!(accepted, (pointer.id.clone(), pointer.label.clone()));
        assert!(!controller.selector_open);
        assert_eq!(controller.theme_id(), pointer.id);
        assert_eq!(controller.base_theme().id, pointer.id);

        controller.open_selector(&catalog);
        controller.move_selector(&catalog, 1);
        let keyboard = catalog[controller.selected_index(&catalog)].clone();
        let accepted = controller.accept_selector(&catalog).unwrap();
        assert_eq!(accepted, (keyboard.id.clone(), keyboard.label));
        assert_eq!(controller.theme_id(), keyboard.id);
    }

    #[test]
    fn movement_immediately_followed_by_acceptance_commits_latest_identity() {
        let mut controller =
            ThemeController::from_options(Some(DEFAULT_DARK_THEME_ID), None, Vec::new(), false);
        let catalog = controller.catalog();
        controller.open_selector(&catalog);
        let next = (controller.selected_index(&catalog) + 1) % catalog.len();
        let expected = catalog[next].id.clone();
        controller.move_selector(&catalog, 1);
        controller.accept_selector(&catalog);
        assert_eq!(controller.theme_id(), expected);
        assert_eq!(controller.base_theme().id, expected);
    }

    #[test]
    fn transparent_background_only_projects_resolved_base_theme_surfaces() {
        let custom = custom_theme("team-dark", "Team Dark", "#8877cc");
        let controller =
            ThemeController::from_options(Some(&custom.id), None, vec![custom.clone()], true);
        assert_eq!(controller.theme_id(), custom.id);
        assert_eq!(controller.base_theme().id, custom.id);
        assert_ne!(controller.base_theme().background, TRANSPARENT_BACKGROUND);
        assert_eq!(controller.active_theme().background, TRANSPARENT_BACKGROUND);
        assert_eq!(
            controller.active_theme().added_bg,
            controller.base_theme().added_bg
        );
    }

    #[test]
    fn catalog_replacement_re_resolves_palettes_and_preserves_valid_selection() {
        let alpha = custom_theme("alpha-theme", "Alpha", "#112233");
        let beta = custom_theme("beta-theme", "Beta", "#445566");
        let mut controller = ThemeController::from_options(
            Some(&alpha.id),
            None,
            vec![alpha.clone(), beta.clone()],
            false,
        );
        let catalog = controller.catalog();
        controller.open_selector(&catalog);
        let beta_index = catalog
            .iter()
            .position(|theme| theme.id == beta.id)
            .unwrap();
        controller.preview_index(&catalog, beta_index);

        let next_alpha = custom_theme("alpha-theme", "Alpha updated", "#778899");
        let next_beta = custom_theme("beta-theme", "Beta updated", "#aabbcc");
        controller.replace_options(
            vec![next_beta.clone(), next_alpha.clone()],
            Some(DEFAULT_LIGHT_THEME_ID),
            Some(ThemeAppearance::Light),
            false,
        );
        let catalog = controller.catalog();
        assert_eq!(controller.theme_id(), alpha.id);
        assert_eq!(catalog[controller.selected_index(&catalog)].id, beta.id);
        assert_eq!(controller.base_theme().id, beta.id);
        assert_eq!(controller.base_theme().accent, "#aabbcc");

        controller.replace_options(
            vec![next_alpha.clone()],
            Some(DEFAULT_LIGHT_THEME_ID),
            Some(ThemeAppearance::Light),
            false,
        );
        assert_eq!(controller.theme_id(), alpha.id);
        assert_eq!(controller.base_theme().id, alpha.id);
        assert!(controller.selected_index(&controller.catalog()) < controller.catalog().len());

        controller.replace_options(
            Vec::new(),
            Some(DEFAULT_LIGHT_THEME_ID),
            Some(ThemeAppearance::Light),
            false,
        );
        assert_eq!(controller.theme_id(), alpha.id);
        assert_eq!(controller.base_theme().id, DEFAULT_DARK_THEME_ID);
        assert!(controller.selected_index(&controller.catalog()) < controller.catalog().len());

        controller.replace_options(
            vec![next_alpha],
            Some(DEFAULT_LIGHT_THEME_ID),
            Some(ThemeAppearance::Light),
            false,
        );
        assert_eq!(controller.theme_id(), alpha.id);
        assert_eq!(controller.base_theme().id, alpha.id);
        assert_eq!(controller.base_theme().accent, "#778899");
    }

    #[test]
    fn soft_bootstrap_replacement_preserves_detected_mode_and_in_session_choice() {
        let mut controller = ThemeController::from_options(
            Some("auto"),
            Some(ThemeAppearance::Dark),
            Vec::new(),
            false,
        );
        controller.replace_options(
            Vec::new(),
            Some("auto"),
            Some(ThemeAppearance::Light),
            false,
        );
        assert_eq!(controller.theme_id(), DEFAULT_DARK_THEME_ID);

        let catalog = controller.catalog();
        let dracula = catalog
            .iter()
            .position(|theme| theme.id == "dracula")
            .unwrap();
        controller.accept_selector_index(&catalog, dracula);
        controller.replace_options(
            Vec::new(),
            Some(DEFAULT_LIGHT_THEME_ID),
            Some(ThemeAppearance::Light),
            false,
        );
        assert_eq!(controller.theme_id(), "dracula");
        assert_eq!(controller.base_theme().id, "dracula");
    }
}
