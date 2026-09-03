//! Atomic native adaptation of Hunk's in-process extension factory boundary.

use workdeck_diff::validate_language_glob;
use workdeck_extension_api::{
    ExtensionManifest, FileLanguageMatcher, HandshakeResponse, LIFECYCLE_EVENT_NAMES, Registration,
    extension_pane_size, is_reserved_extension_cli_command_name,
    is_valid_extension_cli_command_name, is_vertical_pane_placement, parse_key_chord,
};

/// Validated provider detection returned across the native process boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVcsDetection {
    pub id: String,
    pub repo_root: std::path::PathBuf,
}

/// Result of normalizing one extension VCS detection response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVcsDetectionOutcome {
    pub detection: Option<NativeVcsDetection>,
    /// The foreign id on the first mismatch only.
    pub mismatched_id: Option<String>,
}

/// Stateful adapter-local boundary that repairs foreign detection IDs and reports one mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVcsDetectionNormalizer {
    adapter_id: String,
    mismatch_reported: bool,
}

impl NativeVcsDetectionNormalizer {
    #[must_use]
    pub fn new(adapter_id: impl Into<String>) -> Self {
        Self {
            adapter_id: adapter_id.into(),
            mismatch_reported: false,
        }
    }

    /// Convert the untrusted JSON result of a native `detect` request.
    ///
    /// Non-objects and detections without a non-empty `repoRoot` are ordinary misses. The
    /// registered adapter ID is authoritative because all catalog lookups are keyed by it.
    #[must_use]
    pub fn normalize(&mut self, value: &serde_json::Value) -> NativeVcsDetectionOutcome {
        let Some(object) = value.as_object() else {
            return NativeVcsDetectionOutcome {
                detection: None,
                mismatched_id: None,
            };
        };
        let Some(repo_root) = object
            .get("repoRoot")
            .and_then(serde_json::Value::as_str)
            .filter(|repo_root| !repo_root.is_empty())
        else {
            return NativeVcsDetectionOutcome {
                detection: None,
                mismatched_id: None,
            };
        };

        let returned_id = object.get("id").map_or_else(
            || "undefined".into(),
            |value| {
                value.as_str().map_or_else(
                    || match value {
                        serde_json::Value::Null => "null".into(),
                        serde_json::Value::Bool(value) => value.to_string(),
                        serde_json::Value::Number(value) => value.to_string(),
                        serde_json::Value::Array(_) => value.to_string(),
                        serde_json::Value::Object(_) => "[object Object]".into(),
                        serde_json::Value::String(_) => unreachable!(),
                    },
                    str::to_owned,
                )
            },
        );
        let mismatched_id = if returned_id == self.adapter_id || self.mismatch_reported {
            None
        } else {
            self.mismatch_reported = true;
            Some(returned_id)
        };
        NativeVcsDetectionOutcome {
            detection: Some(NativeVcsDetection {
                id: self.adapter_id.clone(),
                repo_root: repo_root.into(),
            }),
            mismatched_id,
        }
    }
}

