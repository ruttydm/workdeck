#[cfg(test)]
use std::fmt;

#[cfg(test)]
use serde_json::Value;

use crate::{
    AppCommand, AppCommandDispatch, MAX_APP_COMMAND_COUNT, execute_app_command_with_count,
    find_app_command_by_id,
};

const NATIVE_COMPATIBILITY_ALIASES: &[(&str, &str)] = &[
    (
        "workdeck.view.cursor-line-row",
        "workdeck.view.cursorLineRow",
    ),
    ("workdeck.review.step-down", "workdeck.review.stepDown"),
    ("workdeck.review.step-up", "workdeck.review.stepUp"),
    (
        "workdeck.review.previous-hunk",
        "workdeck.review.previousHunk",
    ),
    ("workdeck.review.next-hunk", "workdeck.review.nextHunk"),
    (
        "workdeck.review.align-current-line-top",
        "workdeck.review.alignCurrentLineTop",
    ),
    (
        "workdeck.review.align-current-line-center",
        "workdeck.review.alignCurrentLineCenter",
    ),
    (
        "workdeck.review.align-current-line-bottom",
        "workdeck.review.alignCurrentLineBottom",
    ),
    (
        "workdeck.review.half-page-down",
        "workdeck.review.halfPageDown",
    ),
    ("workdeck.review.half-page-up", "workdeck.review.halfPageUp"),
    ("workdeck.review.jump-to-top", "workdeck.review.jumpToTop"),
    (
        "workdeck.review.jump-to-bottom",
        "workdeck.review.jumpToBottom",
    ),
];

pub(crate) fn canonical_extension_review_command_id(id: &str) -> &str {
    NATIVE_COMPATIBILITY_ALIASES
        .iter()
        .find_map(|(alias, canonical)| (*alias == id).then_some(*canonical))
        .unwrap_or(id)
}

pub(crate) fn native_compatibility_aliases(id: &str) -> impl Iterator<Item = &'static str> {
    NATIVE_COMPATIBILITY_ALIASES
        .iter()
        .filter_map(move |(alias, canonical)| (*canonical == id).then_some(*alias))
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExtensionCommandControlError {
    EmptyCommandId,
    InvalidOptions(crate::AppCommandCountError),
}

#[cfg(test)]
impl fmt::Display for ExtensionCommandControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCommandId => {
                formatter.write_str("Extension command controls require a non-empty command id.")
            }
            Self::InvalidOptions(error) => error.fmt(formatter),
        }
    }
}

#[cfg(test)]
impl std::error::Error for ExtensionCommandControlError {}

fn find_public_command<'a>(commands: &'a [AppCommand], id: &str) -> Option<&'a AppCommand> {
    find_app_command_by_id(commands, canonical_extension_review_command_id(id))
        .filter(|command| command.id.starts_with("workdeck.") && command.public_to_extensions)
}

/// Probe the current live command table without turning malformed input into an extension failure.
#[must_use]
#[cfg(test)]
pub(crate) fn extension_command_is_enabled(
    commands: &[AppCommand],
    is_live: bool,
    command_id: Option<&str>,
) -> bool {
    let Some(command_id) = command_id.filter(|id| !id.trim().is_empty()) else {
        return false;
    };
    if !is_live {
        return false;
    }
    find_public_command(commands, command_id).is_some_and(crate::is_command_enabled)
}

/// Validate and execute against the supplied live table.
///
/// ID and option validation deliberately precede the liveness check. A retained handler therefore
/// still sees its programming error after the review that created it has retired.
#[cfg(test)]
pub(crate) fn execute_extension_command(
    commands: &[AppCommand],
    is_live: bool,
    command_id: &str,
    options: Option<&Value>,
) -> Result<Option<AppCommandDispatch>, ExtensionCommandControlError> {
    if command_id.trim().is_empty() {
        return Err(ExtensionCommandControlError::EmptyCommandId);
    }
    let count = crate::normalize_app_command_count(options)
        .map_err(ExtensionCommandControlError::InvalidOptions)?;
    Ok(execute_extension_command_with_count(
        commands, is_live, command_id, count,
    ))
}

