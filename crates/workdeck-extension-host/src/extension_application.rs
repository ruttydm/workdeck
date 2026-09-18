//! Shared, deterministic application of native extension registrations.

use std::collections::BTreeSet;

use workdeck_core::StartupNotice;
use workdeck_diff::{BUILT_IN_FILE_LANGUAGE_EXTENSIONS, sanitize_terminal_line};
use workdeck_extension_api::{
    ExtensionNotificationHub, ExtensionNotifyType, FileLanguageMatcher, Registration,
};
use workdeck_vcs::VcsCatalog;

use crate::LoadedExtension;

/// Stable address of one declaration in extension load and registration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistrationLocation {
    pub extension_index: usize,
    pub registration_index: usize,
}

/// One syntactically valid declaration refused while joining the shared session registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionApplyIssue {
    pub extension_id: String,
    pub message: String,
}

/// First-wins application result shared by startup, reload, and the live TUI registries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionRegistrationResolution {
    accepted: BTreeSet<RegistrationLocation>,
    pub issues: Vec<ExtensionApplyIssue>,
}

impl ExtensionRegistrationResolution {
    #[must_use]
    pub fn accepts(&self, extension_index: usize, registration_index: usize) -> bool {
        self.accepted.contains(&RegistrationLocation {
            extension_index,
            registration_index,
        })
    }

    #[must_use]
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }
}

/// Resolve every loaded declaration using Hunk's category-local collision policies.
#[must_use]
pub fn resolve_loaded_extension_registrations(
    extensions: &[LoadedExtension],
    base_vcs_catalog: &VcsCatalog,
) -> ExtensionRegistrationResolution {
    resolve_extension_registrations(
        extensions.iter().map(|extension| {
            (
                extension.manifest.id.as_str(),
                extension.handshake.registrations.as_slice(),
            )
        }),
        base_vcs_catalog,
    )
}

/// Resolve an ordered registry without requiring live subprocesses.
///
/// File-language selectors deliberately do not claim each other: later selectors of the same
/// category remain accepted and therefore win in the language registry. Every identity-bearing
/// UI declaration and VCS adapter is first-wins.
#[must_use]
pub fn resolve_extension_registrations<'a>(
    extensions: impl IntoIterator<Item = (&'a str, &'a [Registration])>,
    base_vcs_catalog: &VcsCatalog,
) -> ExtensionRegistrationResolution {
    let mut answer = ExtensionRegistrationResolution::default();
    let mut vcs_ids = BTreeSet::new();
    let mut pane_keys = BTreeSet::new();
    let mut pane_replacements = BTreeSet::new();
    let mut file_view_keys = BTreeSet::new();
    let mut highlighter_keys = BTreeSet::new();
    let mut keyboard_mode_keys = BTreeSet::new();
    let mut command_ids = BTreeSet::new();

    for (extension_index, (extension_id, registrations)) in extensions.into_iter().enumerate() {
        for (registration_index, registration) in registrations.iter().enumerate() {
            let issue = match registration {
                Registration::FileLanguage(language)
                    if matches!(
                        &language.matcher,
                        FileLanguageMatcher::Extension { value }
                            if BUILT_IN_FILE_LANGUAGE_EXTENSIONS.contains(&value.as_str())
                    ) =>
                {
                    let FileLanguageMatcher::Extension { value } = &language.matcher else {
                        unreachable!();
                    };
                    Some(format!(
                        "Skipped file language .{value} from extension {extension_id} • Workdeck defines it"
                    ))
                }
                Registration::VcsAdapter(adapter)
                    if base_vcs_catalog.reserved_ids.contains(&adapter.id) =>
                {
                    Some(format!(
                        "Skipped VCS adapter \"{}\" from extension {extension_id} • a built-in backend owns that id",
                        adapter.id
                    ))
                }
                Registration::VcsAdapter(adapter) if !vcs_ids.insert(adapter.id.clone()) => {
                    Some(format!(
                        "Skipped VCS adapter \"{}\" from extension {extension_id} • another extension already registered it",
                        adapter.id
                    ))
                }
                Registration::Pane(pane) => {
                    let key = qualified_view_key(extension_id, &pane.id);
                    if pane_keys.contains(&key) {
                        Some(format!(
                            "Skipped duplicate pane \"{key}\" from extension {extension_id}"
                        ))
                    } else if pane
                        .replaces
                        .as_ref()
                        .is_some_and(|target| pane_replacements.contains(target))
                    {
                        Some(format!(
                            "Skipped pane \"{key}\" from extension {extension_id} • another pane already replaces \"{}\"",
                            pane.replaces.as_deref().unwrap_or_default()
                        ))
                    } else {
                        pane_keys.insert(key);
                        if let Some(target) = &pane.replaces {
                            pane_replacements.insert(target.clone());
                        }
                        None
                    }
                }
                Registration::FileView { id, .. } => duplicate_key_issue(
                    &mut file_view_keys,
                    qualified_view_key(extension_id, id),
                    "file view",
                    extension_id,
                ),
                Registration::LineHighlighter { id } => duplicate_key_issue(
                    &mut highlighter_keys,
                    qualified_view_key(extension_id, id),
                    "line highlighter",
                    extension_id,
                ),
                Registration::KeyboardMode(mode) => duplicate_key_issue(
                    &mut keyboard_mode_keys,
                    qualified_view_key(extension_id, &mode.id),
                    "keyboard mode",
                    extension_id,
                ),
                Registration::Command(command) => duplicate_key_issue(
                    &mut command_ids,
                    format!("{extension_id}.{}", command.id),
                    "command",
                    extension_id,
                ),
                _ => None,
            };

            if let Some(message) = issue {
                answer.issues.push(ExtensionApplyIssue {
                    extension_id: extension_id.to_owned(),
                    message,
                });
            } else {
                answer.accepted.insert(RegistrationLocation {
                    extension_index,
                    registration_index,
                });
            }
        }
    }
    answer
}

