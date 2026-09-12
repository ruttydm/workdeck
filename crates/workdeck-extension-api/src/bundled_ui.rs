//! Process-cached registrations for Workdeck's host-rendered bundled UI.

use crate::{
    CommandRegistration, ExtensionPaneSize, PanePlacement, PaneRegistration, Registration,
    WORKDECK_VENDOR_EXTENSION_ID,
};
use std::sync::OnceLock;

pub const BUNDLED_SIDEBAR_EXTENSION_ID: &str = WORKDECK_VENDOR_EXTENSION_ID;
pub const BUNDLED_SIDEBAR_VIEW_ID: &str = "files";
pub const BUNDLED_SIDEBAR_SOURCE: &str = "workdeck:bundled/ui/files";

/// Hunk-local id of the bundled search's line highlighter (`search.matches`).
pub const BUNDLED_SEARCH_HIGHLIGHTER_ID: &str = "search.matches";
/// Hunk-local id of the bundled search's status-row item (`search.status`).
pub const BUNDLED_SEARCH_STATUS_ITEM_ID: &str = "search.status";
/// Fully qualified command ids, exactly as user `[keybindings]` configuration sees them.
pub const BUNDLED_SEARCH_FIND_COMMAND_ID: &str = "workdeck.search.find";
pub const BUNDLED_SEARCH_NEXT_COMMAND_ID: &str = "workdeck.search.next";
pub const BUNDLED_SEARCH_PREVIOUS_COMMAND_ID: &str = "workdeck.search.previous";

/// Load host-rendered bundled registrations once, just as native subprocess registrations are
/// loaded once after their handshake. Ratatui owns the component implementation; the bundled
/// content search owns its own session, prompt, and marks.
#[must_use]
pub fn bundled_ui_registry() -> &'static [Registration] {
    static REGISTRY: OnceLock<Vec<Registration>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        vec![
            Registration::Pane(PaneRegistration {
                id: BUNDLED_SIDEBAR_VIEW_ID.into(),
                title: "Files".into(),
                placement: PanePlacement::Left,
                default_open: true,
                preferred_size: None,
                width: Some(ExtensionPaneSize {
                    preferred: 34,
                    min: Some(22),
                    max: Some(56),
                    fraction: Some(0.16),
                }),
                height: None,
                replaces: None,
                current_line: false,
                available: false,
            }),
            Registration::Command(CommandRegistration {
                id: "search.find".into(),
                title: "Search diff content".into(),
                description: None,
                default_keys: vec!["/".into()],
            }),
            Registration::Command(CommandRegistration {
                id: "search.next".into(),
                title: "Next search match".into(),
                description: None,
                default_keys: vec!["n".into()],
            }),
            Registration::Command(CommandRegistration {
                id: "search.previous".into(),
                title: "Previous search match".into(),
                description: None,
                default_keys: vec!["N".into()],
            }),
            Registration::LineHighlighter {
                id: BUNDLED_SEARCH_HIGHLIGHTER_ID.into(),
            },
        ]
    })
}

#[must_use]
pub fn bundled_files_pane() -> &'static PaneRegistration {
    let Some(Registration::Pane(pane)) = bundled_ui_registry().first() else {
        unreachable!("the validated bundled UI registry always contains its files pane")
    };
    pane
}

/// The bundled search's command registrations, in registration order.
#[must_use]
pub fn bundled_search_commands() -> &'static [CommandRegistration] {
    static COMMANDS: OnceLock<Vec<CommandRegistration>> = OnceLock::new();
    COMMANDS.get_or_init(|| {
        bundled_ui_registry()
            .iter()
            .filter_map(|registration| match registration {
                Registration::Command(command) => Some(command.clone()),
                _ => None,
            })
            .collect()
    })
}

/// Fully qualified id of one bundled command, namespaced under the vendor id.
#[must_use]
pub fn bundled_search_command_id(command: &CommandRegistration) -> String {
    format!("{WORKDECK_VENDOR_EXTENSION_ID}.{}", command.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_only_the_built_in_files_pane() {
        let registry = bundled_ui_registry();
        assert_eq!(
            registry
                .iter()
                .filter(|registration| matches!(registration, Registration::Pane(_)))
                .count(),
            1
        );
        assert_eq!(
            format!(
                "{}:{}",
                BUNDLED_SIDEBAR_EXTENSION_ID,
                bundled_files_pane().id
            ),
            crate::WORKDECK_FILES_PANE_KEY
        );
    }

    #[test]
    fn registration_uses_the_exact_responsive_size_contract() {
        let pane = bundled_files_pane();
        assert_eq!(pane.id, BUNDLED_SIDEBAR_VIEW_ID);
        assert_eq!(pane.title, "Files");
        assert_eq!(pane.placement, PanePlacement::Left);
        assert!(pane.default_open);
        assert_eq!(
            pane.width,
            Some(ExtensionPaneSize {
                preferred: 34,
                min: Some(22),
                max: Some(56),
                fraction: Some(0.16),
            })
        );
    }

    #[test]
    fn reserved_vendor_identity_cannot_be_minted_by_a_disk_extension() {
        assert_eq!(BUNDLED_SIDEBAR_EXTENSION_ID, "workdeck");
        assert_eq!(crate::WORKDECK_FILES_PANE_KEY, "workdeck:files");
    }

    #[test]
    fn loads_once_and_returns_the_same_registration() {
        assert!(std::ptr::eq(bundled_files_pane(), bundled_files_pane()));
    }

    #[test]
    fn optional_hunk_pane_lifecycle_fields_are_wire_compatible() {
        let mut pane = bundled_files_pane().clone();
        pane.replaces = Some("vendor:files".into());
        pane.current_line = true;
        pane.available = true;
        let value = serde_json::to_value(&pane).unwrap();
        assert_eq!(value["replaces"], "vendor:files");
        assert_eq!(value["currentLine"], true);
        assert_eq!(value["available"], true);

        let legacy = serde_json::json!({
            "id": "files",
            "title": "Files",
            "placement": "left",
            "default_open": true
        });
        let decoded: PaneRegistration = serde_json::from_value(legacy).unwrap();
        assert_eq!(decoded.replaces, None);
        assert!(!decoded.current_line);
        assert!(!decoded.available);
    }

    #[test]
    fn bundles_content_search_commands_and_match_highlighter_under_the_vendor_id() {
        let commands = bundled_search_commands();
        let projected = commands
            .iter()
            .map(|command| {
                (
                    bundled_search_command_id(command),
                    command.title.as_str(),
                    command.default_keys.first().cloned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            projected,
            [
                (
                    BUNDLED_SEARCH_FIND_COMMAND_ID.to_owned(),
                    "Search diff content",
                    Some("/".to_owned())
                ),
                (
                    BUNDLED_SEARCH_NEXT_COMMAND_ID.to_owned(),
                    "Next search match",
                    Some("n".to_owned())
                ),
                (
                    BUNDLED_SEARCH_PREVIOUS_COMMAND_ID.to_owned(),
                    "Previous search match",
                    Some("N".to_owned())
                ),
            ]
        );
        assert_eq!(BUNDLED_SEARCH_FIND_COMMAND_ID, "workdeck.search.find");
        assert!(bundled_ui_registry().iter().any(|registration| {
            matches!(
                registration,
                Registration::LineHighlighter { id }
                if id == BUNDLED_SEARCH_HIGHLIGHTER_ID
            )
        }));
    }
}
