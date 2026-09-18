//! Live application menus derived from the command dispatcher.

use workdeck_review::LayoutMode;

use crate::{AppCommand, AppMenus, CommandCursorLine, MenuEntry, MenuId};

/// The menu-facing projection of either a built-in or extension command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMenuCommand {
    pub id: String,
    pub title: String,
    pub key_labels: Vec<String>,
    pub enabled: bool,
}

impl From<&AppCommand> for AppMenuCommand {
    fn from(command: &AppCommand) -> Self {
        Self {
            id: command.id.into(),
            title: command.title.into(),
            key_labels: command.key_labels.clone(),
            enabled: command.enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildAppMenusOptions {
    pub commands: Vec<AppMenuCommand>,
    /// Extension-contributed subset in registration order.
    pub extension_commands: Vec<AppMenuCommand>,
    /// Host-owned per-file presentation choices appended to View.
    pub file_view_entries: Vec<MenuEntry>,
    /// Host-owned escape hatch while an extension input mode is active.
    pub keyboard_mode_exit_entry: Option<MenuEntry>,
    pub file_view_apply_all_label: Option<String>,
    pub copy_decorations: bool,
    pub cursor_line: CommandCursorLine,
    pub layout_mode: LayoutMode,
    pub files_pane_visible: bool,
    pub show_agent_notes: bool,
    pub show_help: bool,
    pub show_hunk_headers: bool,
    pub show_line_numbers: bool,
    pub show_menu_bar: bool,
    pub wrap_lines: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MenuEntrySpec {
    Command {
        id: &'static str,
        label: Option<&'static str>,
        checked: Option<bool>,
    },
    OwnedCommand {
        id: String,
    },
    Separator,
}

fn command(id: &'static str) -> MenuEntrySpec {
    MenuEntrySpec::Command {
        id,
        label: None,
        checked: None,
    }
}

fn labeled(id: &'static str, label: &'static str) -> MenuEntrySpec {
    MenuEntrySpec::Command {
        id,
        label: Some(label),
        checked: None,
    }
}

fn checked(id: &'static str, label: &'static str, value: bool) -> MenuEntrySpec {
    MenuEntrySpec::Command {
        id,
        label: Some(label),
        checked: Some(value),
    }
}

/// Resolve menu declarations against the live command table.
fn to_menu_entries(commands: &[AppMenuCommand], specs: &[MenuEntrySpec]) -> Vec<MenuEntry> {
    specs
        .iter()
        .filter_map(|spec| match spec {
            MenuEntrySpec::Separator => Some(MenuEntry::Separator),
            MenuEntrySpec::Command { id, label, checked } => commands
                .iter()
                .find(|candidate| candidate.id == *id && candidate.enabled)
                .map(|command| MenuEntry::Item {
                    label: label.unwrap_or(&command.title).to_string(),
                    command_id: Some((*id).into()),
                    hint: command.key_labels.first().cloned(),
                    checked: *checked,
                }),
            MenuEntrySpec::OwnedCommand { id } => commands
                .iter()
                .find(|candidate| candidate.id == *id && candidate.enabled)
                .map(|command| MenuEntry::Item {
                    label: command.title.clone(),
                    command_id: Some(id.clone()),
                    hint: command.key_labels.first().cloned(),
                    checked: None,
                }),
        })
        .collect()
}

fn extension_menu_entries(
    commands: &[AppMenuCommand],
    extension_commands: &[AppMenuCommand],
) -> Vec<MenuEntry> {
    let mut specs = Vec::new();
    let mut previous_owner = None;
    for command in extension_commands {
        let owner = command.id.split_once('.').map_or("", |(owner, _)| owner);
        if previous_owner.is_some_and(|previous| previous != owner) {
            specs.push(MenuEntrySpec::Separator);
        }
        previous_owner = Some(owner);
        specs.push(MenuEntrySpec::OwnedCommand {
            id: command.id.clone(),
        });
    }
    to_menu_entries(commands, &specs)
}

/// Build every top-level menu from one command table and the current view state.
#[must_use]
pub fn build_app_menus(options: BuildAppMenusOptions) -> AppMenus {
    let mut menus = AppMenus::new();
    let file = vec![
        labeled("workdeck.app.toggleFocusArea", "Toggle files/filter focus"),
        labeled("workdeck.review.focusFilter", "Focus filter"),
        labeled("workdeck.review.editSelectedFile", "Open file in editor"),
        labeled("workdeck.app.refresh", "Reload"),
        MenuEntrySpec::Separator,
        command("workdeck.app.quit"),
    ];
    let mut view = vec![
        checked(
            "workdeck.view.layoutSplit",
            "Split view",
            options.layout_mode == LayoutMode::Split,
        ),
        checked(
            "workdeck.view.layoutStack",
            "Stacked view",
            options.layout_mode == LayoutMode::Stack,
        ),
        checked(
            "workdeck.view.layoutAuto",
            "Auto layout",
            options.layout_mode == LayoutMode::Auto,
        ),
        MenuEntrySpec::Separator,
        checked(
            "workdeck.view.toggleFilesPane",
            "Files pane",
            options.files_pane_visible,
        ),
        checked(
            "workdeck.view.toggleMenuBar",
            "Menu bar",
            options.show_menu_bar,
        ),
        MenuEntrySpec::Separator,
        labeled("workdeck.view.openThemeSelector", "Themes…"),
        MenuEntrySpec::Separator,
        checked(
            "workdeck.view.toggleAgentNotes",
            "Agent notes",
            options.show_agent_notes,
        ),
        checked(
            "workdeck.view.toggleLineNumbers",
            "Line numbers",
            options.show_line_numbers,
        ),
        checked(
            "workdeck.view.toggleLineWrap",
            "Line wrapping",
            options.wrap_lines,
        ),
        checked(
            "workdeck.view.toggleHunkHeaders",
            "Hunk metadata",
            options.show_hunk_headers,
        ),
        checked(
            "workdeck.view.toggleCopyDecorations",
            "Copy decorations",
            options.copy_decorations,
        ),
        checked(
            "workdeck.view.cursorLineRow",
            "Current line: full row",
            options.cursor_line == CommandCursorLine::Row,
        ),
        checked(
            "workdeck.view.cursorLineNumber",
            "Current line: line number",
            options.cursor_line == CommandCursorLine::Number,
        ),
        checked(
            "workdeck.view.cursorLineOff",
            "Current line: off",
            options.cursor_line == CommandCursorLine::Off,
        ),
    ];
    if !options.file_view_entries.is_empty() {
        view.push(MenuEntrySpec::Separator);
    }
    let navigate = vec![
        command("workdeck.review.previousHunk"),
        command("workdeck.review.nextHunk"),
        MenuEntrySpec::Separator,
        labeled("workdeck.review.previousAnnotatedHunk", "Previous comment"),
        labeled("workdeck.review.nextAnnotatedHunk", "Next comment"),
        MenuEntrySpec::Separator,
        // Bundled content search lives in the menu that names it, not under
        // Extensions: it is Workdeck's own tier.
        labeled("workdeck.search.find", "Search diff content…"),
        labeled("workdeck.search.next", "Next match"),
        labeled("workdeck.search.previous", "Previous match"),
        MenuEntrySpec::Separator,
        labeled("workdeck.review.focusFilter", "Focus filter"),
    ];
    let agent = vec![
        checked(
            "workdeck.view.toggleAgentNotes",
            "Agent notes",
            options.show_agent_notes,
        ),
        labeled("workdeck.app.openAgentSkill", "Agent skill"),
        MenuEntrySpec::Separator,
        command("workdeck.review.nextAnnotatedFile"),
        command("workdeck.review.previousAnnotatedFile"),
    ];
    let help = vec![checked(
        "workdeck.app.toggleHelp",
        "Controls help",
        options.show_help,
    )];

    menus.insert(MenuId::File, to_menu_entries(&options.commands, &file));
    let mut view_entries = to_menu_entries(&options.commands, &view);
    view_entries.extend(options.file_view_entries);
    if let Some(label) = options.file_view_apply_all_label {
        let mut apply = to_menu_entries(
            &options.commands,
            &[MenuEntrySpec::Command {
                id: "workdeck.view.applyFilePresentationToAllMatching",
                label: None,
                checked: None,
            }],
        );
        if let Some(MenuEntry::Item { label: row, .. }) = apply.first_mut() {
            *row = label;
        }
        if !apply.is_empty() {
            view_entries.push(MenuEntry::Separator);
            view_entries.extend(apply);
        }
    }
    menus.insert(MenuId::View, view_entries);
    menus.insert(
        MenuId::Navigate,
        to_menu_entries(&options.commands, &navigate),
    );
    menus.insert(MenuId::Agent, to_menu_entries(&options.commands, &agent));

    let extension_commands = extension_menu_entries(&options.commands, &options.extension_commands);
    let extensions = match options.keyboard_mode_exit_entry {
        Some(exit) if extension_commands.is_empty() => vec![exit],
        Some(exit) => {
            let mut entries = vec![exit, MenuEntry::Separator];
            entries.extend(extension_commands);
            entries
        }
        None => extension_commands,
    };
    if !extensions.is_empty() {
        menus.insert(MenuId::Extensions, extensions);
    }
    menus.insert(MenuId::Help, to_menu_entries(&options.commands, &help));
    menus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BuiltinCommandAvailability, UserKeyBinding, UserKeyBindingEntry, build_app_commands,
        builtin_command_key_defaults, resolve_command_keys,
    };

    fn commands(availability: BuiltinCommandAvailability) -> Vec<AppMenuCommand> {
        let mut commands = build_app_commands(None, availability)
            .iter()
            .map(AppMenuCommand::from)
            .collect::<Vec<_>>();
        // The live app composes bundled search commands beside the built-ins;
        // menus must resolve them from the same table.
        let mut defaults = builtin_command_key_defaults();
        defaults.extend(crate::bundled_search_command_defaults());
        let resolved = resolve_command_keys(&defaults, &[]);
        commands.extend(
            crate::bundled_search_command_views(&resolved)
                .into_iter()
                .map(|view| AppMenuCommand {
                    id: view.id.into(),
                    title: view.title.into(),
                    key_labels: view.key_labels,
                    enabled: true,
                }),
        );
        commands
    }

    fn base_options() -> BuildAppMenusOptions {
        BuildAppMenusOptions {
            commands: commands(BuiltinCommandAvailability {
                can_align_current_line: true,
                can_apply_file_presentation_to_all_matching: false,
                can_edit_active_note: false,
                can_reply_to_active_note: false,
                can_refresh_current_input: true,
            }),
            extension_commands: Vec::new(),
            file_view_entries: Vec::new(),
            keyboard_mode_exit_entry: None,
            file_view_apply_all_label: None,
            copy_decorations: true,
            cursor_line: CommandCursorLine::Row,
            layout_mode: LayoutMode::Stack,
            files_pane_visible: false,
            show_agent_notes: true,
            show_help: false,
            show_hunk_headers: false,
            show_line_numbers: true,
            show_menu_bar: true,
            wrap_lines: true,
        }
    }

    fn items(menus: &AppMenus, id: MenuId) -> Vec<&MenuEntry> {
        menus
            .get(&id)
            .into_iter()
            .flatten()
            .filter(|entry| matches!(entry, MenuEntry::Item { .. }))
            .collect()
    }

    fn item<'a>(menus: &'a AppMenus, id: MenuId, label: &str) -> &'a MenuEntry {
        items(menus, id)
            .into_iter()
            .find(|entry| matches!(entry, MenuEntry::Item { label: value, .. } if value == label))
            .unwrap_or_else(|| panic!("missing {label:?} in {id:?}"))
    }

    fn item_fields(entry: &MenuEntry) -> (&str, Option<&str>, Option<&str>, Option<bool>) {
        match entry {
            MenuEntry::Item {
                label,
                command_id,
                hint,
                checked,
            } => (label, command_id.as_deref(), hint.as_deref(), *checked),
            MenuEntry::Separator => panic!("expected an item"),
        }
    }

    #[test]
    fn cursor_entries_check_only_the_active_style() {
        for (mode, expected) in [
            (CommandCursorLine::Row, "Current line: full row"),
            (CommandCursorLine::Number, "Current line: line number"),
            (CommandCursorLine::Off, "Current line: off"),
        ] {
            let mut options = base_options();
            options.cursor_line = mode;
            let menus = build_app_menus(options);
            let checked = items(&menus, MenuId::View)
                .into_iter()
                .filter_map(|entry| {
                    let (label, _, _, checked) = item_fields(entry);
                    (checked == Some(true) && label.starts_with("Current line:")).then_some(label)
                })
                .collect::<Vec<_>>();
            assert_eq!(checked, [expected]);
        }
    }

    #[test]
    fn labels_hints_and_checks_come_from_commands_and_live_state() {
        let menus = build_app_menus(base_options());
        assert_eq!(
            items(&menus, MenuId::File)
                .iter()
                .map(|entry| item_fields(entry).0)
                .collect::<Vec<_>>(),
            [
                "Toggle files/filter focus",
                "Focus filter",
                "Open file in editor",
                "Reload",
                "Quit",
            ]
        );
        assert_eq!(
            item_fields(menus.get(&MenuId::File).unwrap().first().unwrap()),
            (
                "Toggle files/filter focus",
                Some("workdeck.app.toggleFocusArea"),
                Some("Tab"),
                None,
            )
        );
        assert_eq!(
            items(&menus, MenuId::View)
                .into_iter()
                .filter_map(|entry| {
                    let (label, _, _, checked) = item_fields(entry);
                    (checked == Some(true)).then_some(label)
                })
                .collect::<Vec<_>>(),
            [
                "Stacked view",
                "Menu bar",
                "Agent notes",
                "Line numbers",
                "Line wrapping",
                "Copy decorations",
                "Current line: full row",
            ]
        );
        assert!(
            items(&menus, MenuId::View)
                .into_iter()
                .any(|entry| item_fields(entry).0 == "Themes…")
        );
        assert_eq!(
            items(&menus, MenuId::Agent)
                .iter()
                .map(|entry| item_fields(entry).0)
                .collect::<Vec<_>>(),
            [
                "Agent notes",
                "Agent skill",
                "Next annotated file",
                "Previous annotated file",
            ]
        );
        assert_eq!(
            items(&menus, MenuId::Navigate)
                .iter()
                .map(|entry| item_fields(entry).2)
                .collect::<Vec<_>>(),
            // The filter ships unbound, so its Navigate entry carries no hint.
            [
                Some("["),
                Some("]"),
                Some("{"),
                Some("}"),
                Some("/"),
                Some("n"),
                Some("N"),
                None
            ]
        );
    }

    #[test]
    fn remapped_and_unbound_commands_change_the_canonical_menu_item() {
        let resolved = resolve_command_keys(
            &builtin_command_key_defaults(),
            &[
                UserKeyBindingEntry {
                    command_id: "workdeck.view.toggleSidebar".into(),
                    binding: UserKeyBinding::Chords(vec!["ctrl+b".into()]),
                },
                UserKeyBindingEntry {
                    command_id: "workdeck.app.quit".into(),
                    binding: UserKeyBinding::Disabled,
                },
            ],
        );
        let mut options = base_options();
        options.commands = build_app_commands(
            Some(&resolved),
            BuiltinCommandAvailability {
                can_refresh_current_input: true,
                ..BuiltinCommandAvailability::default()
            },
        )
        .iter()
        .map(AppMenuCommand::from)
        .collect();
        let menus = build_app_menus(options);
        assert_eq!(
            item_fields(item(&menus, MenuId::View, "Files pane")),
            (
                "Files pane",
                Some("workdeck.view.toggleFilesPane"),
                Some("Ctrl+B"),
                Some(false),
            )
        );
        assert_eq!(item_fields(item(&menus, MenuId::File, "Quit")).2, None);
        assert_eq!(
            item_fields(item(&menus, MenuId::View, "Copy decorations")).2,
            None
        );
    }

    #[test]
    fn disabled_reload_drops_out_and_enabled_bulk_action_follows_file_view_rows() {
        let mut options = base_options();
        options.commands = commands(BuiltinCommandAvailability {
            can_apply_file_presentation_to_all_matching: true,
            can_refresh_current_input: false,
            ..BuiltinCommandAvailability::default()
        });
        options.file_view_entries.push(MenuEntry::Item {
            label: "File presentation: Preview".into(),
            command_id: Some("workdeck.view.filePresentation.preview".into()),
            hint: None,
            checked: None,
        });
        options.file_view_apply_all_label = Some("Apply \"Preview\" to all matching files".into());
        let menus = build_app_menus(options);
        assert!(
            !items(&menus, MenuId::File)
                .into_iter()
                .any(|entry| item_fields(entry).0 == "Reload")
        );
        assert_eq!(
            item_fields(item(
                &menus,
                MenuId::View,
                "Apply \"Preview\" to all matching files"
            ))
            .1,
            Some("workdeck.view.applyFilePresentationToAllMatching")
        );
    }

    fn extension(id: &str, title: &str, hint: Option<&str>) -> AppMenuCommand {
        AppMenuCommand {
            id: id.into(),
            title: title.into(),
            key_labels: hint.into_iter().map(str::to_owned).collect(),
            enabled: true,
        }
    }

    #[test]
    fn extensions_are_absent_or_grouped_in_registration_order_with_distinct_ids() {
        assert!(!build_app_menus(base_options()).contains_key(&MenuId::Extensions));
        let mut options = base_options();
        options.extension_commands = vec![
            extension("notes.refresh", "Refresh", Some("y")),
            extension("notes.quiet", "Quiet mode", None),
            extension("blame.refresh", "Refresh", None),
        ];
        options.commands.extend(options.extension_commands.clone());
        let menus = build_app_menus(options);
        let extensions = menus.get(&MenuId::Extensions).unwrap();
        assert_eq!(
            extensions,
            &[
                MenuEntry::Item {
                    label: "Refresh".into(),
                    command_id: Some("notes.refresh".into()),
                    hint: Some("y".into()),
                    checked: None,
                },
                MenuEntry::Item {
                    label: "Quiet mode".into(),
                    command_id: Some("notes.quiet".into()),
                    hint: None,
                    checked: None,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "Refresh".into(),
                    command_id: Some("blame.refresh".into()),
                    hint: None,
                    checked: None,
                },
            ]
        );
    }

    #[test]
    fn active_extension_mode_prepends_the_host_exit_action() {
        let mut options = base_options();
        options.keyboard_mode_exit_entry = Some(MenuEntry::Item {
            label: "Exit Vim navigation".into(),
            command_id: Some("workdeck.extensions.exitKeyboardMode".into()),
            hint: None,
            checked: None,
        });
        let menus = build_app_menus(options);
        assert_eq!(
            items(&menus, MenuId::Extensions)
                .iter()
                .map(|entry| item_fields(entry).0)
                .collect::<Vec<_>>(),
            ["Exit Vim navigation"]
        );
    }
}
