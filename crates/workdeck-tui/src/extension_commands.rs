//! Conflict-aware dispatch table for native extension commands.

use workdeck_extension_api::{
    CommandRegistration, ExtensionKeyEvent, ParsedKeyChord, matches_key_chord, parse_key_chord,
};

use crate::{
    AppCommand, CommandKeyDefaults, ResolvedKeymap, format_key_chord, matches_any_key_chord,
    synthesize_key_event,
};

/// One extension command after registry ownership has been resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredExtensionCommand {
    pub extension_index: usize,
    pub extension_id: String,
    pub command: CommandRegistration,
}

impl RegisteredExtensionCommand {
    #[must_use]
    pub fn full_id(&self) -> String {
        format!("{}.{}", self.extension_id, self.command.id)
    }
}

/// One extension binding refused because another command already owns its chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCommandConflict {
    pub extension_id: String,
    pub full_id: String,
    pub key: String,
    pub conflicting_id: String,
}

impl ExtensionCommandConflict {
    #[must_use]
    pub fn warning(&self) -> String {
        format!(
            "Extension {} key \"{}\" is taken by {} • command \"{}\" left unbound",
            self.extension_id, self.key, self.conflicting_id, self.full_id
        )
    }
}

/// Menu and key-dispatch projection of one registered native command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionAppCommand {
    pub registration: RegisteredExtensionCommand,
    pub id: String,
    pub title: String,
    pub keys: Vec<String>,
    pub key_labels: Vec<String>,
    pub public_to_extensions: bool,
    pub closes_menu: bool,
}