/// Normalize and validate every declaration produced by one native extension handshake.
///
/// Hunk let a factory append declarations while it was running, then rolled every append back if
/// any call or the factory itself failed. A native process instead returns one handshake value, so
/// validation happens against that owned value and the caller publishes it only on success. This
/// deliberately does not reject duplicate registrations: Hunk retained them and its downstream
/// resolvers applied declaration-order, first-wins collision policy.
pub fn normalize_and_validate_registrations(
    manifest: &ExtensionManifest,
    handshake: &mut HandshakeResponse,
) -> Result<(), String> {
    for registration in &mut handshake.registrations {
        let required = registration.required_capability();
        if !manifest.capabilities.contains(&required) {
            return Err(format!(
                "registration {} requires undeclared capability {required:?}",
                registration.key()
            ));
        }

        match registration {
            Registration::SessionOptions(_) => {}
            Registration::CliCommand(command) => {
                if !is_valid_extension_cli_command_name(&command.name) {
                    return Err(format!(
                        "CLI command {:?} must use lowercase kebab case and start with a letter",
                        command.name
                    ));
                }
                if is_reserved_extension_cli_command_name(&command.name) {
                    return Err(format!(
                        "CLI command {:?} cannot replace a built-in command",
                        command.name
                    ));
                }
                if command.summary.trim().is_empty() {
                    return Err(format!(
                        "CLI command {:?} requires a non-empty summary",
                        command.name
                    ));
                }
                if command
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.trim().is_empty())
                {
                    return Err(format!(
                        "CLI command {:?} usage must be non-empty when provided",
                        command.name
                    ));
                }
            }
            Registration::Command(command) => {
                if command.id.trim().is_empty() || command.title.trim().is_empty() {
                    return Err("commands require non-empty ids and titles".into());
                }
                for chord in &command.default_keys {
                    parse_key_chord(chord).map_err(|error| {
                        format!("command {:?} has invalid key chord: {error}", command.id)
                    })?;
                }
            }
            Registration::Pane(pane) => {
                if pane.id.trim().is_empty() || pane.id.contains(':') {
                    return Err("panes require non-empty local ids".into());
                }
                let vertical = is_vertical_pane_placement(pane.placement);
                if vertical && pane.height.is_some() || !vertical && pane.width.is_some() {
                    return Err(format!(
                        "pane {:?} uses the dimension opposite its {:?} placement",
                        pane.id, pane.placement
                    ));
                }
                let size = extension_pane_size(pane, None);
                let min = size.min.unwrap_or(1);
                let max = size.max.unwrap_or(u16::MAX);
                if size.preferred == 0
                    || min == 0
                    || max == 0
                    || min > size.preferred
                    || size.preferred > max
                {
                    return Err(format!(
                        "pane {:?} size must satisfy 0 < min <= preferred <= max",
                        pane.id
                    ));
                }
                if size.fraction.is_some_and(|fraction| {
                    !fraction.is_finite() || fraction <= 0.0 || fraction > 1.0
                }) {
                    return Err(format!(
                        "pane {:?} fraction must be greater than 0 and at most 1",
                        pane.id
                    ));
                }
                if let Some(replaces) = &pane.replaces {
                    if replaces.trim().is_empty() {
                        return Err("pane replacement keys must be non-empty".into());
                    }
                    if replaces == &format!("{}:{}", manifest.id, pane.id) {
                        return Err("a pane cannot replace itself".into());
                    }
                }
            }
            Registration::Theme(theme) => {
                if theme.id.trim().is_empty() {
                    return Err("themes require non-empty ids".into());
                }
            }
            Registration::VcsAdapter(adapter) => {
                if adapter.id.trim().is_empty() || adapter.name.trim().is_empty() {
                    return Err("VCS adapters require non-empty ids and names".into());
                }
            }
            Registration::ChangesetTransform { id } => {
                if id.trim().is_empty() {
                    return Err("changeset transforms require non-empty ids".into());
                }
            }
            Registration::FileView { id, title, .. } => {
                if id.trim().is_empty() || id.contains(':') || title.trim().is_empty() {
                    return Err("file views require non-empty local ids and titles".into());
                }
            }
            Registration::FileLanguage(registration) => {
                if registration.language.trim().is_empty() {
                    return Err("file languages require a non-empty language".into());
                }
                normalize_file_language_matcher(&mut registration.matcher)?;
            }
            Registration::KeyboardMode(mode) => {
                if mode.id.trim().is_empty()
                    || mode.id.contains(':')
                    || mode.title.trim().is_empty()
                {
                    return Err("keyboard modes require non-empty local ids and titles".into());
                }
            }
            Registration::LineHighlighter { id } => {
                if id.trim().is_empty() {
                    return Err("line highlighters require non-empty ids".into());
                }
            }
            Registration::EventSubscription { names } => {
                if names.is_empty() {
                    return Err("event subscriptions require at least one event name".into());
                }
                if let Some(name) = names
                    .iter()
                    .find(|name| !LIFECYCLE_EVENT_NAMES.contains(&name.as_str()))
                {
                    return Err(format!(
                        "unknown Workdeck extension lifecycle event: {name}"
                    ));
                }
            }
            Registration::CustomEventSubscription { names } => {
                if names.is_empty() || names.iter().any(|name| name.trim().is_empty()) {
                    return Err("custom event subscriptions require non-empty event names".into());
                }
            }
            Registration::PendingCustomEvent { name, .. } => {
                if name.trim().is_empty() {
                    return Err("pending custom events require non-empty event names".into());
                }
            }
        }
    }
    Ok(())
}

