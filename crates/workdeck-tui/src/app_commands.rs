//! Terminal dispatch over the renderer-neutral built-in command catalog.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;
use workdeck_extension_api::{ExtensionKeyEvent, parse_key_chord};
use workdeck_review::{
    APP_COMMAND_CATALOG, AppCommandCatalogEntry, AppCommandReviewEffect, LayoutMode,
    ReviewSelectionScope, VerticalCommandDirection,
};

use crate::{
    CommandKeyDefaults, ResolvedKeymap, SyntheticKeyEvent, format_key_chord, matches_any_key_chord,
    synthesize_key_event,
};

pub const MAX_APP_COMMAND_COUNT: usize = 10_000;
pub(crate) const FAST_CODE_HORIZONTAL_SCROLL_COLUMNS: isize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollUnit {
    Step,
    Viewport,
    Content,
    Half,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommandLineAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCursorLine {
    Row,
    Number,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommandAction {
    ScrollDiff {
        delta: isize,
        unit: ScrollUnit,
    },
    RequestQuit,
    ToggleHelp,
    OpenAgentSkill,
    ToggleFocusArea,
    FocusFilter,
    StartUserNote,
    EditActiveNote,
    ReplyToActiveNote,
    StepDiffLine(isize),
    ScrollCodeHorizontally(isize),
    AlignCurrentLine(AppCommandLineAlignment),
    SelectCursorLine(CommandCursorLine),
    SelectLayoutMode(LayoutMode),
    ApplyFilePresentationToAllMatching,
    ToggleFilesPane,
    RefreshCurrentInput,
    OpenThemeSelector,
    ToggleAgentNotes,
    ToggleLineNumbers,
    ToggleLineWrap,
    ToggleMenuBar,
    ToggleHunkHeaders,
    ToggleCopyDecorations,
    ToggleGapForSelectedHunk,
    EditSelectedFile,
    MoveSelection {
        scope: ReviewSelectionScope,
        delta: isize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinCommandAvailability {
    pub can_align_current_line: bool,
    pub can_apply_file_presentation_to_all_matching: bool,
    pub can_edit_active_note: bool,
    pub can_reply_to_active_note: bool,
    pub can_refresh_current_input: bool,
}

impl Default for BuiltinCommandAvailability {
    fn default() -> Self {
        Self {
            can_align_current_line: false,
            can_apply_file_presentation_to_all_matching: false,
            can_edit_active_note: false,
            can_reply_to_active_note: false,
            can_refresh_current_input: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCommand {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub title: &'static str,
    pub keys: Vec<String>,
    pub key_labels: Vec<String>,
    pub default_keys: &'static [&'static str],
    pub enabled: bool,
    pub public_to_extensions: bool,
    pub vertical_direction: Option<VerticalCommandDirection>,
    pub closes_menu: bool,
    entry: &'static AppCommandCatalogEntry,
}

fn command_is_available(
    entry: &AppCommandCatalogEntry,
    availability: BuiltinCommandAvailability,
) -> bool {
    match entry.id {
        "workdeck.review.alignCurrentLineTop"
        | "workdeck.review.alignCurrentLineCenter"
        | "workdeck.review.alignCurrentLineBottom" => availability.can_align_current_line,
        "workdeck.view.applyFilePresentationToAllMatching" => {
            availability.can_apply_file_presentation_to_all_matching
        }
        "workdeck.review.editActiveNote" => availability.can_edit_active_note,
        "workdeck.review.replyToActiveNote" => availability.can_reply_to_active_note,
        "workdeck.app.refresh" => availability.can_refresh_current_input,
        _ => true,
    }
}

/// Build the built-in dispatch table in catalog order.
#[must_use]
pub fn build_app_commands(
    resolved: Option<&ResolvedKeymap>,
    availability: BuiltinCommandAvailability,
) -> Vec<AppCommand> {
    APP_COMMAND_CATALOG
        .iter()
        .map(|entry| {
            let keys = resolved
                .and_then(|resolved| resolved.keys.get(entry.id))
                .cloned()
                .unwrap_or_else(|| {
                    entry
                        .default_keys
                        .iter()
                        .map(|key| (*key).to_owned())
                        .collect()
                });
            AppCommand {
                id: entry.id,
                aliases: entry.aliases,
                title: entry.title,
                key_labels: keys.iter().map(|key| format_key_chord(key)).collect(),
                keys,
                default_keys: entry.default_keys,
                enabled: command_is_available(entry, availability),
                public_to_extensions: entry.public_to_extensions,
                vertical_direction: entry.vertical_direction,
                closes_menu: entry.closes_menu,
                entry,
            }
        })
        .collect()
}

/// Every built-in default, read from the catalog rather than restated.
#[must_use]
pub fn builtin_command_key_defaults() -> Vec<CommandKeyDefaults> {
    APP_COMMAND_CATALOG
        .iter()
        .map(|entry| CommandKeyDefaults {
            id: entry.id.into(),
            aliases: entry.aliases.iter().map(|alias| (*alias).into()).collect(),
            default_keys: entry.default_keys.iter().map(|key| (*key).into()).collect(),
        })
        .collect()
}

#[must_use]
pub fn builtin_command_match_probes(resolved: Option<&ResolvedKeymap>) -> Vec<AppCommand> {
    build_app_commands(resolved, BuiltinCommandAvailability::default())
}

#[must_use]
pub const fn is_command_enabled(command: &AppCommand) -> bool {
    command.enabled
}

#[must_use]
pub fn find_app_command_by_id<'a>(commands: &'a [AppCommand], id: &str) -> Option<&'a AppCommand> {
    commands
        .iter()
        .find(|command| command.id == id || command.aliases.contains(&id))
}

fn multiply_count(direction: isize, count: usize) -> isize {
    direction.saturating_mul(isize::try_from(count).unwrap_or(isize::MAX))
}

fn command_action(command: &AppCommand, key: &ExtensionKeyEvent, count: usize) -> AppCommandAction {
    match command.id {
        "workdeck.review.jumpToBottom" => AppCommandAction::ScrollDiff {
            delta: 1,
            unit: ScrollUnit::Content,
        },
        "workdeck.review.jumpToTop" => AppCommandAction::ScrollDiff {
            delta: -1,
            unit: ScrollUnit::Content,
        },
        "workdeck.app.quit" => AppCommandAction::RequestQuit,
        "workdeck.app.toggleHelp" => AppCommandAction::ToggleHelp,
        "workdeck.app.openAgentSkill" => AppCommandAction::OpenAgentSkill,
        "workdeck.app.toggleFocusArea" => AppCommandAction::ToggleFocusArea,
        "workdeck.review.focusFilter" => AppCommandAction::FocusFilter,
        "workdeck.review.editActiveNote" => AppCommandAction::EditActiveNote,
        "workdeck.review.replyToActiveNote" => AppCommandAction::ReplyToActiveNote,
        "workdeck.review.pageDown" => AppCommandAction::ScrollDiff {
            delta: multiply_count(1, count),
            unit: ScrollUnit::Viewport,
        },
        "workdeck.review.pageUp" => AppCommandAction::ScrollDiff {
            delta: multiply_count(-1, count),
            unit: ScrollUnit::Viewport,
        },
        "workdeck.review.halfPageDown" => AppCommandAction::ScrollDiff {
            delta: multiply_count(1, count),
            unit: ScrollUnit::Half,
        },
        "workdeck.review.halfPageUp" => AppCommandAction::ScrollDiff {
            delta: multiply_count(-1, count),
            unit: ScrollUnit::Half,
        },
        "workdeck.review.stepDown" => AppCommandAction::StepDiffLine(multiply_count(1, count)),
        "workdeck.review.stepUp" => AppCommandAction::StepDiffLine(multiply_count(-1, count)),
        "workdeck.review.scrollCodeLeft" => {
            AppCommandAction::ScrollCodeHorizontally(multiply_count(
                if key.shift {
                    -FAST_CODE_HORIZONTAL_SCROLL_COLUMNS
                } else {
                    -1
                },
                count,
            ))
        }
        "workdeck.review.scrollCodeRight" => {
            AppCommandAction::ScrollCodeHorizontally(multiply_count(
                if key.shift {
                    FAST_CODE_HORIZONTAL_SCROLL_COLUMNS
                } else {
                    1
                },
                count,
            ))
        }
        "workdeck.review.alignCurrentLineTop" => {
            AppCommandAction::AlignCurrentLine(AppCommandLineAlignment::Top)
        }
        "workdeck.review.alignCurrentLineCenter" => {
            AppCommandAction::AlignCurrentLine(AppCommandLineAlignment::Center)
        }
        "workdeck.review.alignCurrentLineBottom" => {
            AppCommandAction::AlignCurrentLine(AppCommandLineAlignment::Bottom)
        }
        "workdeck.view.cursorLineRow" => AppCommandAction::SelectCursorLine(CommandCursorLine::Row),
        "workdeck.view.cursorLineNumber" => {
            AppCommandAction::SelectCursorLine(CommandCursorLine::Number)
        }
        "workdeck.view.cursorLineOff" => AppCommandAction::SelectCursorLine(CommandCursorLine::Off),
        "workdeck.view.layoutSplit" => AppCommandAction::SelectLayoutMode(LayoutMode::Split),
        "workdeck.view.layoutStack" => AppCommandAction::SelectLayoutMode(LayoutMode::Stack),
        "workdeck.view.layoutAuto" => AppCommandAction::SelectLayoutMode(LayoutMode::Auto),
        "workdeck.view.applyFilePresentationToAllMatching" => {
            AppCommandAction::ApplyFilePresentationToAllMatching
        }
        "workdeck.view.toggleFilesPane" => AppCommandAction::ToggleFilesPane,
        "workdeck.app.refresh" => AppCommandAction::RefreshCurrentInput,
        "workdeck.view.openThemeSelector" => AppCommandAction::OpenThemeSelector,
        "workdeck.view.toggleLineNumbers" => AppCommandAction::ToggleLineNumbers,
        "workdeck.view.toggleLineWrap" => AppCommandAction::ToggleLineWrap,
        "workdeck.view.toggleMenuBar" => AppCommandAction::ToggleMenuBar,
        "workdeck.view.toggleHunkHeaders" => AppCommandAction::ToggleHunkHeaders,
        "workdeck.view.toggleCopyDecorations" => AppCommandAction::ToggleCopyDecorations,
        "workdeck.review.editSelectedFile" => AppCommandAction::EditSelectedFile,
        _ => match command.entry.review {
            Some(AppCommandReviewEffect::StartDraft) => AppCommandAction::StartUserNote,
            Some(AppCommandReviewEffect::ToggleNoteVisibility) => {
                AppCommandAction::ToggleAgentNotes
            }
            Some(AppCommandReviewEffect::ToggleSelectedGap) => {
                AppCommandAction::ToggleGapForSelectedHunk
            }
            Some(AppCommandReviewEffect::MoveSelection { scope, direction }) => {
                AppCommandAction::MoveSelection {
                    scope,
                    delta: multiply_count(direction.delta(), count),
                }
            }
            _ => unreachable!("catalog entry has no terminal action: {}", command.id),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCommandDispatch {
    pub command_id: &'static str,
    pub closes_menu: bool,
    pub action: AppCommandAction,
}

/// Run the first enabled command matching one event.
#[must_use]
pub fn dispatch_app_command(
    commands: &[AppCommand],
    key: &ExtensionKeyEvent,
) -> Option<AppCommandDispatch> {
    commands.iter().find_map(|command| {
        (command.enabled && matches_any_key_chord(&command.keys).matches(key)).then(|| {
            AppCommandDispatch {
                command_id: command.id,
                closes_menu: command.closes_menu,
                action: command_action(command, key, 1),
            }
        })
    })
}

/// OpenTUI's preventDefault is represented explicitly for oracle tests.
#[must_use]
pub fn dispatch_synthetic_app_command(
    commands: &[AppCommand],
    key: &mut SyntheticKeyEvent,
) -> Option<AppCommandDispatch> {
    let dispatch = dispatch_app_command(commands, key.as_extension_event());
    if dispatch.is_some() {
        key.prevent_default();
    }
    dispatch
}

/// Apply a dispatch and notify only after the command effect succeeds.
pub fn observe_app_command_dispatch<E>(
    dispatch: Option<AppCommandDispatch>,
    mut run: impl FnMut(&AppCommandAction) -> Result<(), E>,
    mut observer: impl FnMut(&str),
) -> Result<Option<AppCommandDispatch>, E> {
    let Some(dispatch) = dispatch else {
        return Ok(None);
    };
    run(&dispatch.action)?;
    observer(dispatch.command_id);
    Ok(Some(dispatch))
}

#[must_use]
pub fn vertical_command_direction(
    commands: &[AppCommand],
    key: &ExtensionKeyEvent,
) -> Option<VerticalCommandDirection> {
    commands.iter().find_map(|command| {
        (command.enabled && matches_any_key_chord(&command.keys).matches(key))
            .then_some(command.vertical_direction)
            .flatten()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommandCountErrorKind {
    OptionsType,
    CountRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCommandCountError {
    pub kind: AppCommandCountErrorKind,
    message: &'static str,
}

impl fmt::Display for AppCommandCountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for AppCommandCountError {}

/// Validate a JSON-RPC options payload at the same boundary as Hunk.
pub fn normalize_app_command_count(options: Option<&Value>) -> Result<usize, AppCommandCountError> {
    let Some(options) = options else {
        return Ok(1);
    };
    let Value::Object(options) = options else {
        return Err(AppCommandCountError {
            kind: AppCommandCountErrorKind::OptionsType,
            message: "Command execution options must be an object.",
        });
    };
    let Some(count) = options.get("count").filter(|value| !value.is_null()) else {
        return Ok(1);
    };
    let Some(count) = count.as_f64() else {
        return Err(AppCommandCountError {
            kind: AppCommandCountErrorKind::CountRange,
            message: "Command execution count must be a positive safe integer no greater than 10000.",
        });
    };
    if count.fract() != 0.0 || !(1.0..=MAX_APP_COMMAND_COUNT as f64).contains(&count) {
        return Err(AppCommandCountError {
            kind: AppCommandCountErrorKind::CountRange,
            message: "Command execution count must be a positive safe integer no greater than 10000.",
        });
    }
    Ok(count as usize)
}

fn command_invocation_event(command: &AppCommand) -> ExtensionKeyEvent {
    command
        .default_keys
        .iter()
        .filter_map(|chord| parse_key_chord(chord).ok())
        .next()
        .map_or_else(ExtensionKeyEvent::default, |chord| {
            synthesize_key_event(&chord).key
        })
}

#[must_use]
pub fn execute_app_command_with_count(
    commands: &[AppCommand],
    id: &str,
    count: usize,
) -> Option<AppCommandDispatch> {
    let command = find_app_command_by_id(commands, id).filter(|command| command.enabled)?;
    Some(AppCommandDispatch {
        command_id: command.id,
        closes_menu: command.closes_menu,
        action: command_action(command, &command_invocation_event(command), count),
    })
}

pub fn execute_app_command(
    commands: &[AppCommand],
    id: &str,
    options: Option<&Value>,
) -> Result<Option<AppCommandDispatch>, AppCommandCountError> {
    Ok(execute_app_command_with_count(
        commands,
        id,
        normalize_app_command_count(options)?,
    ))
}

#[must_use]
pub fn command_map(commands: &[AppCommand]) -> BTreeMap<&str, &AppCommand> {
    commands
        .iter()
        .map(|command| (command.id, command))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{UserKeyBinding, UserKeyBindingEntry, resolve_command_keys};

    fn commands() -> Vec<AppCommand> {
        build_app_commands(
            None,
            BuiltinCommandAvailability {
                can_align_current_line: true,
                can_apply_file_presentation_to_all_matching: false,
                can_edit_active_note: false,
                can_reply_to_active_note: false,
                can_refresh_current_input: true,
            },
        )
    }

    fn event(name: &str, sequence: &str) -> ExtensionKeyEvent {
        ExtensionKeyEvent {
            name: name.into(),
            sequence: sequence.into(),
            ..ExtensionKeyEvent::default()
        }
    }

    fn press(commands: &[AppCommand], key: ExtensionKeyEvent) -> Option<AppCommandDispatch> {
        dispatch_app_command(commands, &key)
    }

    #[test]
    fn note_and_gap_dispatch_follow_catalog_effects_not_command_identity() {
        let commands = commands();
        let vectors = [
            (
                AppCommandReviewEffect::StartDraft,
                AppCommandAction::StartUserNote,
            ),
            (
                AppCommandReviewEffect::ToggleNoteVisibility,
                AppCommandAction::ToggleAgentNotes,
            ),
            (
                AppCommandReviewEffect::ToggleSelectedGap,
                AppCommandAction::ToggleGapForSelectedHunk,
            ),
        ];
        for (id, (declared_effect, _)) in [
            "workdeck.review.startNote",
            "workdeck.view.toggleAgentNotes",
            "workdeck.review.toggleHunkGap",
        ]
        .into_iter()
        .zip(vectors.iter())
        {
            let original = find_app_command_by_id(&commands, id).unwrap();
            assert_eq!(original.entry.review, Some(*declared_effect));
            for (effect, expected) in &vectors {
                let mut command = original.clone();
                let mut entry = *command.entry;
                entry.review = Some(*effect);
                // The dispatch table owns static catalog references. These
                // nine test-only entries let us deliberately vary declarations.
                command.entry = Box::leak(Box::new(entry));
                assert_eq!(
                    command_action(&command, &ExtensionKeyEvent::default(), 1),
                    expected.clone(),
                    "{id} must follow {effect:?}"
                );
            }
        }
    }

    #[test]
    fn every_scroll_alias_dispatches_the_exact_unit_and_direction() {
        let commands = commands();
        let vectors = [
            (
                event("pagedown", ""),
                "workdeck.review.pageDown",
                1,
                ScrollUnit::Viewport,
            ),
            (
                event("space", " "),
                "workdeck.review.pageDown",
                1,
                ScrollUnit::Viewport,
            ),
            (
                event("f", "f"),
                "workdeck.review.pageDown",
                1,
                ScrollUnit::Viewport,
            ),
            (
                event("pageup", ""),
                "workdeck.review.pageUp",
                -1,
                ScrollUnit::Viewport,
            ),
            (
                event("b", "b"),
                "workdeck.review.pageUp",
                -1,
                ScrollUnit::Viewport,
            ),
            (
                ExtensionKeyEvent {
                    name: "space".into(),
                    sequence: " ".into(),
                    shift: true,
                    ..ExtensionKeyEvent::default()
                },
                "workdeck.review.pageUp",
                -1,
                ScrollUnit::Viewport,
            ),
            (
                event("d", "d"),
                "workdeck.review.halfPageDown",
                1,
                ScrollUnit::Half,
            ),
            (
                ExtensionKeyEvent {
                    name: "d".into(),
                    ctrl: true,
                    ..ExtensionKeyEvent::default()
                },
                "workdeck.review.halfPageDown",
                1,
                ScrollUnit::Half,
            ),
            (
                event("u", "u"),
                "workdeck.review.halfPageUp",
                -1,
                ScrollUnit::Half,
            ),
            (
                ExtensionKeyEvent {
                    name: "u".into(),
                    ctrl: true,
                    ..ExtensionKeyEvent::default()
                },
                "workdeck.review.halfPageUp",
                -1,
                ScrollUnit::Half,
            ),
        ];
        for (key, id, delta, unit) in vectors {
            let dispatch = press(&commands, key).unwrap();
            assert_eq!(dispatch.command_id, id);
            assert_eq!(
                dispatch.action,
                AppCommandAction::ScrollDiff { delta, unit }
            );
        }
        for (key, id, delta) in [
            (event("down", ""), "workdeck.review.stepDown", 1),
            (event("j", "j"), "workdeck.review.stepDown", 1),
            (event("up", ""), "workdeck.review.stepUp", -1),
            (event("k", "k"), "workdeck.review.stepUp", -1),
        ] {
            let dispatch = press(&commands, key).unwrap();
            assert_eq!(dispatch.command_id, id);
            assert_eq!(dispatch.action, AppCommandAction::StepDiffLine(delta));
        }
    }

    #[test]
    fn shifted_forms_stay_separate_and_arrows_accelerate_in_one_command() {
        let commands = commands();
        assert_eq!(
            press(&commands, event("g", "g")).unwrap().command_id,
            "workdeck.review.jumpToTop"
        );
        assert_eq!(
            press(
                &commands,
                ExtensionKeyEvent {
                    name: "g".into(),
                    sequence: "G".into(),
                    shift: true,
                    ..ExtensionKeyEvent::default()
                }
            )
            .unwrap()
            .command_id,
            "workdeck.review.jumpToBottom"
        );
        assert_eq!(
            press(&commands, event("m", "m")).unwrap().command_id,
            "workdeck.view.toggleHunkHeaders"
        );
        assert_eq!(
            press(
                &commands,
                ExtensionKeyEvent {
                    name: "m".into(),
                    sequence: "M".into(),
                    shift: true,
                    ..ExtensionKeyEvent::default()
                }
            )
            .unwrap()
            .command_id,
            "workdeck.view.toggleMenuBar"
        );
        let mut control_c = event("c", "c");
        control_c.ctrl = true;
        assert!(press(&commands, control_c).is_none());
        assert_eq!(
            press(&commands, event("left", "")).unwrap().action,
            AppCommandAction::ScrollCodeHorizontally(-1)
        );
        assert_eq!(
            press(
                &commands,
                ExtensionKeyEvent {
                    name: "left".into(),
                    shift: true,
                    ..ExtensionKeyEvent::default()
                }
            )
            .unwrap()
            .action,
            AppCommandAction::ScrollCodeHorizontally(-8)
        );
    }

    #[test]
    fn vertical_probes_do_not_run_commands_and_synthetic_dispatch_claims_only_matches() {
        let commands = commands();
        assert_eq!(
            vertical_command_direction(&commands, &event("down", "")),
            Some(VerticalCommandDirection::Down)
        );
        assert_eq!(
            vertical_command_direction(&commands, &event("[", "[")),
            Some(VerticalCommandDirection::Up)
        );
        assert_eq!(
            vertical_command_direction(&commands, &event("q", "q")),
            None
        );

        let mut matched = synthesize_key_event(&parse_key_chord("j").unwrap());
        assert!(dispatch_synthetic_app_command(&commands, &mut matched).is_some());
        assert!(matched.default_prevented);
        let mut unmatched = synthesize_key_event(&parse_key_chord("f9").unwrap());
        assert!(dispatch_synthetic_app_command(&commands, &mut unmatched).is_none());
        assert!(!unmatched.default_prevented);
    }

    #[test]
    fn labels_identity_defaults_and_catalog_order_are_derived() {
        let commands = commands();
        let by_id = command_map(&commands);
        assert_eq!(
            by_id["workdeck.review.pageUp"].key_labels,
            ["PageUp", "b", "Shift+Space"]
        );
        assert_eq!(
            by_id["workdeck.review.jumpToBottom"].key_labels,
            ["G", "End"]
        );
        assert_eq!(
            commands
                .iter()
                .map(|command| command.id)
                .collect::<Vec<_>>(),
            APP_COMMAND_CATALOG
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            builtin_command_key_defaults()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            commands
                .iter()
                .map(|command| command.id)
                .collect::<Vec<_>>()
        );
        for (command, entry) in commands.iter().zip(APP_COMMAND_CATALOG) {
            assert_eq!(command.title, entry.title);
            assert_eq!(command.aliases, entry.aliases);
            assert_eq!(command.default_keys, entry.default_keys);
            assert_eq!(command.keys, entry.default_keys);
            assert_eq!(command.public_to_extensions, entry.public_to_extensions);
            assert_eq!(command.closes_menu, entry.closes_menu);
        }
        assert_eq!(
            builtin_command_key_defaults()
                .iter()
                .filter(|entry| entry.default_keys.is_empty())
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            [
                "workdeck.app.openAgentSkill",
                "workdeck.review.focusFilter",
                "workdeck.review.alignCurrentLineTop",
                "workdeck.review.alignCurrentLineCenter",
                "workdeck.review.alignCurrentLineBottom",
                "workdeck.view.cursorLineRow",
                "workdeck.view.cursorLineNumber",
                "workdeck.view.cursorLineOff",
                "workdeck.view.applyFilePresentationToAllMatching",
                "workdeck.view.toggleCopyDecorations",
                "workdeck.review.previousAnnotatedFile",
                "workdeck.review.nextAnnotatedFile",
            ]
        );
    }

    #[test]
    fn canonical_keybinding_document_is_sorted_and_identical_to_runtime_catalog() {
        let markdown = include_str!("../../../docs/keybindings.md");
        let documented = markdown
            .lines()
            .filter_map(|line| {
                line.strip_prefix("| `workdeck.")
                    .and_then(|suffix| suffix.split('`').next())
                    .map(|suffix| format!("workdeck.{suffix}"))
            })
            .collect::<Vec<_>>();
        let mut runtime = APP_COMMAND_CATALOG
            .iter()
            .map(|command| command.id.to_owned())
            .collect::<Vec<_>>();
        // Bundled commands are remappable under the same `workdeck.` owner, so
        // the documented table lists them beside the catalog.
        runtime.extend(
            crate::bundled_search_command_defaults()
                .into_iter()
                .map(|entry| entry.id),
        );
        runtime.sort();
        assert_eq!(documented, runtime);
    }

    #[test]
    fn remapping_releases_defaults_vertical_commands_and_unbinding_matches_nothing() {
        let defaults = builtin_command_key_defaults();
        let remapped = resolve_command_keys(
            &defaults,
            &[
                UserKeyBindingEntry::new(
                    "workdeck.app.quit",
                    UserKeyBinding::Chord("ctrl+x".into()),
                ),
                UserKeyBindingEntry::new(
                    "workdeck.review.stepDown",
                    UserKeyBinding::Chords(vec!["down".into(), "j".into(), "ctrl+n".into()]),
                ),
            ],
        );
        let commands = build_app_commands(Some(&remapped), BuiltinCommandAvailability::default());
        assert_eq!(
            press(
                &commands,
                ExtensionKeyEvent {
                    name: "x".into(),
                    ctrl: true,
                    ..ExtensionKeyEvent::default()
                }
            )
            .unwrap()
            .command_id,
            "workdeck.app.quit"
        );
        assert!(press(&commands, event("q", "q")).is_none());
        assert_eq!(
            vertical_command_direction(
                &commands,
                &ExtensionKeyEvent {
                    name: "n".into(),
                    ctrl: true,
                    ..ExtensionKeyEvent::default()
                }
            ),
            Some(VerticalCommandDirection::Down)
        );

        let claimed = resolve_command_keys(
            &defaults,
            &[UserKeyBindingEntry::new(
                "workdeck.review.focusFilter",
                UserKeyBinding::Chords(vec!["f".into(), "/".into()]),
            )],
        );
        let commands = build_app_commands(Some(&claimed), BuiltinCommandAvailability::default());
        assert_eq!(
            press(&commands, event("f", "f")).unwrap().command_id,
            "workdeck.review.focusFilter"
        );
        assert_eq!(
            press(&commands, event("space", " ")).unwrap().command_id,
            "workdeck.review.pageDown"
        );
    }

    #[test]
    fn unbound_commands_remain_programmatically_reachable_and_aliases_resolve() {
        let commands = commands();
        let copy =
            find_app_command_by_id(&commands, "workdeck.view.toggleCopyDecorations").unwrap();
        assert!(copy.keys.is_empty());
        assert!(press(&commands, ExtensionKeyEvent::default()).is_none());
        assert_eq!(
            execute_app_command(&commands, "workdeck.app.openAgentSkill", None)
                .unwrap()
                .unwrap()
                .action,
            AppCommandAction::OpenAgentSkill
        );
        assert_eq!(
            execute_app_command(&commands, "workdeck.view.toggleSidebar", None)
                .unwrap()
                .unwrap()
                .action,
            AppCommandAction::ToggleFilesPane
        );
    }

    #[test]
    fn programmatic_execution_uses_shipped_semantics_counts_and_enablement() {
        let defaults = builtin_command_key_defaults();
        let resolved = resolve_command_keys(
            &defaults,
            &[UserKeyBindingEntry::new(
                "workdeck.review.scrollCodeLeft",
                UserKeyBinding::Chord("shift+left".into()),
            )],
        );
        let commands = build_app_commands(Some(&resolved), BuiltinCommandAvailability::default());
        assert_eq!(
            execute_app_command(
                &commands,
                "workdeck.review.scrollCodeLeft",
                Some(&json!({"count": 3}))
            )
            .unwrap()
            .unwrap()
            .action,
            AppCommandAction::ScrollCodeHorizontally(-3)
        );
        assert_eq!(
            press(
                &commands,
                ExtensionKeyEvent {
                    name: "left".into(),
                    shift: true,
                    ..ExtensionKeyEvent::default()
                }
            )
            .unwrap()
            .action,
            AppCommandAction::ScrollCodeHorizontally(-8)
        );
        assert_eq!(
            execute_app_command(
                &commands,
                "workdeck.review.nextHunk",
                Some(&json!({"count": 3}))
            )
            .unwrap()
            .unwrap()
            .action,
            AppCommandAction::MoveSelection {
                scope: ReviewSelectionScope::Hunk,
                delta: 3
            }
        );
        assert_eq!(
            execute_app_command(
                &commands,
                "workdeck.review.stepUp",
                Some(&json!({"count": 4}))
            )
            .unwrap()
            .unwrap()
            .action,
            AppCommandAction::StepDiffLine(-4)
        );
        assert!(
            execute_app_command(&commands, "workdeck.app.refresh", None)
                .unwrap()
                .is_some()
        );
        let disabled = build_app_commands(
            None,
            BuiltinCommandAvailability {
                can_refresh_current_input: false,
                ..BuiltinCommandAvailability::default()
            },
        );
        assert!(
            execute_app_command(&disabled, "workdeck.app.refresh", None)
                .unwrap()
                .is_none()
        );
        assert!(
            execute_app_command(&commands, "nobody.registered.this", None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn count_validation_accepts_defaults_and_integers_and_rejects_every_bad_shape() {
        assert_eq!(normalize_app_command_count(None), Ok(1));
        assert_eq!(normalize_app_command_count(Some(&json!({}))), Ok(1));
        assert_eq!(
            normalize_app_command_count(Some(&json!({"count": null}))),
            Ok(1)
        );
        assert_eq!(
            normalize_app_command_count(Some(&json!({"count": 2.0}))),
            Ok(2)
        );
        for invalid in [json!(null), json!([]), json!("x")] {
            let error = normalize_app_command_count(Some(&invalid)).unwrap_err();
            assert_eq!(error.kind, AppCommandCountErrorKind::OptionsType);
            assert_eq!(
                error.to_string(),
                "Command execution options must be an object."
            );
        }
        for invalid in [
            json!({"count": 0}),
            json!({"count": -1}),
            json!({"count": 1.5}),
            json!({"count": 10001}),
            json!({"count": "2"}),
        ] {
            let error = normalize_app_command_count(Some(&invalid)).unwrap_err();
            assert_eq!(error.kind, AppCommandCountErrorKind::CountRange);
            assert_eq!(
                error.to_string(),
                "Command execution count must be a positive safe integer no greater than 10000."
            );
        }
    }

    #[test]
    fn observers_receive_only_successful_dispatches_and_skip_failures() {
        let commands = commands();
        let mut observed = Vec::new();
        let dispatch = observe_app_command_dispatch(
            dispatch_app_command(&commands, &event("q", "q")),
            |_| Ok::<_, &str>(()),
            |id| observed.push(id.to_owned()),
        )
        .unwrap();
        assert_eq!(dispatch.unwrap().action, AppCommandAction::RequestQuit);
        assert_eq!(observed, ["workdeck.app.quit"]);
        assert!(
            observe_app_command_dispatch(
                dispatch_app_command(&commands, &event("f9", "")),
                |_| Ok::<_, &str>(()),
                |id| observed.push(id.to_owned()),
            )
            .unwrap()
            .is_none()
        );
        let error = observe_app_command_dispatch(
            dispatch_app_command(&commands, &event("q", "q")),
            |_| Err::<(), _>("boom"),
            |id| observed.push(id.to_owned()),
        )
        .unwrap_err();
        assert_eq!(error, "boom");
        assert_eq!(observed, ["workdeck.app.quit"]);
    }
}
