//! Presentation-neutral startup notices shared by the CLI and Ratatui shell.

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// One transient message shown in the footer during application startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupNotice {
    pub key: String,
    pub message: String,
}

impl StartupNotice {
    #[must_use]
    pub fn new(key: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            message: message.into(),
        }
    }
}

/// Warn when Workdeck had to approximate deprecated semantic syntax colors.
pub static LEGACY_CUSTOM_SYNTAX_NOTICE: LazyLock<StartupNotice> = LazyLock::new(|| {
    StartupNotice::new(
        "deprecated:custom-theme-syntax",
        "Deprecated [custom_theme.syntax] translated approximately • migrate to [custom_theme.syntax_scopes]",
    )
});

/// Reuse one array identity so unchanged configuration reloads do not restart the queue.
pub static LEGACY_CUSTOM_SYNTAX_NOTICES: LazyLock<[&'static StartupNotice; 1]> =
    LazyLock::new(|| [&LEGACY_CUSTOM_SYNTAX_NOTICE]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_notice_has_stable_identity_and_workdeck_copy() {
        let first = LEGACY_CUSTOM_SYNTAX_NOTICES[0];
        let second = LEGACY_CUSTOM_SYNTAX_NOTICES[0];
        assert!(std::ptr::eq(first, second));
        assert_eq!(first.key, "deprecated:custom-theme-syntax");
        assert!(first.message.contains("[custom_theme.syntax_scopes]"));
    }
}
