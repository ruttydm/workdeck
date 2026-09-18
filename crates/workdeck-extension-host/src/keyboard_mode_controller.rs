//! Session-wide ownership state for native extension keyboard modes.

use workdeck_extension_api::KeyRoutingResult;

/// Authority carried by one batch of declarative native-extension actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardModeActionAuthority {
    /// A normal command, pane, dialog, or event callback owned by the extension.
    Unscoped,
    /// A key callback bound to one exact activation.
    ActiveKey(u64),
    /// An enter/exit lifecycle callback; ownership changes are intentionally inert.
    Lifecycle,
}

/// Identity and reentrancy state for the one mode active in a review session.
#[derive(Debug, Clone)]
pub struct KeyboardModeControllerState {
    alive: bool,
    lifecycle_depth: usize,
    next_activation_id: u64,
    active_activation_id: Option<u64>,
}

impl Default for KeyboardModeControllerState {
    fn default() -> Self {
        Self {
            alive: true,
            lifecycle_depth: 0,
            next_activation_id: 0,
            active_activation_id: None,
        }
    }
}

impl KeyboardModeControllerState {
    #[must_use]
    pub fn active_activation_id(&self) -> Option<u64> {
        self.active_activation_id
    }

    /// Allocate a fresh identity after the caller has torn down any predecessor.
    pub fn activate(&mut self) -> Option<u64> {
        if !self.ownership_change_allowed(KeyboardModeActionAuthority::Unscoped) {
            return None;
        }
        self.next_activation_id = self.next_activation_id.saturating_add(1);
        self.active_activation_id = Some(self.next_activation_id);
        self.active_activation_id
    }

    /// Clear exactly the activation the caller is about to tear down.
    pub fn retire(&mut self, activation_id: u64) -> bool {
        if self.active_activation_id != Some(activation_id) {
            return false;
        }
        self.active_activation_id = None;
        true
    }

    /// Clear a failed entry only if it has not already been replaced.
    pub fn fail_entry(&mut self, activation_id: u64) -> bool {
        self.retire(activation_id)
    }

    /// Preserve a live activation across content updates or return it for stale teardown.
    pub fn reconcile(&mut self, still_valid: bool) -> Option<u64> {
        if still_valid {
            None
        } else {
            self.active_activation_id.take()
        }
    }

    /// Make retained extension controls inert and return the one activation requiring teardown.
    pub fn shutdown(&mut self) -> Option<u64> {
        self.alive = false;
        self.active_activation_id.take()
    }

    /// Execute lifecycle work under the reentrancy barrier used by mode-scoped controls.
    pub fn run_lifecycle<R>(&mut self, callback: impl FnOnce(&mut Self) -> R) -> R {
        self.lifecycle_depth = self.lifecycle_depth.saturating_add(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(self)));
        self.lifecycle_depth = self.lifecycle_depth.saturating_sub(1);
        match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    #[must_use]
    pub fn ownership_change_allowed(&self, authority: KeyboardModeActionAuthority) -> bool {
        self.alive
            && self.lifecycle_depth == 0
            && match authority {
                KeyboardModeActionAuthority::Unscoped => true,
                KeyboardModeActionAuthority::ActiveKey(activation_id) => {
                    self.active_activation_id == Some(activation_id)
                }
                KeyboardModeActionAuthority::Lifecycle => false,
            }
    }

    #[must_use]
    pub fn scoped_observation_allowed(&self, activation_id: u64) -> bool {
        self.alive && self.active_activation_id == Some(activation_id)
    }

