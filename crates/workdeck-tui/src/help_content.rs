//! Curated controls help derived from the live command table.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpCommand {
    pub id: String,
    pub key_labels: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpRow {
    pub keys: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpSection {
    pub title: String,
    pub rows: Vec<HelpRow>,
}

#[derive(Debug, Clone, Copy)]
enum HelpEntryKeys {
    Literal(&'static str),
    Commands(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
struct HelpEntrySpec {
    keys: HelpEntryKeys,
    description: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct HelpSectionSpec {
    title: &'static str,
    entries: &'static [HelpEntrySpec],
}

const NAVIGATION: &[HelpEntrySpec] = &[
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.stepUp", "workdeck.review.stepDown"]),
        description: "move line-by-line",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.pageDown"]),
        description: "page down",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.pageUp"]),
        description: "page up",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.review.halfPageDown",
            "workdeck.review.halfPageUp",
        ]),
        description: "half page down / up",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.review.previousHunk",
            "workdeck.review.nextHunk",
        ]),
        description: "previous / next hunk",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.review.previousFile",
            "workdeck.review.nextFile",
        ]),
        description: "previous / next file",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.review.previousAnnotatedHunk",
            "workdeck.review.nextAnnotatedHunk",
        ]),
        description: "previous / next comment",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.review.scrollCodeLeft",
            "workdeck.review.scrollCodeRight",
        ]),
        description: "scroll code sideways (Shift = faster)",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.jumpToTop"]),
        description: "jump to start",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.jumpToBottom"]),
        description: "jump to end",
    },
];

const MOUSE: &[HelpEntrySpec] = &[
    HelpEntrySpec {
        keys: HelpEntryKeys::Literal("Wheel"),
        description: "scroll vertically",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Literal("Shift+Wheel"),
        description: "scroll code horizontally",
    },
];

const VIEW: &[HelpEntrySpec] = &[
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.view.layoutSplit",
            "workdeck.view.layoutStack",
            "workdeck.view.layoutAuto",
        ]),
        description: "split / stack / auto",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.view.toggleFilesPane",
            "workdeck.view.openThemeSelector",
        ]),
        description: "sidebar / theme selector",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.view.toggleAgentNotes"]),
        description: "toggle AI notes",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.toggleHunkGap"]),
        description: "toggle unchanged context",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.view.toggleLineNumbers",
            "workdeck.view.toggleLineWrap",
            "workdeck.view.toggleHunkHeaders",
            "workdeck.view.toggleMenuBar",
        ]),
        description: "lines / wrap / metadata / menu",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.editSelectedFile"]),
        description: "open file in $EDITOR",
    },
];

const REVIEW: &[HelpEntrySpec] = &[
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.focusFilter"]),
        description: "focus file filter",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.review.startNote"]),
        description: "create review note",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&[
            "workdeck.review.editActiveNote",
            "workdeck.review.replyToActiveNote",
        ]),
        description: "edit / reply to active note",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.app.toggleFocusArea"]),
        description: "toggle files/filter focus",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Literal("F10"),
        description: "open menus",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.app.refresh"]),
        description: "reload the review",
    },
    HelpEntrySpec {
        keys: HelpEntryKeys::Commands(&["workdeck.app.quit"]),
        description: "quit",
    },
];

const HELP_SECTIONS: &[HelpSectionSpec] = &[
    HelpSectionSpec {
        title: "Navigation",
        entries: NAVIGATION,
    },
    HelpSectionSpec {
        title: "Mouse",
        entries: MOUSE,
    },
    HelpSectionSpec {
        title: "View",
        entries: VIEW,
    },
    HelpSectionSpec {
        title: "Review",
        entries: REVIEW,
    },
];