impl ExtensionAppCommand {
    #[must_use]
    pub fn matches(&self, key: &ExtensionKeyEvent) -> bool {
        matches_any_key_chord(&self.keys).matches(key)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionAppCommands {
    pub commands: Vec<ExtensionAppCommand>,
    pub conflicts: Vec<ExtensionCommandConflict>,
}

#[derive(Debug)]
struct ClaimedChord {
    command_id: String,
    chord: ParsedKeyChord,
}

/// Adapt registered extension commands into the live dispatch table.
///
/// Built-ins always win. Between extensions, registration order breaks ties.
/// Refusal is per chord, so a multi-key command retains every unclaimed chord.
#[must_use]
pub fn build_extension_app_commands(
    registered: &[RegisteredExtensionCommand],
    builtins: &[AppCommand],
    resolved_keys: Option<&ResolvedKeymap>,
) -> ExtensionAppCommands {
    let mut answer = ExtensionAppCommands::default();
    let mut claimed = Vec::<ClaimedChord>::new();

    for registration in registered {
        let full_id = registration.full_id();
        let declared = resolved_keys
            .and_then(|resolved| resolved.keys.get(&full_id))
            .cloned()
            .unwrap_or_else(|| registration.command.default_keys.clone());
        let mut keys = Vec::new();

        for chord in declared {
            // Extension registration validates chords. Skipping is the safe
            // response if an in-memory registry is nevertheless corrupted.
            let Ok(parsed) = parse_key_chord(&chord) else {
                continue;
            };
            let probe = synthesize_key_event(&parsed).key;
            let taken_by = builtins
                .iter()
                .find(|builtin| matches_any_key_chord(&builtin.keys).matches(&probe))
                .map(|builtin| builtin.id.to_owned())
                .or_else(|| {
                    claimed
                        .iter()
                        .find(|entry| matches_key_chord(&entry.chord, &probe))
                        .map(|entry| entry.command_id.clone())
                });
            if let Some(conflicting_id) = taken_by {
                answer.conflicts.push(ExtensionCommandConflict {
                    extension_id: registration.extension_id.clone(),
                    full_id: full_id.clone(),
                    key: chord,
                    conflicting_id,
                });
                continue;
            }

            claimed.push(ClaimedChord {
                command_id: full_id.clone(),
                chord: parsed,
            });
            keys.push(chord);
        }

        answer.commands.push(ExtensionAppCommand {
            registration: registration.clone(),
            id: full_id,
            title: registration.command.title.clone(),
            key_labels: keys.iter().map(|key| format_key_chord(key)).collect(),
            keys,
            public_to_extensions: false,
            closes_menu: true,
        });
    }

    answer
}

/// Every extension command default, namespaced exactly as user configuration sees it.
#[must_use]
pub fn extension_command_key_defaults(
    registered: &[RegisteredExtensionCommand],
) -> Vec<CommandKeyDefaults> {
    registered
        .iter()
        .map(|registration| CommandKeyDefaults {
            id: registration.full_id(),
            aliases: Vec::new(),
            default_keys: registration.command.default_keys.clone(),
        })
        .collect()
}

#[must_use]
pub fn dispatch_extension_app_command<'a>(
    commands: &'a [ExtensionAppCommand],
    key: &ExtensionKeyEvent,
) -> Option<&'a RegisteredExtensionCommand> {
    commands
        .iter()
        .find(|command| command.matches(key))
        .map(|command| &command.registration)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        BuiltinCommandAvailability, UserKeyBinding, UserKeyBindingEntry, build_app_commands,
        builtin_command_key_defaults, builtin_command_match_probes, resolve_command_keys,
    };

    fn registered(
        extension_index: usize,
        extension_id: &str,
        id: &str,
        keys: &[&str],
    ) -> RegisteredExtensionCommand {
        RegisteredExtensionCommand {
            extension_index,
            extension_id: extension_id.into(),
            command: CommandRegistration {
                id: id.into(),
                title: id.into(),
                description: None,
                default_keys: keys.iter().map(|key| (*key).into()).collect(),
            },
        }
    }

    fn chord_event(chord: &str) -> ExtensionKeyEvent {
        let parsed = parse_key_chord(chord).unwrap();
        synthesize_key_event(&parsed).key
    }

    fn builtins(resolved: Option<&ResolvedKeymap>) -> Vec<AppCommand> {
        builtin_command_match_probes(resolved)
    }

    #[test]
    fn adapts_bound_commands_into_dispatchable_review_scope_entries() {
        let registrations = vec![
            registered(0, "meta", "toggle", &["y"]),
            registered(0, "meta", "silent", &[]),
        ];
        let result = build_extension_app_commands(&registrations, &builtins(None), None);

        assert!(result.conflicts.is_empty());
        assert_eq!(
            result
                .commands
                .iter()
                .map(|command| command.id.as_str())
                .collect::<Vec<_>>(),
            ["meta.toggle", "meta.silent"]
        );
        assert_eq!(result.commands[0].key_labels, ["y"]);
        assert!(result.commands[1].key_labels.is_empty());
        assert!(
            result
                .commands
                .iter()
                .all(|command| !command.public_to_extensions)
        );
        assert_eq!(
            dispatch_extension_app_command(&result.commands, &chord_event("y"))
                .map(RegisteredExtensionCommand::full_id),
            Some("meta.toggle".into())
        );
    }

    #[test]
    fn refuses_chords_owned_by_builtin_shortcuts() {
        let registrations = vec![
            registered(0, "meta", "steal-s", &["s"]),
            registered(0, "meta", "ok", &["y"]),
        ];
        let result = build_extension_app_commands(&registrations, &builtins(None), None);

        assert_eq!(result.commands[0].key_labels, Vec::<String>::new());
        assert_eq!(result.commands[1].key_labels, ["y"]);
        assert_eq!(
            result.conflicts,
            [ExtensionCommandConflict {
                extension_id: "meta".into(),
                full_id: "meta.steal-s".into(),
                key: "s".into(),
                conflicting_id: "workdeck.view.toggleFilesPane".into(),
            }]
        );
    }

    #[test]
    fn resolves_chords_between_extensions_by_load_order() {
        let registrations = vec![
            registered(0, "first", "mine", &["y"]),
            registered(1, "second", "mine", &["y"]),
        ];
        let result = build_extension_app_commands(&registrations, &builtins(None), None);

        assert_eq!(result.commands[0].key_labels, ["y"]);
        assert!(result.commands[1].key_labels.is_empty());
        assert_eq!(result.conflicts[0].full_id, "second.mine");
        assert_eq!(result.conflicts[0].conflicting_id, "first.mine");
    }

    #[test]
    fn binds_one_command_to_every_declared_chord() {
        let registrations = vec![registered(0, "meta", "toggle", &["y", "ctrl+o"])];
        let result = build_extension_app_commands(&registrations, &builtins(None), None);

        assert!(result.conflicts.is_empty());
        assert_eq!(result.commands[0].key_labels, ["y", "Ctrl+O"]);
        assert!(dispatch_extension_app_command(&result.commands, &chord_event("y")).is_some());
        assert!(dispatch_extension_app_command(&result.commands, &chord_event("ctrl+o")).is_some());
    }

    #[test]
    fn drops_only_the_conflicting_chord_of_a_multi_key_command() {
        let registrations = vec![registered(0, "meta", "toggle", &["s", "y"])];
        let result = build_extension_app_commands(&registrations, &builtins(None), None);

        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].key, "s");
        assert_eq!(result.commands[0].keys, ["y"]);
        assert!(dispatch_extension_app_command(&result.commands, &chord_event("y")).is_some());
    }

    #[test]
    fn a_user_keybinding_replaces_the_declared_chords() {
        let registrations = vec![registered(0, "meta", "toggle", &["y"])];
        let resolved = ResolvedKeymap {
            keys: BTreeMap::from([("meta.toggle".into(), vec!["ctrl+j".into()])]),
            issues: Vec::new(),
        };
        let result = build_extension_app_commands(&registrations, &builtins(None), Some(&resolved));

        assert!(dispatch_extension_app_command(&result.commands, &chord_event("ctrl+j")).is_some());
        assert!(dispatch_extension_app_command(&result.commands, &chord_event("y")).is_none());
    }

    #[test]
    fn a_chord_a_builtin_released_is_free_for_an_extension_to_claim() {
        let registrations = vec![registered(0, "meta", "steal-s", &["s"])];
        let mut defaults = builtin_command_key_defaults();
        defaults.extend(extension_command_key_defaults(&registrations));
        let resolved = resolve_command_keys(
            &defaults,
            &[UserKeyBindingEntry::new(
                "workdeck.view.toggleFilesPane",
                UserKeyBinding::Chord("ctrl+b".into()),
            )],
        );
        let result = build_extension_app_commands(
            &registrations,
            &build_app_commands(Some(&resolved), BuiltinCommandAvailability::default()),
            Some(&resolved),
        );

        assert!(result.conflicts.is_empty());
        assert!(dispatch_extension_app_command(&result.commands, &chord_event("s")).is_some());
    }

    #[test]
    fn defaults_keep_registration_order_and_namespace() {
        let registrations = vec![registered(3, "meta", "toggle", &["y", "ctrl+o"])];
        assert_eq!(
            extension_command_key_defaults(&registrations),
            [CommandKeyDefaults::new("meta.toggle", &["y", "ctrl+o"])]
        );
    }

    #[test]
    fn frozen_hunk_oracle_records_both_pinned_runs() {
        let oracle = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../port/hunk/oracles/extension-commands.json"
        ));
        assert!(oracle.contains("\"passed\": 7"));
        assert!(oracle.contains("0c0306ade48817c299cda1d93fc1efb05cc139c3"));
        assert!(oracle.contains("2208c532eb83ccc47248d9d92f35f4ad6afebfb4"));
    }
}
