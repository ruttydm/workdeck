//! The fully resolved boundary between launch composition and an interactive review shell.

use std::path::PathBuf;

use crate::{
    Changeset, CliInput, InputCursorLine, InputLayoutMode, NamedCustomThemeConfig,
    SidebarVisibility, StartupNotice, UserKeyBindingEntry,
};

/// Appearance detected from the terminal before the first frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalThemeMode {
    Light,
    Dark,
}

/// Source state retained so the live review can reload and watch the same input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadContext<VcsCatalogState = ()> {
    pub cwd: PathBuf,
    pub repo_root: Option<PathBuf>,
    pub initial_watch_signature: Option<String>,
    /// Complete provider catalog used for this launch, without coupling core to its host type.
    pub vcs_catalog: Option<VcsCatalogState>,
}

/// One fully resolved review launch.
///
/// Generic host state lets the composition root carry native extension and VCS implementations
/// without introducing reverse dependencies from `workdeck-core`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppBootstrap<ExtensionState = (), VcsCatalogState = ()> {
    pub input: CliInput,
    pub reload_context: ReloadContext<VcsCatalogState>,
    pub changeset: Changeset,
    pub initial_mode: InputLayoutMode,
    pub initial_theme: Option<String>,
    pub initial_theme_mode: Option<TerminalThemeMode>,
    pub custom_themes: Vec<NamedCustomThemeConfig>,
    pub initial_show_line_numbers: bool,
    pub initial_tab_width: u16,
    pub initial_file_gap: u16,
    pub initial_hunk_gap: u16,
    pub initial_wrap_lines: bool,
    pub initial_show_hunk_headers: bool,
    pub initial_show_menu_bar: bool,
    pub initial_sidebar: SidebarVisibility,
    pub initial_show_agent_notes: bool,
    pub initial_copy_decorations: bool,
    pub initial_cursor_line: InputCursorLine,
    pub startup_notices: Vec<StartupNotice>,
    pub view_preferences_config_path: Option<PathBuf>,
    pub keybindings: Vec<UserKeyBindingEntry>,
    pub keybinding_notices: Vec<String>,
    pub extensions: Option<ExtensionState>,
}

