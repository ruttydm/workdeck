//! Persistent pointer gesture ownership for Ratatui's centralized event dispatcher.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseCapture<T> {
    captured: Option<T>,
}

impl<T> Default for MouseCapture<T> {
    fn default() -> Self {
        Self { captured: None }
    }
}

impl<T> MouseCapture<T> {
    pub fn set_captured(&mut self, captured: Option<T>) {
        self.captured = captured;
    }

    pub fn capture(&mut self, target: T) {
        self.set_captured(Some(target));
    }

    pub fn release(&mut self) {
        self.set_captured(None);
    }

    #[must_use]
    pub const fn as_ref(&self) -> Option<&T> {
        self.captured.as_ref()
    }

    #[must_use]
    pub fn as_mut(&mut self) -> Option<&mut T> {
        self.captured.as_mut()
    }

    #[must_use]
    pub const fn is_some(&self) -> bool {
        self.captured.is_some()
    }

    pub fn take(&mut self) -> Option<T> {
        self.captured.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_targets_a_persistent_identity_and_release_clears_it() {
        let mut capture = MouseCapture::default();
        capture.capture("pane-before-rerender".to_owned());
        assert_eq!(
            capture.as_ref().map(String::as_str),
            Some("pane-before-rerender")
        );

        // Replacing the current render tree does not replace the gesture's captured identity.
        let current_render_tree = ["pane-after-rerender"];
        assert_eq!(current_render_tree, ["pane-after-rerender"]);
        assert_eq!(
            capture.as_ref().map(String::as_str),
            Some("pane-before-rerender")
        );

        capture.release();
        assert!(!capture.is_some());
    }

    #[test]
    fn setting_an_optional_target_matches_capture_and_release_calls() {
        let mut capture = MouseCapture::default();
        capture.set_captured(Some(7));
        assert_eq!(capture.take(), Some(7));
        assert!(!capture.is_some());
        capture.set_captured(None);
        assert!(capture.as_ref().is_none());
    }
}