    /// An old key callback returning `exit` cannot retire the mode it installed as a replacement.
    #[must_use]
    pub fn normalize_key_result(
        &self,
        source_activation_id: u64,
        result: KeyRoutingResult,
    ) -> KeyRoutingResult {
        if result == KeyRoutingResult::Exit
            && self.active_activation_id != Some(source_activation_id)
        {
            KeyRoutingResult::Handled
        } else {
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_hunk_keyboard_mode_controller_oracle_records_both_pinned_baselines() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/keyboard-mode-controller.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineOracle"]["passed"], 10);
        assert_eq!(oracle["baselineOracle"]["expectations"], 52);
        assert_eq!(oracle["stableOracle"]["passed"], 10);
        assert_eq!(oracle["stableOracle"]["expectations"], 52);
    }

    #[test]
    fn scopes_observation_and_exit_while_a_later_extension_replaces_the_mode() {
        let mut controller = KeyboardModeControllerState::default();
        let alpha = controller.activate().unwrap();
        assert!(controller.scoped_observation_allowed(alpha));
        assert!(controller.retire(alpha));
        let beta = controller.activate().unwrap();
        assert!(!controller.scoped_observation_allowed(alpha));
        assert!(!controller.retire(alpha));
        assert!(controller.scoped_observation_allowed(beta));
    }

    #[test]
    fn closed_authority_retires_synchronously_and_controls_stay_inert() {
        let mut controller = KeyboardModeControllerState::default();
        let activation = controller.activate().unwrap();
        assert_eq!(controller.reconcile(false), Some(activation));
        assert!(!controller.scoped_observation_allowed(activation));
        controller.shutdown();
        assert_eq!(controller.activate(), None);
        assert!(!controller.ownership_change_allowed(KeyboardModeActionAuthority::Unscoped));
    }

    #[test]
    fn lifecycle_callbacks_cannot_change_keyboard_ownership() {
        let mut controller = KeyboardModeControllerState::default();
        let activation = controller.activate().unwrap();
        controller.run_lifecycle(|controller| {
            assert!(
                !controller
                    .ownership_change_allowed(KeyboardModeActionAuthority::ActiveKey(activation))
            );
            assert!(!controller.ownership_change_allowed(KeyboardModeActionAuthority::Unscoped));
            assert!(!controller.ownership_change_allowed(KeyboardModeActionAuthority::Lifecycle));
        });
        assert!(
            controller.ownership_change_allowed(KeyboardModeActionAuthority::ActiveKey(activation))
        );
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            controller.run_lifecycle(|_| panic!("contained by test"));
        }));
        assert!(
            controller.ownership_change_allowed(KeyboardModeActionAuthority::ActiveKey(activation))
        );
    }

    #[test]
    fn failed_entry_cleans_up_and_permits_a_later_command_entry() {
        let mut controller = KeyboardModeControllerState::default();
        let failed = controller.activate().unwrap();
        assert!(controller.fail_entry(failed));
        assert_eq!(controller.active_activation_id(), None);
        let healthy = controller.activate().unwrap();
        assert!(controller.scoped_observation_allowed(healthy));
    }

    #[test]
    fn an_on_key_replacement_turns_the_predecessors_exit_into_handled() {
        let mut controller = KeyboardModeControllerState::default();
        let alpha = controller.activate().unwrap();
        assert!(controller.retire(alpha));
        let beta = controller.activate().unwrap();
        assert_eq!(
            controller.normalize_key_result(alpha, KeyRoutingResult::Exit),
            KeyRoutingResult::Handled
        );
        assert!(controller.scoped_observation_allowed(beta));
    }

    #[test]
    fn host_exit_invalidates_lifecycle_controls_now_and_later() {
        let mut controller = KeyboardModeControllerState::default();
        let alpha = controller.activate().unwrap();
        assert!(controller.retire(alpha));
        controller.run_lifecycle(|controller| {
            assert!(!controller.scoped_observation_allowed(alpha));
            assert!(
                !controller.ownership_change_allowed(KeyboardModeActionAuthority::ActiveKey(alpha))
            );
            assert!(!controller.ownership_change_allowed(KeyboardModeActionAuthority::Unscoped));
        });
        assert!(!controller.scoped_observation_allowed(alpha));
    }

    #[test]
    fn an_old_mode_scope_cannot_inspect_or_exit_a_same_extension_replacement() {
        let mut controller = KeyboardModeControllerState::default();
        let old = controller.activate().unwrap();
        assert!(controller.retire(old));
        let replacement = controller.activate().unwrap();
        assert!(!controller.scoped_observation_allowed(old));
        assert!(!controller.retire(old));
        assert!(controller.scoped_observation_allowed(replacement));
    }

    #[test]
    fn content_updates_preserve_but_registry_replacement_retires_the_mode() {
        let mut controller = KeyboardModeControllerState::default();
        let activation = controller.activate().unwrap();
        assert_eq!(controller.reconcile(true), None);
        assert!(controller.scoped_observation_allowed(activation));
        assert_eq!(controller.reconcile(false), Some(activation));
        assert!(!controller.scoped_observation_allowed(activation));
    }

    #[test]
    fn unmount_exits_once_and_makes_retained_controls_inert() {
        let mut controller = KeyboardModeControllerState::default();
        let activation = controller.activate().unwrap();
        assert_eq!(controller.shutdown(), Some(activation));
        assert_eq!(controller.shutdown(), None);
        assert!(!controller.scoped_observation_allowed(activation));
        assert_eq!(controller.activate(), None);
    }

    #[test]
    fn a_throwing_key_result_exits_through_the_controller_once() {
        let mut controller = KeyboardModeControllerState::default();
        let activation = controller.activate().unwrap();
        assert_eq!(
            controller.normalize_key_result(activation, KeyRoutingResult::Exit),
            KeyRoutingResult::Exit
        );
        assert!(controller.retire(activation));
        assert!(!controller.retire(activation));
    }
}