impl<ExtensionState, VcsCatalogState> AppBootstrap<ExtensionState, VcsCatalogState> {
    /// Assemble the source-neutral launch defaults from already resolved command options.
    /// Host-owned themes, extensions and notices are attached by the composition root afterward.
    #[must_use]
    pub fn new(
        input: CliInput,
        reload_context: ReloadContext<VcsCatalogState>,
        changeset: Changeset,
    ) -> Self {
        let options = input.options();
        Self {
            reload_context,
            changeset,
            initial_mode: options.mode.unwrap_or_default(),
            initial_theme: options.theme.clone(),
            initial_theme_mode: None,
            custom_themes: Vec::new(),
            initial_show_line_numbers: options.line_numbers.unwrap_or(true),
            initial_tab_width: options.tab_width.unwrap_or(4),
            initial_file_gap: options.file_gap.unwrap_or(1),
            initial_hunk_gap: options.hunk_gap.unwrap_or(0),
            initial_wrap_lines: options.wrap_lines.unwrap_or(false),
            initial_show_hunk_headers: options.hunk_headers.unwrap_or(true),
            initial_show_menu_bar: options.menu_bar.unwrap_or(true),
            initial_sidebar: options.sidebar.unwrap_or_default(),
            initial_show_agent_notes: options.agent_notes.unwrap_or(false),
            initial_copy_decorations: options.copy_decorations.unwrap_or(false),
            initial_cursor_line: options.cursor_line.unwrap_or_default(),
            startup_notices: Vec::new(),
            view_preferences_config_path: None,
            keybindings: Vec::new(),
            keybinding_notices: Vec::new(),
            extensions: None,
            input,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChangesetSource, CommonOptions, PatchCommandInput, UserKeyBinding};

    #[test]
    fn bootstrap_retains_every_resolved_launch_and_reload_field() {
        let input = CliInput::Patch(PatchCommandInput {
            file: Some("review.patch".into()),
            text: None,
            options: CommonOptions {
                mode: Some(InputLayoutMode::Split),
                theme: Some("nord".into()),
                line_numbers: Some(false),
                tab_width: Some(8),
                file_gap: Some(2),
                hunk_gap: Some(1),
                wrap_lines: Some(true),
                hunk_headers: Some(false),
                menu_bar: Some(false),
                sidebar: Some(SidebarVisibility::Hidden),
                agent_notes: Some(true),
                copy_decorations: Some(true),
                cursor_line: Some(InputCursorLine::Number),
                ..CommonOptions::default()
            },
        });
        let bootstrap = AppBootstrap {
            input: input.clone(),
            reload_context: ReloadContext {
                cwd: PathBuf::from("/repo/subdir"),
                repo_root: Some(PathBuf::from("/repo")),
                initial_watch_signature: Some("signature".into()),
                vcs_catalog: Some("catalog"),
            },
            changeset: Changeset {
                id: "patch:review".into(),
                source_label: "review.patch".into(),
                title: "Patch review: review.patch".into(),
                summary: None,
                agent_summary: None,
                source: ChangesetSource::Patch {
                    label: "review.patch".into(),
                },
                files: Vec::new(),
            },
            initial_mode: InputLayoutMode::Split,
            initial_theme: Some("nord".into()),
            initial_theme_mode: Some(TerminalThemeMode::Dark),
            custom_themes: Vec::new(),
            initial_show_line_numbers: false,
            initial_tab_width: 8,
            initial_file_gap: 2,
            initial_hunk_gap: 1,
            initial_wrap_lines: true,
            initial_show_hunk_headers: false,
            initial_show_menu_bar: false,
            initial_sidebar: SidebarVisibility::Hidden,
            initial_show_agent_notes: true,
            initial_copy_decorations: true,
            initial_cursor_line: InputCursorLine::Number,
            startup_notices: vec![StartupNotice::new("notice", "message")],
            view_preferences_config_path: Some(PathBuf::from("/config/workdeck/config.toml")),
            keybindings: vec![UserKeyBindingEntry::new(
                "workdeck.app.quit",
                UserKeyBinding::Chord("ctrl+q".into()),
            )],
            keybinding_notices: vec!["diagnostic".into()],
            extensions: Some("extension state"),
        };

        assert_eq!(bootstrap.input, input);
        assert_eq!(bootstrap.reload_context.vcs_catalog, Some("catalog"));
        assert_eq!(
            bootstrap.reload_context.initial_watch_signature.as_deref(),
            Some("signature")
        );
        assert_eq!(bootstrap.initial_cursor_line, InputCursorLine::Number);
        assert!(bootstrap.initial_copy_decorations);
        assert_eq!(bootstrap.extensions, Some("extension state"));

        let mut expected = bootstrap.clone();
        expected.initial_theme_mode = None;
        expected.startup_notices.clear();
        expected.view_preferences_config_path = None;
        expected.keybindings.clear();
        expected.keybinding_notices.clear();
        expected.extensions = None;
        assert_eq!(
            AppBootstrap::new(
                input,
                bootstrap.reload_context.clone(),
                bootstrap.changeset.clone()
            ),
            expected
        );

        let default_input = CliInput::Patch(PatchCommandInput {
            file: None,
            text: Some(String::new()),
            options: CommonOptions::default(),
        });
        let defaults: AppBootstrap<(), &str> =
            AppBootstrap::new(default_input, bootstrap.reload_context, bootstrap.changeset);
        assert_eq!(defaults.initial_mode, InputLayoutMode::Auto);
        assert!(defaults.initial_theme.is_none());
        assert!(defaults.initial_theme_mode.is_none());
        assert!(defaults.custom_themes.is_empty());
        assert!(defaults.initial_show_line_numbers);
        assert_eq!(defaults.initial_tab_width, 4);
        assert_eq!(defaults.initial_file_gap, 1);
        assert_eq!(defaults.initial_hunk_gap, 0);
        assert!(!defaults.initial_wrap_lines);
        assert!(defaults.initial_show_hunk_headers && defaults.initial_show_menu_bar);
        assert_eq!(defaults.initial_sidebar, SidebarVisibility::Auto);
        assert!(!defaults.initial_show_agent_notes && !defaults.initial_copy_decorations);
        assert_eq!(defaults.initial_cursor_line, InputCursorLine::Row);
        assert!(defaults.extensions.is_none());
    }
}