/// Execute an already-decoded native-protocol count while retaining the same public-command gate.
#[must_use]
pub(crate) fn execute_extension_command_with_count(
    commands: &[AppCommand],
    is_live: bool,
    command_id: &str,
    count: usize,
) -> Option<AppCommandDispatch> {
    if !is_live || !(1..=MAX_APP_COMMAND_COUNT).contains(&count) {
        return None;
    }
    let command = find_public_command(commands, command_id)?;
    execute_app_command_with_count(std::slice::from_ref(command), command.id, count)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{BuiltinCommandAvailability, build_app_commands};

    fn commands() -> Vec<AppCommand> {
        build_app_commands(None, BuiltinCommandAvailability::default())
    }

    #[test]
    fn frozen_hunk_command_controls_oracle_records_both_pinned_baselines() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-command-controls.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["stablePresence"], "identical");
        assert_eq!(oracle["executedOracle"]["passed"], 5);
        assert_eq!(oracle["executedOracle"]["expectations"], 31);
    }

    #[test]
    fn enabled_public_commands_execute_with_a_normalized_count() {
        let commands = commands();
        assert!(extension_command_is_enabled(
            &commands,
            true,
            Some("workdeck.review.nextHunk")
        ));
        let dispatch = execute_extension_command(
            &commands,
            true,
            "workdeck.review.nextHunk",
            Some(&json!({"count": 7})),
        )
        .unwrap()
        .unwrap();
        assert_eq!(dispatch.command_id, "workdeck.review.nextHunk");
        assert_eq!(
            dispatch.action,
            crate::AppCommandAction::MoveSelection {
                scope: workdeck_review::ReviewSelectionScope::Hunk,
                delta: 7,
            }
        );
    }

    #[test]
    fn public_compatibility_alias_resolves_without_a_duplicate_command() {
        let commands = commands();
        assert!(extension_command_is_enabled(
            &commands,
            true,
            Some("workdeck.view.toggleSidebar")
        ));
        let dispatch = execute_extension_command(
            &commands,
            true,
            "workdeck.view.toggleSidebar",
            Some(&json!({"count": 2})),
        )
        .unwrap()
        .unwrap();
        assert_eq!(dispatch.command_id, "workdeck.view.toggleFilesPane");
    }

    #[test]
    fn every_probe_observes_the_current_table() {
        let mut commands = commands();
        let command = commands
            .iter_mut()
            .find(|command| command.id == "workdeck.review.nextHunk")
            .unwrap();
        command.enabled = false;
        assert!(!extension_command_is_enabled(
            &commands,
            true,
            Some("workdeck.review.nextHunk")
        ));
        let command = commands
            .iter_mut()
            .find(|command| command.id == "workdeck.review.nextHunk")
            .unwrap();
        command.enabled = true;
        assert!(extension_command_is_enabled(
            &commands,
            true,
            Some("workdeck.review.nextHunk")
        ));
    }

    #[test]
    fn unknown_disabled_private_extension_owned_and_stale_commands_are_refused() {
        let mut commands = commands();
        let disabled = commands
            .iter_mut()
            .find(|command| command.id == "workdeck.app.refresh")
            .unwrap();
        disabled.enabled = false;
        let private = commands
            .iter_mut()
            .find(|command| command.id == "workdeck.app.quit")
            .unwrap();
        private.public_to_extensions = false;
        let extension_owned = commands
            .iter_mut()
            .find(|command| command.id == "workdeck.app.toggleHelp")
            .unwrap();
        extension_owned.id = "example.run";

        for id in [
            "workdeck.missing",
            "workdeck.app.refresh",
            "workdeck.app.quit",
            "example.run",
        ] {
            assert!(!extension_command_is_enabled(&commands, true, Some(id)));
            assert!(
                execute_extension_command(&commands, true, id, None)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(!extension_command_is_enabled(
            &commands,
            false,
            Some("workdeck.review.nextHunk")
        ));
        assert!(
            execute_extension_command(&commands, false, "workdeck.review.nextHunk", None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn probes_do_not_error_but_execution_validates_before_stale_liveness() {
        let commands = commands();
        assert!(!extension_command_is_enabled(&commands, true, Some("")));
        assert!(!extension_command_is_enabled(&commands, true, None));
        assert_eq!(
            execute_extension_command(&commands, true, "", None).unwrap_err(),
            ExtensionCommandControlError::EmptyCommandId
        );
        assert!(
            execute_extension_command(&commands, true, "workdeck.unknown", Some(&json!(null)),)
                .unwrap_err()
                .to_string()
                .contains("options must be an object")
        );
        for count in [json!(0), json!(-1), json!(1.5), json!(10001)] {
            assert!(
                execute_extension_command(
                    &commands,
                    false,
                    "workdeck.review.nextHunk",
                    Some(&json!({"count": count})),
                )
                .unwrap_err()
                .to_string()
                .contains("no greater than 10000")
            );
        }
    }
}
