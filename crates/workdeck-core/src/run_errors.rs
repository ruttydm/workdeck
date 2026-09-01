//! Stable user-facing failure vocabulary shared by CLI and native extensions.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::fmt;

pub const WORKDECK_EXTENSION_USER_ERROR_NAME: &str = "WorkdeckExtensionUserError";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkdeckExtensionUserError {
    pub name: String,
    pub message: String,
    #[serde(default)]
    pub suggestions: Vec<String>,
}

impl WorkdeckExtensionUserError {
    #[must_use]
    pub fn new(message: impl Into<String>, suggestions: Vec<String>) -> Self {
        Self {
            name: WORKDECK_EXTENSION_USER_ERROR_NAME.into(),
            message: message.into(),
            suggestions,
        }
    }
}

impl fmt::Display for WorkdeckExtensionUserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WorkdeckExtensionUserError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckUserError {
    pub message: String,
    pub suggestions: Vec<String>,
}

impl WorkdeckUserError {
    #[must_use]
    pub fn new(message: impl Into<String>, suggestions: Vec<String>) -> Self {
        Self {
            message: message.into(),
            suggestions,
        }
    }
}

impl fmt::Display for WorkdeckUserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WorkdeckUserError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliFailure {
    User(WorkdeckUserError),
    Published(WorkdeckExtensionUserError),
    Unexpected {
        message: String,
        stack: Option<String>,
    },
    Thrown(String),
}

#[must_use]
pub fn is_user_facing_error(error: &(dyn Error + 'static)) -> bool {
    error.is::<WorkdeckUserError>() || error.is::<WorkdeckExtensionUserError>()
}

/// Detect the published shape structurally after crossing a JSON-RPC boundary.
#[must_use]
pub fn is_user_facing_error_value(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.get("name").and_then(Value::as_str) == Some(WORKDECK_EXTENSION_USER_ERROR_NAME)
            && object.get("message").is_some_and(Value::is_string)
    })
}

/// Normalize a structurally valid native-extension failure into Workdeck's local type.
#[must_use]
pub fn to_user_facing_error(value: &Value) -> Option<WorkdeckUserError> {
    let object = value
        .as_object()
        .filter(|_| is_user_facing_error_value(value))?;
    let suggestions = object
        .get("suggestions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    Some(WorkdeckUserError::new(
        object.get("message")?.as_str()?,
        suggestions,
    ))
}

#[must_use]
pub fn format_cli_error(error: &CliFailure, debug: bool) -> String {
    match error {
        CliFailure::User(error) => format_user_error(&error.message, &error.suggestions),
        CliFailure::Published(error) => format_user_error(&error.message, &error.suggestions),
        CliFailure::Unexpected {
            message,
            stack: Some(stack),
        } if debug => format!("{stack}\n"),
        CliFailure::Unexpected { message, .. } => format!("workdeck: {message}\n"),
        CliFailure::Thrown(value) => format!("workdeck: {value}\n"),
    }
}

#[must_use]
pub fn format_cli_error_from_environment(error: &CliFailure) -> String {
    format_cli_error(error, std::env::var("WORKDECK_DEBUG").as_deref() == Ok("1"))
}

fn format_user_error(message: &str, suggestions: &[String]) -> String {
    if suggestions.is_empty() {
        return format!("workdeck: {message}\n");
    }
    format!("workdeck: {message}\n\n{}\n", suggestions.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_expected_errors_with_optional_details_and_no_stack() {
        assert_eq!(
            format_cli_error(
                &CliFailure::User(WorkdeckUserError::new("Not in a repo", Vec::new())),
                true,
            ),
            "workdeck: Not in a repo\n"
        );
        assert_eq!(
            format_cli_error(
                &CliFailure::User(WorkdeckUserError::new(
                    "Invalid ref",
                    vec!["Try `HEAD~1`.".into()],
                )),
                false,
            ),
            "workdeck: Invalid ref\n\nTry `HEAD~1`.\n"
        );
    }

    #[test]
    fn unexpected_stacks_are_visible_only_in_debug_mode() {
        let error = CliFailure::Unexpected {
            message: "Boom".into(),
            stack: Some("Error: Boom\n    at internal".into()),
        };
        assert_eq!(format_cli_error(&error, false), "workdeck: Boom\n");
        assert_eq!(
            format_cli_error(&error, true),
            "Error: Boom\n    at internal\n"
        );
    }

    #[test]
    fn stringifies_non_error_thrown_values() {
        assert_eq!(
            format_cli_error(&CliFailure::Thrown("plain failure".into()), false),
            "workdeck: plain failure\n"
        );
    }

    #[test]
    fn published_extension_errors_use_the_same_format() {
        let published =
            WorkdeckExtensionUserError::new("Invalid ref", vec!["Try `HEAD~1`.".into()]);
        assert!(is_user_facing_error(&published));
        assert_eq!(
            format_cli_error(&CliFailure::Published(published), false),
            "workdeck: Invalid ref\n\nTry `HEAD~1`.\n"
        );
    }

    #[test]
    fn recognizes_and_defensively_normalizes_the_structural_wire_shape() {
        let value = json!({
            "name": "WorkdeckExtensionUserError",
            "message": "No backend",
            "suggestions": ["Install one.", 42, null]
        });
        assert!(is_user_facing_error_value(&value));
        assert_eq!(
            to_user_facing_error(&value),
            Some(WorkdeckUserError::new(
                "No backend",
                vec!["Install one.".into()]
            ))
        );
        assert!(!is_user_facing_error_value(&json!({
            "name": "Error",
            "message": "No backend"
        })));
    }
}
