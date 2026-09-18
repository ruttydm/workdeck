//! Identity-safe session keyboard-mode helpers for the native extension host.

use std::sync::Arc;

use workdeck_diff::sanitize_terminal_line;
use workdeck_extension_api::{ExtensionKeyEvent, KeyRoutingResult, KeyboardModeRegistration};

use crate::{
    ExtensionEventBusPhase, ExtensionRuntimeRegistry, NativeModeCallbackError,
    SynchronousCallbackValue, SynchronousExtensionCallbackResult, call_extension_synchronously,
};

/// One keyboard-mode registration with stable identity inside a loaded registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredKeyboardMode {
    pub extension_id: String,
    pub mode: KeyboardModeRegistration,
}

/// Everything the host retains while one session keyboard mode is active.
#[derive(Debug, Clone)]
pub struct ActiveSessionKeyboardMode {
    pub extension_id: String,
    pub mode_id: String,
    pub registered: Arc<RegisteredKeyboardMode>,
    pub registry: Arc<ExtensionRuntimeRegistry>,
}

/// Report whether an activation still belongs to the live extension registry.
#[must_use]
pub fn session_keyboard_mode_still_valid(
    active: &ActiveSessionKeyboardMode,
    registry: Option<&Arc<ExtensionRuntimeRegistry>>,
    modes: &[Arc<RegisteredKeyboardMode>],
) -> bool {
    registry.is_some_and(|registry| Arc::ptr_eq(&active.registry, registry))
        && active.registry.phase() != ExtensionEventBusPhase::Closed
        && modes
            .iter()
            .any(|registered| Arc::ptr_eq(registered, &active.registered))
}

/// Return the terminal-safe human label for one extension-authored mode title.
#[must_use]
pub fn session_keyboard_mode_display_title(active: &ActiveSessionKeyboardMode) -> String {
    let title = sanitize_terminal_line(&active.registered.mode.title)
        .trim()
        .to_owned();
    if title.is_empty() {
        sanitize_terminal_line(&format!("{}:{}", active.extension_id, active.mode_id))
    } else {
        title
    }
}

/// Build the persistent status label for one active session mode.
#[must_use]
pub fn session_keyboard_mode_status_hint(active: &ActiveSessionKeyboardMode) -> String {
    let owner = sanitize_terminal_line(&format!("{}:{}", active.extension_id, active.mode_id));
    format!(
        "{} — ext {owner} — Esc exits",
        session_keyboard_mode_display_title(active)
    )
}

