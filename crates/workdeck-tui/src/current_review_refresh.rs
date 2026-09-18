//! Reload descriptors for the review currently mounted in the terminal.
//!
//! Manual refreshes, watch mode, session requests, extension replacement, and
//! completed workspace writes all rebuild the same original input. The
//! descriptor reapplies live view state so a soft reload cannot silently fall
//! back to launch-time settings. Inputs that consumed standard input are never
//! offered as reloadable.

use workdeck_core::{CliInput, InputLayoutMode};
use workdeck_session::SessionReloadReason;

/// Live view settings that survive an in-session review refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentReviewViewOptions {
    pub layout_mode: InputLayoutMode,
    pub theme_id: String,
    pub show_agent_notes: bool,
    pub show_hunk_headers: bool,
    pub show_line_numbers: bool,
    pub show_menu_bar: bool,
    pub wrap_lines: bool,
}

/// Caller-selected provenance and extension behavior for one refresh.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CurrentReviewRefreshOptions {
    pub reason: Option<SessionReloadReason>,
    pub reload_extensions: Option<bool>,
}

/// Full reload options sent to the application host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentReviewReloadOptions {
    pub reason: Option<SessionReloadReason>,
    pub reload_extensions: Option<bool>,
    pub reset_app: bool,
    pub source_path: Option<String>,
}

/// Reopenable input and source provenance registered by the mounted review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRefreshRequest {
    pub next_input: CliInput,
    pub source_path: Option<String>,
}

/// Apply all mounted view settings while retaining unrelated input options.
#[must_use]
pub fn with_current_review_view_options(
    input: &CliInput,
    view: &CurrentReviewViewOptions,
) -> CliInput {
    let mut next = input.clone();
    let options = next.options_mut();
    options.mode = Some(view.layout_mode);
    options.theme = Some(view.theme_id.clone());
    options.agent_notes = Some(view.show_agent_notes);
    options.hunk_headers = Some(view.show_hunk_headers);
    options.line_numbers = Some(view.show_line_numbers);
    options.menu_bar = Some(view.show_menu_bar);
    options.wrap_lines = Some(view.wrap_lines);
    next
}

/// Whether the input can be rebuilt without consuming standard input again.
#[must_use]
pub fn can_reload_cli_input(input: &CliInput) -> bool {
    if input.options().agent_context.as_deref() == Some("-") {
        return false;
    }
    match input {
        CliInput::Patch(input) => input.file.as_deref().is_some_and(|path| path != "-"),
        CliInput::Vcs(_)
        | CliInput::Show(_)
        | CliInput::StashShow(_)
        | CliInput::Files(_)
        | CliInput::DiffTool(_) => true,
    }
}

/// Derive the mounted input descriptor, or `None` when it cannot be reopened.
#[must_use]
pub fn derive_workspace_refresh_request(
    input: &CliInput,
    source_label: &str,
    view: &CurrentReviewViewOptions,
) -> Option<WorkspaceRefreshRequest> {
    can_reload_cli_input(input).then(|| WorkspaceRefreshRequest {
        next_input: with_current_review_view_options(input, view),
        source_path: matches!(
            input,
            CliInput::Vcs(_) | CliInput::Show(_) | CliInput::StashShow(_)
        )
        .then(|| source_label.to_owned()),
    })
}

#[cfg(test)]
mod tests;
