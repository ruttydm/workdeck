//! Stable operations for refreshing the currently mounted review.

use workdeck_core::CliInput;
use workdeck_session::SessionReloadReason;

use crate::{
    CurrentReviewRefreshOptions, CurrentReviewReloadOptions, CurrentReviewViewOptions,
    WorkspaceRefreshRequest, derive_workspace_refresh_request,
};

/// Owns the latest reload descriptor while exposing stable Rust methods.
///
/// React needed refs and memoized callbacks to keep operations stable while the
/// descriptor was replaced. The Rust owner provides that property directly:
/// callers keep the controller, update its descriptor, and every operation
/// dereferences the current request at invocation time.
#[derive(Debug, Default)]
pub struct CurrentReviewRefreshController {
    request: Option<WorkspaceRefreshRequest>,
}

impl CurrentReviewRefreshController {
    #[must_use]
    pub fn new(input: &CliInput, source_label: &str, view: &CurrentReviewViewOptions) -> Self {
        Self {
            request: derive_workspace_refresh_request(input, source_label, view),
        }
    }

    /// Replace the registered descriptor after input or live view state changes.
    pub fn update(
        &mut self,
        input: &CliInput,
        source_label: &str,
        view: &CurrentReviewViewOptions,
    ) {
        self.request = derive_workspace_refresh_request(input, source_label, view);
    }

    #[must_use]
    pub const fn can_refresh_current_input(&self) -> bool {
        self.request.is_some()
    }

    #[must_use]
    pub const fn request(&self) -> Option<&WorkspaceRefreshRequest> {
        self.request.as_ref()
    }

    /// Reload with caller-provided provenance while preserving the mounted app.
    pub fn refresh_current_input<E>(
        &self,
        options: CurrentReviewRefreshOptions,
        reload: &mut impl FnMut(&CliInput, &CurrentReviewReloadOptions) -> Result<(), E>,
    ) -> Result<bool, E> {
        let Some(request) = &self.request else {
            return Ok(false);
        };
        reload(
            &request.next_input,
            &CurrentReviewReloadOptions {
                reason: options.reason,
                reload_extensions: options.reload_extensions,
                reset_app: false,
                source_path: request.source_path.clone(),
            },
        )?;
        Ok(true)
    }

    /// Start a user-driven reload and contain its error at the UI boundary.
    pub fn trigger_refresh_current_input<E>(
        &self,
        reload: &mut impl FnMut(&CliInput, &CurrentReviewReloadOptions) -> Result<(), E>,
        report_error: &mut impl FnMut(&E),
    ) -> bool {
        match self.refresh_current_input(
            CurrentReviewRefreshOptions {
                reason: Some(SessionReloadReason::Manual),
                reload_extensions: None,
            },
            reload,
        ) {
            Ok(reloaded) => reloaded,
            Err(error) => {
                report_error(&error);
                false
            }
        }
    }

    /// Reload because watch mode observed a source change.
    pub fn refresh_watched_input<E>(
        &self,
        reload: &mut impl FnMut(&CliInput, &CurrentReviewReloadOptions) -> Result<(), E>,
    ) -> Result<bool, E> {
        self.refresh_current_input(
            CurrentReviewRefreshOptions {
                reason: Some(SessionReloadReason::Watch),
                reload_extensions: None,
            },
            reload,
        )
    }
}

#[cfg(test)]
mod tests;