fn normalize_file_language_matcher(matcher: &mut FileLanguageMatcher) -> Result<(), String> {
    match matcher {
        FileLanguageMatcher::Extension { value } => {
            let normalized = value.trim().trim_start_matches('.').to_ascii_lowercase();
            if normalized.is_empty() {
                return Err("file-language extensions must be non-empty".into());
            }
            *value = normalized;
        }
        FileLanguageMatcher::Filename { value } => {
            if value.is_empty() {
                return Err("file-language matcher values must be non-empty".into());
            }
            if value.contains('/') {
                return Err("file-language filename matchers cannot contain `/`".into());
            }
        }
        FileLanguageMatcher::Glob { value, .. } => {
            validate_language_glob(value)
                .map_err(|error| format!("invalid file-language glob matcher: {error}"))?;
        }
    }
    Ok(())
}

/// Whether any loaded extension asked the host not to persist review-view preferences.
#[must_use]
pub fn uses_transient_view_preferences<'a>(
    handshakes: impl IntoIterator<Item = &'a HandshakeResponse>,
) -> bool {
    handshakes.into_iter().any(|handshake| {
        handshake.registrations.iter().any(|registration| {
            matches!(
                registration,
                Registration::SessionOptions(options)
                    if options.view_preferences
                        == Some(workdeck_extension_api::ViewPreferencesPolicy::Transient)
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use workdeck_extension_api::{
        API_VERSION, Capability, CliCommandRegistration, CommandRegistration, ExtensionPaneSize,
        FileLanguageGlobTarget, FileLanguageRegistration, KeyboardModeRegistration, PanePlacement,
        PaneRegistration, SessionOptionsRegistration, ThemeRegistration, ViewPreferencesPolicy,
    };

    fn manifest(capabilities: Vec<Capability>) -> ExtensionManifest {
        ExtensionManifest {
            id: "probe".into(),
            name: "Probe".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: PathBuf::from("probe"),
            capabilities,
            description: None,
        }
    }

    fn handshake(registrations: Vec<Registration>) -> HandshakeResponse {
        HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations,
        }
    }

    fn pane(id: &str, placement: PanePlacement) -> PaneRegistration {
        PaneRegistration {
            id: id.into(),
            title: String::new(),
            placement,
            default_open: false,
            preferred_size: None,
            width: None,
            height: None,
            replaces: None,
            current_line: false,
            available: false,
        }
    }

    #[test]
    fn normalizes_extension_language_matchers_and_preserves_exact_filename_and_glob_values() {
        let mut response = handshake(vec![
            Registration::FileLanguage(FileLanguageRegistration {
                matcher: FileLanguageMatcher::Extension {
                    value: "  ..DeMo ".into(),
                },
                language: "demo".into(),
            }),
            Registration::FileLanguage(FileLanguageRegistration {
                matcher: FileLanguageMatcher::Filename {
                    value: " Tool\\Hunkfile ".into(),
                },
                language: "ruby".into(),
            }),
            Registration::FileLanguage(FileLanguageRegistration {
                matcher: FileLanguageMatcher::Glob {
                    value: "generated/**/*.Demo".into(),
                    target: FileLanguageGlobTarget::Path,
                },
                language: "typescript".into(),
            }),
        ]);
        normalize_and_validate_registrations(
            &manifest(vec![Capability::FileLanguages]),
            &mut response,
        )
        .unwrap();

        let Registration::FileLanguage(extension) = &response.registrations[0] else {
            panic!("expected extension matcher");
        };
        assert_eq!(
            extension.matcher,
            FileLanguageMatcher::Extension {
                value: "demo".into()
            }
        );
        let Registration::FileLanguage(filename) = &response.registrations[1] else {
            panic!("expected filename matcher");
        };
        assert_eq!(
            filename.matcher,
            FileLanguageMatcher::Filename {
                value: " Tool\\Hunkfile ".into()
            }
        );
        let Registration::FileLanguage(glob) = &response.registrations[2] else {
            panic!("expected glob matcher");
        };
        assert_eq!(
            glob.matcher,
            FileLanguageMatcher::Glob {
                value: "generated/**/*.Demo".into(),
                target: FileLanguageGlobTarget::Path,
            }
        );
    }

    #[test]
    fn invalid_language_declarations_reject_the_complete_handshake() {
        let invalid = [
            FileLanguageRegistration {
                matcher: FileLanguageMatcher::Extension {
                    value: "...".into(),
                },
                language: "rust".into(),
            },
            FileLanguageRegistration {
                matcher: FileLanguageMatcher::Filename {
                    value: "path/file".into(),
                },
                language: "rust".into(),
            },
            FileLanguageRegistration {
                matcher: FileLanguageMatcher::Glob {
                    value: "[".into(),
                    target: FileLanguageGlobTarget::Basename,
                },
                language: "rust".into(),
            },
            FileLanguageRegistration {
                matcher: FileLanguageMatcher::Extension { value: "rs".into() },
                language: " ".into(),
            },
        ];
        for registration in invalid {
            let mut response = handshake(vec![Registration::FileLanguage(registration)]);
            assert!(
                normalize_and_validate_registrations(
                    &manifest(vec![Capability::FileLanguages]),
                    &mut response,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn pane_declarations_apply_hunks_defaults_and_validate_every_geometry_constraint() {
        let decoded: PaneRegistration = serde_json::from_value(serde_json::json!({
            "id": "side"
        }))
        .unwrap();
        assert_eq!(decoded.placement, PanePlacement::Left);
        assert!(decoded.title.is_empty());
        let mut response = handshake(vec![Registration::Pane(decoded.clone())]);
        normalize_and_validate_registrations(&manifest(vec![Capability::Panes]), &mut response)
            .unwrap();
        assert_eq!(
            extension_pane_size(&decoded, None),
            workdeck_extension_api::DEFAULT_VERTICAL_PANE_WIDTH
        );

        let invalid = [
            pane("", PanePlacement::Left),
            PaneRegistration {
                width: Some(ExtensionPaneSize::fixed(4)),
                ..pane("wrong-axis", PanePlacement::Top)
            },
            PaneRegistration {
                width: Some(ExtensionPaneSize::fixed(0)),
                ..pane("zero", PanePlacement::Left)
            },
            PaneRegistration {
                width: Some(ExtensionPaneSize {
                    preferred: 3,
                    min: Some(4),
                    max: None,
                    fraction: None,
                }),
                ..pane("bounds", PanePlacement::Left)
            },
            PaneRegistration {
                width: Some(ExtensionPaneSize {
                    preferred: 3,
                    min: None,
                    max: None,
                    fraction: Some(f64::NAN),
                }),
                ..pane("fraction", PanePlacement::Left)
            },
            PaneRegistration {
                replaces: Some("probe:self".into()),
                ..pane("self", PanePlacement::Left)
            },
            PaneRegistration {
                replaces: Some(" ".into()),
                ..pane("empty-target", PanePlacement::Left)
            },
        ];
        for pane in invalid {
            let mut response = handshake(vec![Registration::Pane(pane)]);
            assert!(
                normalize_and_validate_registrations(
                    &manifest(vec![Capability::Panes]),
                    &mut response,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn duplicates_survive_for_downstream_first_wins_resolution() {
        let first = Registration::Pane(pane("tree", PanePlacement::Left));
        let duplicate = Registration::Pane(pane("tree", PanePlacement::Right));
        let mut response = handshake(vec![first.clone(), duplicate.clone()]);
        normalize_and_validate_registrations(&manifest(vec![Capability::Panes]), &mut response)
            .unwrap();
        assert_eq!(response.registrations, vec![first, duplicate]);
    }

    #[test]
    fn session_policy_is_owned_by_the_handshake_and_any_transient_request_wins() {
        let ordinary = handshake(vec![Registration::SessionOptions(
            SessionOptionsRegistration {
                view_preferences: Some(ViewPreferencesPolicy::Default),
            },
        )]);
        let transient = handshake(vec![Registration::SessionOptions(
            SessionOptionsRegistration {
                view_preferences: Some(ViewPreferencesPolicy::Transient),
            },
        )]);
        assert!(!uses_transient_view_preferences([&ordinary]));
        assert!(uses_transient_view_preferences([&ordinary, &transient]));
    }

    #[test]
    fn validates_all_method_backed_registration_metadata_before_publication() {
        let capabilities = vec![
            Capability::Commands,
            Capability::CliCommands,
            Capability::Themes,
            Capability::VcsAdapters,
            Capability::ChangesetTransforms,
            Capability::FileViews,
            Capability::KeyboardModes,
            Capability::LineHighlighters,
            Capability::Events,
            Capability::Configuration,
        ];
        let registrations = vec![
            Registration::SessionOptions(SessionOptionsRegistration::default()),
            Registration::Command(CommandRegistration {
                id: "toggle".into(),
                title: "Toggle".into(),
                description: None,
                default_keys: vec!["ctrl+y".into(), "f9".into()],
            }),
            Registration::CliCommand(CliCommandRegistration {
                name: "greptile".into(),
                summary: "Work with Greptile".into(),
                usage: Some("<action>".into()),
            }),
            Registration::Theme(ThemeRegistration {
                id: "night".into(),
                base: None,
                colors: BTreeMap::new(),
            }),
            Registration::VcsAdapter(workdeck_extension_api::ExtensionVcsAdapterRegistration {
                id: "hg".into(),
                name: "Mercurial".into(),
                operations: BTreeMap::new(),
                detection_priority: None,
            }),
            Registration::ChangesetTransform { id: "clean".into() },
            Registration::FileView {
                id: "plain".into(),
                title: "Plain".into(),
                priority: 0,
                interactive_mode: true,
            },
            Registration::KeyboardMode(KeyboardModeRegistration {
                id: "normal".into(),
                title: "Vim normal".into(),
            }),
            Registration::LineHighlighter {
                id: "matches".into(),
            },
            Registration::EventSubscription {
                names: vec!["selection_changed".into()],
            },
            Registration::CustomEventSubscription {
                names: vec!["summary ready 🧭".into()],
            },
            Registration::PendingCustomEvent {
                name: "summary ready 🧭".into(),
                payload: serde_json::json!({ "sequence": 1 }),
            },
        ];
        let mut response = handshake(registrations);
        normalize_and_validate_registrations(&manifest(capabilities), &mut response).unwrap();
    }

    #[test]
    fn rejects_reserved_cli_names_bad_callbacks_metadata_and_undeclared_capabilities() {
        let cases = vec![
            (
                Capability::CliCommands,
                Registration::CliCommand(CliCommandRegistration {
                    name: "diff".into(),
                    summary: "Shadow".into(),
                    usage: None,
                }),
            ),
            (
                Capability::Commands,
                Registration::Command(CommandRegistration {
                    id: "toggle".into(),
                    title: "Toggle".into(),
                    description: None,
                    default_keys: vec!["f13".into()],
                }),
            ),
            (
                Capability::Themes,
                Registration::Theme(ThemeRegistration {
                    id: " ".into(),
                    base: None,
                    colors: BTreeMap::new(),
                }),
            ),
            (
                Capability::ChangesetTransforms,
                Registration::ChangesetTransform { id: "".into() },
            ),
            (
                Capability::LineHighlighters,
                Registration::LineHighlighter { id: " ".into() },
            ),
        ];
        for (capability, registration) in cases {
            let mut response = handshake(vec![registration]);
            assert!(
                normalize_and_validate_registrations(&manifest(vec![capability]), &mut response)
                    .is_err()
            );
        }

        let mut response = handshake(vec![Registration::Command(CommandRegistration {
            id: "toggle".into(),
            title: "Toggle".into(),
            description: None,
            default_keys: Vec::new(),
        })]);
        assert!(
            normalize_and_validate_registrations(&manifest(Vec::new()), &mut response).is_err()
        );
    }

    #[test]
    fn vcs_detection_uses_the_registered_id_and_reports_only_the_first_mismatch() {
        let mut normalizer = NativeVcsDetectionNormalizer::new("hg");
        let first = normalizer.normalize(&serde_json::json!({
            "id": "mercurial",
            "repoRoot": "/repo"
        }));
        assert_eq!(
            first,
            NativeVcsDetectionOutcome {
                detection: Some(NativeVcsDetection {
                    id: "hg".into(),
                    repo_root: "/repo".into(),
                }),
                mismatched_id: Some("mercurial".into()),
            }
        );
        assert_eq!(
            normalizer
                .normalize(&serde_json::json!({"id": "mercurial", "repoRoot": "/other"}))
                .mismatched_id,
            None
        );
        assert_eq!(
            normalizer
                .normalize(&serde_json::json!({"id": "hg", "repoRoot": "/repo"}))
                .mismatched_id,
            None
        );
    }

    #[test]
    fn vcs_detection_treats_nonobjects_and_unusable_repo_roots_as_no_detection() {
        for value in [
            serde_json::Value::Null,
            serde_json::json!("yes"),
            serde_json::json!({"id": "hg"}),
            serde_json::json!({"id": "hg", "repoRoot": null}),
            serde_json::json!({"id": "hg", "repoRoot": 7}),
            serde_json::json!({"id": "hg", "repoRoot": ""}),
        ] {
            assert_eq!(
                NativeVcsDetectionNormalizer::new("hg")
                    .normalize(&value)
                    .detection,
                None
            );
        }
    }

    #[test]
    fn frozen_run_extension_oracle_maps_every_source_test_at_both_pins() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-run-factory.json"
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
        assert_eq!(
            baselines[0]["source_blob"],
            "063c51881c5e05bddf8563b3feea1725106c0efb"
        );
        assert_eq!(
            baselines[0]["test_blob"],
            "5c31e85c3c355e9b5ff14d072e46c7420e4be5c3"
        );
        assert_eq!(baselines[0]["tests"], 38);
        assert_eq!(baselines[0]["passed"], 38);
        assert_eq!(baselines[0]["failed"], 0);
        assert_eq!(baselines[0]["expect_calls"], 150);
        assert_eq!(
            baselines[1]["source_blob"],
            "3fdb00b5890180702e571b9f37ec2ffc5be8382e"
        );
        assert_eq!(
            baselines[1]["test_blob"],
            "f9e4c97a940c5fb0a597c9d5a5afb3ba922452e7"
        );
        assert_eq!(baselines[1]["tests"], 36);
        assert_eq!(baselines[1]["passed"], 36);
        assert_eq!(baselines[1]["failed"], 0);
        assert_eq!(baselines[1]["expect_calls"], 129);

        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 38);
        let names = mappings
            .iter()
            .map(|mapping| mapping["source_test"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), 38);
        assert!(mappings.iter().all(|mapping| {
            mapping["rust_tests"].as_array().is_some_and(|tests| {
                !tests.is_empty() && tests.iter().all(serde_json::Value::is_string)
            })
        }));
        assert_eq!(oracle["baseline_only_tests"].as_array().unwrap().len(), 2);
    }
}