pub const HELP_COMMAND_IDS: &[&str] = &[
    "workdeck.review.stepUp",
    "workdeck.review.stepDown",
    "workdeck.review.pageDown",
    "workdeck.review.pageUp",
    "workdeck.review.halfPageDown",
    "workdeck.review.halfPageUp",
    "workdeck.review.previousHunk",
    "workdeck.review.nextHunk",
    "workdeck.review.previousFile",
    "workdeck.review.nextFile",
    "workdeck.review.previousAnnotatedHunk",
    "workdeck.review.nextAnnotatedHunk",
    "workdeck.review.scrollCodeLeft",
    "workdeck.review.scrollCodeRight",
    "workdeck.review.jumpToTop",
    "workdeck.review.jumpToBottom",
    "workdeck.view.layoutSplit",
    "workdeck.view.layoutStack",
    "workdeck.view.layoutAuto",
    "workdeck.view.toggleFilesPane",
    "workdeck.view.openThemeSelector",
    "workdeck.view.toggleAgentNotes",
    "workdeck.review.toggleHunkGap",
    "workdeck.view.toggleLineNumbers",
    "workdeck.view.toggleLineWrap",
    "workdeck.view.toggleHunkHeaders",
    "workdeck.view.toggleMenuBar",
    "workdeck.review.editSelectedFile",
    "workdeck.review.focusFilter",
    "workdeck.review.startNote",
    "workdeck.review.editActiveNote",
    "workdeck.review.replyToActiveNote",
    "workdeck.app.toggleFocusArea",
    "workdeck.app.refresh",
    "workdeck.app.quit",
];

fn help_entry_keys(commands: &[HelpCommand], spec: HelpEntrySpec) -> Option<String> {
    match spec.keys {
        HelpEntryKeys::Literal(keys) => Some(keys.into()),
        HelpEntryKeys::Commands(command_ids) => {
            let labels = command_ids
                .iter()
                .flat_map(|id| {
                    let Some(command) = commands
                        .iter()
                        .find(|command| command.id == *id && command.enabled)
                    else {
                        return Vec::new();
                    };
                    if command_ids.len() == 1 {
                        command.key_labels.clone()
                    } else {
                        command.key_labels.first().cloned().into_iter().collect()
                    }
                })
                .collect::<Vec<_>>();
            (!labels.is_empty()).then(|| labels.join(" / "))
        }
    }
}

#[must_use]
pub fn build_help_sections(commands: &[HelpCommand]) -> Vec<HelpSection> {
    HELP_SECTIONS
        .iter()
        .filter_map(|section| {
            let rows = section
                .entries
                .iter()
                .filter_map(|spec| {
                    help_entry_keys(commands, *spec).map(|keys| HelpRow {
                        keys,
                        description: spec.description.into(),
                    })
                })
                .collect::<Vec<_>>();
            (!rows.is_empty()).then(|| HelpSection {
                title: section.title.into(),
                rows,
            })
        })
        .collect()
}

