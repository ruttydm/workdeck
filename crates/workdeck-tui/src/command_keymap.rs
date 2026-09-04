//! Resolve command chords once for dispatch, help, menus, and extension panes.

use std::collections::{BTreeMap, BTreeSet};

pub use workdeck_core::{UserKeyBinding, UserKeyBindingEntry};
use workdeck_extension_api::{
    ExtensionKeyEvent, ParsedKeyChord, matches_key_chord, parse_key_chord,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandKeyDefaults {
    pub id: String,
    pub aliases: Vec<String>,
    pub default_keys: Vec<String>,
}

impl CommandKeyDefaults {
    #[must_use]
    pub fn new(id: impl Into<String>, default_keys: &[impl AsRef<str>]) -> Self {
        Self {
            id: id.into(),
            aliases: Vec::new(),
            default_keys: default_keys
                .iter()
                .map(|key| key.as_ref().to_owned())
                .collect(),
        }
    }

    #[must_use]
    pub fn with_aliases(mut self, aliases: &[impl AsRef<str>]) -> Self {
        self.aliases = aliases
            .iter()
            .map(|alias| alias.as_ref().to_owned())
            .collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapIssue {
    pub command_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedKeymap {
    pub keys: BTreeMap<String, Vec<String>>,
    pub issues: Vec<KeymapIssue>,
}

fn canonicalize_chord(parsed: &ParsedKeyChord) -> String {
    format!(
        "{}{}{}{}{}",
        if parsed.ctrl { "ctrl+" } else { "" },
        if parsed.meta { "meta+" } else { "" },
        if parsed.option { "option+" } else { "" },
        if parsed.shift { "shift+" } else { "" },
        parsed.base
    )
}

fn canonicalize_chord_string(chord: &str) -> Option<String> {
    parse_key_chord(chord)
        .ok()
        .map(|parsed| canonicalize_chord(&parsed))
}

fn command_owner(id: &str) -> &str {
    id.split_once('.').map_or("", |(owner, _)| owner)
}

fn names_an_absent_extension(id: &str, known_owners: &BTreeSet<&str>) -> bool {
    let owner = command_owner(id);
    !owner.is_empty() && owner != "workdeck" && !known_owners.contains(owner)
}

fn mirror_command_aliases(
    keys: &mut BTreeMap<String, Vec<String>>,
    commands: &[&CommandKeyDefaults],
    canonical_by_name: &BTreeMap<String, String>,
) {
    for command in commands {
        let chords = keys.get(&command.id).cloned().unwrap_or_default();
        for alias in &command.aliases {
            if canonical_by_name.get(alias) == Some(&command.id) {
                keys.insert(alias.clone(), chords.clone());
            }
        }
    }
}

/// Fold ordered user declarations over ordered command defaults.
#[must_use]
pub fn resolve_command_keys(
    defaults: &[CommandKeyDefaults],
    user_bindings: &[UserKeyBindingEntry],
) -> ResolvedKeymap {
    let mut issues = Vec::new();
    let mut keys = BTreeMap::<String, Vec<String>>::new();
    let mut commands = Vec::<&CommandKeyDefaults>::new();
    for command in defaults {
        if keys.contains_key(&command.id) {
            issues.push(KeymapIssue {
                command_id: Some(command.id.clone()),
                message: format!(
                    "Duplicate command id \"{}\" ignored • the first registration keeps the keys",
                    command.id
                ),
            });
            continue;
        }
        keys.insert(command.id.clone(), command.default_keys.clone());
        commands.push(command);
    }

    let mut canonical_by_name = commands
        .iter()
        .map(|command| (command.id.clone(), command.id.clone()))
        .collect::<BTreeMap<_, _>>();
    for command in &commands {
        for alias in &command.aliases {
            if let Some(existing) = canonical_by_name.get(alias) {
                issues.push(KeymapIssue {
                    command_id: Some(alias.clone()),
                    message: format!(
                        "Duplicate command alias \"{alias}\" ignored • already resolves to \"{existing}\""
                    ),
                });
            } else {
                canonical_by_name.insert(alias.clone(), command.id.clone());
            }
        }
    }

    if user_bindings.is_empty() {
        mirror_command_aliases(&mut keys, &commands, &canonical_by_name);
        return ResolvedKeymap { keys, issues };
    }

    let known_owners = commands
        .iter()
        .map(|command| command_owner(&command.id))
        .collect::<BTreeSet<_>>();
    let mut claimed = BTreeMap::<String, String>::new();
    let mut user_chords = BTreeMap::<String, Vec<String>>::new();
    let mut configured_by = BTreeMap::<String, String>::new();

    for entry in user_bindings {
        let Some(canonical_id) = canonical_by_name.get(&entry.command_id).cloned() else {
            let message = if names_an_absent_extension(&entry.command_id, &known_owners) {
                format!(
                    "Keybinding for \"{}\" ignored • no command with that id is registered (the extension may not be loaded)",
                    entry.command_id
                )
            } else {
                format!(
                    "Keybinding for unknown command \"{}\" ignored",
                    entry.command_id
                )
            };
            issues.push(KeymapIssue {
                command_id: Some(entry.command_id.clone()),
                message,
            });
            continue;
        };
        if let Some(previous_name) = configured_by.get(&canonical_id) {
            issues.push(KeymapIssue {
                command_id: Some(entry.command_id.clone()),
                message: format!(
                    "Keybinding for \"{}\" ignored • \"{previous_name}\" already configures \"{canonical_id}\"",
                    entry.command_id
                ),
            });
            continue;
        }
        configured_by.insert(canonical_id.clone(), entry.command_id.clone());

        let mut accepted = Vec::new();
        let requested_chords = match &entry.binding {
            UserKeyBinding::Disabled => Vec::new(),
            UserKeyBinding::Chord(chord) => vec![chord.clone()],
            UserKeyBinding::Chords(chords) => chords.clone(),
        };
        for chord in requested_chords {
            let Some(canonical) = canonicalize_chord_string(&chord) else {
                issues.push(KeymapIssue {
                    command_id: Some(entry.command_id.clone()),
                    message: format!(
                        "Keybinding \"{chord}\" for \"{}\" ignored • not a usable key chord",
                        entry.command_id
                    ),
                });
                continue;
            };
            if let Some(owner) = claimed.get(&canonical) {
                issues.push(KeymapIssue {
                    command_id: Some(entry.command_id.clone()),
                    message: format!(
                        "Keybinding \"{chord}\" for \"{}\" ignored • already bound to \"{owner}\"",
                        entry.command_id
                    ),
                });
                continue;
            }
            claimed.insert(canonical, entry.command_id.clone());
            accepted.push(chord);
        }
        user_chords.insert(canonical_id, accepted);
    }

    for (command_id, chords) in &user_chords {
        keys.insert(command_id.clone(), chords.clone());
    }
    for command in &commands {
        if user_chords.contains_key(&command.id) {
            continue;
        }
        let kept = command
            .default_keys
            .iter()
            .filter(|chord| {
                canonicalize_chord_string(chord)
                    .is_none_or(|canonical| !claimed.contains_key(&canonical))
            })
            .cloned()
            .collect::<Vec<_>>();
        if kept.len() != command.default_keys.len() {
            keys.insert(command.id.clone(), kept);
        }
    }
    mirror_command_aliases(&mut keys, &commands, &canonical_by_name);
    ResolvedKeymap { keys, issues }
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionPaneKeybindings {
    keys_by_command: BTreeMap<String, Vec<String>>,
    parsed_by_command: BTreeMap<String, Vec<ParsedKeyChord>>,
}

impl ExtensionPaneKeybindings {
    #[must_use]
    pub fn matches(&self, key: &ExtensionKeyEvent, command_id: &str) -> bool {
        self.parsed_by_command
            .get(command_id)
            .is_some_and(|chords| chords.iter().any(|chord| matches_key_chord(chord, key)))
    }

    #[must_use]
    pub fn get_keys(&self, command_id: &str) -> &[String] {
        self.keys_by_command
            .get(command_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

#[must_use]
pub fn create_extension_pane_keybindings(
    resolved_keys: &BTreeMap<String, Vec<String>>,
) -> ExtensionPaneKeybindings {
    ExtensionPaneKeybindings {
        keys_by_command: resolved_keys.clone(),
        parsed_by_command: resolved_keys
            .iter()
            .map(|(id, keys)| {
                (
                    id.clone(),
                    keys.iter()
                        .filter_map(|key| parse_key_chord(key).ok())
                        .collect(),
                )
            })
            .collect(),
    }
}

#[must_use]
pub fn format_key_chord(chord: &str) -> String {
    let Ok(parsed) = parse_key_chord(chord) else {
        return chord.to_owned();
    };
    let is_letter = parsed.base.len() == 1 && parsed.base.as_bytes()[0].is_ascii_lowercase();
    let mut modifiers = Vec::new();
    if parsed.ctrl {
        modifiers.push("Ctrl");
    }
    if parsed.meta {
        modifiers.push("Cmd");
    }
    if parsed.option {
        modifiers.push("Alt");
    }
    if parsed.shift && !is_letter {
        modifiers.push("Shift");
    }
    let base = if is_letter && (parsed.shift || !modifiers.is_empty()) {
        parsed.base.to_ascii_uppercase()
    } else {
        match parsed.base.as_str() {
            "escape" => "Esc".into(),
            "pageup" => "PageUp".into(),
            "pagedown" => "PageDown".into(),
            "space" => "Space".into(),
            "backspace" => "Backspace".into(),
            "insert" => "Insert".into(),
            "delete" => "Delete".into(),
            "enter" | "return" => "Enter".into(),
            "tab" => "Tab".into(),
            "up" => "Up".into(),
            "down" => "Down".into(),
            "left" => "Left".into(),
            "right" => "Right".into(),
            "home" => "Home".into(),
            "end" => "End".into(),
            base if base.starts_with('f')
                && base[1..].parse::<u8>().is_ok_and(|number| number <= 99) =>
            {
                base.to_ascii_uppercase()
            }
            base => base.to_owned(),
        }
    };
    modifiers.push(&base);
    modifiers.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Vec<CommandKeyDefaults> {
        vec![
            CommandKeyDefaults::new("workdeck.app.quit", &["q"]),
            CommandKeyDefaults::new("workdeck.review.pageDown", &["pagedown", "space", "f"]),
            CommandKeyDefaults::new("workdeck.review.nextHunk", &["]"]),
            CommandKeyDefaults::new("workdeck.view.toggleFilesPane", &["s"])
                .with_aliases(&["workdeck.view.toggleSidebar"]),
            CommandKeyDefaults::new("meta.toggle", &["y"]),
        ]
    }

    fn binding(command_id: &str, binding: UserKeyBinding) -> UserKeyBindingEntry {
        UserKeyBindingEntry::new(command_id, binding)
    }

    fn one(command_id: &str, chord: &str) -> UserKeyBindingEntry {
        binding(command_id, UserKeyBinding::Chord(chord.into()))
    }

    #[test]
    fn defaults_survive_and_user_entries_replace_or_unbind_outright() {
        let resolved = resolve_command_keys(&defaults(), &[]);
        assert!(resolved.issues.is_empty());
        assert_eq!(
            resolved.keys["workdeck.review.pageDown"],
            ["pagedown", "space", "f"]
        );
        assert_eq!(resolved.keys["workdeck.app.quit"], ["q"]);

        let remapped =
            resolve_command_keys(&defaults(), &[one("workdeck.review.pageDown", "ctrl+d")]);
        assert_eq!(remapped.keys["workdeck.review.pageDown"], ["ctrl+d"]);
        assert_eq!(remapped.keys["workdeck.app.quit"], ["q"]);
        for requested in [UserKeyBinding::Disabled, UserKeyBinding::Chords(Vec::new())] {
            assert!(
                resolve_command_keys(&defaults(), &[binding("workdeck.app.quit", requested)]).keys
                    ["workdeck.app.quit"]
                    .is_empty()
            );
        }
    }

    #[test]
    fn aliases_remap_canonical_keys_and_first_configuration_wins() {
        let resolved =
            resolve_command_keys(&defaults(), &[one("workdeck.view.toggleSidebar", "ctrl+b")]);
        assert!(resolved.issues.is_empty());
        assert_eq!(resolved.keys["workdeck.view.toggleFilesPane"], ["ctrl+b"]);
        assert_eq!(resolved.keys["workdeck.view.toggleSidebar"], ["ctrl+b"]);

        let duplicate = resolve_command_keys(
            &defaults(),
            &[
                one("workdeck.view.toggleSidebar", "ctrl+b"),
                one("workdeck.view.toggleFilesPane", "f6"),
            ],
        );
        assert_eq!(duplicate.keys["workdeck.view.toggleFilesPane"], ["ctrl+b"]);
        assert_eq!(duplicate.issues.len(), 1);
        assert!(duplicate.issues[0].message.contains("already configures"));
    }

    #[test]
    fn user_claims_are_exclusive_and_compared_by_meaning() {
        let claimed = resolve_command_keys(
            &defaults(),
            &[binding(
                "meta.toggle",
                UserKeyBinding::Chords(vec!["f".into(), "ctrl+y".into()]),
            )],
        );
        assert_eq!(claimed.keys["meta.toggle"], ["f", "ctrl+y"]);
        assert_eq!(
            claimed.keys["workdeck.review.pageDown"],
            ["pagedown", "space"]
        );

        let semantic = resolve_command_keys(
            &[
                CommandKeyDefaults::new("workdeck.review.jumpToBottom", &["G", "end"]),
                CommandKeyDefaults::new("meta.toggle", &[] as &[&str]),
            ],
            &[one("meta.toggle", "shift+g")],
        );
        assert_eq!(semantic.keys["workdeck.review.jumpToBottom"], ["end"]);
    }

    #[test]
    fn duplicate_user_claims_and_invalid_chords_report_issues_but_keep_valid_chords() {
        let duplicate = resolve_command_keys(
            &defaults(),
            &[
                one("workdeck.app.quit", "ctrl+x"),
                binding(
                    "meta.toggle",
                    UserKeyBinding::Chords(vec!["ctrl+x".into(), "ctrl+y".into()]),
                ),
            ],
        );
        assert_eq!(duplicate.keys["workdeck.app.quit"], ["ctrl+x"]);
        assert_eq!(duplicate.keys["meta.toggle"], ["ctrl+y"]);
        assert_eq!(duplicate.issues.len(), 1);
        assert!(
            duplicate.issues[0]
                .message
                .contains("already bound to \"workdeck.app.quit\"")
        );

        let invalid = resolve_command_keys(
            &defaults(),
            &[binding(
                "workdeck.app.quit",
                UserKeyBinding::Chords(vec!["ctlr+q".into(), "ctrl+q".into()]),
            )],
        );
        assert_eq!(invalid.keys["workdeck.app.quit"], ["ctrl+q"]);
        assert_eq!(invalid.issues.len(), 1);
        assert!(invalid.issues[0].message.contains("not a usable key chord"));
    }

    #[test]
    fn unknown_vendor_loaded_and_absent_extension_ids_get_exact_severity() {
        let unknown = resolve_command_keys(
            &defaults(),
            &[one("workdeck.app.quti", "x"), one("ghost.command", "z")],
        );
        assert_eq!(unknown.issues.len(), 2);
        assert!(
            unknown.issues[0]
                .message
                .contains("unknown command \"workdeck.app.quti\"")
        );
        assert!(
            unknown.issues[1]
                .message
                .contains("extension may not be loaded")
        );

        let loaded = resolve_command_keys(&defaults(), &[one("meta.togle", "z")]);
        assert!(
            loaded.issues[0]
                .message
                .contains("unknown command \"meta.togle\"")
        );
        assert!(!loaded.issues[0].message.contains("may not be loaded"));

        let vendor = resolve_command_keys(
            &[CommandKeyDefaults::new("meta.toggle", &["y"])],
            &[one("workdeck.review.nextHnuk", "z")],
        );
        assert!(
            vendor.issues[0]
                .message
                .contains("unknown command \"workdeck.review.nextHnuk\"")
        );
        assert!(!vendor.issues[0].message.contains("may not be loaded"));
    }

    #[test]
    fn duplicate_table_ids_and_aliases_keep_first_registration() {
        let duplicate = resolve_command_keys(
            &[
                CommandKeyDefaults::new("meta.toggle", &["y"]),
                CommandKeyDefaults::new("meta.toggle", &["z"]),
            ],
            &[],
        );
        assert_eq!(duplicate.keys["meta.toggle"], ["y"]);
        assert_eq!(duplicate.issues.len(), 1);
        assert!(duplicate.issues[0].message.contains("Duplicate command id"));

        let alias_collision = resolve_command_keys(
            &[
                CommandKeyDefaults::new("owner.first", &["a"]).with_aliases(&["owner.alias"]),
                CommandKeyDefaults::new("owner.second", &["b"]).with_aliases(&["owner.alias"]),
            ],
            &[],
        );
        assert_eq!(alias_collision.keys["owner.alias"], ["a"]);
        assert!(
            alias_collision.issues[0]
                .message
                .contains("Duplicate command alias")
        );
    }

    #[test]
    fn extension_pane_lookup_matches_resolved_commands_and_unknowns_are_unbound() {
        let resolved = resolve_command_keys(
            &defaults(),
            &[
                one("workdeck.review.nextHunk", "ctrl+n"),
                binding("workdeck.app.quit", UserKeyBinding::Disabled),
            ],
        );
        let keybindings = create_extension_pane_keybindings(&resolved.keys);
        let ctrl_n = ExtensionKeyEvent {
            name: "n".into(),
            ctrl: true,
            ..ExtensionKeyEvent::default()
        };
        let bracket = ExtensionKeyEvent {
            sequence: "]".into(),
            ..ExtensionKeyEvent::default()
        };
        assert_eq!(keybindings.get_keys("workdeck.review.nextHunk"), ["ctrl+n"]);
        assert!(keybindings.matches(&ctrl_n, "workdeck.review.nextHunk"));
        assert!(!keybindings.matches(&bracket, "workdeck.review.nextHunk"));
        assert!(keybindings.get_keys("workdeck.app.quit").is_empty());
        assert_eq!(keybindings.get_keys("workdeck.view.toggleSidebar"), ["s"]);
        assert!(keybindings.get_keys("missing.command").is_empty());
        assert!(!keybindings.matches(&ctrl_n, "missing.command"));
    }

    #[test]
    fn chord_labels_match_keyboard_spelling_and_retain_invalid_input() {
        assert_eq!(format_key_chord("q"), "q");
        assert_eq!(format_key_chord("G"), "G");
        assert_eq!(format_key_chord("ctrl+m"), "Ctrl+M");
        assert_eq!(format_key_chord("pageup"), "PageUp");
        assert_eq!(format_key_chord("shift+space"), "Shift+Space");
        assert_eq!(format_key_chord("f10"), "F10");
        assert_eq!(format_key_chord("alt+left"), "Alt+Left");
        assert_eq!(format_key_chord("{"), "{");
        assert_eq!(format_key_chord("ctlr+s"), "ctlr+s");
    }
}