/// Attribute one contained callback failure to its extension and mode.
#[must_use]
pub fn format_keyboard_mode_failure(
    active: &ActiveSessionKeyboardMode,
    action: &str,
    detail: &str,
) -> String {
    format!(
        "Extension {} keyboard mode \"{}\" failed {action} • {detail}",
        active.extension_id, active.mode_id
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKeyboardModeLifecyclePhase {
    OnEnter,
    OnExit,
}

impl SessionKeyboardModeLifecyclePhase {
    const fn label(self) -> &'static str {
        match self {
            Self::OnEnter => "onEnter",
            Self::OnExit => "onExit",
        }
    }
}

/// Run one lifecycle callback synchronously with extension failure containment.
pub fn run_session_keyboard_mode_lifecycle(
    active: &ActiveSessionKeyboardMode,
    phase: SessionKeyboardModeLifecyclePhase,
    has_callback: bool,
    callback: impl FnOnce(&ActiveSessionKeyboardMode) -> Result<(), NativeModeCallbackError>,
    mut notify: impl FnMut(String),
) -> bool {
    if !has_callback {
        return true;
    }
    let result = call_extension_synchronously(|| match callback(active) {
        Ok(()) => Ok(SynchronousCallbackValue::Returned(())),
        Err(NativeModeCallbackError::Failed(detail)) => Err(detail),
        Err(NativeModeCallbackError::MustReturnSynchronously) => {
            Ok(SynchronousCallbackValue::Thenable)
        }
    });
    match result {
        SynchronousExtensionCallbackResult::Returned(()) => true,
        SynchronousExtensionCallbackResult::Thenable => {
            let detail = format!("{} must return synchronously", phase.label());
            notify(format_keyboard_mode_failure(active, phase.label(), &detail));
            false
        }
        SynchronousExtensionCallbackResult::Threw(detail) => {
            notify(format_keyboard_mode_failure(active, phase.label(), &detail));
            false
        }
    }
}

/// Deliver one public key snapshot and normalize the extension's routing answer.
pub fn deliver_session_keyboard_mode_key(
    active: &ActiveSessionKeyboardMode,
    key: &ExtensionKeyEvent,
    callback: impl FnOnce(
        &ActiveSessionKeyboardMode,
        &ExtensionKeyEvent,
    ) -> Result<Option<KeyRoutingResult>, NativeModeCallbackError>,
    mut notify: impl FnMut(String),
) -> KeyRoutingResult {
    let result = call_extension_synchronously(|| match callback(active, key) {
        Ok(result) => Ok(SynchronousCallbackValue::Returned(result)),
        Err(NativeModeCallbackError::Failed(detail)) => Err(detail),
        Err(NativeModeCallbackError::MustReturnSynchronously) => {
            Ok(SynchronousCallbackValue::Thenable)
        }
    });
    match result {
        SynchronousExtensionCallbackResult::Returned(Some(result)) => result,
        SynchronousExtensionCallbackResult::Returned(None) => KeyRoutingResult::Pass,
        SynchronousExtensionCallbackResult::Thenable => {
            notify(format_keyboard_mode_failure(
                active,
                "onKey",
                "onKey must return synchronously",
            ));
            KeyRoutingResult::Exit
        }
        SynchronousExtensionCallbackResult::Threw(detail) => {
            notify(format_keyboard_mode_failure(active, "onKey", &detail));
            KeyRoutingResult::Exit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_hunk_keyboard_mode_oracle_records_both_pinned_baselines() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/keyboard-mode.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineOracle"]["passed"], 4);
        assert_eq!(oracle["baselineOracle"]["expectations"], 12);
        assert_eq!(oracle["stableOracle"]["passed"], 4);
        assert_eq!(oracle["stableOracle"]["expectations"], 12);
    }

    fn active(title: &str) -> (ActiveSessionKeyboardMode, Vec<Arc<RegisteredKeyboardMode>>) {
        let registry = Arc::new(ExtensionRuntimeRegistry::new());
        registry.set_phase(ExtensionEventBusPhase::Ready);
        let registered = Arc::new(RegisteredKeyboardMode {
            extension_id: "vim".into(),
            mode: KeyboardModeRegistration {
                id: "normal".into(),
                title: title.into(),
            },
        });
        (
            ActiveSessionKeyboardMode {
                extension_id: "vim".into(),
                mode_id: "normal".into(),
                registered: Arc::clone(&registered),
                registry,
            },
            vec![registered],
        )
    }

    #[test]
    fn validity_requires_registry_authority_phase_and_registration_identity() {
        let (active, modes) = active("Vim normal");
        assert!(session_keyboard_mode_still_valid(
            &active,
            Some(&active.registry),
            &modes
        ));

        active.registry.set_phase(ExtensionEventBusPhase::Closed);
        assert!(!session_keyboard_mode_still_valid(
            &active,
            Some(&active.registry),
            &modes
        ));
        active.registry.set_phase(ExtensionEventBusPhase::Ready);
        let other_registry = Arc::new(ExtensionRuntimeRegistry::new());
        assert!(!session_keyboard_mode_still_valid(
            &active,
            Some(&other_registry),
            &modes
        ));
        let copied = vec![Arc::new((*modes[0]).clone())];
        assert!(!session_keyboard_mode_still_valid(
            &active,
            Some(&active.registry),
            &copied
        ));
    }

    #[test]
    fn status_hint_is_attributed_and_terminal_safe() {
        let (active_mode, _) = active("Vim\u{1b}]0;spoof\u{7} normal");
        assert_eq!(
            session_keyboard_mode_status_hint(&active_mode),
            "Vim normal — ext vim:normal — Esc exits"
        );
        let (fallback, _) = active("\u{1b}]0;spoof\u{7}");
        assert_eq!(session_keyboard_mode_display_title(&fallback), "vim:normal");
    }

    #[test]
    fn invalid_key_results_failures_and_deferred_answers_are_contained() {
        let (active, _) = active("Normal");
        let key = ExtensionKeyEvent {
            name: "j".into(),
            ..ExtensionKeyEvent::default()
        };
        let mut warnings = Vec::new();
        assert_eq!(
            deliver_session_keyboard_mode_key(
                &active,
                &key,
                |_, _| Ok(None),
                |warning| warnings.push(warning),
            ),
            KeyRoutingResult::Pass
        );
        assert_eq!(
            deliver_session_keyboard_mode_key(
                &active,
                &key,
                |_, _| Err(NativeModeCallbackError::MustReturnSynchronously),
                |warning| warnings.push(warning),
            ),
            KeyRoutingResult::Exit
        );
        assert_eq!(
            deliver_session_keyboard_mode_key(
                &active,
                &key,
                |_, _| Err(NativeModeCallbackError::Failed("boom".into())),
                |warning| warnings.push(warning),
            ),
            KeyRoutingResult::Exit
        );
        assert_eq!(
            warnings,
            [
                "Extension vim keyboard mode \"normal\" failed onKey • onKey must return synchronously",
                "Extension vim keyboard mode \"normal\" failed onKey • boom",
            ]
        );
    }

    #[test]
    fn lifecycle_failures_and_deferred_answers_are_contained() {
        let (active, _) = active("Normal");
        let mut warnings = Vec::new();
        assert!(!run_session_keyboard_mode_lifecycle(
            &active,
            SessionKeyboardModeLifecyclePhase::OnEnter,
            true,
            |_| Err(NativeModeCallbackError::MustReturnSynchronously),
            |warning| warnings.push(warning),
        ));
        assert!(!run_session_keyboard_mode_lifecycle(
            &active,
            SessionKeyboardModeLifecyclePhase::OnExit,
            true,
            |_| Err(NativeModeCallbackError::Failed("teardown".into())),
            |warning| warnings.push(warning),
        ));
        assert!(run_session_keyboard_mode_lifecycle(
            &active,
            SessionKeyboardModeLifecyclePhase::OnEnter,
            false,
            |_| panic!("absent callbacks are not invoked"),
            |_| panic!("absent callbacks do not warn"),
        ));
        assert_eq!(
            warnings,
            [
                "Extension vim keyboard mode \"normal\" failed onEnter • onEnter must return synchronously",
                "Extension vim keyboard mode \"normal\" failed onExit • teardown",
            ]
        );
    }
}