fn duplicate_key_issue(
    claimed: &mut BTreeSet<String>,
    key: String,
    kind: &str,
    extension_id: &str,
) -> Option<String> {
    (!claimed.insert(key.clone()))
        .then(|| format!("Skipped duplicate {kind} \"{key}\" from extension {extension_id}"))
}

#[must_use]
pub fn qualified_view_key(extension_id: &str, view_id: &str) -> String {
    format!("{extension_id}:{view_id}")
}

/// Convert application issues to terminal-safe first-launch notices.
#[must_use]
pub fn create_extension_apply_notices(issues: &[ExtensionApplyIssue]) -> Vec<StartupNotice> {
    issues
        .iter()
        .enumerate()
        .map(|(index, issue)| {
            StartupNotice::new(
                format!("extension:apply:{}:{index}", issue.extension_id),
                sanitize_terminal_line(&issue.message),
            )
        })
        .collect()
}

/// Surface the same refused registrations as warnings after an in-session reload.
pub fn report_extension_apply_issues(
    issues: &[ExtensionApplyIssue],
    notifications: &ExtensionNotificationHub,
) {
    for issue in issues {
        notifications.notify(issue.message.clone(), ExtensionNotifyType::Warning);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use workdeck_extension_api::{
        CommandRegistration, ExtensionVcsAdapterRegistration, FileLanguageRegistration,
        KeyboardModeRegistration, PanePlacement, PaneRegistration,
    };
    use workdeck_vcs::create_base_vcs_catalog;

    fn base_catalog() -> VcsCatalog {
        create_base_vcs_catalog(Vec::new(), "git")
    }

    fn pane(id: &str, replaces: Option<&str>) -> Registration {
        Registration::Pane(PaneRegistration {
            id: id.into(),
            title: id.into(),
            placement: PanePlacement::Left,
            default_open: false,
            preferred_size: None,
            width: None,
            height: None,
            replaces: replaces.map(str::to_owned),
            current_line: false,
            available: false,
        })
    }

    #[test]
    fn empty_registry_has_no_registrations_or_application_issues() {
        let catalog = base_catalog();
        let resolved = resolve_extension_registrations(std::iter::empty(), &catalog);
        assert_eq!(resolved.accepted_count(), 0);
        assert!(resolved.issues.is_empty());
    }

    #[test]
    fn resolves_every_identity_category_first_wins_and_reports_the_loser() {
        let first = vec![
            pane("tree", Some("workdeck:files")),
            Registration::FileView {
                id: "raw".into(),
                title: "Raw".into(),
                priority: 0,
                interactive_mode: false,
            },
            Registration::LineHighlighter { id: "lint".into() },
            Registration::KeyboardMode(KeyboardModeRegistration {
                id: "normal".into(),
                title: "Normal".into(),
            }),
            Registration::Command(CommandRegistration {
                id: "toggle".into(),
                title: "Toggle".into(),
                description: None,
                default_keys: Vec::new(),
            }),
            Registration::VcsAdapter(ExtensionVcsAdapterRegistration {
                id: "hg".into(),
                name: "Mercurial".into(),
                operations: BTreeMap::new(),
                detection_priority: None,
            }),
        ];
        let second = vec![
            pane("tree", None),
            pane("other", Some("workdeck:files")),
            Registration::FileView {
                id: "raw".into(),
                title: "Duplicate".into(),
                priority: 0,
                interactive_mode: false,
            },
            Registration::LineHighlighter { id: "lint".into() },
            Registration::KeyboardMode(KeyboardModeRegistration {
                id: "normal".into(),
                title: "Duplicate".into(),
            }),
            Registration::Command(CommandRegistration {
                id: "toggle".into(),
                title: "Duplicate".into(),
                description: None,
                default_keys: Vec::new(),
            }),
            Registration::VcsAdapter(ExtensionVcsAdapterRegistration {
                id: "hg".into(),
                name: "Duplicate".into(),
                operations: BTreeMap::new(),
                detection_priority: None,
            }),
        ];
        let catalog = base_catalog();
        let resolved = resolve_extension_registrations(
            [("meta", first.as_slice()), ("meta", second.as_slice())],
            &catalog,
        );
        assert_eq!(resolved.accepted_count(), first.len());
        assert_eq!(resolved.issues.len(), second.len());
        assert!(resolved.accepts(0, 0));
        assert!(!resolved.accepts(1, 0));
        assert!(
            resolved
                .issues
                .iter()
                .any(|issue| issue.message.contains("already replaces"))
        );
    }

    #[test]
    fn file_language_duplicates_remain_last_wins_but_reserved_extensions_are_refused() {
        let registrations = vec![
            Registration::FileLanguage(FileLanguageRegistration {
                matcher: FileLanguageMatcher::Extension {
                    value: "zig".into(),
                },
                language: "python".into(),
            }),
            Registration::FileLanguage(FileLanguageRegistration {
                matcher: FileLanguageMatcher::Extension {
                    value: "zig".into(),
                },
                language: "zig".into(),
            }),
            Registration::FileLanguage(FileLanguageRegistration {
                matcher: FileLanguageMatcher::Extension {
                    value: "mts".into(),
                },
                language: "javascript".into(),
            }),
        ];
        let catalog = base_catalog();
        let resolved =
            resolve_extension_registrations([("languages", registrations.as_slice())], &catalog);
        assert!(resolved.accepts(0, 0));
        assert!(resolved.accepts(0, 1));
        assert!(!resolved.accepts(0, 2));
        assert_eq!(resolved.issues.len(), 1);
        assert!(resolved.issues[0].message.contains("Workdeck defines it"));
    }

    #[test]
    fn vcs_resolution_refuses_bundled_ids_and_keeps_the_first_extension_claim() {
        let registrations = vec![
            Registration::VcsAdapter(ExtensionVcsAdapterRegistration {
                id: "git".into(),
                name: "Shadow Git".into(),
                operations: BTreeMap::new(),
                detection_priority: None,
            }),
            Registration::VcsAdapter(ExtensionVcsAdapterRegistration {
                id: "hg".into(),
                name: "Mercurial".into(),
                operations: BTreeMap::new(),
                detection_priority: None,
            }),
            Registration::VcsAdapter(ExtensionVcsAdapterRegistration {
                id: "hg".into(),
                name: "Duplicate Mercurial".into(),
                operations: BTreeMap::new(),
                detection_priority: None,
            }),
        ];
        let resolved = resolve_extension_registrations(
            [("vcs", registrations.as_slice())],
            workdeck_vcs::bundled_vcs_catalog(),
        );
        assert!(!resolved.accepts(0, 0));
        assert!(resolved.accepts(0, 1));
        assert!(!resolved.accepts(0, 2));
        assert_eq!(resolved.issues.len(), 2);
        assert!(resolved.issues[0].message.contains("built-in backend owns"));
        assert!(resolved.issues[1].message.contains("already registered"));
    }

    #[test]
    fn startup_and_reload_diagnostics_share_messages_and_sanitize_terminal_controls() {
        let issues = vec![ExtensionApplyIssue {
            extension_id: "bad\u{1b}[31m".into(),
            message: "Skipped\nunsafe\u{1b}[31m pane".into(),
        }];
        let notices = create_extension_apply_notices(&issues);
        assert_eq!(notices[0].key, "extension:apply:bad\u{1b}[31m:0");
        assert_eq!(notices[0].message, "Skippedunsafe pane");

        let hub = ExtensionNotificationHub::new();
        report_extension_apply_issues(&issues, &hub);
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let capture = std::sync::Arc::clone(&received);
        let _subscription = hub.subscribe(move |notice| capture.lock().unwrap().push(notice));
        let messages = received.lock().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].notification_type, ExtensionNotifyType::Warning);
        assert_eq!(messages[0].message, issues[0].message);
    }

    #[test]
    fn frozen_application_oracle_maps_all_tests_at_both_pins() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-application.json"
        ))
        .unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert_eq!(
            baselines
                .iter()
                .map(|baseline| baseline["commit"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
                "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
            ]
        );
        assert_eq!(baselines[0]["tests"], 60);
        assert_eq!(baselines[0]["passed"], 60);
        assert_eq!(baselines[0]["failed"], 0);
        assert_eq!(baselines[0]["expect_calls"], 407);
        assert_eq!(baselines[1]["tests"], 55);
        assert_eq!(baselines[1]["passed"], 55);
        assert_eq!(baselines[1]["failed"], 0);
        assert_eq!(baselines[1]["expect_calls"], 373);

        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 60);
        let source_tests = mappings
            .iter()
            .map(|mapping| mapping["source_test"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(source_tests.len(), 60);
        assert!(mappings.iter().all(|mapping| {
            mapping["rust_tests"].as_array().is_some_and(|tests| {
                !tests.is_empty() && tests.iter().all(serde_json::Value::is_string)
            })
        }));
        assert_eq!(oracle["baseline_only_tests"].as_array().unwrap().len(), 5);
    }
}