/// Default command probes used until the full resolved command table is supplied by the shell.
#[must_use]
pub fn default_help_commands() -> Vec<HelpCommand> {
    let entries: &[(&str, &[&str], bool)] = &[
        ("workdeck.review.stepUp", &["Up", "k"], true),
        ("workdeck.review.stepDown", &["Down", "j"], true),
        (
            "workdeck.review.pageDown",
            &["PageDown", "Space", "f"],
            true,
        ),
        (
            "workdeck.review.pageUp",
            &["PageUp", "b", "Shift+Space"],
            true,
        ),
        ("workdeck.review.halfPageDown", &["d", "Ctrl+D"], true),
        ("workdeck.review.halfPageUp", &["u", "Ctrl+U"], true),
        ("workdeck.review.previousHunk", &["["], true),
        ("workdeck.review.nextHunk", &["]"], true),
        ("workdeck.review.previousFile", &[","], true),
        ("workdeck.review.nextFile", &["."], true),
        ("workdeck.review.previousAnnotatedHunk", &["{"], true),
        ("workdeck.review.nextAnnotatedHunk", &["}"], true),
        (
            "workdeck.review.scrollCodeLeft",
            &["Left", "Shift+Left"],
            true,
        ),
        (
            "workdeck.review.scrollCodeRight",
            &["Right", "Shift+Right"],
            true,
        ),
        ("workdeck.review.jumpToTop", &["g", "Home"], true),
        ("workdeck.review.jumpToBottom", &["G", "End"], true),
        ("workdeck.view.layoutSplit", &["1"], true),
        ("workdeck.view.layoutStack", &["2"], true),
        ("workdeck.view.layoutAuto", &["0"], true),
        ("workdeck.view.toggleFilesPane", &["s"], true),
        ("workdeck.view.openThemeSelector", &["t"], true),
        ("workdeck.view.toggleAgentNotes", &["a"], true),
        ("workdeck.review.toggleHunkGap", &["z"], true),
        ("workdeck.view.toggleLineNumbers", &["l"], true),
        ("workdeck.view.toggleLineWrap", &["w"], true),
        ("workdeck.view.toggleHunkHeaders", &["m"], true),
        ("workdeck.view.toggleMenuBar", &["M"], true),
        ("workdeck.review.editSelectedFile", &["e"], true),
        ("workdeck.review.focusFilter", &["/"], true),
        ("workdeck.review.startNote", &["c"], true),
        ("workdeck.review.editActiveNote", &["E"], false),
        ("workdeck.review.replyToActiveNote", &["R"], false),
        ("workdeck.app.toggleFocusArea", &["Tab"], true),
        ("workdeck.app.refresh", &["r"], true),
        ("workdeck.app.quit", &["q"], true),
    ];
    entries
        .iter()
        .map(|(id, labels, enabled)| HelpCommand {
            id: (*id).into(),
            key_labels: labels.iter().map(|label| (*label).into()).collect(),
            enabled: *enabled,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn keys_for<'a>(sections: &'a [HelpSection], description: &str) -> Option<&'a str> {
        sections
            .iter()
            .flat_map(|section| &section.rows)
            .find(|row| row.description == description)
            .map(|row| row.keys.as_str())
    }

    fn oracle() -> Value {
        serde_json::from_str(include_str!("../../../port/hunk/oracles/help-content.json")).unwrap()
    }

    #[test]
    fn paired_rows_use_each_commands_primary_key() {
        let sections = build_help_sections(&default_help_commands());
        assert_eq!(
            sections
                .iter()
                .map(|section| section.title.as_str())
                .collect::<Vec<_>>(),
            ["Navigation", "Mouse", "View", "Review"]
        );
        let expected = &oracle()["baseline"]["defaultKeys"];
        for description in [
            "previous / next hunk",
            "half page down / up",
            "move line-by-line",
            "split / stack / auto",
            "lines / wrap / metadata / menu",
        ] {
            assert_eq!(
                keys_for(&sections, description),
                expected[description].as_str()
            );
        }
    }

    #[test]
    fn single_command_rows_list_every_chord() {
        let sections = build_help_sections(&default_help_commands());
        assert_eq!(
            keys_for(&sections, "page down"),
            Some("PageDown / Space / f")
        );
        assert_eq!(
            keys_for(&sections, "page up"),
            Some("PageUp / b / Shift+Space")
        );
        assert_eq!(keys_for(&sections, "jump to start"), Some("g / Home"));
    }

    #[test]
    fn non_command_rows_keep_their_literal_keys() {
        let sections = build_help_sections(&default_help_commands());
        assert_eq!(keys_for(&sections, "scroll vertically"), Some("Wheel"));
        assert_eq!(keys_for(&sections, "open menus"), Some("F10"));
    }

    #[test]
    fn remapped_commands_change_the_advertised_keys() {
        let mut commands = default_help_commands();
        commands
            .iter_mut()
            .find(|command| command.id == "workdeck.review.nextHunk")
            .unwrap()
            .key_labels = vec!["Ctrl+N".into()];
        commands
            .iter_mut()
            .find(|command| command.id == "workdeck.app.quit")
            .unwrap()
            .key_labels = vec!["Ctrl+X".into()];
        let sections = build_help_sections(&commands);
        assert_eq!(
            keys_for(&sections, "previous / next hunk"),
            Some("[ / Ctrl+N")
        );
        assert_eq!(keys_for(&sections, "quit"), Some("Ctrl+X"));
    }

    #[test]
    fn unbound_commands_drop_out_of_rows_and_empty_rows_disappear() {
        let mut commands = default_help_commands();
        for id in ["workdeck.review.previousHunk", "workdeck.review.startNote"] {
            commands
                .iter_mut()
                .find(|command| command.id == id)
                .unwrap()
                .key_labels
                .clear();
        }
        let sections = build_help_sections(&commands);
        assert_eq!(keys_for(&sections, "previous / next hunk"), Some("]"));
        assert_eq!(keys_for(&sections, "create review note"), None);
    }

    #[test]
    fn disabled_commands_are_documented_only_while_runnable() {
        let mut commands = default_help_commands();
        assert_eq!(
            keys_for(&build_help_sections(&commands), "reload the review"),
            Some("r")
        );
        commands
            .iter_mut()
            .find(|command| command.id == "workdeck.app.refresh")
            .unwrap()
            .enabled = false;
        assert_eq!(
            keys_for(&build_help_sections(&commands), "reload the review"),
            None
        );
    }

    #[test]
    fn every_documented_command_has_a_default_probe() {
        let commands = default_help_commands();
        assert!(
            HELP_COMMAND_IDS
                .iter()
                .all(|id| commands.iter().any(|command| command.id == *id))
        );
    }
}
